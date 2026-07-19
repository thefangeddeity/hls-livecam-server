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
    if std::env::var("CARGO_CFG_TARGET_OS").as_deref() == Ok("windows") {
        embed_manifest::embed_manifest(
            embed_manifest::new_manifest("hls-livecam-win")
                .requested_execution_level(embed_manifest::manifest::ExecutionLevel::RequireAdministrator),
        )
        .expect("failed to embed application manifest");
    }
    println!("cargo:rerun-if-changed=build.rs");
}
