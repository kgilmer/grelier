use iced::futures::channel::mpsc;
use std::borrow::Cow;
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone)]
pub struct GaugeModel {
    pub title: Cow<'static, str>,
    pub value: String,
}

/// Create a gauge stream that polls on a (potentially dynamic) interval.
pub fn fixed_interval(
    title: &'static str,
    interval: impl Fn() -> Duration + Send + 'static,
    tick: impl Fn() -> Option<String> + Send + 'static,
) -> impl iced::futures::Stream<Item = GaugeModel> {
    let (mut sender, receiver) = mpsc::channel(1);

    thread::spawn(move || loop {
        if let Some(value) = tick() {
            let _ = sender.try_send(GaugeModel {
                title: title.into(),
                value,
            });
        }

        thread::sleep(interval());
    });

    receiver
}

/// Create a gauge stream driven by external events.
pub fn event_stream(
    title: &'static str,
    start: impl Fn(mpsc::Sender<GaugeModel>) + Send + 'static,
) -> impl iced::futures::Stream<Item = GaugeModel> {
    let (sender, receiver) = mpsc::channel(16);

    thread::spawn(move || {
        start(sender);
    });

    receiver
}

pub enum GaugeKind {
    Interval {
        title: &'static str,
        interval: Box<dyn Fn() -> Duration + Send + 'static>,
        tick: Box<dyn Fn() -> Option<String> + Send + 'static>,
    },
    Event {
        title: &'static str,
        start: Box<dyn Fn(mpsc::Sender<GaugeModel>) + Send + 'static>,
    },
}

impl GaugeKind {
    pub fn spawn(self) -> Box<dyn iced::futures::Stream<Item = GaugeModel> + Send + Unpin> {
        match self {
            GaugeKind::Interval { title, interval, tick } => {
                Box::new(fixed_interval(title, move || interval(), move || tick()))
            }
            GaugeKind::Event { title, start } => {
                Box::new(event_stream(title, move |tx| start(tx)))
            }
        }
    }
}
