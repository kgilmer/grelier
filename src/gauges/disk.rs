// Disk usage gauge for a configurable filesystem path.
// Consumes Settings: grelier.gauge.disk.*.
use crate::gauge::{GaugeValue, GaugeValueAttention, SettingSpec, fixed_interval};
use crate::gauge_registry::{GaugeSpec, GaugeStream};
use crate::icon::{icon_quantity, svg_asset};
use crate::info_dialog::InfoDialog;
use crate::settings;
use iced::futures::StreamExt;
use std::cmp::Ordering;
use std::ffi::CString;
use std::fs;
use std::mem::MaybeUninit;
use std::os::raw::{c_char, c_int, c_ulong};
use std::sync::{Arc, Mutex};
use std::time::Duration;

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
) -> (Option<GaugeValue>, GaugeValueAttention) {
    match utilization {
        Some(util) => (
            Some(GaugeValue::Svg(icon_quantity(util))),
            attention_for(util, warning_threshold, danger_threshold),
        ),
        None => (None, GaugeValueAttention::Danger),
    }
}

fn disk_stream() -> impl iced::futures::Stream<Item = crate::gauge::GaugeModel> {
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
    let info_state = Arc::new(Mutex::new(InfoDialog {
        title: "Disk".to_string(),
        lines: vec![
            "Unknown device".to_string(),
            "Total: N/A".to_string(),
            "Used: N/A".to_string(),
        ],
    }));

    fixed_interval(
        "disk",
        Some(svg_asset("disk.svg")),
        move || Duration::from_secs(poll_interval_secs),
        {
            let path = path.clone();
            let info_state = Arc::clone(&info_state);
            move || {
                let usage = disk_usage(&path);
                let utilization = usage.and_then(|usage| {
                    if usage.total == 0 {
                        None
                    } else {
                        Some((usage.used as f32 / usage.total as f32).clamp(0.0, 1.0))
                    }
                });
                let (value, attention) =
                    disk_value(utilization, warning_threshold, danger_threshold);
                if let Ok(mut info) = info_state.lock() {
                    let device = mount_device_for_path(&path)
                        .unwrap_or_else(|| "Unknown device".to_string());
                    let (total_line, used_line) = usage
                        .map(|usage| {
                            (
                                format!("Total: {}", format_bytes(usage.total)),
                                format!("Used: {}", format_bytes(usage.used)),
                            )
                        })
                        .unwrap_or_else(|| ("Total: N/A".to_string(), "Used: N/A".to_string()));
                    info.lines = vec![device, total_line, used_line];
                }
                Some((value, attention))
            }
        },
        None,
    )
    .map({
        let info_state = Arc::clone(&info_state);
        move |mut model| {
            if let Ok(info) = info_state.lock() {
                model.info = Some(info.clone());
            }
            model
        }
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

fn stream() -> GaugeStream {
    Box::new(disk_stream())
}

inventory::submit! {
    GaugeSpec {
        id: "disk",
        label: "Disk",
        description: "Disk usage gauge showing percent utilization for a configured path.",
        default_enabled: false,
        settings,
        stream,
        validate: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attention_thresholds_match_spec() {
        assert_eq!(
            attention_for(0.80, DEFAULT_WARNING_THRESHOLD, DEFAULT_DANGER_THRESHOLD),
            GaugeValueAttention::Nominal
        );
        assert_eq!(
            attention_for(0.86, DEFAULT_WARNING_THRESHOLD, DEFAULT_DANGER_THRESHOLD),
            GaugeValueAttention::Warning
        );
        assert_eq!(
            attention_for(0.95, DEFAULT_WARNING_THRESHOLD, DEFAULT_DANGER_THRESHOLD),
            GaugeValueAttention::Warning
        );
        assert_eq!(
            attention_for(0.96, DEFAULT_WARNING_THRESHOLD, DEFAULT_DANGER_THRESHOLD),
            GaugeValueAttention::Danger
        );
    }

    #[test]
    fn returns_none_on_missing_utilization() {
        let (value, attention) =
            disk_value(None, DEFAULT_WARNING_THRESHOLD, DEFAULT_DANGER_THRESHOLD);
        assert!(value.is_none());
        assert_eq!(attention, GaugeValueAttention::Danger);
    }
}
