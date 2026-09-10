//! Notch-filter comb, persisted to disk, mirroring broadcast-api's
//! /api/notches* endpoints byte-for-byte (same JSON shape, same validation,
//! same status codes) so a Windows node's notch API is indistinguishable
//! from a Linux one. See broadcast-api's `_notch_filters`/`notches_post`/
//! `notches_delete`/`notches_patch`/`notches_sort` -- this is a straight
//! port of that logic, not a redesign.
//!
//! Deliberately untyped (serde_json::Value per entry, like the Python dict
//! it mirrors) rather than a struct: an entry is either {"range":[lo,hi]}
//! or {"center":Hz,"width":Hz}, plus "note"/"enabled"/"added". Staying
//! permissive here means a field this file doesn't specifically know about
//! round-trips unchanged instead of being silently dropped.

use serde_json::{json, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

pub enum NotchError {
    /// Malformed request -- Flask's abort(400).
    Invalid,
    /// Index out of range -- Flask's abort(404).
    NotFound,
    /// Write to disk failed -- Flask's abort(500). A bad filter list must
    /// never cost the room its audio, but a failed *write* means the
    /// change didn't take -- worth telling the operator, unlike a skipped
    /// malformed entry.
    Io,
}

pub struct Notches {
    path: PathBuf,
    entries: Mutex<Vec<Value>>,
}

impl Notches {
    pub fn load(dir: &Path) -> Self {
        let path = dir.join("notches.json");
        let entries = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v.get("notches").cloned())
            .and_then(|v| v.as_array().cloned())
            .unwrap_or_default();
        Self {
            path,
            entries: Mutex::new(entries),
        }
    }

    fn persist(&self, entries: &[Value]) -> std::io::Result<()> {
        let s = serde_json::to_string_pretty(&json!({ "notches": entries }))
            .map_err(|e| std::io::Error::other(e))?;
        std::fs::write(&self.path, s)
    }

    pub fn list(&self) -> Vec<Value> {
        self.entries.lock().unwrap().clone()
    }

    /// Body: {"center":Hz,"width":Hz,"note":"why"} or {"range":[lo,hi],
    /// "note":"why"}. Validated (audible band, ascending range, sane
    /// width), "note" defaulted/truncated, "added" server-stamped --
    /// exactly notches_post's rules.
    pub fn add(&self, body: &Value) -> Result<Vec<Value>, NotchError> {
        let entry = validate_and_normalize(body).ok_or(NotchError::Invalid)?;
        let mut g = self.entries.lock().unwrap();
        g.push(entry);
        self.persist(&g).map_err(|_| NotchError::Io)?;
        Ok(g.clone())
    }

    pub fn delete(&self, i: i64) -> Result<Vec<Value>, NotchError> {
        let mut g = self.entries.lock().unwrap();
        if i < 0 || i as usize >= g.len() {
            return Err(NotchError::NotFound);
        }
        g.remove(i as usize);
        self.persist(&g).map_err(|_| NotchError::Io)?;
        Ok(g.clone())
    }

    pub fn set_enabled_one(&self, i: i64, enabled: bool) -> Result<Vec<Value>, NotchError> {
        let mut g = self.entries.lock().unwrap();
        if i < 0 || i as usize >= g.len() {
            return Err(NotchError::NotFound);
        }
        if let Value::Object(m) = &mut g[i as usize] {
            m.insert("enabled".to_string(), Value::Bool(enabled));
        }
        self.persist(&g).map_err(|_| NotchError::Io)?;
        Ok(g.clone())
    }

    /// No `?i` -- bulk action, every entry set to the same value in one
    /// write. NOT a master switch: there is no separate "all off" flag
    /// layered on top, so per-entry toggles stay editable always (ron:
    /// "Logic of 'All notch filters' toggle is wrong ... you should still
    /// be able to hand-select which ones you want" -- the sticky-master
    /// design this replaced blocked exactly that).
    pub fn set_enabled_all(&self, enabled: bool) -> Result<Vec<Value>, NotchError> {
        let mut g = self.entries.lock().unwrap();
        for entry in g.iter_mut() {
            if let Value::Object(m) = entry {
                m.insert("enabled".to_string(), Value::Bool(enabled));
            }
        }
        self.persist(&g).map_err(|_| NotchError::Io)?;
        Ok(g.clone())
    }

    /// Reorder by centre frequency. Order has no audible effect (bandreject
    /// filters are independent of each other in the -af chain), so unlike
    /// every other mutator here this does NOT need a capture restart.
    pub fn sort(&self) -> Result<Vec<Value>, NotchError> {
        let mut g = self.entries.lock().unwrap();
        g.sort_by(|a, b| {
            entry_center_hz(a)
                .unwrap_or(0.0)
                .partial_cmp(&entry_center_hz(b).unwrap_or(0.0))
                .unwrap_or(std::cmp::Ordering::Equal)
        });
        self.persist(&g).map_err(|_| NotchError::Io)?;
        Ok(g.clone())
    }

    /// The ffmpeg -af chain: enabled notches first, then the fixed
    /// highpass/lowpass/gain/limiter tail -- same order, same defaults as
    /// broadcast-api's audio publisher command, so a room sounds the same
    /// on this node as on any Linux one. A malformed entry is skipped, not
    /// fatal -- see broadcast-api's own comment: a bad filter list must
    /// never cost the room its audio.
    pub fn build_af_chain(&self) -> String {
        let g = self.entries.lock().unwrap();
        let mut parts: Vec<String> = g
            .iter()
            .filter(|e| e.get("enabled").and_then(Value::as_bool).unwrap_or(true))
            .filter_map(entry_ffmpeg_filter)
            .collect();
        if let Some(f) = af_optional("HLS_AUDIO_HIGHPASS_HZ", "80", |v| format!("highpass=f={v}")) {
            parts.push(f);
        }
        if let Some(f) = af_optional("HLS_AUDIO_LOWPASS_HZ", "14000", |v| format!("lowpass=f={v}")) {
            parts.push(f);
        }
        let gain: f64 = std::env::var("HLS_AUDIO_GAIN_DB")
            .ok()
            .and_then(|s| s.parse().ok())
            .unwrap_or(12.0);
        if gain != 0.0 {
            parts.push(format!("volume={gain}dB"));
        }
        parts.push("alimiter=limit=0.95".to_string());
        parts.join(",")
    }
}

