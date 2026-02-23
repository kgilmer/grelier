// PulseAudio output volume gauge with mute/adjust actions and device menu.
// Consumes Settings: grelier.gauge.audio_out.step_percent.
use crate::dialog::info::InfoDialog;
use crate::icon::{icon_quantity, svg_asset};
use crate::panels::gauges::gauge::{Gauge, GaugeEventSource, GaugeReadyNotify, GaugeRegistrar};
use crate::panels::gauges::gauge::{
    GaugeClick, GaugeClickAction, GaugeDisplay, GaugeMenu, GaugeMenuItem, GaugeValue,
    GaugeValueAttention, MenuSelectAction,
};
use crate::panels::gauges::gauge_registry::GaugeSpec;
use crate::settings;
use crate::settings::SettingSpec;
use libpulse_binding as pulse;
use pulse::callbacks::ListResult;
use pulse::context::subscribe::{Facility, InterestMaskSet};
use pulse::context::{Context, FlagSet, State as ContextState};
use pulse::def;
use pulse::mainloop::standard::{IterateResult, Mainloop};
use pulse::volume::{ChannelVolumes, Volume};
use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[cfg(test)]
const IDLE_WAIT: Duration = Duration::from_millis(25);
#[cfg(not(test))]
const IDLE_WAIT: Duration = Duration::from_millis(250);
const DEFAULT_STEP_PERCENT: i8 = 5;
const IDLE_RUN_INTERVAL_SECS: u64 = 300;
const MENU_REFRESH_INTERVAL_SECS: u64 = 15;

fn format_level(percent: Option<u8>) -> GaugeDisplay {
    match percent {
        Some(value) => {
            let ratio = if value == 0 {
                0.0
            } else {
                value.min(99) as f32 / 99.0
            };
            GaugeDisplay::Value {
                value: GaugeValue::Svg(icon_quantity(ratio)),
                attention: GaugeValueAttention::Nominal,
            }
        }
        None => GaugeDisplay::Error,
    }
}

#[derive(Clone, Copy)]
struct SinkStatus {
    percent: u8,
    muted: bool,
    channels: u8,
}

#[derive(Debug, Clone, PartialEq, Eq)]
struct AudioOutSignature {
    percent: Option<u8>,
    muted: Option<bool>,
    connected: bool,
    device_label: Option<String>,
    menu: Vec<(String, bool)>,
}

fn signature_for_snapshot(
    status: Option<SinkStatus>,
    connected: bool,
    device_label: Option<&str>,
    menu_items: &[GaugeMenuItem],
) -> AudioOutSignature {
    AudioOutSignature {
        percent: status.map(|s| s.percent),
        muted: status.map(|s| s.muted),
        connected,
        device_label: device_label.map(ToString::to_string),
        menu: menu_items
            .iter()
            .map(|item| (item.id.clone(), item.selected))
            .collect(),
    }
}

#[derive(Clone)]
struct SinkMenuEntry {
    name: String,
    description: Option<String>,
}

fn percent_from_volume(volume: Volume) -> u8 {
    let percent = (volume.0 as f64 * 100.0 / Volume::NORMAL.0 as f64).round();
    percent.clamp(0.0, 99.0) as u8
}

fn iterate(mainloop: &mut Mainloop) -> Option<()> {
    match mainloop.iterate(false) {
        IterateResult::Success(_) => Some(()),
        IterateResult::Quit(_) | IterateResult::Err(_) => None,
    }
}

fn operation_in_flight(state: pulse::operation::State) -> bool {
    state == pulse::operation::State::Running
}

fn wait_for_operation<C: ?Sized>(
    mainloop: &mut Mainloop,
    context: &Context,
    operation: &pulse::operation::Operation<C>,
) -> Option<()> {
    while operation_in_flight(operation.get_state()) {
        iterate(mainloop)?;
        if matches!(
            context.get_state(),
            ContextState::Failed | ContextState::Terminated
        ) {
            return None;
        }
    }
    Some(())
}

