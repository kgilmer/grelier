// Disk usage gauge for a configurable filesystem path.
// Consumes Settings: grelier.gauge.disk.*.
use crate::dialog::info::InfoDialog;
use crate::icon::{icon_quantity, svg_asset};
use crate::panels::gauges::gauge::Gauge;
use crate::panels::gauges::gauge::{
    GaugeDisplay, GaugeInteractionModel, GaugeModel, GaugePointerInteraction, GaugeValue,
    GaugeValueAttention,
};
use crate::panels::gauges::gauge_registry::GaugeSpec;
use crate::settings;
use crate::settings::SettingSpec;
use std::cmp::Ordering;
use std::ffi::CString;
use std::fs;
use std::mem::MaybeUninit;
use std::os::raw::{c_char, c_int, c_ulong};
use std::time::{Duration, Instant};

const DEFAULT_ROOT_PATH: &str = "/";
const DEFAULT_POLL_INTERVAL_SECS: u64 = 60;
const DEFAULT_WARNING_THRESHOLD: f32 = 0.85;
const DEFAULT_DANGER_THRESHOLD: f32 = 0.95;

#[repr(C)]
#[derive(Clone, Copy)]
struct Statvfs {
    f_bsize: c_ulong,
    f_frsize: c_ulong,
    f_blocks: c_ulong,
    f_bfree: c_ulong,
    f_bavail: c_ulong,
    f_files: c_ulong,
    f_ffree: c_ulong,
    f_favail: c_ulong,
    f_fsid: c_ulong,
    f_flag: c_ulong,
    f_namemax: c_ulong,
    f_spare: [c_int; 6],
}

unsafe extern "C" {
    fn statvfs(path: *const c_char, buf: *mut Statvfs) -> c_int;
}

#[derive(Clone, Copy)]
struct DiskUsage {
    used: u64,
    total: u64,
}

fn disk_usage(path: &str) -> Option<DiskUsage> {
    let c_path = CString::new(path).ok()?;

    let mut stats = MaybeUninit::<Statvfs>::uninit();
    let result = unsafe { statvfs(c_path.as_ptr(), stats.as_mut_ptr()) };
    if result != 0 {
        return None;
    }

    let stats = unsafe { stats.assume_init() };
    let fragment_size = stats.f_frsize;
    if fragment_size == 0 {
        return None;
    }

    let total_blocks = stats.f_blocks;
    if total_blocks == 0 {
        return None;
    }

    let used_blocks = total_blocks.saturating_sub(stats.f_bfree);

    let total = total_blocks.saturating_mul(fragment_size);
    let used = used_blocks.saturating_mul(fragment_size);

    Some(DiskUsage { used, total })
}

fn mount_device_for_path(path: &str) -> Option<String> {
    let mounts = fs::read_to_string("/proc/self/mounts").ok()?;
    let mut best: Option<(usize, String)> = None;
    for line in mounts.lines() {
        let mut parts = line.split_whitespace();
        let device = parts.next()?;
        let mount_point = parts.next()?;
        let mount_point = unescape_mount_field(mount_point);
        if !path_matches_mount(path, &mount_point) {
            continue;
        }
        let len = mount_point.len();
        match best.as_ref().map(|(best_len, _)| len.cmp(best_len)) {
            Some(Ordering::Greater) | None => {
                best = Some((len, unescape_mount_field(device)));
            }
            _ => {}
        }
    }
    best.map(|(_, device)| device)
}

fn unescape_mount_field(field: &str) -> String {
    field
        .replace("\\040", " ")
        .replace("\\011", "\t")
        .replace("\\012", "\n")
        .replace("\\134", "\\")
}

fn path_matches_mount(path: &str, mount_point: &str) -> bool {
    if mount_point == "/" {
        return path.starts_with('/');
    }
    if !path.starts_with(mount_point) {
        return false;
    }
    matches!(path.as_bytes().get(mount_point.len()), Some(b'/') | None)
}

fn format_bytes(bytes: u64) -> String {
    const UNITS: [&str; 5] = ["B", "KB", "MB", "GB", "TB"];
    let mut value = bytes as f64;
    let mut unit = 0usize;
    while value >= 1024.0 && unit < UNITS.len() - 1 {
        value /= 1024.0;
        unit += 1;
    }
    if unit == 0 {
        format!("{:.0} {}", value, UNITS[unit])
    } else if value < 10.0 {
        format!("{:.1} {}", value, UNITS[unit])
    } else {
        format!("{:.0} {}", value, UNITS[unit])
    }
}

