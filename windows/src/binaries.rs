//! Locating ffmpeg.exe and mediamtx.exe.
//!
//! Run 2 does not bundle or install these -- that's run 3's job. This just
//! resolves a path to each and fails loudly, with the exact location it
//! looked, if either is missing. That failure mode is deliberate: run 3
//! can satisfy this resolver by *placing* the binaries at the documented
//! spot, nothing about the resolution order needs to change.
//!
//! Resolution order, first hit wins:
//!   1. HLS_FFMPEG / HLS_MEDIAMTX env var (explicit override, for dev/test)
//!   2. bin\ffmpeg.exe / bin\mediamtx.exe next to the running exe (the
//!      run-3 bundle target -- an installer just needs to drop files here)
//!   3. PATH lookup
//!
//! Versions in use this run (recorded here, not enforced -- run 3 decides
//! whether to pin/verify a hash at install time):
//!   ffmpeg   8.1.2-essentials_build (gyan.dev Windows static build)
//!   mediamtx v1.15.2 (matches the version hls-livecam-setup pins on Linux)

use std::path::PathBuf;

pub struct ResolveError {
    pub tool: &'static str,
    pub env_var: &'static str,
    pub checked: Vec<String>,
}

impl std::fmt::Display for ResolveError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        writeln!(f, "error: can't find {}.exe", self.tool)?;
        writeln!(f, "       checked, in order:")?;
        for c in &self.checked {
            writeln!(f, "         - {c}")?;
        }
        write!(
            f,
            "       set {} or place {}.exe in a \"bin\" folder next to this exe.",
            self.env_var, self.tool
        )
    }
}

pub fn resolve_ffmpeg() -> Result<PathBuf, ResolveError> {
    resolve("ffmpeg", "HLS_FFMPEG")
}

pub fn resolve_mediamtx() -> Result<PathBuf, ResolveError> {
    resolve("mediamtx", "HLS_MEDIAMTX")
}

fn resolve(tool: &'static str, env_var: &'static str) -> Result<PathBuf, ResolveError> {
    let exe_name = format!("{tool}.exe");
    let mut checked = Vec::new();

    if let Ok(p) = std::env::var(env_var) {
        checked.push(format!("{env_var}={p}"));
        let path = PathBuf::from(&p);
        if path.is_file() {
            return Ok(path);
        }
    }

    if let Ok(exe) = std::env::current_exe() {
        if let Some(dir) = exe.parent() {
            let candidate = dir.join("bin").join(&exe_name);
            checked.push(candidate.display().to_string());
            if candidate.is_file() {
                return Ok(candidate);
            }
        }
    }

    checked.push(format!("{exe_name} on PATH"));
    if let Some(p) = which_on_path(&exe_name) {
        return Ok(p);
    }

    Err(ResolveError {
        tool,
        env_var,
        checked,
    })
}

fn which_on_path(exe_name: &str) -> Option<PathBuf> {
    let path_var = std::env::var_os("PATH")?;
    std::env::split_paths(&path_var)
        .map(|dir| dir.join(exe_name))
        .find(|p| p.is_file())
}