fn wait_for_context_ready(mainloop: &mut Mainloop, context: &Context) -> Option<()> {
    loop {
        match context.get_state() {
            ContextState::Ready => return Some(()),
            ContextState::Failed | ContextState::Terminated => return None,
            _ => {}
        }

        iterate(mainloop)?;
    }
}

fn default_sink_name(mainloop: &mut Mainloop, context: &Context) -> Option<String> {
    let sink_name = Rc::new(RefCell::new(None));
    let done = Rc::new(Cell::new(false));

    {
        let sink_name = Rc::clone(&sink_name);
        let done = Rc::clone(&done);
        context.introspect().get_server_info(move |info| {
            *sink_name.borrow_mut() = info.default_sink_name.as_ref().map(|n| n.to_string());
            done.set(true);
        });
    }

    while !done.get() {
        iterate(mainloop)?;
        match context.get_state() {
            ContextState::Failed | ContextState::Terminated => return None,
            _ => {}
        }
    }

    sink_name.borrow().clone()
}

fn read_sink_status(
    mainloop: &mut Mainloop,
    context: &Context,
    sink_name: &str,
) -> Option<SinkStatus> {
    let status = Rc::new(RefCell::new(None::<SinkStatus>));
    let done = Rc::new(Cell::new(false));

    {
        let status = Rc::clone(&status);
        let done = Rc::clone(&done);
        context
            .introspect()
            .get_sink_info_by_name(sink_name, move |result| match result {
                ListResult::Item(info) => {
                    let avg = info.volume.avg();
                    let percent = percent_from_volume(avg);
                    let muted = info.mute;
                    let channels = info.volume.len();
                    *status.borrow_mut() = Some(SinkStatus {
                        percent,
                        muted,
                        channels,
                    });
                }
                ListResult::End | ListResult::Error => done.set(true),
            });
    }

    while !done.get() {
        iterate(mainloop)?;
        match context.get_state() {
            ContextState::Failed | ContextState::Terminated => return None,
            _ => {}
        }
    }

    *status.borrow()
}

#[derive(Debug, PartialEq)]
enum SoundCommand {
    ToggleMute,
    AdjustVolume(i8),
    SetDefaultSink(String),
}

fn volume_from_percent(percent: u8) -> Volume {
    let ratio = percent as f64 / 100.0;
    let raw = (Volume::NORMAL.0 as f64 * ratio).round() as u32;
    Volume(raw)
}

fn recv_with_idle_wait(
    receiver: &mpsc::Receiver<SoundCommand>,
) -> Result<SoundCommand, mpsc::RecvTimeoutError> {
    receiver.recv_timeout(IDLE_WAIT)
}

fn collect_sinks(mainloop: &mut Mainloop, context: &Context) -> Option<Vec<SinkMenuEntry>> {
    let sinks = Rc::new(RefCell::new(Vec::new()));
    let done = Rc::new(Cell::new(false));

    {
        let sinks = Rc::clone(&sinks);
        let done = Rc::clone(&done);
        context
            .introspect()
            .get_sink_info_list(move |result| match result {
                ListResult::Item(info) => {
                    if let Some(port) = info.active_port.as_ref()
                        && matches!(port.available, def::PortAvailable::No)
                    {
                        return;
                    }

                    let name = info.name.as_ref().map(|n| n.to_string());
                    let description = info.description.as_ref().map(|d| d.to_string());

                    if let Some(name) = name {
                        sinks.borrow_mut().push(SinkMenuEntry { name, description });
                    }
                }
                ListResult::End | ListResult::Error => done.set(true),
            });
    }

    while !done.get() {
        iterate(mainloop)?;
        if matches!(
            context.get_state(),
            ContextState::Failed | ContextState::Terminated
        ) {
            return None;
        }
    }

    let mut entries = sinks.borrow().clone();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Some(entries)
}

fn sinks_to_menu_items(
    entries: &[SinkMenuEntry],
    default_sink: Option<&str>,
) -> Vec<GaugeMenuItem> {
    entries
        .iter()
        .map(|entry| {
            let raw_label = entry.description.clone().unwrap_or_else(|| {
                entry
                    .name
                    .split(" - ")
                    .last()
                    .unwrap_or(&entry.name)
                    .to_string()
            });
            let label = truncate_label(raw_label);
            GaugeMenuItem {
                id: entry.name.clone(),
                label,
                selected: default_sink.map(|d| d == entry.name).unwrap_or(false),
            }
        })
        .collect()
}