fn attention_for(
    utilization: f32,
    warning_threshold: f32,
    danger_threshold: f32,
) -> GaugeValueAttention {
    if utilization > danger_threshold {
        GaugeValueAttention::Danger
    } else if utilization > warning_threshold {
        GaugeValueAttention::Warning
    } else {
        GaugeValueAttention::Nominal
    }
}

fn disk_value(
    utilization: Option<f32>,
    warning_threshold: f32,
    danger_threshold: f32,
) -> GaugeDisplay {
    match utilization {
        Some(util) => GaugeDisplay::Value {
            value: GaugeValue::Svg(icon_quantity(util)),
            attention: attention_for(util, warning_threshold, danger_threshold),
        },
        None => GaugeDisplay::Error,
    }
}

/// Gauge that reports filesystem usage for a configured path.
struct DiskGauge {
    /// Filesystem path whose mount usage is sampled.
    path: String,
    /// Utilization threshold where the gauge switches to warning attention.
    warning_threshold: f32,
    /// Utilization threshold where the gauge switches to danger attention.
    danger_threshold: f32,
    /// Poll cadence for filesystem usage sampling.
    poll_interval: Duration,
    /// Scheduler deadline for the next run.
    next_deadline: Instant,
}

impl Gauge for DiskGauge {
    fn id(&self) -> &'static str {
        "disk"
    }

    fn next_deadline(&self) -> Instant {
        self.next_deadline
    }

    fn run_once(&mut self, now: Instant) -> Option<GaugeModel> {
        let usage = disk_usage(&self.path);
        let utilization = usage.and_then(|usage| {
            if usage.total == 0 {
                None
            } else {
                Some((usage.used as f32 / usage.total as f32).clamp(0.0, 1.0))
            }
        });
        let display = disk_value(utilization, self.warning_threshold, self.danger_threshold);

        let device =
            mount_device_for_path(&self.path).unwrap_or_else(|| "Unknown device".to_string());
        let (total_line, used_line) = usage
            .map(|usage| {
                (
                    format!("Total: {}", format_bytes(usage.total)),
                    format!("Used: {}", format_bytes(usage.used)),
                )
            })
            .unwrap_or_else(|| ("Total: N/A".to_string(), "Used: N/A".to_string()));

        self.next_deadline = now + self.poll_interval;

        Some(GaugeModel {
            id: "disk",
            icon: svg_asset("disk.svg"),
            display,
            interactions: GaugeInteractionModel {
                left_click: GaugePointerInteraction {
                    info: Some(InfoDialog {
                        title: "Disk".to_string(),
                        lines: vec![device, total_line, used_line],
                    }),
                    ..GaugePointerInteraction::default()
                },
                ..GaugeInteractionModel::default()
            },
        })
    }
}

pub fn create_gauge(now: Instant) -> Box<dyn Gauge> {
    let path = settings::settings().get_or("grelier.gauge.disk.path", DEFAULT_ROOT_PATH);
    let poll_interval_secs = settings::settings().get_parsed_or(
        "grelier.gauge.disk.poll_interval_secs",
        DEFAULT_POLL_INTERVAL_SECS,
    );
    let warning_threshold = settings::settings().get_parsed_or(
        "grelier.gauge.disk.warning_threshold",
        DEFAULT_WARNING_THRESHOLD,
    );
    let danger_threshold = settings::settings().get_parsed_or(
        "grelier.gauge.disk.danger_threshold",
        DEFAULT_DANGER_THRESHOLD,
    );

    Box::new(DiskGauge {
        path,
        warning_threshold,
        danger_threshold,
        poll_interval: Duration::from_secs(poll_interval_secs),
        next_deadline: now,
    })
}

pub fn settings() -> &'static [SettingSpec] {
    const SETTINGS: &[SettingSpec] = &[
        SettingSpec {
            key: "grelier.gauge.disk.path",
            default: DEFAULT_ROOT_PATH,
        },
        SettingSpec {
            key: "grelier.gauge.disk.poll_interval_secs",
            default: "60",
        },
        SettingSpec {
            key: "grelier.gauge.disk.warning_threshold",
            default: "0.85",
        },
        SettingSpec {
            key: "grelier.gauge.disk.danger_threshold",
            default: "0.95",
        },
    ];
    SETTINGS
}

inventory::submit! {
    GaugeSpec {
        id: "disk",
        description: "Disk usage gauge showing percent utilization for a configured path.",
        default_enabled: false,
        settings,
        create: create_gauge,
        validate: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_none_on_missing_utilization() {
        assert!(matches!(
            disk_value(None, DEFAULT_WARNING_THRESHOLD, DEFAULT_DANGER_THRESHOLD),
            GaugeDisplay::Error
        ));
    }
}
