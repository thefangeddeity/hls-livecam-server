//! DISK / SMART panel data -- Windows-native, not a port.
//!
//! camdash's REALLOC/PENDING/UNCORR/TEMP/WRITE are legacy ATA SMART
//! attribute IDs (5, 197, 198, 194) read via `smartctl`. On 7elwe:
//!   - the disk is NVMe (Samsung MZVL4512HBLU) -- those ATA attribute IDs
//!     don't exist on NVMe at all, elevated or not. REALLOC and PENDING
//!     specifically have no NVMe equivalent and stay n/a regardless.
//!   - UNCORR and TEMP *do* have real NVMe equivalents, gated behind
//!     `Get-StorageReliabilityCounter` (WMI), which denies a standard
//!     token -- verified live, "Access to a CIM resource was not
//!     available to the client." PM decision: elevate the whole app
//!     (see build.rs) rather than leave these permanently dimmed. With
//!     admin, ReadErrorsUncorrected/WriteErrorsUncorrected map honestly
//!     onto UNCORR, and Temperature is a real reading, not a guess.
//!   - WRITE (MB/s) stays n/a regardless of elevation -- it's a live
//!     throughput rate, not something a reliability-counter snapshot
//!     query provides. Not wired, unrelated to admin rights.
//!
//! `smartctl.exe` (what the Linux fleet actually uses) isn't installed on
//! this box, and installing new system software wasn't judged in scope
//! for "it's guts, your call" -- Windows-native APIs only.

use crate::winproc::hidden;

pub struct DiskInfo {
    pub disk_name: String,
    pub health_status: Option<String>, // "Healthy" / "Warning" / "Unhealthy" / None if query failed
    pub operational_status: Option<String>,
    pub media_type: Option<String>,
    /// Only populated when Get-StorageReliabilityCounter succeeds, which
    /// needs admin. None (not zero) when unavailable -- distinguishes
    /// "checked, no errors" from "couldn't check."
    pub reliability: Option<Reliability>,
}

pub struct Reliability {
    pub read_errors_uncorrected: i64,
    pub write_errors_uncorrected: i64,
    pub temperature_c: Option<f32>,
}

pub fn query() -> DiskInfo {
    let base = query_physical_disk();
    let reliability = query_reliability_counters();
    DiskInfo {
        reliability,
        ..base
    }
}

fn query_physical_disk() -> DiskInfo {
    let output = hidden("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-PhysicalDisk | Select-Object -First 1 FriendlyName,HealthStatus,OperationalStatus,MediaType | ConvertTo-Json -Compress",
        ])
        .output();

    let Ok(output) = output else {
        return DiskInfo {
            disk_name: "?".into(),
            health_status: None,
            operational_status: None,
            media_type: None,
            reliability: None,
        };
    };
    let text = String::from_utf8_lossy(&output.stdout);
    DiskInfo {
        disk_name: extract_json_string(&text, "FriendlyName").unwrap_or_else(|| "?".into()),
        health_status: extract_json_string(&text, "HealthStatus"),
        operational_status: extract_json_string(&text, "OperationalStatus"),
        media_type: extract_json_string(&text, "MediaType"),
        reliability: None,
    }
}

/// Needs admin. Returns None (not a zeroed-out struct) only when the
/// command itself didn't run or exited non-zero -- the caller must be
/// able to tell "no admin" apart from "admin, zero errors."
///
/// Real bug, found from a screenshot: this used to `?`-propagate on
/// `ReadErrorsUncorrected` specifically, treating that one field being
/// null as total failure and discarding a query that otherwise
/// succeeded, including a possibly-valid Temperature reading. NVMe
/// reliability counters aren't uniformly populated across vendors/
/// drivers -- a missing individual counter is a realistic, non-fatal
/// case, not a sign the query failed. `WriteErrorsUncorrected` already
/// tolerated this correctly (`.unwrap_or(0)`); `ReadErrorsUncorrected`
/// didn't, for no principled reason -- just an inconsistency. Both are
/// lenient now; only the command's own success/failure gates None.
fn query_reliability_counters() -> Option<Reliability> {
    let output = match hidden("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Import-Module Storage -ErrorAction SilentlyContinue; \
             Get-PhysicalDisk | Get-StorageReliabilityCounter | Select-Object -First 1 \
             ReadErrorsUncorrected,WriteErrorsUncorrected,Temperature | ConvertTo-Json -Compress",
        ])
        .output()
    {
        Ok(o) => o,
        Err(e) => {
            log_failure(&format!("spawn failed: {e}"));
            return None;
        }
    };
    if !output.status.success() {
        log_failure(&format!(
            "exit {:?}, stderr: {}",
            output.status.code(),
            String::from_utf8_lossy(&output.stderr).trim()
        ));
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    if text.trim().is_empty() {
        log_failure("empty stdout on success exit -- no physical disk matched?");
        return None;
    }
    let read_errors_uncorrected = extract_json_number(&text, "ReadErrorsUncorrected").unwrap_or(0);
    let write_errors_uncorrected = extract_json_number(&text, "WriteErrorsUncorrected").unwrap_or(0);
    let temperature_c = extract_json_number(&text, "Temperature").map(|t| t as f32);
    Some(Reliability {
        read_errors_uncorrected,
        write_errors_uncorrected,
        temperature_c,
    })
}

/// The app has no visible console when launched normally (double-click,
/// scheduled task, Start-Process) -- eprintln! goes nowhere reachable.
/// One line per failure, in the state dir next to everything else this
/// app persists, so a real failure (as opposed to "just not elevated
/// yet") is diagnosable after the fact instead of only during a session
/// launched from a terminal.
fn log_failure(msg: &str) {
    use std::io::Write;
    let path = match std::env::var("APPDATA") {
        Ok(appdata) => std::path::PathBuf::from(appdata).join("hls-livecam-win").join("diskhealth.log"),
        Err(_) => return,
    };
    if let Some(parent) = path.parent() {
        let _ = std::fs::create_dir_all(parent);
    }
    if let Ok(mut f) = std::fs::OpenOptions::new().create(true).append(true).open(&path) {
        let _ = writeln!(f, "[{:?}] {msg}", std::time::SystemTime::now());
    }
}

/// Minimal single-object JSON field extraction. `ConvertTo-Json -Compress`
/// on one object gives one flat `{"Key":"Value",...}` line -- not worth a
/// JSON crate dependency for that.
fn extract_json_string(text: &str, key: &str) -> Option<String> {
    let needle = format!("\"{key}\":\"");
    let start = text.find(&needle)? + needle.len();
    let end = text[start..].find('"')? + start;
    Some(text[start..end].to_string())
}

fn extract_json_number(text: &str, key: &str) -> Option<i64> {
    let needle = format!("\"{key}\":");
    let start = text.find(&needle)? + needle.len();
    let rest = text[start..].trim_start();
    let end = rest
        .find(|c: char| c == ',' || c == '}')
        .unwrap_or(rest.len());
    rest[..end].trim().parse().ok()
}
