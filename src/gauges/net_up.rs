use crate::app::Message;
use crate::gauge::{GaugeValue, GaugeValueAttention, SettingSpec, fixed_interval};
use crate::gauges::net_common::{
    NetIntervalState, format_rate, net_interval_config_from_settings, shared_net_sampler,
};
use crate::icon::svg_asset;
use iced::Subscription;
use iced::futures::StreamExt;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn map_rate(rate: Option<f64>) -> (Option<GaugeValue>, GaugeValueAttention, f64) {
    match rate {
        Some(bytes_per_sec) => (
            Some(GaugeValue::Text(format_rate(bytes_per_sec))),
            GaugeValueAttention::Nominal,
            bytes_per_sec,
        ),
        None => (None, GaugeValueAttention::Danger, 0.0),
    }
}

fn net_up_stream() -> impl iced::futures::Stream<Item = crate::gauge::GaugeModel> {
    let sampler = shared_net_sampler();
    let interval_state = Arc::new(Mutex::new(NetIntervalState::new(
        net_interval_config_from_settings(),
    )));

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
            move || {
                let rate = sampler
                    .lock()
                    .ok()
                    .and_then(|mut s| s.rates().map(|r| r.upload_bytes_per_sec));

                let (value, attention, bytes_per_sec) = map_rate(rate);

                if let Ok(mut state) = state.lock() {
                    state.update(bytes_per_sec);
                }

                Some((value, attention))
            }
        },
        None,
    )
}

pub fn net_up_subscription() -> Subscription<Message> {
    Subscription::run(|| net_up_stream().map(Message::Gauge))
}

pub fn settings() -> &'static [SettingSpec] {
    const SETTINGS: &[SettingSpec] = &[
        SettingSpec {
            key: "grelier.net.idle_threshold_bps",
            default: "10240",
        },
        SettingSpec {
            key: "grelier.net.fast_interval_secs",
            default: "1",
        },
        SettingSpec {
            key: "grelier.net.slow_interval_secs",
            default: "3",
        },
        SettingSpec {
            key: "grelier.net.calm_ticks",
            default: "4",
        },
        SettingSpec {
            key: "grelier.net.iface_cache_ttl_secs",
            default: "10",
        },
        SettingSpec {
            key: "grelier.net.iface_ttl_secs",
            default: "5",
        },
        SettingSpec {
            key: "grelier.net.sampler_min_interval_ms",
            default: "900",
        },
        SettingSpec {
            key: "grelier.net.sys_class_net_path",
            default: "/sys/class/net",
        },
        SettingSpec {
            key: "grelier.net.proc_net_route_path",
            default: "/proc/net/route",
        },
        SettingSpec {
            key: "grelier.net.proc_net_dev_path",
            default: "/proc/net/dev",
        },
    ];
    SETTINGS
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn returns_none_on_missing_rate() {
        let (value, attention, bytes) = map_rate(None);
        assert!(value.is_none());
        assert_eq!(attention, GaugeValueAttention::Danger);
        assert_eq!(bytes, 0.0);
    }
}
