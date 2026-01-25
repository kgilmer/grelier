// Upload rate gauge backed by the shared network sampler.
// Consumes Settings: grelier.gauge.net.* (via net_common).
use crate::gauge::{GaugeValue, GaugeValueAttention, SettingSpec, fixed_interval};
use crate::gauge_registry::{GaugeSpec, GaugeStream};
use crate::gauges::net_common::{
    NetIntervalState, SlidingWindow, net_interval_config_from_settings, shared_net_sampler,
};
use crate::icon::{icon_quantity, svg_asset};
use std::sync::{Arc, Mutex};
use std::time::Duration;

const RATE_WINDOW_SAMPLES: usize = 60;

fn map_rate(
    rate: Option<f64>,
    window: &mut SlidingWindow,
) -> (Option<GaugeValue>, GaugeValueAttention, f64) {
    match rate {
        Some(bytes_per_sec) => {
            let ratio = window.push(bytes_per_sec);
            (
                Some(GaugeValue::Svg(icon_quantity(ratio))),
                GaugeValueAttention::Nominal,
                bytes_per_sec,
            )
        }
        None => (None, GaugeValueAttention::Danger, 0.0),
    }
}

fn net_up_stream() -> impl iced::futures::Stream<Item = crate::gauge::GaugeModel> {
    let sampler = shared_net_sampler();
    let interval_state = Arc::new(Mutex::new(NetIntervalState::new(
        net_interval_config_from_settings(),
    )));
    let rate_window = Arc::new(Mutex::new(SlidingWindow::new(RATE_WINDOW_SAMPLES)));

    fixed_interval(
        "net_up",
        Some(svg_asset("upload.svg")),
        {
            let state = Arc::clone(&interval_state);
            move || {
                state
                    .lock()
                    .map(|s| s.interval())
                    .unwrap_or(Duration::from_secs(1))
            }
        },
        {
            let sampler = Arc::clone(&sampler);
            let state = Arc::clone(&interval_state);
            let window = Arc::clone(&rate_window);
            move || {
                let rate = sampler
                    .lock()
                    .ok()
                    .and_then(|mut s| s.rates().map(|r| r.upload_bytes_per_sec));

                let (value, attention, bytes_per_sec) = match window.lock() {
                    Ok(mut window) => map_rate(rate, &mut window),
                    Err(_) => map_rate(rate, &mut SlidingWindow::new(RATE_WINDOW_SAMPLES)),
                };

                if let Ok(mut state) = state.lock() {
                    state.update(bytes_per_sec);
                }

                Some((value, attention))
            }
        },
        None,
    )
}

pub fn settings() -> &'static [SettingSpec] {
    const SETTINGS: &[SettingSpec] = &[
        SettingSpec {
            key: "grelier.gauge.net.idle_threshold_bps",
            default: "10240",
        },
        SettingSpec {
            key: "grelier.gauge.net.fast_interval_secs",
            default: "1",
        },
        SettingSpec {
            key: "grelier.gauge.net.slow_interval_secs",
            default: "3",
        },
        SettingSpec {
            key: "grelier.gauge.net.calm_ticks",
            default: "4",
        },
        SettingSpec {
            key: "grelier.gauge.net.iface_cache_ttl_secs",
            default: "10",
        },
        SettingSpec {
            key: "grelier.gauge.net.iface_ttl_secs",
            default: "5",
        },
        SettingSpec {
            key: "grelier.gauge.net.sampler_min_interval_ms",
            default: "900",
        },
        SettingSpec {
            key: "grelier.gauge.net.sys_class_net_path",
            default: "/sys/class/net",
        },
        SettingSpec {
            key: "grelier.gauge.net.proc_net_route_path",
            default: "/proc/net/route",
        },
        SettingSpec {
            key: "grelier.gauge.net.proc_net_dev_path",
            default: "/proc/net/dev",
        },
    ];
    SETTINGS
}

fn stream() -> GaugeStream {
    Box::new(net_up_stream())
}

inventory::submit! {
    GaugeSpec {
        id: "net_up",
        label: "Net Up",
        description: "Network upload rate gauge displaying a relative icon.",
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
    fn returns_none_on_missing_rate() {
        let mut window = SlidingWindow::new(RATE_WINDOW_SAMPLES);
        let (value, attention, bytes) = map_rate(None, &mut window);
        assert!(value.is_none());
        assert_eq!(attention, GaugeValueAttention::Danger);
        assert_eq!(bytes, 0.0);
    }
}
