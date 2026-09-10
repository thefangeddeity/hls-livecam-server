//! User-tunable audio settings, persisted to audio.json in the state dir.
//!
//! Mirrors broadcast-api's two-tier split (see the 2026-09-09 handoff):
//! anything that can break the pipeline outright if fat-fingered stays
//! admin-only in the environment (device names, HLS_AUDIO_ENABLED); only
//! the safe, clamped tunables below are reachable from the Settings
//! panel. One home per key, no override tangle -- a key lives EITHER in
//! the environment or here, never both.
//!
//! Every value is clamped server-side on write and the clamped result is
//! echoed back, so overshooting a bound snaps visibly in the UI instead
//! of silently doing nothing.
//!
//! Env vars of the same name still act as the DEFAULT for a key that has
//! never been written here (so an operator's existing HLS_AUDIO_GAIN_DB
//! export keeps working), but once the panel writes a key, audio.json
//! wins for it.

use serde_json::{json, Map, Value};
use std::path::{Path, PathBuf};
use std::sync::Mutex;

/// (key, min, max, default). TALK_GAIN_DB's ceiling is deliberately
/// lower than the inbound gain's: outbound is the acoustic-feedback
/// knob (room speaker into room mic), so there is less headroom to give
/// away before it starts howling.
const SPEC: &[(&str, f64, f64, f64)] = &[
    ("AUDIO_HIGHPASS_HZ", 0.0, 1000.0, 80.0),
    // Cascaded highpass stages. ffmpeg's highpass is 2nd-order (12
    // dB/oct), which is a gentle shoulder: against city road noise,
    // raising the CORNER to chase it just eats speech and cat
    // fundamentals, while each extra STAGE doubles the slope and leaves
    // everything above the corner alone. Measured on 7elwe: 200 -> 300 Hz
    // bought only -3.6 dB at 125-250, where a second stage is worth far
    // more without moving the corner at all.
    ("AUDIO_HIGHPASS_STAGES", 1.0, 4.0, 1.0),
    ("AUDIO_LOWPASS_HZ", 1000.0, 24000.0, 14000.0),
    // Same story at the top end: `lowpass` is also 2nd-order, and HF
    // hiss was this room's loudest octave before treatment. Symmetric
    // with the high-pass so neither shoulder is the weak one.
    ("AUDIO_LOWPASS_STAGES", 1.0, 4.0, 1.0),
    ("AUDIO_GAIN_DB", -20.0, 30.0, 12.0),
    ("TALK_GAIN_DB", -20.0, 12.0, 0.0),
];

/// Not a number, so it sits outside SPEC's clamp table. Test-only: cuts
/// the room speaker while leaving mic, room audio and signalling up, so
/// an operator can verify a call end to end without the room hearing it.
const MUTE_KEY: &str = "TALK_MUTE_SPEAKER";

pub struct AudioSettings {
    path: PathBuf,
    values: Mutex<Map<String, Value>>,
}

impl AudioSettings {
    pub fn load(dir: &Path) -> Self {
        let path = dir.join("audio.json");
        let values = std::fs::read_to_string(&path)
            .ok()
            .and_then(|s| serde_json::from_str::<Value>(&s).ok())
            .and_then(|v| v.as_object().cloned())
            .unwrap_or_default();
        Self {
            path,
            values: Mutex::new(values),
        }
    }

    /// Every key with its effective value -- persisted if written, else
    /// the environment's, else the built-in default. The panel populates
    /// its fields from this, so an unwritten key still shows the number
    /// actually in force rather than a blank.
    pub fn all(&self) -> Value {
        let g = self.values.lock().unwrap();
        let mut out = Map::new();
        for (key, _, _, default) in SPEC {
            let v = g
                .get(*key)
                .and_then(Value::as_f64)
                .or_else(|| env_f64(key))
                .unwrap_or(*default);
            out.insert((*key).to_string(), json!(v));
        }
        // Read the mute flag straight off the guard we already hold.
        // Calling talk_mute_speaker() here would re-lock the same
        // non-reentrant Mutex and deadlock the request -- and worse,
        // never release, wedging every later reader including
        // build_af_chain's capture restart. (Hit exactly that.)
        let muted = g.get(MUTE_KEY).and_then(Value::as_bool).unwrap_or(false);
        out.insert(MUTE_KEY.to_string(), json!(muted));
        Value::Object(out)
    }