fn truncate_label(raw: String) -> String {
    let max = 92usize;
    let count = raw.chars().count();
    if count <= max {
        return raw;
    }

    let keep = max.saturating_sub(3);
    let mut truncated: String = raw.chars().take(keep).collect();
    truncated.push_str("...");
    truncated
}

fn device_label_for_sink(entries: Option<&[SinkMenuEntry]>, sink: &str) -> String {
    if let Some(entries) = entries
        && let Some(entry) = entries.iter().find(|entry| entry.name == sink)
        && let Some(description) = &entry.description
    {
        return description.clone();
    }

    sink.split(" - ").last().unwrap_or(sink).to_string()
}

struct AudioOutMenuCache {
    menu_items: Option<Vec<GaugeMenuItem>>,
    sink_labels: HashMap<String, String>,
    default_sink: Option<String>,
    next_refresh_deadline: Instant,
}

fn apply_output_command(
    command: SoundCommand,
    mainloop: &mut Mainloop,
    context: &mut Context,
) -> Option<()> {
    match command {
        SoundCommand::SetDefaultSink(name) => {
            let operation = context.set_default_sink(&name, |_| {});
            wait_for_operation(mainloop, context, &operation)?;
        }
        SoundCommand::ToggleMute => {
            if let Some(sink) = default_sink_name(mainloop, context)
                && let Some(status) = read_sink_status(mainloop, context, &sink)
            {
                let operation = context.introspect().set_sink_mute_by_name(
                    &sink,
                    !status.muted,
                    None::<Box<dyn FnMut(bool)>>,
                );
                wait_for_operation(mainloop, context, &operation)?;
            }
        }
        SoundCommand::AdjustVolume(delta) => {
            if let Some(sink) = default_sink_name(mainloop, context)
                && let Some(status) = read_sink_status(mainloop, context, &sink)
                && status.channels > 0
            {
                let new_percent = status.percent.saturating_add_signed(delta).clamp(0, 99);
                let mut volumes = ChannelVolumes::default();
                volumes.set(status.channels, volume_from_percent(new_percent));
                let operation = context.introspect().set_sink_volume_by_name(
                    &sink,
                    &volumes,
                    None::<Box<dyn FnMut(bool)>>,
                );
                wait_for_operation(mainloop, context, &operation)?;
            }
        }
    }
    Some(())
}

struct AudioOutSnapshot {
    status: Option<SinkStatus>,
    menu_items: Option<Vec<GaugeMenuItem>>,
    device_label: Option<String>,
    connected: bool,
}

impl AudioOutSnapshot {
    fn disconnected() -> Self {
        Self {
            status: None,
            menu_items: None,
            device_label: None,
            connected: false,
        }
    }
}

fn snapshot_audio_out_from_context(
    mainloop: &mut Mainloop,
    context: &Context,
    now: Instant,
    menu_cache: &mut AudioOutMenuCache,
) -> AudioOutSnapshot {
    let sink = default_sink_name(mainloop, context);
    // Rebuild sink menu infrequently unless the default sink changed.
    let should_refresh_menu = menu_cache.menu_items.is_none()
        || menu_cache.default_sink != sink
        || now >= menu_cache.next_refresh_deadline;
    if should_refresh_menu && let Some(sink_entries) = collect_sinks(mainloop, context) {
        menu_cache.menu_items = Some(sinks_to_menu_items(&sink_entries, sink.as_deref()));
        menu_cache.sink_labels = sink_entries
            .iter()
            .map(|entry| {
                let label = entry
                    .description
                    .clone()
                    .unwrap_or_else(|| device_label_for_sink(None, &entry.name));
                (entry.name.clone(), label)
            })
            .collect();
        menu_cache.default_sink = sink.clone();
        menu_cache.next_refresh_deadline = now + Duration::from_secs(MENU_REFRESH_INTERVAL_SECS);
    }
    let status = sink
        .as_deref()
        .and_then(|name| read_sink_status(mainloop, context, name));
    let device_label = sink.as_deref().map(|name| {
        menu_cache
            .sink_labels
            .get(name)
            .cloned()
            .unwrap_or_else(|| device_label_for_sink(None, name))
    });

    AudioOutSnapshot {
        status,
        menu_items: menu_cache.menu_items.clone(),
        device_label,
        connected: true,
    }
}

