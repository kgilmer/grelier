use crate::app::Message;
use crate::gauge::{
    GaugeClick, GaugeClickAction, GaugeInput, GaugeValue, GaugeValueAttention, SettingSpec,
    fixed_interval,
};
use crate::icon::{QuantityStyle, icon_quantity, svg_asset};
use crate::settings;
use iced::{Subscription, mouse};
use iced::futures::StreamExt;
use std::ffi::CString;
use std::mem::MaybeUninit;
use std::os::raw::{c_char, c_int, c_ulong};
use std::sync::{Arc, Mutex};
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

fn disk_value(
    utilization: Option<f32>,
    style: QuantityStyle,
) -> (Option<GaugeValue>, GaugeValueAttention) {
    match utilization {
        Some(util) => (
            Some(GaugeValue::Svg(icon_quantity(style, util))),
            attention_for(util),
        ),
        None => (None, GaugeValueAttention::Danger),
    }
}

#[derive(Default)]
struct DiskState {
    quantity_style: QuantityStyle,
}

fn disk_stream() -> impl iced::futures::Stream<Item = crate::gauge::GaugeModel> {
    let style_value = settings::settings().get_or("grelier.disk.quantitystyle", "grid");
    let style = QuantityStyle::parse_setting("grelier.disk.quantitystyle", &style_value);
    let state = Arc::new(Mutex::new(DiskState {
        quantity_style: style,
    }));
    let on_click: GaugeClickAction = {
        let state = Arc::clone(&state);
        Arc::new(move |click: GaugeClick| {
            if matches!(click.input, GaugeInput::Button(mouse::Button::Left)) {
                if let Ok(mut state) = state.lock() {
                    state.quantity_style = state.quantity_style.toggle();
                    settings::settings().update(
                        "grelier.disk.quantitystyle",
                        state.quantity_style.as_setting_value(),
                    );
                }
            }
        })
    };

    fixed_interval(
        "disk",
        Some(svg_asset("disk.svg")),
        || Duration::from_secs(60),
        {
            let state = Arc::clone(&state);
            move || {
                let utilization = root_utilization();
                let style = state
                    .lock()
                    .map(|state| state.quantity_style)
                    .unwrap_or(QuantityStyle::Grid);
                let (value, attention) = disk_value(utilization, style);
                Some((value, attention))
            }
        },
        Some(on_click),
    )
}

pub fn disk_subscription() -> Subscription<Message> {
    Subscription::run(|| disk_stream().map(Message::Gauge))
}

pub fn settings() -> &'static [SettingSpec] {
    const SETTINGS: &[SettingSpec] = &[SettingSpec {
        key: "grelier.disk.quantitystyle",
        default: "grid",
    }];
    SETTINGS
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

    #[test]
    fn returns_none_on_missing_utilization() {
        let (value, attention) = disk_value(None, QuantityStyle::Grid);
        assert!(value.is_none());
        assert_eq!(attention, GaugeValueAttention::Danger);
    }
}