    /// Clamps, persists, and returns the accepted value. Err means the
    /// key isn't one we own or the value wasn't a number/bool at all --
    /// not that it was out of range, which clamps rather than fails.
    pub fn set(&self, key: &str, value: &Value) -> Result<Value, ()> {
        if key == MUTE_KEY {
            let b = value.as_bool().ok_or(())?;
            let mut g = self.values.lock().unwrap();
            g.insert(key.to_string(), json!(b));
            self.persist(&g);
            return Ok(json!(b));
        }
        let (_, min, max, _) = SPEC.iter().find(|(k, ..)| *k == key).ok_or(())?;
        let n = value.as_f64().ok_or(())?;
        let clamped = n.clamp(*min, *max);
        let mut g = self.values.lock().unwrap();
        g.insert(key.to_string(), json!(clamped));
        self.persist(&g);
        Ok(json!(clamped))
    }

    /// Whether a key feeds the room/HLS publisher (and so needs a capture
    /// restart to take effect) or only the talk leg. Restarting the room
    /// publisher for a talk-only tweak needlessly bounces HLS audio for
    /// every listener -- a real bug hit on Tanzania, avoided here by
    /// construction.
    /// Every key that feeds build_af_chain belongs here. Missing one is
    /// silent and confusing: the value persists and the API echoes it
    /// back as accepted, but the chain is never rebuilt, so it simply
    /// has no effect (hit exactly that with AUDIO_HIGHPASS_STAGES --
    /// three cascade stages measured as doing nothing until a different
    /// key forced a restart).
    pub fn affects_room(key: &str) -> bool {
        matches!(
            key,
            "AUDIO_HIGHPASS_HZ"
                | "AUDIO_HIGHPASS_STAGES"
                | "AUDIO_LOWPASS_HZ"
                | "AUDIO_LOWPASS_STAGES"
                | "AUDIO_GAIN_DB"
        )
    }

    pub fn highpass_hz(&self) -> f64 {
        self.num("AUDIO_HIGHPASS_HZ")
    }
    /// How many times to cascade the high-pass. Each stage doubles the
    /// slope (12 dB/oct per stage) -- see the SPEC comment.
    pub fn highpass_stages(&self) -> u32 {
        self.num("AUDIO_HIGHPASS_STAGES").round().clamp(1.0, 4.0) as u32
    }
    pub fn lowpass_hz(&self) -> f64 {
        self.num("AUDIO_LOWPASS_HZ")
    }
    pub fn lowpass_stages(&self) -> u32 {
        self.num("AUDIO_LOWPASS_STAGES").round().clamp(1.0, 4.0) as u32
    }
    pub fn gain_db(&self) -> f64 {
        self.num("AUDIO_GAIN_DB")
    }
    pub fn talk_gain_db(&self) -> f64 {
        self.num("TALK_GAIN_DB")
    }

    pub fn talk_mute_speaker(&self) -> bool {
        self.values
            .lock()
            .unwrap()
            .get(MUTE_KEY)
            .and_then(Value::as_bool)
            .unwrap_or(false)
    }

    fn num(&self, key: &str) -> f64 {
        let (_, _, _, default) = SPEC.iter().find(|(k, ..)| *k == key).expect("key in SPEC");
        self.values
            .lock()
            .unwrap()
            .get(key)
            .and_then(Value::as_f64)
            .or_else(|| env_f64(key))
            .unwrap_or(*default)
    }

    fn persist(&self, values: &Map<String, Value>) {
        if let Ok(s) = serde_json::to_string_pretty(&Value::Object(values.clone())) {
            let _ = std::fs::write(&self.path, s);
        }
    }
}

/// The env var for a key, under the HLS_ prefix this node already uses
/// (HLS_AUDIO_GAIN_DB, HLS_TALK_GAIN_DB, ...).
fn env_f64(key: &str) -> Option<f64> {
    std::env::var(format!("HLS_{key}"))
        .ok()?
        .trim()
        .parse()
        .ok()
}