fn run_audio_out_worker(
    command_rx: mpsc::Receiver<SoundCommand>,
    snapshot_tx: mpsc::Sender<AudioOutSnapshot>,
    ready_notify: GaugeReadyNotify,
) {
    let mut mainloop = match Mainloop::new() {
        Some(mainloop) => mainloop,
        None => {
            let _ = snapshot_tx.send(AudioOutSnapshot::disconnected());
            ready_notify("audio_out");
            return;
        }
    };
    let mut context = match Context::new(&mainloop, "grelier-audio-out") {
        Some(context) => context,
        None => {
            let _ = snapshot_tx.send(AudioOutSnapshot::disconnected());
            ready_notify("audio_out");
            return;
        }
    };
    if context.connect(None, FlagSet::NOFLAGS, None).is_err()
        || wait_for_context_ready(&mut mainloop, &context).is_none()
    {
        let _ = snapshot_tx.send(AudioOutSnapshot::disconnected());
        ready_notify("audio_out");
        return;
    }

    let refresh_needed = Rc::new(Cell::new(true));
    context.set_subscribe_callback(Some(Box::new({
        let refresh_needed = Rc::clone(&refresh_needed);
        move |facility, _operation, _index| {
            if matches!(facility, Some(Facility::Sink) | Some(Facility::Server)) {
                refresh_needed.set(true);
            }
        }
    })));
    context.subscribe(InterestMaskSet::SINK | InterestMaskSet::SERVER, |_| {});
    let mut menu_cache = AudioOutMenuCache {
        menu_items: None,
        sink_labels: HashMap::new(),
        default_sink: None,
        next_refresh_deadline: Instant::now(),
    };
    let mut last_signature: Option<AudioOutSignature> = None;

    loop {
        while let Ok(command) = command_rx.try_recv() {
            if apply_output_command(command, &mut mainloop, &mut context).is_none() {
                let _ = snapshot_tx.send(AudioOutSnapshot::disconnected());
                ready_notify("audio_out");
                return;
            }
            refresh_needed.set(true);
        }

        if refresh_needed.replace(false) {
            let snapshot = snapshot_audio_out_from_context(
                &mut mainloop,
                &context,
                Instant::now(),
                &mut menu_cache,
            );
            let empty_menu = Vec::new();
            let signature = signature_for_snapshot(
                snapshot.status,
                snapshot.connected,
                snapshot.device_label.as_deref(),
                snapshot.menu_items.as_deref().unwrap_or(&empty_menu),
            );
            // Coalesce unchanged snapshots before waking the scheduler.
            if last_signature.as_ref() != Some(&signature) {
                last_signature = Some(signature);
                let _ = snapshot_tx.send(snapshot);
                ready_notify("audio_out");
            }
        }

        if matches!(
            context.get_state(),
            ContextState::Failed | ContextState::Terminated
        ) {
            let _ = snapshot_tx.send(AudioOutSnapshot::disconnected());
            ready_notify("audio_out");
            return;
        }

        if iterate(&mut mainloop).is_none() {
            let _ = snapshot_tx.send(AudioOutSnapshot::disconnected());
            ready_notify("audio_out");
            return;
        }

        match recv_with_idle_wait(&command_rx) {
            Ok(command) => {
                if apply_output_command(command, &mut mainloop, &mut context).is_none() {
                    let _ = snapshot_tx.send(AudioOutSnapshot::disconnected());
                    ready_notify("audio_out");
                    return;
                }
                refresh_needed.set(true);
            }
            Err(mpsc::RecvTimeoutError::Timeout) => {}
            Err(mpsc::RecvTimeoutError::Disconnected) => return,
        }
    }
}

