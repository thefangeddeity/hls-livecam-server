//! Shared helper for shelling out to Windows console tools (powershell.exe,
//! schtasks.exe) without flashing a console window -- this app has no
//! console of its own (it's a GUI subsystem exe), so a plain `Command`
//! would otherwise pop a visible conhost window for each call.

use std::process::Command;

/// The Windows `CREATE_NO_WINDOW` process-creation flag. Exposed so the
/// async spawn sites (tokio::process::Command in pipeline.rs and
/// video_preview.rs) can apply it too via their own inherent
/// `creation_flags` method. EVERY child this app spawns must set it: the
/// app is a GUI-subsystem exe with no console of its own, so without the
/// flag Windows gives each spawned console tool (ffmpeg, mediamtx,
/// powershell, tailscale, ...) its own black console window -- the
/// operator was "drowning in them" (one per action).
pub const CREATE_NO_WINDOW: u32 = 0x0800_0000;

pub fn hidden(program: &str) -> Command {
    let mut cmd = Command::new(program);
    apply_hidden(&mut cmd);
    cmd
}

#[cfg(windows)]
fn apply_hidden(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn apply_hidden(_cmd: &mut Command) {}
