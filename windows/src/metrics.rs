//! System metrics for the SYSTEM/PROCESSES panels -- Windows-native via
//! `sysinfo`, not a port of camdash's psutil calls (there's nothing to port,
//! the underlying OS facilities are different). Same display shape, honest
//! sourcing underneath.
//!
//! camdash's LOAD is a Unix load average, which Windows has no kernel
//! equivalent for. Rather than fake it or dim it, the SYSTEM panel shows
//! the honest Windows analog under its own name: PQL (Processor Queue
//! Length -- threads waiting for a core), read from the OS perf counter.
//! CPU TEMP needs a WMI ACPI thermal zone or vendor sensor most laptops
//! (this HP included) don't expose; None when sysinfo's Components list
//! is empty, and the row is hidden rather than shown as a dead n/a.

use std::time::{Duration, Instant};

use sysinfo::{Components, System};

/// PQL is a perf-counter read (shelling out to PowerShell/CIM), too heavy
/// for the 1s metrics tick, so it's sampled at this slower cadence and
/// cached between samples. A processor queue trend doesn't need 1s
/// granularity for an operator glance.
const PQL_REFRESH: Duration = Duration::from_secs(5);

pub struct Snapshot {
    pub cpu_percent: f32,
    pub mem_percent: f32,
    pub mem_used_mb: u64,
    pub mem_avail_mb: u64,
    pub swap_percent: f32,
    pub swap_label: &'static str,
    /// Processor Queue Length (Windows' honest analog to Unix load).
    /// None only if the perf-counter query failed.
    pub pql: Option<f64>,
    pub cpu_temp_c: Option<f32>,
    pub uptime_secs: u64,
    pub cores: usize,
    pub top_processes: Vec<ProcRow>,
}

pub struct ProcRow {
    pub name: String,
    pub cpu_percent: f32,
}

pub struct Metrics {
    sys: System,
    components: Components,
    pql_cache: Option<f64>,
    pql_at: Option<Instant>,
}

impl Metrics {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        Self {
            sys,
            components: Components::new_with_refreshed_list(),
            pql_cache: None,
            pql_at: None,
        }
    }

    pub fn refresh(&mut self) -> Snapshot {
        self.sys.refresh_cpu_usage();
        self.sys.refresh_memory();
        self.sys.refresh_processes(sysinfo::ProcessesToUpdate::All, true);
        self.components.refresh(true);

        let cpu_percent = self.sys.global_cpu_usage();
        let total_mem = self.sys.total_memory();
        let used_mem = self.sys.used_memory();
        let mem_percent = if total_mem > 0 {
            used_mem as f32 / total_mem as f32 * 100.0
        } else {
            0.0
        };
        let total_swap = self.sys.total_swap();
        let used_swap = self.sys.used_swap();
        let swap_percent = if total_swap > 0 {
            used_swap as f32 / total_swap as f32 * 100.0
        } else {
            0.0
        };

        let cores = self.sys.cpus().len().max(1);

        // PQL, sampled at PQL_REFRESH and cached in between (the query
        // shells out; too heavy for every 1s tick).
        if self.pql_at.map_or(true, |t| t.elapsed() >= PQL_REFRESH) {
            self.pql_cache = query_pql();
            self.pql_at = Some(Instant::now());
        }
        let pql = self.pql_cache;

        let cpu_temp_c = self
            .components
            .iter()
            .find(|c| {
                let l = c.label().to_lowercase();
                l.contains("cpu") || l.contains("package") || l.contains("core")
            })
            .and_then(|c| c.temperature());

        let mut top_processes: Vec<ProcRow> = self
            .sys
            .processes()
            .values()
            .map(|p| ProcRow {
                name: p.name().to_string_lossy().to_string(),
                cpu_percent: p.cpu_usage(),
            })
            .collect();
        top_processes.sort_by(|a, b| b.cpu_percent.partial_cmp(&a.cpu_percent).unwrap());
        // Upper bound, NOT the display count -- the PROCESSES panel draws
        // as many as fit its height and stops. 15 ran short in the tall
        // panel on a portrait/vertical monitor (operator), so this is
        // sized to fill a very tall box; extra rows beyond what fits are
        // simply never drawn (no cost but the Vec entries). Rows past the
        // active few are near-0% and render muted, so they read as a quiet
        // tail rather than noise.
        top_processes.truncate(64);

        Snapshot {
            cpu_percent,
            mem_percent,
            mem_used_mb: used_mem / (1024 * 1024),
            mem_avail_mb: (total_mem.saturating_sub(used_mem)) / (1024 * 1024),
            swap_percent,
            swap_label: "pagefile",
            pql,
            cpu_temp_c,
            uptime_secs: System::uptime(),
            cores,
            top_processes,
        }
    }
}

/// Processor Queue Length from the Windows perf subsystem -- the count of
/// threads waiting for a processor (the honest Windows analog to Unix
/// load average). Cooked counter, so a single CIM read returns a current
/// value. Readable without admin. None only on query failure.
fn query_pql() -> Option<f64> {
    let output = crate::winproc::hidden("powershell.exe")
        .args([
            "-NoProfile",
            "-NonInteractive",
            "-Command",
            "(Get-CimInstance Win32_PerfFormattedData_PerfOS_System).ProcessorQueueLength",
        ])
        .output()
        .ok()?;
    if !output.status.success() {
        return None;
    }
    String::from_utf8_lossy(&output.stdout).trim().parse().ok()
}
