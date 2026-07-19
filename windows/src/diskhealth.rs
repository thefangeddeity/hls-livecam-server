//! DISK / SMART panel data -- Windows-native, not a port.
//!
//! camdash's REALLOC/PENDING/UNCORR/TEMP/WRITE are legacy ATA SMART
//! attribute IDs (5, 197, 198, 194) read via `smartctl`. On 7elwe:
//!   - the disk is NVMe (Samsung MZVL4512HBLU) -- those ATA attribute IDs
//!     don't exist on NVMe at all, elevated or not. There's no honest
//!     value to show regardless of permissions.
//!   - `Get-StorageReliabilityCounter` (the WMI class that *would* carry
//!     NVMe's equivalents -- wear, temperature, read errors) requires
//!     admin: verified live, "Access to a CIM resource was not available
//!     to the client" on a standard user token. Requiring elevation would
//!     mean either a UAC prompt on every autostart launch or running the
//!     whole operator app as admin persistently -- disproportionate for
//!     one panel's worth of numbers, so not done.
//!   - `smartctl.exe` (what the Linux fleet actually uses) isn't installed
//!     on this box, and installing new system software wasn't judged to
//!     be in scope for "it's guts, your call."
//!
//! What's shown instead: `Get-PhysicalDisk`, unelevated, which does work
//! and gives a real (if coarser) health signal -- HealthStatus maps to
//! ASSESS, OperationalStatus feeds RISK. REALLOC/PENDING/UNCORR/TEMP/WRITE
//! are dimmed N/A rather than faked.

use std::process::Command;

pub struct DiskInfo {
    pub disk_name: String,
    pub health_status: Option<String>, // "Healthy" / "Warning" / "Unhealthy" / None if query failed
    pub operational_status: Option<String>,
    pub media_type: Option<String>,
}

pub fn query() -> DiskInfo {
    let output = Command::new("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "Get-PhysicalDisk | Select-Object -First 1 FriendlyName,HealthStatus,OperationalStatus,MediaType | ConvertTo-Json -Compress",
        ])
        .creation_flags_hidden()
        .output();

    let Ok(output) = output else {
        return DiskInfo {
            disk_name: "?".into(),
            health_status: None,
            operational_status: None,
            media_type: None,
        };
    };
    let text = String::from_utf8_lossy(&output.stdout);
    DiskInfo {
        disk_name: extract_json_string(&text, "FriendlyName").unwrap_or_else(|| "?".into()),
        health_status: extract_json_string(&text, "HealthStatus"),
        operational_status: extract_json_string(&text, "OperationalStatus"),
        media_type: extract_json_string(&text, "MediaType"),
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

trait HideWindow {
    fn creation_flags_hidden(&mut self) -> &mut Self;
}
impl HideWindow for Command {
    #[cfg(windows)]
    fn creation_flags_hidden(&mut self) -> &mut Self {
        use std::os::windows::process::CommandExt;
        const CREATE_NO_WINDOW: u32 = 0x0800_0000;
        self.creation_flags(CREATE_NO_WINDOW)
    }
    #[cfg(not(windows))]
    fn creation_flags_hidden(&mut self) -> &mut Self {
        self
    }
}
