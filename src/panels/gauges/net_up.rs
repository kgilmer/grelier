// Upload rate gauge backed by the shared network sampler.
// Consumes Settings: grelier.gauge.net.* (via net_common).
use crate::dialog::info::InfoDialog;
use crate::icon::{icon_quantity, svg_asset};
use crate::panels::gauges::gauge::Gauge;
use crate::panels::gauges::gauge::{GaugeDisplay, GaugeModel, GaugeValue, GaugeValueAttention};
use crate::panels::gauges::gauge_registry::GaugeSpec;
use crate::panels::gauges::net_common::{
    NetIntervalState, SlidingWindow, format_rate_per_sec, net_interval_config_from_settings,
    shared_net_sampler,
};
use crate::settings::SettingSpec;
use std::sync::{Arc, Mutex};
use std::time::Instant;

const RATE_WINDOW_SAMPLES: usize = 60;

fn map_rate(rate: Option<f64>, window: &mut SlidingWindow) -> (GaugeDisplay, f64) {
    match rate {
        Some(bytes_per_sec) => {
            let ratio = window.push(bytes_per_sec);
            (
                GaugeDisplay::Value {
                    value: GaugeValue::Svg(icon_quantity(ratio)),
                    attention: GaugeValueAttention::Nominal,
                },
                bytes_per_sec,
            )
        }
        None => (
            GaugeDisplay::Value {
                value: GaugeValue::Svg(icon_quantity(0.0)),
                attention: GaugeValueAttention::Warning,
            },
            0.0,
        ),
    }
}

/// Gauge that displays recent upload throughput.
struct NetUpGauge {
    /// Shared network sampler that provides interface rates.
    sampler: Arc<Mutex<crate::panels::gauges::net_common::NetSampler>>,
    /// Adaptive interval controller based on recent throughput.
    interval_state: NetIntervalState,
    /// Sliding window used to smooth and classify sampled rates.
    rate_window: SlidingWindow,
    /// Scheduler deadline for the next run.
    next_deadline: Instant,
}

impl Gauge for NetUpGauge {
    fn id(&self) -> &'static str {
        "net_up"
    }

    fn next_deadline(&self) -> Instant {
        self.next_deadline
    }

    fn run_once(&mut self, now: Instant) -> Option<GaugeModel> {
        let (rate, iface) = self
            .sampler
            .lock()
            .ok()
            .map(|mut sampler| {
                let rates = sampler.rates();
                let iface = sampler.cached_interface();
                (rates, iface)
            })
            .unwrap_or((None, None));
        let rate = rate.map(|rates| rates.upload_bytes_per_sec);
        let (display, bytes_per_sec) = map_rate(rate, &mut self.rate_window);

        self.interval_state.update(bytes_per_sec);
        self.next_deadline = now + self.interval_state.interval();

        Some(GaugeModel {
            id: "net_up",
            icon: svg_asset("upload.svg"),
            display,
            on_click: None,
            menu: None,
            action_dialog: None,
            info: Some(InfoDialog {
                title: "Net Up".to_string(),
                lines: vec![
                    iface.unwrap_or_else(|| "No active interface".to_string()),
                    format_rate_per_sec(bytes_per_sec),
                ],
            }),
        })
    }
}

pub fn create_gauge(now: Instant) -> Box<dyn Gauge> {
    Box::new(NetUpGauge {
        sampler: shared_net_sampler(),
        interval_state: NetIntervalState::new(net_interval_config_from_settings()),
        rate_window: SlidingWindow::new(RATE_WINDOW_SAMPLES),
        next_deadline: now,
    })
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

inventory::submit! {
    GaugeSpec {
        id: "net_up",
        description: "Network upload rate gauge displaying a relative icon.",
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
    fn returns_none_on_missing_rate() {
        let mut window = SlidingWindow::new(RATE_WINDOW_SAMPLES);
        let (display, bytes) = map_rate(None, &mut window);
        let GaugeDisplay::Value {
            value: GaugeValue::Svg(handle),
            attention,
        } = display
        else {
            panic!("expected svg value for missing rate");
        };
        assert_eq!(handle, icon_quantity(0.0));
        assert_eq!(attention, GaugeValueAttention::Warning);
        assert_eq!(bytes, 0.0);
    }
}