fn entry_center_hz(entry: &Value) -> Option<f64> {
    if let Some(range) = entry.get("range").and_then(Value::as_array) {
        let lo = range.first()?.as_f64()?;
        let hi = range.get(1)?.as_f64()?;
        return Some((lo + hi) / 2.0);
    }
    entry.get("center")?.as_f64()
}

fn entry_ffmpeg_filter(entry: &Value) -> Option<String> {
    let (centre, width) = if let Some(range) = entry.get("range").and_then(Value::as_array) {
        let lo = range.first()?.as_f64()?;
        let hi = range.get(1)?.as_f64()?;
        if hi <= lo {
            return None;
        }
        ((lo + hi) / 2.0, hi - lo)
    } else {
        let centre = entry.get("center")?.as_f64()?;
        let width = entry.get("width").and_then(Value::as_f64).unwrap_or(20.0);
        (centre, width)
    };
    Some(format!("bandreject=f={centre:.1}:t=h:w={width:.1}"))
}

/// Mirrors notches_post's validation exactly: range must be ascending and
/// within the audible band (20-20000Hz); center/width the same band plus a
/// sane width (1-500Hz). note defaults to "added from Settings", truncated
/// to 200 chars; "added" is server-stamped, never client-supplied.
fn validate_and_normalize(body: &Value) -> Option<Value> {
    let note_raw = body.get("note").and_then(Value::as_str).unwrap_or("");
    let note: String = note_raw.trim().chars().take(200).collect();
    let note = if note.is_empty() {
        "added from Settings".to_string()
    } else {
        note
    };

    let mut entry = json!({ "note": note, "added": today_ymd() });

    if let Some(range) = body.get("range") {
        let arr = range.as_array()?;
        let lo = arr.first()?.as_f64()?;
        let hi = arr.get(1)?.as_f64()?;
        if !(20.0 <= lo && lo < hi && hi <= 20000.0) {
            return None;
        }
        entry["range"] = json!([lo, hi]);
    } else {
        let c = body.get("center")?.as_f64()?;
        let w = body.get("width").and_then(Value::as_f64).unwrap_or(20.0);
        if !(20.0..=20000.0).contains(&c) || !(1.0..=500.0).contains(&w) {
            return None;
        }
        entry["center"] = json!(c);
        entry["width"] = json!(w);
    }
    Some(entry)
}

fn af_optional(env_var: &str, default: &str, template: impl Fn(&str) -> String) -> Option<String> {
    let v = std::env::var(env_var).unwrap_or_else(|_| default.to_string());
    let v = v.trim();
    if v.is_empty() || matches!(v.to_lowercase().as_str(), "0" | "off" | "none" | "false") {
        None
    } else {
        Some(template(v))
    }
}

/// UTC calendar date as YYYY-MM-DD, no chrono/time crate needed for one
/// stamp. Howard Hinnant's civil_from_days (see
/// http://howardhinnant.github.io/date_algorithms.html), days since the
/// Unix epoch.
fn today_ymd() -> String {
    let secs = std::time::SystemTime::now()
        .duration_since(std::time::UNIX_EPOCH)
        .map(|d| d.as_secs())
        .unwrap_or(0);
    let days = (secs / 86400) as i64;
    let (y, m, d) = civil_from_days(days);
    format!("{y:04}-{m:02}-{d:02}")
}

fn civil_from_days(z: i64) -> (i64, u32, u32) {
    let z = z + 719468;
    let era = if z >= 0 { z } else { z - 146096 } / 146097;
    let doe = (z - era * 146097) as u64;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe as i64 + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = (doy - (153 * mp + 2) / 5 + 1) as u32;
    let m = if mp < 10 { mp + 3 } else { mp - 9 } as u32;
    let y = if m <= 2 { y + 1 } else { y };
    (y, m, d)
}
