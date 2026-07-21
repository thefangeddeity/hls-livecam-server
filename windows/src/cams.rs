//! The node's fleet roster (`cams.json`) as an editable store.
//!
//! This is the exact file the fleet's `cams.html` aggregator fetches from
//! `/cams/cams.json` (served by routes.rs off the state dir), and the one
//! `AppState::cams_json()` reads. The schema is the fleet's, not ours:
//! each entry is a JSON object the viewer reads `ip`, `label`, `pinned`,
//! and optional `api_port` / `stream_path` off of. The IP manager only
//! edits name/ip/port, so entries are kept as raw JSON objects and only
//! those keys are touched -- any other field the fleet uses (`pinned`,
//! `stream_path`, future additions) round-trips untouched. New entries
//! are created `pinned: true` so they actually appear in the grid.
//!
//! Scope (run-6 brief): a working entry manager over THIS node's
//! cams.json. No fleet sync -- writing here edits this node's file, which
//! is what a peer polling this node's `/cams/cams.json` would then see.

use std::net::IpAddr;
use std::path::PathBuf;
use std::str::FromStr;

use serde_json::{Map, Value};

/// One editable row, projected from a JSON object. `raw` keeps the whole
/// original object so save() can write back fields the manager doesn't
/// surface.
#[derive(Clone)]
pub struct Cam {
    pub label: String,
    pub ip: String,
    /// None = no `api_port` key (viewer then omits the port suffix).
    pub port: Option<u16>,
    raw: Map<String, Value>,
}

impl Cam {
    fn from_object(obj: &Map<String, Value>) -> Self {
        let label = obj.get("label").and_then(|v| v.as_str()).unwrap_or("").to_string();
        let ip = obj.get("ip").and_then(|v| v.as_str()).unwrap_or("").to_string();
        // api_port may be a JSON number or a stringified number in the
        // wild; accept either.
        let port = obj.get("api_port").and_then(|v| {
            v.as_u64()
                .or_else(|| v.as_str().and_then(|s| s.parse::<u64>().ok()))
        });
        let port = port.and_then(|p| u16::try_from(p).ok());
        Cam {
            label,
            ip,
            port,
            raw: obj.clone(),
        }
    }

    fn new(label: String, ip: String, port: Option<u16>) -> Self {
        let mut raw = Map::new();
        // Default a new entry to pinned so it shows up in the aggregator
        // grid -- an unpinned entry is invisible there, which would read
        // as "nothing happened" after Add.
        raw.insert("pinned".to_string(), Value::Bool(true));
        let mut cam = Cam { label, ip, port, raw };
        cam.sync_raw();
        cam
    }

    /// Writes the editable fields back into the retained JSON object.
    fn sync_raw(&mut self) {
        self.raw.insert("label".to_string(), Value::String(self.label.clone()));
        self.raw.insert("ip".to_string(), Value::String(self.ip.clone()));
        match self.port {
            Some(p) => {
                self.raw.insert("api_port".to_string(), Value::Number(p.into()));
            }
            None => {
                self.raw.remove("api_port");
            }
        }
    }

    fn into_value(mut self) -> Value {
        self.sync_raw();
        Value::Object(self.raw)
    }
}

pub struct CamStore {
    path: PathBuf,
    pub cams: Vec<Cam>,
    /// True when the on-disk file existed but did not parse as a JSON
    /// array of objects -- surfaced in the UI so an edit doesn't silently
    /// overwrite a hand-maintained file the manager couldn't read.
    pub parse_failed: bool,
}

impl CamStore {
    pub fn load() -> Self {
        Self::load_from(crate::state::state_dir().join("cams.json"))
    }

