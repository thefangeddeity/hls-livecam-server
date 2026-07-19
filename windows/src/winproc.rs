//! Shared helper for shelling out to Windows console tools (powershell.exe,
//! schtasks.exe) without flashing a console window -- this app has no
//! console of its own (it's a GUI subsystem exe), so a plain `Command`
//! would otherwise pop a visible conhost window for each call.

use std::process::Command;

pub fn hidden(program: &str) -> Command {
    let mut cmd = Command::new(program);
    apply_hidden(&mut cmd);
    cmd
}

#[cfg(windows)]
fn apply_hidden(cmd: &mut Command) {
    use std::os::windows::process::CommandExt;
    const CREATE_NO_WINDOW: u32 = 0x0800_0000;
    cmd.creation_flags(CREATE_NO_WINDOW);
}

#[cfg(not(windows))]
fn apply_hidden(_cmd: &mut Command) {}
