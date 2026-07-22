//! Embeds a Windows application manifest requesting `requireAdministrator`.
//!
//! PM decision: the DISK/SMART panel needs `Get-StorageReliabilityCounter`
//! for real NVMe wear/temperature/error-count data, and that WMI class
//! denies access to a standard token (verified live: "Access to a CIM
//! resource was not available to the client"). Elevating the whole app
//! is the tradeoff Ron chose over leaving the panel dimmed. See
//! autostart.rs for the corresponding lifecycle change this forces
//! (Registry Run key -> a Scheduled Task with "run with highest
//! privileges", since a Run-key launch of a `requireAdministrator` exe
//! prompts UAC on every login -- silent auto-elevation only works from a
//! pre-approved elevated task).

fn main() {
    // embed-manifest emits `rustc-link-arg-bins`, which also lands on a
    // bin crate's *unit-test* harness -- so `cargo test` produces an exe
    // that itself requires elevation, and the test runner can't launch it
    // ("os error 740: requires elevation"). Gate embedding behind an env
    // var so the test run can opt out (HLS_SKIP_MANIFEST=1 cargo test);
    // the shipped release build, run without it, still gets the manifest.
    let is_windows = std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows");

    let skip = std::env::var("HLS_SKIP_MANIFEST").is_ok();
    if is_windows && !skip {
        embed_manifest::embed_manifest(
            embed_manifest::new_manifest("hls-livecam-win")
                .requested_execution_level(embed_manifest::manifest::ExecutionLevel::RequireAdministrator),
        )
        .expect("failed to embed application manifest");
    }

    // Embed the app icon on the exe file itself (separate from the
    // manifest and NOT gated by HLS_SKIP_MANIFEST -- an icon doesn't block
    // the test harness the way requireAdministrator does, so the test/
    // probe builds get it too). winresource shells out to the Windows SDK
    // rc.exe; a failure is a warning, not a hard build error -- a missing
    // icon is cosmetic, not a reason to fail to build.
    if is_windows {
        let mut res = winresource::WindowsResource::new();
        res.set_icon("assets/icon.ico");
        if let Err(e) = res.compile() {
            println!("cargo:warning=app icon embed failed: {e}");
        }
    }

    println!("cargo:rerun-if-changed=build.rs");
    println!("cargo:rerun-if-changed=assets/icon.ico");
    println!("cargo:rerun-if-env-changed=HLS_SKIP_MANIFEST");
}
