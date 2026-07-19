//! Autostart via the per-user Registry Run key.
//!
//! `HKCU\...\Run` over a Startup-folder shortcut or a Scheduled Task: no
//! .lnk/COM shell-link machinery needed, no task-scheduler XML, and HKCU
//! (not HKLM) means no elevation to install it. Starts on login, same as
//! any ordinary Run-key app -- not before login. For a dedicated family-
//! camera box the pragmatic answer for "survives a reboot with no operator
//! action" is Windows auto-logon + this, which was already the run-1/2
//! plan's stated lifecycle call; nothing here changes that trade-off.

use winreg::enums::*;
use winreg::RegKey;

const RUN_KEY_PATH: &str = r"Software\Microsoft\Windows\CurrentVersion\Run";
const VALUE_NAME: &str = "hls-livecam-win";

pub fn is_installed() -> bool {
    current_value().is_some()
}

/// Registers the *currently running* exe's path. Idempotent -- safe to
/// call on every launch; only writes if the path actually changed (e.g.
/// after an update moved the exe).
pub fn ensure_installed() -> std::io::Result<bool> {
    let exe = std::env::current_exe()?;
    let exe_str = format!("\"{}\"", exe.display());

    if current_value().as_deref() == Some(exe_str.as_str()) {
        return Ok(false); // already correct, no write needed
    }

    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let (key, _) = hkcu.create_subkey(RUN_KEY_PATH)?;
    key.set_value(VALUE_NAME, &exe_str)?;
    Ok(true)
}

pub fn remove() -> std::io::Result<()> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    if let Ok(key) = hkcu.open_subkey_with_flags(RUN_KEY_PATH, KEY_SET_VALUE) {
        let _ = key.delete_value(VALUE_NAME);
    }
    Ok(())
}

fn current_value() -> Option<String> {
    let hkcu = RegKey::predef(HKEY_CURRENT_USER);
    let key = hkcu.open_subkey(RUN_KEY_PATH).ok()?;
    key.get_value(VALUE_NAME).ok()
}
