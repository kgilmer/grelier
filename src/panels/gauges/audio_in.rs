// PulseAudio input volume gauge with mute/adjust actions and device menu.
// Consumes Settings: grelier.gauge.audio_in.step_percent.
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
struct SourceStatus {
    percent: u8,
    muted: bool,
    channels: u8,
}

#[derive(Clone)]
struct SourceMenuEntry {
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

fn default_source_name(mainloop: &mut Mainloop, context: &Context) -> Option<String> {
    let source_name = Rc::new(RefCell::new(None));
    let done = Rc::new(Cell::new(false));

    {
        let source_name = Rc::clone(&source_name);
        let done = Rc::clone(&done);
        context.introspect().get_server_info(move |info| {
            *source_name.borrow_mut() = info.default_source_name.as_ref().map(|n| n.to_string());
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

    source_name.borrow().clone()
}

fn read_source_status(
    mainloop: &mut Mainloop,
    context: &Context,
    source_name: &str,
) -> Option<SourceStatus> {
    let status = Rc::new(RefCell::new(None::<SourceStatus>));
    let done = Rc::new(Cell::new(false));

    {
        let status = Rc::clone(&status);
        let done = Rc::clone(&done);
        context
            .introspect()
            .get_source_info_by_name(source_name, move |result| match result {
                ListResult::Item(info) => {
                    let avg = info.volume.avg();
                    let percent = percent_from_volume(avg);
                    let muted = info.mute;
                    let channels = info.volume.len();
                    *status.borrow_mut() = Some(SourceStatus {
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
enum InputCommand {
    ToggleMute,
    AdjustVolume(i8),
    SetDefaultSource(String),
}

fn volume_from_percent(percent: u8) -> Volume {
    let ratio = percent as f64 / 100.0;
    let raw = (Volume::NORMAL.0 as f64 * ratio).round() as u32;
    Volume(raw)
}

#[cfg(test)]
fn recv_with_idle_wait(
    receiver: &mpsc::Receiver<InputCommand>,
) -> Result<InputCommand, mpsc::RecvTimeoutError> {
    receiver.recv_timeout(IDLE_WAIT)
}

fn collect_sources(mainloop: &mut Mainloop, context: &Context) -> Option<Vec<SourceMenuEntry>> {
    let sources = Rc::new(RefCell::new(Vec::new()));
    let done = Rc::new(Cell::new(false));

    {
        let sources = Rc::clone(&sources);
        let done = Rc::clone(&done);
        context
            .introspect()
            .get_source_info_list(move |result| match result {
                ListResult::Item(info) => {
                    if info.monitor_of_sink.is_some() {
                        return;
                    }

                    if let Some(port) = info.active_port.as_ref()
                        && matches!(port.available, def::PortAvailable::No)
                    {
                        return;
                    }

                    let name = info.name.as_ref().map(|n| n.to_string());
                    let description = info.description.as_ref().map(|d| d.to_string());

                    if let Some(name) = name {
                        sources
                            .borrow_mut()
                            .push(SourceMenuEntry { name, description });
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

    let mut entries = sources.borrow().clone();
    entries.sort_by(|a, b| a.name.cmp(&b.name));
    Some(entries)
}

fn sources_to_menu_items(
    entries: &[SourceMenuEntry],
    default_source: Option<&str>,
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
                selected: default_source.map(|d| d == entry.name).unwrap_or(false),
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

fn device_label_for_source(entries: Option<&[SourceMenuEntry]>, source: &str) -> String {
    if let Some(entries) = entries
        && let Some(entry) = entries.iter().find(|entry| entry.name == source)
        && let Some(description) = &entry.description
    {
        return description.clone();
    }

    source.split(" - ").last().unwrap_or(source).to_string()
}

fn apply_input_command(command: InputCommand, mainloop: &mut Mainloop, context: &mut Context) {
    match command {
        InputCommand::SetDefaultSource(name) => {
            let _ = context.set_default_source(&name, |_| {});
        }
        InputCommand::ToggleMute => {
            if let Some(source) = default_source_name(mainloop, context)
                && let Some(status) = read_source_status(mainloop, context, &source)
            {
                context.introspect().set_source_mute_by_name(
                    &source,
                    !status.muted,
                    None::<Box<dyn FnMut(bool)>>,
                );
            }
        }
        InputCommand::AdjustVolume(delta) => {
            if let Some(source) = default_source_name(mainloop, context)
                && let Some(status) = read_source_status(mainloop, context, &source)
                && status.channels > 0
            {
                let new_percent = status.percent.saturating_add_signed(delta).clamp(0, 99);
                let mut volumes = ChannelVolumes::default();
                volumes.set(status.channels, volume_from_percent(new_percent));
                context.introspect().set_source_volume_by_name(
                    &source,
                    &volumes,
                    None::<Box<dyn FnMut(bool)>>,
                );
            }
        }
    }
}

struct AudioInSnapshot {
    status: Option<SourceStatus>,
    menu_items: Option<Vec<GaugeMenuItem>>,
    device_label: Option<String>,
    connected: bool,
}

impl AudioInSnapshot {
    fn disconnected() -> Self {
        Self {
            status: None,
            menu_items: None,
            device_label: None,
            connected: false,
        }
    }
}

fn snapshot_audio_in(commands: Vec<InputCommand>) -> AudioInSnapshot {
    let mut mainloop = match Mainloop::new() {
        Some(mainloop) => mainloop,
        None => return AudioInSnapshot::disconnected(),
    };
    let mut context = match Context::new(&mainloop, "grelier-audio-in") {
        Some(context) => context,
        None => return AudioInSnapshot::disconnected(),
    };
    if context.connect(None, FlagSet::NOFLAGS, None).is_err()
        || wait_for_context_ready(&mut mainloop, &context).is_none()
    {
        return AudioInSnapshot::disconnected();
    }

    for command in commands {
        apply_input_command(command, &mut mainloop, &mut context);
    }

    for _ in 0..4 {
        if iterate(&mut mainloop).is_none() {
            return AudioInSnapshot::disconnected();
        }
    }

    let source = default_source_name(&mut mainloop, &context);
    let entries = collect_sources(&mut mainloop, &context);
    let status = source
        .as_deref()
        .and_then(|name| read_source_status(&mut mainloop, &context, name));
    let menu_items = entries
        .as_ref()
        .map(|entries| sources_to_menu_items(entries, source.as_deref()));
    let device_label = source
        .as_deref()
        .map(|name| device_label_for_source(entries.as_deref(), name));

    AudioInSnapshot {
        status,
        menu_items,
        device_label,
        connected: true,
    }
}

struct AudioInGauge {
    step_percent: i8,
    command_tx: mpsc::Sender<InputCommand>,
    command_rx: mpsc::Receiver<InputCommand>,
    ready_notify: Option<GaugeReadyNotify>,
    last_menu_items: Option<Vec<GaugeMenuItem>>,
    next_deadline: Instant,
}

impl Gauge for AudioInGauge {
    fn id(&self) -> &'static str {
        "audio_in"
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

        let snapshot = snapshot_audio_in(commands);
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
                let _ = command_tx.send(InputCommand::ToggleMute);
                if let Some(ready_notify) = &ready_notify {
                    ready_notify("audio_in");
                }
            }
            crate::panels::gauges::gauge::GaugeInput::ScrollUp => {
                let _ = command_tx.send(InputCommand::AdjustVolume(step_percent));
                if let Some(ready_notify) = &ready_notify {
                    ready_notify("audio_in");
                }
            }
            crate::panels::gauges::gauge::GaugeInput::ScrollDown => {
                let _ = command_tx.send(InputCommand::AdjustVolume(-step_percent));
                if let Some(ready_notify) = &ready_notify {
                    ready_notify("audio_in");
                }
            }
            _ => {}
        });
        let menu_select: MenuSelectAction = {
            let command_tx = self.command_tx.clone();
            let ready_notify = self.ready_notify.clone();
            Arc::new(move |source: String| {
                let _ = command_tx.send(InputCommand::SetDefaultSource(source));
                if let Some(ready_notify) = &ready_notify {
                    ready_notify("audio_in");
                }
            })
        };

        let status = snapshot.status;
        let icon = status
            .map(|status| {
                if status.muted {
                    svg_asset("microphone-disabled.svg")
                } else {
                    svg_asset("microphone.svg")
                }
            })
            .unwrap_or_else(|| svg_asset("microphone.svg"));
        self.next_deadline = now + POLL_INTERVAL;

        Some(crate::panels::gauges::gauge::GaugeModel {
            id: "audio_in",
            icon: Some(icon),
            display: format_level(status.map(|status| status.percent)),
            on_click: Some(on_click),
            menu: if snapshot.connected {
                Some(GaugeMenu {
                    title: "Input Devices".to_string(),
                    items: menu_snapshot,
                    on_select: Some(menu_select),
                })
            } else {
                None
            },
            action_dialog: None,
            info: Some(InfoDialog {
                title: "Audio In".to_string(),
                lines: vec![
                    snapshot
                        .device_label
                        .unwrap_or_else(|| "No input device".to_string()),
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
        .get_parsed_or("grelier.gauge.audio_in.step_percent", DEFAULT_STEP_PERCENT);
    if step_percent == 0 {
        step_percent = DEFAULT_STEP_PERCENT;
    }
    let (command_tx, command_rx) = mpsc::channel::<InputCommand>();
    Box::new(AudioInGauge {
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
        key: "grelier.gauge.audio_in.step_percent",
        default: "5",
    }];
    SETTINGS
}

inventory::submit! {
    GaugeSpec {
        id: "audio_in",
        description: "Audio input volume gauge reporting percent level and mute state.",
        default_enabled: false,
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
            SourceMenuEntry {
                name: "alsa_input.foo - Long Name".into(),
                description: Some("Human Name".into()),
            },
            SourceMenuEntry {
                name: "alsa_input.bar - Pretty Label".into(),
                description: None,
            },
        ];
        let items = sources_to_menu_items(&entries, Some("alsa_input.bar - Pretty Label"));

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
        let (_tx, rx) = mpsc::channel::<InputCommand>();
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
