// Download rate gauge backed by the shared network sampler.
// Consumes Settings: grelier.gauge.net.* (via net_common).
use crate::icon::{icon_quantity, svg_asset};
use crate::info_dialog::InfoDialog;
use crate::panels::gauges::gauge::{
    GaugeValue, GaugeValueAttention, NO_SETTINGS, SettingSpec, fixed_interval,
};
use crate::panels::gauges::gauge_registry::{GaugeSpec, GaugeStream};
use crate::panels::gauges::net_common::{
    NetIntervalState, SlidingWindow, format_rate_per_sec, net_interval_config_from_settings,
    shared_net_sampler,
};
use iced::futures::StreamExt;
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

fn net_down_stream() -> impl iced::futures::Stream<Item = crate::panels::gauges::gauge::GaugeModel>
{
    let sampler = shared_net_sampler();
    let interval_state = Arc::new(Mutex::new(NetIntervalState::new(
        net_interval_config_from_settings(),
    )));
    let rate_window = Arc::new(Mutex::new(SlidingWindow::new(RATE_WINDOW_SAMPLES)));
    let info_state = Arc::new(Mutex::new(InfoDialog {
        title: "Net Down".to_string(),
        lines: vec!["No active interface".to_string(), "0 KB/s".to_string()],
    }));

    fixed_interval(
        "net_down",
        Some(svg_asset("download.svg")),
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
            let info_state = Arc::clone(&info_state);
            move || {
                let (rate, iface) = sampler
                    .lock()
                    .ok()
                    .map(|mut s| {
                        let rates = s.rates();
                        let iface = s.cached_interface();
                        (rates, iface)
                    })
                    .unwrap_or((None, None));
                let rate = rate.map(|r| r.download_bytes_per_sec);

                let (value, attention, bytes_per_sec) = match window.lock() {
                    Ok(mut window) => map_rate(rate, &mut window),
                    Err(_) => map_rate(rate, &mut SlidingWindow::new(RATE_WINDOW_SAMPLES)),
                };

                if let Ok(mut info) = info_state.lock() {
                    let iface_line = iface.unwrap_or_else(|| "No active interface".to_string());
                    info.lines = vec![iface_line, format_rate_per_sec(bytes_per_sec)];
                }

                if let Ok(mut state) = state.lock() {
                    state.update(bytes_per_sec);
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
    NO_SETTINGS
}

fn stream() -> GaugeStream {
    Box::new(net_down_stream())
}

inventory::submit! {
    GaugeSpec {
        id: "net_down",
        description: "Network download rate gauge displaying a relative icon.",
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
