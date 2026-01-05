use crate::app::Message;
use crate::gauge::{fixed_interval, GaugeValue, GaugeValueAttention};
use crate::gauges::net_common::{format_rate, NetRateTracker, RateDirection};
use crate::icon::svg_asset;
use iced::futures::StreamExt;
use iced::Subscription;
use std::sync::{Arc, Mutex};
use std::time::Duration;

fn net_upload_stream() -> impl iced::futures::Stream<Item = crate::gauge::GaugeModel> {
    let tracker = Arc::new(Mutex::new(NetRateTracker::new()));

    fixed_interval(
        "net_upload",
        Some(svg_asset("upload.svg")),
        || Duration::from_secs(1),
        {
            let tracker = Arc::clone(&tracker);
            move || {
                let rate = tracker
                    .lock()
                    .ok()
                    .and_then(|mut t| t.rate(RateDirection::Upload));

                match rate {
                    Some(bytes_per_sec) => Some((
                        GaugeValue::Text(format_rate(bytes_per_sec)),
                        GaugeValueAttention::Nominal,
                    )),
                    None => Some((
                        GaugeValue::Text("--".to_string()),
                        GaugeValueAttention::Danger,
                    )),
                }
            }
        },
        None,
    )
}

pub fn net_upload_subscription() -> Subscription<Message> {
    Subscription::run(|| net_upload_stream().map(Message::Gauge))
}
