//! Autostart via a Scheduled Task, not the Registry Run key.
//!
//! Run 3 used HKCU\...\Run, which needs no elevation to install and works
//! fine for an unprivileged exe. That stopped being true once this app
//! started requiring admin (build.rs's manifest, added so DISK/SMART can
//! read Get-StorageReliabilityCounter): a Run-key launch of a
//! requireAdministrator exe still triggers a UAC consent prompt on every
//! login -- Windows does not silently elevate Run-key entries, manifest
//! or not. The only way to get a genuinely silent auto-elevated launch at
//! logon is a Scheduled Task with "run with highest privileges" set --
//! Windows pre-approves that at task-registration time (which itself
//! needs admin, but by the time this code runs the whole process is
//! already elevated, so registering it here needs no extra prompt).
//!
//! schtasks.exe rather than the Task Scheduler COM API -- shelling out
//! matches the pattern already used for Get-PhysicalDisk/tailscale
//! (diskhealth.rs, routes.rs) rather than pulling in a COM binding for a
//! one-shot registration.

use crate::winproc::hidden;

const TASK_NAME: &str = "hls-livecam-win";

pub fn is_installed() -> bool {
    current_task_target().is_some()
}

/// Registers the *currently running* exe's path. Idempotent -- safe to
/// call on every launch; only re-creates the task if the path actually
/// changed (e.g. after an update moved the exe).
pub fn ensure_installed() -> std::io::Result<bool> {
    let exe = std::env::current_exe()?;
    let exe_str = exe.display().to_string();

    if current_task_target().as_deref() == Some(exe_str.as_str()) {
        return Ok(false); // already correct, no re-registration needed
    }

    let status = hidden("schtasks")
        .args([
            "/Create",
            "/TN",
            TASK_NAME,
            "/TR",
            &format!("\"{exe_str}\""),
            "/SC",
            "ONLOGON",
            "/RL",
            "HIGHEST",
            "/F", // overwrite any existing registration
        ])
        .status()?;

    if !status.success() {
        return Err(std::io::Error::other(
            "schtasks /Create failed -- is this process actually elevated?",
        ));
    }
    Ok(true)
}

pub fn remove() -> std::io::Result<()> {
    let _ = hidden("schtasks")
        .args(["/Delete", "/TN", TASK_NAME, "/F"])
        .status();
    Ok(())
}

/// Parses `schtasks /Query /V /FO LIST`'s "Task To Run" line, which wraps
/// the target path in quotes exactly as registered. Returns None if the
/// task doesn't exist or the field can't be found -- both treated the
/// same by the idempotency check (register/re-register).
fn current_task_target() -> Option<String> {
    let output = hidden("schtasks")
        .args(["/Query", "/TN", TASK_NAME, "/V", "/FO", "LIST"])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    let text = String::from_utf8_lossy(&output.stdout);
    let line = text.lines().find(|l| l.starts_with("Task To Run:"))?;
    let value = line.splitn(2, ':').nth(1)?.trim();
    Some(value.trim_matches('"').to_string())
}
