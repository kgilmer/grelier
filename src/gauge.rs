use iced::futures::channel::mpsc;
use iced::widget::svg;
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
#[derive(Default)]
pub enum GaugeValueAttention {
    #[default]
    Nominal,
    Warning,
    Danger,
}


#[derive(Debug, Clone)]
pub enum GaugeValue {
    Text(String),
    Svg(svg::Handle),
}

#[derive(Debug, Clone)]
pub struct GaugeModel {
    pub id: &'static str,
    pub icon: Option<svg::Handle>,
    pub value: GaugeValue,
    pub attention: GaugeValueAttention,
}

/// Create a gauge stream that polls on a (potentially dynamic) interval.
pub fn fixed_interval(
    id: &'static str,
    icon: Option<svg::Handle>,
    interval: impl Fn() -> Duration + Send + 'static,
    tick: impl Fn() -> Option<(GaugeValue, GaugeValueAttention)> + Send + 'static,
) -> impl iced::futures::Stream<Item = GaugeModel> {
    let (mut sender, receiver) = mpsc::channel(1);

    thread::spawn(move || {
        loop {
            if let Some((value, attention)) = tick() {
                let _ = sender.try_send(GaugeModel {
                    id,
                    icon: icon.clone(),
                    value,
                    attention,
                });
            }

            thread::sleep(interval());
        }
    });

    receiver
}

/// Create a gauge stream driven by external events.
pub fn event_stream(
    _id: &'static str,
    _icon: Option<svg::Handle>,
    start: impl Fn(mpsc::Sender<GaugeModel>) + Send + 'static,
) -> impl iced::futures::Stream<Item = GaugeModel> {
    let (sender, receiver) = mpsc::channel(16);

    thread::spawn(move || start(sender));

    receiver
}

pub enum GaugeKind {
    Interval {
        id: &'static str,
        icon: Option<svg::Handle>,
        interval: Box<dyn Fn() -> Duration + Send + 'static>,
        tick: Box<dyn Fn() -> Option<(GaugeValue, GaugeValueAttention)> + Send + 'static>,
    },
    Event {
        id: &'static str,
        icon: Option<svg::Handle>,
        start: Box<dyn Fn(mpsc::Sender<GaugeModel>) + Send + 'static>,
    },
}

impl GaugeKind {
    pub fn spawn(self) -> Box<dyn iced::futures::Stream<Item = GaugeModel> + Send + Unpin> {
        match self {
            GaugeKind::Interval {
                id,
                icon,
                interval,
                tick,
            } => Box::new(fixed_interval(id, icon, interval, tick)),
            GaugeKind::Event { id, icon, start } => {
                Box::new(event_stream(id, icon, start))
            }
        }
    }
}
