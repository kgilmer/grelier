// PulseAudio output volume gauge with mute/adjust actions and device menu.
// Consumes Settings: grelier.gauge.audio_out.step_percent.
use crate::app::Message;
use crate::gauge::{
    GaugeClick, GaugeClickAction, GaugeMenu, GaugeMenuItem, GaugeValue, GaugeValueAttention,
    MenuSelectAction, SettingSpec, event_stream,
};
use crate::icon::svg_asset;
use crate::settings;
use iced::Subscription;
use iced::futures::StreamExt;
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

fn format_percent(value: u8) -> String {
    format!("{:02}", value.min(99))
}

fn format_level(percent: Option<u8>) -> (Option<GaugeValue>, GaugeValueAttention) {
    match percent {
        Some(value) => (
            Some(GaugeValue::Text(format_percent(value))),
            GaugeValueAttention::Nominal,
        ),
        None => (None, GaugeValueAttention::Danger),
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
) -> Result<SoundCommand, RecvTimeoutError> {
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

fn handle_command(
    command: SoundCommand,
    last_status: &Option<SinkStatus>,
    mainloop: &mut Mainloop,
    context: &mut Context,
    refresh_needed: &Cell<bool>,
) {
    if let SoundCommand::SetDefaultSink(name) = &command {
        context.set_default_sink(name, |_| {});
        refresh_needed.set(true);
        return;
    }

    if let Some(status) = last_status
        && let Some(sink) = default_sink_name(mainloop, context)
    {
        match command {
            SoundCommand::ToggleMute => {
                let target = !status.muted;
                context.introspect().set_sink_mute_by_name(
                    &sink,
                    target,
                    None::<Box<dyn FnMut(bool)>>,
                );
            }
            SoundCommand::AdjustVolume(delta) => {
                if status.channels > 0 {
                    let new_percent = status.percent.saturating_add_signed(delta).clamp(0, 99);
                    let mut volumes = ChannelVolumes::default();
                    volumes.set(status.channels, volume_from_percent(new_percent));
                    context.introspect().set_sink_volume_by_name(
                        &sink,
                        &volumes,
                        None::<Box<dyn FnMut(bool)>>,
                    );
                }
            }
            SoundCommand::SetDefaultSink(_) => {}
        }
        refresh_needed.set(true);
    }
}

fn audio_out_stream() -> impl iced::futures::Stream<Item = crate::gauge::GaugeModel> {
    let (command_tx, command_rx) = mpsc::channel::<SoundCommand>();
    let mut step_percent = settings::settings()
        .get_parsed_or("grelier.gauge.audio_out.step_percent", DEFAULT_STEP_PERCENT);
    if step_percent == 0 {
        step_percent = DEFAULT_STEP_PERCENT;
    }
    let on_click: GaugeClickAction = {
        let command_tx = command_tx.clone();
        Arc::new(move |click: GaugeClick| match click.input {
            crate::gauge::GaugeInput::Button(iced::mouse::Button::Left) => {
                let _ = command_tx.send(SoundCommand::ToggleMute);
            }
            crate::gauge::GaugeInput::ScrollUp => {
                let _ = command_tx.send(SoundCommand::AdjustVolume(step_percent));
            }
            crate::gauge::GaugeInput::ScrollDown => {
                let _ = command_tx.send(SoundCommand::AdjustVolume(-step_percent));
            }
            _ => {}
        })
    };
    let menu_select: MenuSelectAction = {
        let command_tx = command_tx.clone();
        Arc::new(move |sink: String| {
            let _ = command_tx.send(SoundCommand::SetDefaultSink(sink));
        })
    };

    event_stream(
        "audio_out",
        Some(svg_asset("speaker.svg")),
        move |mut sender| {
            let icon_unmuted = svg_asset("speaker.svg");
            let icon_muted = svg_asset("speaker-mute.svg");

            let mut send_value =
                |status: Option<SinkStatus>, menu_items: Option<Vec<GaugeMenuItem>>| {
                    let (value, attention) = format_level(status.map(|s| s.percent));
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
                        title: "Output Devices".to_string(),
                        items,
                        on_select: Some(menu_select.clone()),
                    });

                    let _ = sender.try_send(crate::gauge::GaugeModel {
                        id: "audio_out",
                        icon: Some(icon),
                        value,
                        attention,
                        on_click: Some(on_click.clone()),
                        menu,
                    });
                };

            let mut mainloop = match Mainloop::new() {
                Some(m) => m,
                None => {
                    send_value(None, None);
                    return;
                }
            };
            let mut context = match Context::new(&mainloop, "grelier-audio-out") {
                Some(c) => c,
                None => {
                    send_value(None, None);
                    return;
                }
            };
            if context.connect(None, FlagSet::NOFLAGS, None).is_err() {
                send_value(None, None);
                return;
            }

            if wait_for_context_ready(&mut mainloop, &context).is_none() {
                send_value(None, None);
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
            let mut last_status: Option<SinkStatus> = None;
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
                    let sink = default_sink_name(&mut mainloop, &context);
                    let status = sink
                        .as_deref()
                        .and_then(|name| read_sink_status(&mut mainloop, &context, name));
                    last_status = status;
                    let current_items = collect_sinks(&mut mainloop, &context)
                        .map(|entries| sinks_to_menu_items(&entries, sink.as_deref()));

                    if let Some(items) = current_items.clone() {
                        last_menu_items = Some(items);
                    }

                    let menu_snapshot = current_items
                        .or_else(|| last_menu_items.clone())
                        .unwrap_or_default();

                    send_value(status, Some(menu_snapshot));
                }

                if matches!(
                    context.get_state(),
                    ContextState::Failed | ContextState::Terminated
                ) {
                    send_value(None, None);
                    break;
                }

                if iterate(&mut mainloop).is_none() {
                    send_value(None, None);
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
                        send_value(None, None);
                        break;
                    }
                }
            }
        },
    )
}

pub fn audio_out_subscription() -> Subscription<Message> {
    Subscription::run(|| audio_out_stream().map(Message::Gauge))
}

pub fn settings() -> &'static [SettingSpec] {
    const SETTINGS: &[SettingSpec] = &[SettingSpec {
        key: "grelier.gauge.audio_out.step_percent",
        default: "5",
    }];
    SETTINGS
}

#[cfg(test)]
mod tests {
    use super::*;

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
    fn formats_with_two_digits() {
        assert_eq!(format_percent(0), "00");
        assert_eq!(format_percent(7), "07");
        assert_eq!(format_percent(99), "99");
    }

    #[test]
    fn level_is_none_on_missing_status() {
        let (value, attention) = format_level(None);
        assert!(value.is_none());
        assert_eq!(attention, GaugeValueAttention::Danger);
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