    fn load_from(path: PathBuf) -> Self {
        let (cams, parse_failed) = match std::fs::read_to_string(&path) {
            Ok(text) if !text.trim().is_empty() => match serde_json::from_str::<Value>(&text) {
                Ok(Value::Array(items)) => {
                    let cams = items
                        .iter()
                        .filter_map(|v| v.as_object().map(Cam::from_object))
                        .collect();
                    (cams, false)
                }
                _ => (Vec::new(), true),
            },
            // Missing or empty file is the normal fresh-node state (the
            // default roster is `[]`), not a parse failure.
            _ => (Vec::new(), false),
        };
        CamStore {
            path,
            cams,
            parse_failed,
        }
    }

    pub fn add(&mut self, label: String, ip: String, port: Option<u16>) {
        self.cams.push(Cam::new(label, ip, port));
    }

    pub fn update(&mut self, idx: usize, label: String, ip: String, port: Option<u16>) {
        if let Some(c) = self.cams.get_mut(idx) {
            c.label = label;
            c.ip = ip;
            c.port = port;
            c.sync_raw();
        }
    }

    pub fn remove(&mut self, idx: usize) {
        if idx < self.cams.len() {
            self.cams.remove(idx);
        }
    }

    /// Serialize and write the roster. Pretty-printed with a trailing
    /// newline -- this is a human-editable fleet file, not a hot path.
    pub fn save(&self) -> std::io::Result<()> {
        let arr = Value::Array(self.cams.iter().cloned().map(Cam::into_value).collect());
        let mut body = serde_json::to_string_pretty(&arr).unwrap_or_else(|_| "[]".to_string());
        body.push('\n');
        if let Some(parent) = self.path.parent() {
            let _ = std::fs::create_dir_all(parent);
        }
        std::fs::write(&self.path, body)
    }
}

/// IP validation for the manager. Accepts any valid IPv4 or IPv6 literal
/// (the fleet is Tailscale IPv4 100.x in practice, but there's no reason
/// to reject a valid v6). Rejects hostnames -- the brief asks for IP
/// format specifically, and the viewer builds `http://<ip>...` URLs.
pub fn valid_ip(s: &str) -> bool {
    IpAddr::from_str(s.trim()).is_ok()
}

