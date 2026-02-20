// PulseAudio input volume gauge with mute/adjust actions and device menu.
// Consumes Settings: grelier.gauge.audio_in.step_percent.
use crate::dialog::info::InfoDialog;
use crate::icon::{icon_quantity, svg_asset};
use crate::panels::gauges::gauge::{
    GaugeClick, GaugeClickAction, GaugeDisplay, GaugeMenu, GaugeMenuItem, GaugeValue,
    GaugeValueAttention, MenuSelectAction, event_stream,
};
use crate::panels::gauges::gauge_registry::{GaugeSpec, GaugeStream};
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
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

const IDLE_WAIT: Duration = Duration::from_millis(25);
const DEFAULT_STEP_PERCENT: i8 = 5;

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

fn recv_with_idle_wait(
    receiver: &mpsc::Receiver<InputCommand>,
) -> Result<InputCommand, RecvTimeoutError> {
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

fn handle_command(
    command: InputCommand,
    last_status: &Option<SourceStatus>,
    mainloop: &mut Mainloop,
    context: &mut Context,
    refresh_needed: &Cell<bool>,
) {
    if let InputCommand::SetDefaultSource(name) = &command {
        context.set_default_source(name, |_| {});
        refresh_needed.set(true);
        return;
    }

    if let Some(status) = last_status
        && let Some(source) = default_source_name(mainloop, context)
    {
        match command {
            InputCommand::ToggleMute => {
                let target = !status.muted;
                context.introspect().set_source_mute_by_name(
                    &source,
                    target,
                    None::<Box<dyn FnMut(bool)>>,
                );
            }
            InputCommand::AdjustVolume(delta) => {
                if status.channels > 0 {
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
            InputCommand::SetDefaultSource(_) => {}
        }
        refresh_needed.set(true);
    }
}

fn audio_in_stream() -> impl iced::futures::Stream<Item = crate::panels::gauges::gauge::GaugeModel>
{
    let (command_tx, command_rx) = mpsc::channel::<InputCommand>();
    let mut step_percent = settings::settings()
        .get_parsed_or("grelier.gauge.audio_in.step_percent", DEFAULT_STEP_PERCENT);
    if step_percent == 0 {
        step_percent = DEFAULT_STEP_PERCENT;
    }
    let on_click: GaugeClickAction = {
        let command_tx = command_tx.clone();
        Arc::new(move |click: GaugeClick| match click.input {
            crate::panels::gauges::gauge::GaugeInput::Button(iced::mouse::Button::Middle) => {
                let _ = command_tx.send(InputCommand::ToggleMute);
            }
            crate::panels::gauges::gauge::GaugeInput::ScrollUp => {
                let _ = command_tx.send(InputCommand::AdjustVolume(step_percent));
            }
            crate::panels::gauges::gauge::GaugeInput::ScrollDown => {
                let _ = command_tx.send(InputCommand::AdjustVolume(-step_percent));
            }
            _ => {}
        })
    };
    let menu_select: MenuSelectAction = {
        let command_tx = command_tx.clone();
        Arc::new(move |source: String| {
            let _ = command_tx.send(InputCommand::SetDefaultSource(source));
        })
    };

    event_stream(
        "audio_in",
        Some(svg_asset("microphone.svg")),
        move |mut sender| {
            let icon_unmuted = svg_asset("microphone.svg");
            let icon_muted = svg_asset("microphone-disabled.svg");

            let mut send_value = |status: Option<SourceStatus>,
                                  menu_items: Option<Vec<GaugeMenuItem>>,
                                  device_label: Option<String>| {
                let display = format_level(status.map(|s| s.percent));

                let icon = status
                    .map(|s| {
                        if s.muted {
                            icon_muted.clone()
                        } else {
                            icon_unmuted.clone()
                        }
                    })
                    .unwrap_or_else(|| icon_unmuted.clone());

                let menu = menu_items.map(|items| GaugeMenu {
                    title: "Input Devices".to_string(),
                    items,
                    on_select: Some(menu_select.clone()),
                });

                let info = InfoDialog {
                    title: "Audio In".to_string(),
                    lines: vec![
                        device_label.unwrap_or_else(|| "No input device".to_string()),
                        match status {
                            Some(status) => format!("Level: {}%", status.percent),
                            None => "Level: N/A".to_string(),
                        },
                    ],
                };

                let _ = sender.try_send(crate::panels::gauges::gauge::GaugeModel {
                    id: "audio_in",
                    icon: Some(icon),
                    display,
                    nominal_color: None,
                    on_click: Some(on_click.clone()),
                    menu,
                    action_dialog: None,
                    info: Some(info),
                });
            };

            let mut mainloop = match Mainloop::new() {
                Some(m) => m,
                None => {
                    send_value(None, None, None);
                    return;
                }
            };
            let mut context = match Context::new(&mainloop, "grelier-audio-in") {
                Some(c) => c,
                None => {
                    send_value(None, None, None);
                    return;
                }
            };
            if context.connect(None, FlagSet::NOFLAGS, None).is_err() {
                send_value(None, None, None);
                return;
            }

            if wait_for_context_ready(&mut mainloop, &context).is_none() {
                send_value(None, None, None);
                return;
            }

            let refresh_needed = Rc::new(Cell::new(true));

            context.set_subscribe_callback(Some(Box::new({
                let refresh_needed = Rc::clone(&refresh_needed);
                move |facility, _operation, _index| {
                    if matches!(facility, Some(Facility::Source) | Some(Facility::Server)) {
                        refresh_needed.set(true);
                    }
                }
            })));
            context.subscribe(InterestMaskSet::SOURCE | InterestMaskSet::SERVER, |_| {});
            let mut last_status: Option<SourceStatus> = None;
            let mut last_menu_items: Option<Vec<GaugeMenuItem>> = None;

            loop {
                while let Ok(command) = command_rx.try_recv() {
                    handle_command(
                        command,
                        &last_status,
                        &mut mainloop,
                        &mut context,
                        &refresh_needed,
                    );
                }

                if refresh_needed.replace(false) {
                    let source = default_source_name(&mut mainloop, &context);
                    let current_entries = collect_sources(&mut mainloop, &context);
                    let status = source
                        .as_deref()
                        .and_then(|name| read_source_status(&mut mainloop, &context, name));
                    if status.is_some() {
                        last_status = status;
                    }

                    let current_items = current_entries
                        .as_ref()
                        .map(|entries| sources_to_menu_items(entries, source.as_deref()));

                    if let Some(items) = current_items.clone() {
                        last_menu_items = Some(items);
                    }

                    let menu_snapshot = current_items
                        .or_else(|| last_menu_items.clone())
                        .unwrap_or_default();

                    let device_label = source
                        .as_deref()
                        .map(|name| device_label_for_source(current_entries.as_deref(), name));

                    send_value(status, Some(menu_snapshot), device_label);
                }

                if matches!(
                    context.get_state(),
                    ContextState::Failed | ContextState::Terminated
                ) {
                    send_value(None, None, None);
                    break;
                }

                if iterate(&mut mainloop).is_none() {
                    send_value(None, None, None);
                    break;
                }

                match recv_with_idle_wait(&command_rx) {
                    Ok(command) => handle_command(
                        command,
                        &last_status,
                        &mut mainloop,
                        &mut context,
                        &refresh_needed,
                    ),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => {
                        send_value(None, None, None);
                        break;
                    }
                }
            }
        },
    )
}

pub fn settings() -> &'static [SettingSpec] {
    const SETTINGS: &[SettingSpec] = &[SettingSpec {
        key: "grelier.gauge.audio_in.step_percent",
        default: "5",
    }];
    SETTINGS
}

fn stream() -> GaugeStream {
    Box::new(audio_in_stream())
}

inventory::submit! {
    GaugeSpec {
        id: "audio_in",
        description: "Audio input volume gauge reporting percent level and mute state.",
        default_enabled: false,
        settings,
        stream,
        validate: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

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
