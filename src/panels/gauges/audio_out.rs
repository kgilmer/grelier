// PulseAudio output volume gauge with mute/adjust actions and device menu.
// Consumes Settings: grelier.gauge.audio_out.step_percent.
use crate::dialog::info::InfoDialog;
use crate::icon::{icon_quantity, svg_asset};
use crate::panels::gauges::gauge::{Gauge, GaugeReadyNotify};
use crate::panels::gauges::gauge::{
    GaugeClick, GaugeClickAction, GaugeDisplay, GaugeMenu, GaugeMenuItem, GaugeValue,
    GaugeValueAttention, MenuSelectAction,
};
use crate::panels::gauges::gauge_registry::GaugeSpec;
use crate::settings;
use crate::settings::SettingSpec;
use libpulse_binding as pulse;
use pulse::callbacks::ListResult;
use pulse::context::{Context, FlagSet, State as ContextState};
use pulse::def;
use pulse::mainloop::standard::{IterateResult, Mainloop};
use pulse::volume::{ChannelVolumes, Volume};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc;
use std::time::{Duration, Instant};

#[cfg(test)]
const IDLE_WAIT: Duration = Duration::from_millis(25);
const DEFAULT_STEP_PERCENT: i8 = 5;
const POLL_INTERVAL: Duration = Duration::from_millis(100);

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

#[cfg(test)]
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

fn snapshot_audio_out(commands: Vec<SoundCommand>) -> AudioOutSnapshot {
    let mut mainloop = match Mainloop::new() {
        Some(mainloop) => mainloop,
        None => return AudioOutSnapshot::disconnected(),
    };
    let mut context = match Context::new(&mainloop, "grelier-audio-out") {
        Some(context) => context,
        None => return AudioOutSnapshot::disconnected(),
    };
    if context.connect(None, FlagSet::NOFLAGS, None).is_err()
        || wait_for_context_ready(&mut mainloop, &context).is_none()
    {
        return AudioOutSnapshot::disconnected();
    }

    for command in commands {
        if apply_output_command(command, &mut mainloop, &mut context).is_none() {
            return AudioOutSnapshot::disconnected();
        }
    }

    let sink = default_sink_name(&mut mainloop, &context);
    let entries = collect_sinks(&mut mainloop, &context);
    let status = sink
        .as_deref()
        .and_then(|name| read_sink_status(&mut mainloop, &context, name));
    let menu_items = entries
        .as_ref()
        .map(|entries| sinks_to_menu_items(entries, sink.as_deref()));
    let device_label = sink
        .as_deref()
        .map(|name| device_label_for_sink(entries.as_deref(), name));

    AudioOutSnapshot {
        status,
        menu_items,
        device_label,
        connected: true,
    }
}

struct AudioOutGauge {
    step_percent: i8,
    command_tx: mpsc::Sender<SoundCommand>,
    command_rx: mpsc::Receiver<SoundCommand>,
    ready_notify: Option<GaugeReadyNotify>,
    last_menu_items: Option<Vec<GaugeMenuItem>>,
    next_deadline: Instant,
}

impl Gauge for AudioOutGauge {
    fn id(&self) -> &'static str {
        "audio_out"
    }

    fn bind_ready_notify(&mut self, notify: GaugeReadyNotify) {
        self.ready_notify = Some(notify);
    }

    fn next_deadline(&self) -> Instant {
        self.next_deadline
    }

    fn run_once(&mut self, now: Instant) -> Option<crate::panels::gauges::gauge::GaugeModel> {
        let mut commands = Vec::new();
        while let Ok(command) = self.command_rx.try_recv() {
            commands.push(command);
        }

        let snapshot = snapshot_audio_out(commands);
        if let Some(items) = snapshot.menu_items.clone() {
            self.last_menu_items = Some(items);
        }
        let menu_snapshot = snapshot
            .menu_items
            .or_else(|| self.last_menu_items.clone())
            .unwrap_or_default();

        let step_percent = self.step_percent;
        let ready_notify = self.ready_notify.clone();
        let command_tx = self.command_tx.clone();
        let on_click: GaugeClickAction = Arc::new(move |click: GaugeClick| match click.input {
            crate::panels::gauges::gauge::GaugeInput::Button(iced::mouse::Button::Middle) => {
                let _ = command_tx.send(SoundCommand::ToggleMute);
                if let Some(ready_notify) = &ready_notify {
                    ready_notify("audio_out");
                }
            }
            crate::panels::gauges::gauge::GaugeInput::ScrollUp => {
                let _ = command_tx.send(SoundCommand::AdjustVolume(step_percent));
                if let Some(ready_notify) = &ready_notify {
                    ready_notify("audio_out");
                }
            }
            crate::panels::gauges::gauge::GaugeInput::ScrollDown => {
                let _ = command_tx.send(SoundCommand::AdjustVolume(-step_percent));
                if let Some(ready_notify) = &ready_notify {
                    ready_notify("audio_out");
                }
            }
            _ => {}
        });
        let menu_select: MenuSelectAction = {
            let command_tx = self.command_tx.clone();
            let ready_notify = self.ready_notify.clone();
            Arc::new(move |sink: String| {
                let _ = command_tx.send(SoundCommand::SetDefaultSink(sink));
                if let Some(ready_notify) = &ready_notify {
                    ready_notify("audio_out");
                }
            })
        };

        let status = snapshot.status;
        let icon = status
            .map(|status| {
                if status.muted {
                    svg_asset("speaker-mute.svg")
                } else {
                    svg_asset("speaker.svg")
                }
            })
            .unwrap_or_else(|| svg_asset("speaker.svg"));
        self.next_deadline = now + POLL_INTERVAL;

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
                    snapshot
                        .device_label
                        .unwrap_or_else(|| "No output device".to_string()),
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
    Box::new(AudioOutGauge {
        step_percent,
        command_tx,
        command_rx,
        ready_notify: None,
        last_menu_items: None,
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
}
