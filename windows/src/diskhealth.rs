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

/// Needs admin. Returns None (not a zeroed-out struct) if the CIM call is
/// denied -- the caller must be able to tell "no admin" apart from "admin,
/// zero errors," and the UI dims the former rather than showing a
/// misleadingly clean "0."
fn query_reliability_counters() -> Option<Reliability> {
    let output = hidden("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-PhysicalDisk | Get-StorageReliabilityCounter | Select-Object -First 1 ReadErrorsUncorrected,WriteErrorsUncorrected,Temperature | ConvertTo-Json -Compress",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let read_errors_uncorrected = extract_json_number(&text, "ReadErrorsUncorrected")?;
    let write_errors_uncorrected = extract_json_number(&text, "WriteErrorsUncorrected").unwrap_or(0);
    let temperature_c = extract_json_number(&text, "Temperature").map(|t| t as f32);
    Some(Reliability {
        read_errors_uncorrected,
        write_errors_uncorrected,
        temperature_c,
    })
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
