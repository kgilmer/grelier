use iced::futures::channel::mpsc;
use chrono::Local;
use std::thread;
use std::time::{Duration, SystemTime, UNIX_EPOCH};

use crate::gauge::GaugeModel;

const SECS_PER_DAY: u64 = 86_400;
const DAY_LENGTH: Duration = Duration::from_secs(SECS_PER_DAY);

/// Stream of the current day (day-of-month, zero-padded) published once per day.
pub fn day_stream() -> impl iced::futures::Stream<Item = GaugeModel> {
    let (mut sender, receiver) = mpsc::channel(1);

    thread::spawn(move || loop {
        let now = SystemTime::now();
        if let Ok(elapsed) = now.duration_since(UNIX_EPOCH) {
            let today = Local::now().format("%d").to_string();
            let _ = sender.try_send(GaugeModel {
                title: "date".into(),
                value: today,
            });

            let into_day =
                Duration::new(elapsed.as_secs() % SECS_PER_DAY, elapsed.subsec_nanos());
            let mut sleep_dur = DAY_LENGTH
                .checked_sub(into_day)
                .unwrap_or_else(|| Duration::from_secs(0));

            // If we're exactly on the boundary, sleep a full day.
            if sleep_dur.is_zero() {
                sleep_dur = DAY_LENGTH;
            }

            thread::sleep(sleep_dur);
        } else {
            // If system time went backwards, retry after a short pause.
            thread::sleep(Duration::from_secs(1));
        }
    });

    receiver
}
