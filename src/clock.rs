use iced::futures::channel::mpsc;
use chrono::Local;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::gauge::GaugeModel;

/// Stream of the current wall-clock second published once per second.
pub fn seconds_stream() -> impl iced::futures::Stream<Item = GaugeModel> {
    let (mut sender, receiver) = mpsc::channel(1);

    thread::spawn(move || loop {
        let system_now = SystemTime::now()
            .duration_since(UNIX_EPOCH)
            .unwrap_or_default();
        let nanos = system_now.subsec_nanos();

        let now = Local::now();
        let value = now.format("%S").to_string();

        let _ = sender.try_send(GaugeModel {
            title: "clock".into(),
            value,
        });

        let sleep_nanos = 1_000_000_000u32.saturating_sub(nanos);
        thread::sleep(Duration::new(0, sleep_nanos));
    });

    receiver
}