struct AudioOutEventSource {
    command_rx: mpsc::Receiver<SoundCommand>,
    snapshot_tx: mpsc::Sender<AudioOutSnapshot>,
}

impl GaugeEventSource for AudioOutEventSource {
    fn run(self: Box<Self>, notify: GaugeReadyNotify) {
        run_audio_out_worker(self.command_rx, self.snapshot_tx, notify);
    }
}

struct AudioOutGauge {
    step_percent: i8,
    command_tx: mpsc::Sender<SoundCommand>,
    snapshot_rx: mpsc::Receiver<AudioOutSnapshot>,
    event_source: Option<AudioOutEventSource>,
    last_signature: Option<AudioOutSignature>,
    next_deadline: Instant,
}

impl Gauge for AudioOutGauge {
    fn id(&self) -> &'static str {
        "audio_out"
    }

    fn register(&mut self, registrar: &mut dyn GaugeRegistrar) {
        if let Some(event_source) = self.event_source.take() {
            registrar.add_event_source(Box::new(event_source));
        }
    }

    fn next_deadline(&self) -> Instant {
        self.next_deadline
    }

    fn run_once(&mut self, now: Instant) -> Option<crate::panels::gauges::gauge::GaugeModel> {
        let mut latest_snapshot = None;
        while let Ok(snapshot) = self.snapshot_rx.try_recv() {
            latest_snapshot = Some(snapshot);
        }
        let Some(snapshot) = latest_snapshot else {
            self.next_deadline = now + Duration::from_secs(IDLE_RUN_INTERVAL_SECS);
            return None;
        };
        let menu_snapshot = snapshot.menu_items.clone().unwrap_or_default();

        let status = snapshot.status;
        let device_label = snapshot
            .device_label
            .clone()
            .unwrap_or_else(|| "No output device".to_string());
        let signature = signature_for_snapshot(
            status,
            snapshot.connected,
            Some(&device_label),
            &menu_snapshot,
        );
        if self.last_signature.as_ref() == Some(&signature) {
            self.next_deadline = now + Duration::from_secs(IDLE_RUN_INTERVAL_SECS);
            return None;
        }
        self.last_signature = Some(signature);

        let step_percent = self.step_percent;
        let command_tx = self.command_tx.clone();
        let on_click: GaugeClickAction = Arc::new(move |click: GaugeClick| match click.input {
            crate::panels::gauges::gauge::GaugeInput::Button(iced::mouse::Button::Middle) => {
                let _ = command_tx.send(SoundCommand::ToggleMute);
            }
            crate::panels::gauges::gauge::GaugeInput::ScrollUp => {
                let _ = command_tx.send(SoundCommand::AdjustVolume(step_percent));
            }
            crate::panels::gauges::gauge::GaugeInput::ScrollDown => {
                let _ = command_tx.send(SoundCommand::AdjustVolume(-step_percent));
            }
            _ => {}
        });
        let menu_select: MenuSelectAction = {
            let command_tx = self.command_tx.clone();
            Arc::new(move |sink: String| {
                let _ = command_tx.send(SoundCommand::SetDefaultSink(sink));
            })
        };

        let icon = status
            .map(|status| {
                if status.muted {
                    svg_asset("speaker-mute.svg")
                } else {
                    svg_asset("speaker.svg")
                }
            })
            .unwrap_or_else(|| svg_asset("speaker.svg"));
        self.next_deadline = now + Duration::from_secs(IDLE_RUN_INTERVAL_SECS);

        Some(crate::panels::gauges::gauge::GaugeModel {
            id: "audio_out",
            icon,
            display: format_level(status.map(|status| status.percent)),
            on_click: Some(on_click),
            menu: if snapshot.connected {
                Some(GaugeMenu {
                    title: "Output Devices".to_string(),
                    items: menu_snapshot,
                    on_select: Some(menu_select),
                })
            } else {
                None
            },
            action_dialog: None,
            info: Some(InfoDialog {
                title: "Audio Out".to_string(),
                lines: vec![
                    device_label,
                    match status {
                        Some(status) => format!("Level: {}%", status.percent),
                        None => "Level: N/A".to_string(),
                    },
                ],
            }),
        })
    }
}

