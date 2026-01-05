use chrono::Local;
use iced::Subscription;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::app::Message;
use crate::gauge::{GaugeValue, GaugeValueAttention, fixed_interval};
use crate::icon::svg_asset;
use iced::futures::StreamExt;

/// Stream of the current wall-clock hour/minute, formatted on two lines.
fn seconds_stream() -> impl iced::futures::Stream<Item = crate::gauge::GaugeModel> {
    fixed_interval(
        "clock",
        Some(svg_asset("clock.svg")),
        || {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0);
            // sleep until the next minute boundary
            Duration::from_secs(60).saturating_sub(Duration::new(0, nanos))
        },
        || {
            let now = Local::now();
            Some((
                GaugeValue::Text(format!("{}\n{}", now.format("%H"), now.format("%M"))),
                GaugeValueAttention::Nominal,
            ))
        },
        None,
    )
}

pub fn clock_subscription() -> Subscription<Message> {
    Subscription::run(|| seconds_stream().map(Message::Gauge))
}
