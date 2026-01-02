use chrono::Local;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::gauge::{fixed_interval, GaugeValue, GaugeValueAttention};
use crate::svg_asset;

/// Stream of the current wall-clock hour/minute, formatted on two lines.
pub fn seconds_stream() -> impl iced::futures::Stream<Item = crate::gauge::GaugeModel> {
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
    )
}