pub fn create_gauge(now: Instant) -> Box<dyn Gauge> {
    let mut step_percent = settings::settings()
        .get_parsed_or("grelier.gauge.audio_out.step_percent", DEFAULT_STEP_PERCENT);
    if step_percent == 0 {
        step_percent = DEFAULT_STEP_PERCENT;
    }
    let (command_tx, command_rx) = mpsc::channel::<SoundCommand>();
    let (snapshot_tx, snapshot_rx) = mpsc::channel::<AudioOutSnapshot>();

    Box::new(AudioOutGauge {
        step_percent,
        command_tx,
        snapshot_rx,
        event_source: Some(AudioOutEventSource {
            command_rx,
            snapshot_tx,
        }),
        last_signature: None,
        next_deadline: now,
    })
}

pub fn settings() -> &'static [SettingSpec] {
    const SETTINGS: &[SettingSpec] = &[SettingSpec {
        key: "grelier.gauge.audio_out.step_percent",
        default: "5",
    }];
    SETTINGS
}

inventory::submit! {
    GaugeSpec {
        id: "audio_out",
        description: "Audio output volume gauge showing percent level and mute state.",
        default_enabled: true,
        settings,
        create: create_gauge,
        validate: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::mpsc::RecvTimeoutError;

    #[test]
    fn menu_items_prefer_description_or_suffix() {
        let entries = vec![
            SinkMenuEntry {
                name: "alsa_output.foo - Long Name".into(),
                description: Some("Human Name".into()),
            },
            SinkMenuEntry {
                name: "alsa_output.bar - Pretty Label".into(),
                description: None,
            },
        ];
        let items = sinks_to_menu_items(&entries, Some("alsa_output.bar - Pretty Label"));

        assert_eq!(items[0].label, "Human Name");
        assert_eq!(items[1].label, "Pretty Label");
        assert!(items[1].selected);
    }

    #[test]
    fn truncate_label_limits_to_max_chars() {
        let long = "a".repeat(100);
        let truncated = truncate_label(long);
        assert_eq!(truncated.len(), 92);
        assert!(truncated.ends_with("..."));
    }

    #[test]
    fn level_uses_ratio_icon() {
        match format_level(Some(50)) {
            GaugeDisplay::Value {
                value: GaugeValue::Svg(handle),
                attention,
            } => {
                assert_eq!(handle, icon_quantity(50.0 / 99.0));
                assert_eq!(attention, GaugeValueAttention::Nominal);
            }
            _ => panic!("expected svg value for level"),
        }
    }

    #[test]
    fn level_is_none_on_missing_status() {
        assert!(matches!(format_level(None), GaugeDisplay::Error));
    }

    #[test]
    fn percent_from_volume_scales_and_clamps() {
        assert_eq!(percent_from_volume(Volume(0)), 0);
        assert_eq!(percent_from_volume(Volume::NORMAL), 99);
        assert_eq!(
            percent_from_volume(Volume(Volume::NORMAL.0.saturating_mul(2))),
            99
        );
    }

    #[test]
    fn idle_wait_blocks_when_no_command_is_available() {
        let (_tx, rx) = mpsc::channel::<SoundCommand>();
        let start = std::time::Instant::now();

        assert_eq!(recv_with_idle_wait(&rx), Err(RecvTimeoutError::Timeout));
        assert!(
            start.elapsed() >= IDLE_WAIT,
            "idle wait returned after {:?}, expected at least {:?}",
            start.elapsed(),
            IDLE_WAIT
        );
    }

    #[test]
    fn signature_tracks_visible_state_fields() {
        let items = vec![GaugeMenuItem {
            id: "sink-a".to_string(),
            label: "Sink A".to_string(),
            selected: true,
        }];
        let status = Some(SinkStatus {
            percent: 55,
            muted: false,
            channels: 2,
        });
        let a = signature_for_snapshot(status, true, Some("Speakers"), &items);
        let b = signature_for_snapshot(status, true, Some("Speakers"), &items);
        let c = signature_for_snapshot(status, true, Some("Headset"), &items);
        assert_eq!(a, b);
        assert_ne!(a, c);
    }
}
