use crate::app::Message;
use crate::gauge::{fixed_interval, GaugeValue, GaugeValueAttention};
use crate::icon::{icon_quantity, svg_asset, QuantityStyle};
use iced::futures::StreamExt;
use iced::Subscription;
use std::ffi::CString;
use std::mem::MaybeUninit;
use std::os::raw::{c_char, c_int, c_ulong};
use std::time::Duration;

const ROOT_PATH: &str = "/";

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
    let fragment_size = stats.f_frsize as u64;
    if fragment_size == 0 {
        return None;
    }

    let total_blocks = stats.f_blocks as u64;
    if total_blocks == 0 {
        return None;
    }

    let used_blocks = total_blocks.saturating_sub(stats.f_bfree as u64);

    let total = total_blocks.saturating_mul(fragment_size);
    let used = used_blocks.saturating_mul(fragment_size);

    Some(DiskUsage { used, total })
}

fn root_utilization() -> Option<f32> {
    let usage = disk_usage(ROOT_PATH)?;
    if usage.total == 0 {
        return None;
    }

    Some((usage.used as f32 / usage.total as f32).clamp(0.0, 1.0))
}

fn attention_for(utilization: f32) -> GaugeValueAttention {
    if utilization > 0.95 {
        GaugeValueAttention::Danger
    } else if utilization > 0.85 {
        GaugeValueAttention::Warning
    } else {
        GaugeValueAttention::Nominal
    }
}

fn disk_stream() -> impl iced::futures::Stream<Item = crate::gauge::GaugeModel> {
    fixed_interval(
        "disk",
        Some(svg_asset("disk.svg")),
        || Duration::from_secs(60),
        || {
            let utilization = root_utilization()?;
            let attention = attention_for(utilization);

            Some((
                GaugeValue::Svg(icon_quantity(QuantityStyle::Grid, utilization)),
                attention,
            ))
        },
        None,
    )
}

pub fn disk_subscription() -> Subscription<Message> {
    Subscription::run(|| disk_stream().map(Message::Gauge))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn attention_thresholds_match_spec() {
        assert_eq!(attention_for(0.80), GaugeValueAttention::Nominal);
        assert_eq!(attention_for(0.86), GaugeValueAttention::Warning);
        assert_eq!(attention_for(0.95), GaugeValueAttention::Warning);
        assert_eq!(attention_for(0.96), GaugeValueAttention::Danger);
    }
}