/// Port validation: empty is allowed (optional -> None); otherwise must
/// be a 1..=65535 integer. Returns Ok(None) for empty, Ok(Some(p)) for a
/// valid port, Err for anything else.
pub fn parse_port(s: &str) -> Result<Option<u16>, ()> {
    let t = s.trim();
    if t.is_empty() {
        return Ok(None);
    }
    match t.parse::<u16>() {
        Ok(p) if p >= 1 => Ok(Some(p)),
        _ => Err(()),
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use serde_json::json;

    fn temp_path(tag: &str) -> PathBuf {
        // Test-local file next to the build dir; each test uses a unique
        // tag so parallel runs don't collide. No external temp crate.
        std::env::temp_dir().join(format!("hls_cams_test_{tag}.json"))
    }

    #[test]
    fn ip_validation() {
        assert!(valid_ip("100.100.17.1"));
        assert!(valid_ip("10.0.0.1"));
        assert!(valid_ip("::1")); // v6 accepted
        assert!(!valid_ip(""));
        assert!(!valid_ip("999.1.1.1"));
        assert!(!valid_ip("100.100.17")); // too few octets
        assert!(!valid_ip("camera-1")); // hostname rejected
        assert!(valid_ip("100.100.17.1 ")); // trailing space is trimmed
    }

    #[test]
    fn port_validation() {
        assert_eq!(parse_port(""), Ok(None));
        assert_eq!(parse_port("   "), Ok(None));
        assert_eq!(parse_port("8080"), Ok(Some(8080)));
        assert_eq!(parse_port("1"), Ok(Some(1)));
        assert_eq!(parse_port("65535"), Ok(Some(65535)));
        assert_eq!(parse_port("0"), Err(()));
        assert_eq!(parse_port("70000"), Err(())); // > u16
        assert_eq!(parse_port("abc"), Err(()));
    }

    #[test]
    fn add_save_reload_roundtrip() {
        let path = temp_path("roundtrip");
        let _ = std::fs::remove_file(&path);

        let mut store = CamStore::load_from(path.clone());
        assert!(store.cams.is_empty());
        store.add("Front door".into(), "100.100.17.1".into(), Some(8080));
        store.add("Back yard".into(), "100.100.17.2".into(), None);
        store.save().unwrap();

        // Serialized form is a valid JSON array carrying the fleet's field
        // names, including pinned:true so entries show in the grid.
        let text = std::fs::read_to_string(&path).unwrap();
        let v: Value = serde_json::from_str(&text).unwrap();
        let arr = v.as_array().unwrap();
        assert_eq!(arr.len(), 2);
        assert_eq!(arr[0]["ip"], json!("100.100.17.1"));
        assert_eq!(arr[0]["label"], json!("Front door"));
        assert_eq!(arr[0]["api_port"], json!(8080));
        assert_eq!(arr[0]["pinned"], json!(true));
        // Optional port omitted entirely when None.
        assert_eq!(arr[1].get("api_port"), None);

        // Reload sees the same entries.
        let reloaded = CamStore::load_from(path.clone());
        assert_eq!(reloaded.cams.len(), 2);
        assert_eq!(reloaded.cams[0].port, Some(8080));
        assert_eq!(reloaded.cams[1].port, None);
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn preserves_unknown_fields() {
        let path = temp_path("preserve");
        // A roster the manager didn't create, carrying fields it doesn't
        // surface (stream_path, a hypothetical future key).
        std::fs::write(
            &path,
            r#"[{"label":"Cam","ip":"100.0.0.9","pinned":false,"stream_path":":9999/x.m3u8","future":42}]"#,
        )
        .unwrap();

        let mut store = CamStore::load_from(path.clone());
        assert_eq!(store.cams.len(), 1);
        // Edit only the name; everything else must survive the round-trip.
        store.update(0, "Renamed".into(), "100.0.0.9".into(), None);
        store.save().unwrap();

        let v: Value = serde_json::from_str(&std::fs::read_to_string(&path).unwrap()).unwrap();
        let e = &v.as_array().unwrap()[0];
        assert_eq!(e["label"], json!("Renamed"));
        assert_eq!(e["stream_path"], json!(":9999/x.m3u8")); // preserved
        assert_eq!(e["future"], json!(42)); // preserved
        assert_eq!(e["pinned"], json!(false)); // preserved, not clobbered
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn full_crud_cycle() {
        let path = temp_path("crud");
        let _ = std::fs::remove_file(&path);
        let read_arr = |p: &PathBuf| -> Vec<Value> {
            serde_json::from_str::<Value>(&std::fs::read_to_string(p).unwrap())
                .unwrap()
                .as_array()
                .unwrap()
                .clone()
        };

        // CREATE
        let mut store = CamStore::load_from(path.clone());
        store.add("Living room".into(), "192.168.1.50".into(), Some(8080));
        store.save().unwrap();
        let a = read_arr(&path);
        assert_eq!(a.len(), 1);
        assert_eq!(a[0]["label"], json!("Living room"));

        // UPDATE (edit in place -- the "clicking an entry loads it, Save
        // updates it" path the dialog drives via CamStore::update).
        store.update(0, "Front porch".into(), "192.168.1.51".into(), None);
        store.save().unwrap();
        let a = read_arr(&path);
        assert_eq!(a.len(), 1, "update must not add a row");
        assert_eq!(a[0]["label"], json!("Front porch"));
        assert_eq!(a[0]["ip"], json!("192.168.1.51"));
        assert_eq!(a[0].get("api_port"), None, "cleared port drops the key");

        // DELETE
        store.remove(0);
        store.save().unwrap();
        assert!(read_arr(&path).is_empty());
        let _ = std::fs::remove_file(&path);
    }

    #[test]
    fn bad_json_flags_parse_failure() {
        let path = temp_path("badjson");
        std::fs::write(&path, "{not an array}").unwrap();
        let store = CamStore::load_from(path.clone());
        assert!(store.parse_failed);
        assert!(store.cams.is_empty());
        let _ = std::fs::remove_file(&path);
    }
}
