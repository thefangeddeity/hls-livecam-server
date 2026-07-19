//! System metrics for the SYSTEM/PROCESSES panels -- Windows-native via
//! `sysinfo`, not a port of camdash's psutil calls (there's nothing to port,
//! the underlying OS facilities are different). Same display shape, honest
//! sourcing underneath.
//!
//! Two fields camdash shows have no honest Windows equivalent and are
//! surfaced as `None` here rather than faked:
//!   - LOAD: Unix load average. sysinfo's `System::load_average()` on
//!     Windows is a synthesized approximation, not a kernel-reported
//!     number the way it is on Linux -- displaying it as "LOAD" would
//!     misrepresent what it is. Dimmed to n/a.
//!   - CPU TEMP: needs a WMI ACPI thermal zone or vendor sensor most
//!     laptops (this HP included) don't expose cleanly. If sysinfo's
//!     Components list comes back empty, that's surfaced as None, not a
//!     fabricated reading.

use sysinfo::{Components, System};

pub struct Snapshot {
    pub cpu_percent: f32,
    pub mem_percent: f32,
    pub mem_used_mb: u64,
    pub mem_avail_mb: u64,
    pub swap_percent: f32,
    pub swap_label: &'static str,
    /// None = no honest Windows equivalent; see module docs.
    pub load: Option<(f64, usize)>,
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
}

impl Metrics {
    pub fn new() -> Self {
        let mut sys = System::new_all();
        sys.refresh_all();
        Self {
            sys,
            components: Components::new_with_refreshed_list(),
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
        let load_avg = System::load_average();
        // sysinfo synthesizes this on Windows rather than reading a kernel
        // value -- see module docs. Treat an all-zero reading as "not a
        // real signal" rather than "system idle."
        let load = if load_avg.one > 0.0 {
            Some((load_avg.one, cores))
        } else {
            None
        };

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
        top_processes.truncate(8);

        Snapshot {
            cpu_percent,
            mem_percent,
            mem_used_mb: used_mem / (1024 * 1024),
            mem_avail_mb: (total_mem.saturating_sub(used_mem)) / (1024 * 1024),
            swap_percent,
            swap_label: "pagefile",
            load,
            cpu_temp_c,
            uptime_secs: System::uptime(),
            cores,
            top_processes,
        }
    }
}
