use crate::app::Message;
use crate::gauge::{GaugeClick, GaugeClickAction, GaugeValue, GaugeValueAttention, event_stream};
use crate::icon::svg_asset;
use iced::Subscription;
use iced::futures::StreamExt;
use libpulse_binding as pulse;
use pulse::callbacks::ListResult;
use pulse::context::subscribe::{Facility, InterestMaskSet};
use pulse::context::{Context, FlagSet, State as ContextState};
use pulse::mainloop::standard::{IterateResult, Mainloop};
use pulse::volume::{ChannelVolumes, Volume};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::Arc;
use std::sync::mpsc::{self, RecvTimeoutError};
use std::time::Duration;

const IDLE_WAIT: Duration = Duration::from_millis(25);

fn format_percent(value: u8) -> String {
    format!("{:02}", value.min(99))
}

#[derive(Clone, Copy)]
struct SinkStatus {
    percent: u8,
    muted: bool,
    channels: u8,
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

fn handle_command(
    command: SoundCommand,
    last_status: &Option<SinkStatus>,
    mainloop: &mut Mainloop,
    context: &Context,
    refresh_needed: &Cell<bool>,
) {
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
        }
        refresh_needed.set(true);
    }
}

fn audio_out_stream() -> impl iced::futures::Stream<Item = crate::gauge::GaugeModel> {
    let (command_tx, command_rx) = mpsc::channel::<SoundCommand>();
    let on_click: GaugeClickAction = {
        let command_tx = command_tx.clone();
        Arc::new(move |click: GaugeClick| match click.input {
            crate::gauge::GaugeInput::Button(iced::mouse::Button::Right) => {
                let _ = command_tx.send(SoundCommand::ToggleMute);
            }
            crate::gauge::GaugeInput::ScrollUp => {
                let _ = command_tx.send(SoundCommand::AdjustVolume(5));
            }
            crate::gauge::GaugeInput::ScrollDown => {
                let _ = command_tx.send(SoundCommand::AdjustVolume(-5));
            }
            _ => {}
        })
    };

    event_stream(
        "audio_out",
        Some(svg_asset("speaker.svg")),
        move |mut sender| {
            let icon_unmuted = svg_asset("speaker.svg");
            let icon_muted = svg_asset("speaker-mute.svg");

            let mut send_value = |status: Option<SinkStatus>| {
                let attention = match status {
                    Some(_) => GaugeValueAttention::Nominal,
                    None => GaugeValueAttention::Danger,
                };
                let value = status
                    .map(|s| s.percent)
                    .map(format_percent)
                    .map(GaugeValue::Text)
                    .unwrap_or_else(|| GaugeValue::Text("--".to_string()));
                let icon = status
                    .map(|s| {
                        if s.muted {
                            icon_muted.clone()
                        } else {
                            icon_unmuted.clone()
                        }
                    })
                    .unwrap_or_else(|| icon_unmuted.clone());

                let _ = sender.try_send(crate::gauge::GaugeModel {
                    id: "audio_out",
                    icon: Some(icon),
                    value,
                    attention,
                    on_click: Some(on_click.clone()),
                });
            };

            let mut mainloop = match Mainloop::new() {
                Some(m) => m,
                None => {
                    send_value(None);
                    return;
                }
            };
            let mut context = match Context::new(&mainloop, "grelier-audio-out") {
                Some(c) => c,
                None => {
                    send_value(None);
                    return;
                }
            };
            if context.connect(None, FlagSet::NOFLAGS, None).is_err() {
                send_value(None);
                return;
            }

            if wait_for_context_ready(&mut mainloop, &context).is_none() {
                send_value(None);
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

            loop {
                while let Ok(command) = command_rx.try_recv() {
                    handle_command(
                        command,
                        &last_status,
                        &mut mainloop,
                        &context,
                        &refresh_needed,
                    );
                }

                if refresh_needed.replace(false) {
                    let sink = default_sink_name(&mut mainloop, &context);
                    let status = sink
                        .as_deref()
                        .and_then(|name| read_sink_status(&mut mainloop, &context, name));
                    if status.is_some() {
                        last_status = status;
                    }
                    send_value(status);
                }

                if matches!(
                    context.get_state(),
                    ContextState::Failed | ContextState::Terminated
                ) {
                    send_value(None);
                    break;
                }

                if iterate(&mut mainloop).is_none() {
                    send_value(None);
                    break;
                }

                match recv_with_idle_wait(&command_rx) {
                    Ok(command) => handle_command(
                        command,
                        &last_status,
                        &mut mainloop,
                        &context,
                        &refresh_needed,
                    ),
                    Err(RecvTimeoutError::Timeout) => {}
                    Err(RecvTimeoutError::Disconnected) => {
                        send_value(None);
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

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn formats_with_two_digits() {
        assert_eq!(format_percent(0), "00");
        assert_eq!(format_percent(7), "07");
        assert_eq!(format_percent(99), "99");
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
