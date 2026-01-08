use iced::futures::channel::mpsc;
use iced::mouse;
use iced::widget::svg;
use std::fmt;
use std::sync::{Arc, mpsc as sync_mpsc};
use std::thread;
use std::time::Duration;

#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
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
pub struct GaugeMenuItem {
    pub id: String,
    pub label: String,
    pub selected: bool,
}

pub type MenuSelectAction = Arc<dyn Fn(String) + Send + Sync>;

#[derive(Clone)]
pub struct GaugeMenu {
    pub title: String,
    pub items: Vec<GaugeMenuItem>,
    pub on_select: Option<MenuSelectAction>,
}

#[derive(Clone)]
pub struct GaugeModel {
    pub id: &'static str,
    pub icon: Option<svg::Handle>,
    pub value: Option<GaugeValue>,
    pub attention: GaugeValueAttention,
    pub on_click: Option<GaugeClickAction>,
    pub menu: Option<GaugeMenu>,
}

impl fmt::Debug for GaugeModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GaugeModel")
            .field("id", &self.id)
            .field("icon", &self.icon)
            .field("value", &self.value)
            .field("attention", &self.attention)
            .field(
                "menu",
                &self
                    .menu
                    .as_ref()
                    .map(|menu| menu.title.clone())
                    .unwrap_or_else(|| "<none>".to_string()),
            )
            .finish_non_exhaustive()
    }
}

#[derive(Debug, Clone, Copy)]
pub enum GaugeClickTarget {
    Icon,
    Value,
}

#[derive(Debug, Clone, Copy)]
pub enum GaugeInput {
    Button(mouse::Button),
    ScrollUp,
    ScrollDown,
}

#[derive(Debug, Clone, Copy)]
pub struct GaugeClick {
    pub input: GaugeInput,
    pub target: GaugeClickTarget,
}

pub type GaugeClickAction = Arc<dyn Fn(GaugeClick) + Send + Sync>;

/// Create a gauge stream that polls on a (potentially dynamic) interval.
pub fn fixed_interval(
    id: &'static str,
    icon: Option<svg::Handle>,
    interval: impl Fn() -> Duration + Send + 'static,
    tick: impl Fn() -> Option<(Option<GaugeValue>, GaugeValueAttention)> + Send + 'static,
    on_click: Option<GaugeClickAction>,
) -> impl iced::futures::Stream<Item = GaugeModel> {
    let (mut sender, receiver) = mpsc::channel(1);
    let (trigger_tx, trigger_rx) = sync_mpsc::channel::<()>();

    let on_click = on_click.map(|callback| {
        let trigger_tx = trigger_tx.clone();
        Arc::new(move |click: GaugeClick| {
            callback(click);
            let _ = trigger_tx.send(());
        }) as GaugeClickAction
    });

    thread::spawn(move || {
        // Keep a sender alive even if there is no click handler to prevent the channel
        // from disconnecting and stopping the loop after the first tick.
        let _trigger_tx = trigger_tx;

        loop {
            if let Some((value, attention)) = tick() {
                let attention = if value.is_some() {
                    attention
                } else {
                    GaugeValueAttention::Danger
                };
                let _ = sender.try_send(GaugeModel {
                    id,
                    icon: icon.clone(),
                    value,
                    attention,
                    on_click: on_click.clone(),
                    menu: None,
                });
            }

            let sleep_duration = interval();
            match trigger_rx.recv_timeout(sleep_duration) {
                Ok(_) | Err(sync_mpsc::RecvTimeoutError::Timeout) => continue,
                Err(sync_mpsc::RecvTimeoutError::Disconnected) => break,
            }
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
        tick: Box<dyn Fn() -> Option<(Option<GaugeValue>, GaugeValueAttention)> + Send + 'static>,
        on_click: Option<GaugeClickAction>,
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
                on_click,
            } => Box::new(fixed_interval(id, icon, interval, tick, on_click)),
            GaugeKind::Event { id, icon, start } => Box::new(event_stream(id, icon, start)),
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use iced::futures::{StreamExt, executor::block_on};
    use std::sync::atomic::{AtomicUsize, Ordering};

    #[test]
    fn fixed_interval_keeps_running_without_click_handler() {
        let tick_count = Arc::new(AtomicUsize::new(0));
        let ticks = Arc::clone(&tick_count);

        let mut stream = fixed_interval(
            "test",
            None,
            || Duration::from_millis(5),
            move || {
                ticks.fetch_add(1, Ordering::SeqCst);
                Some((
                    Some(GaugeValue::Text(String::from("ok"))),
                    GaugeValueAttention::Nominal,
                ))
            },
            None,
        );

        let first = block_on(stream.next());
        let second = block_on(stream.next());

        assert!(first.is_some());
        assert!(second.is_some());
        assert!(
            tick_count.load(Ordering::SeqCst) >= 2,
            "expected multiple ticks, got {}",
            tick_count.load(Ordering::SeqCst)
        );
    }
}
