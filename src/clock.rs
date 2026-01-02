use chrono::Local;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::gauge::fixed_interval;

/// Stream of the current wall-clock second published once per second.
pub fn seconds_stream() -> impl iced::futures::Stream<Item = crate::gauge::GaugeModel> {
    fixed_interval(
        "clock",
        || {
            let nanos = SystemTime::now()
                .duration_since(UNIX_EPOCH)
                .map(|d| d.subsec_nanos())
                .unwrap_or(0);
            Duration::new(0, 1_000_000_000u32.saturating_sub(nanos))
        },
        || Some(Local::now().format("%S").to_string()),
    )
}
