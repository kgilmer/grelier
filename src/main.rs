#![allow(dead_code)] // workspace handling will be re-enabled later
mod app;
mod clock;
mod gauge;
mod date;
mod sway_workspace;

use argh::FromArgs;
use iced::Subscription;
use iced::Task;
use iced::futures::{channel::mpsc, StreamExt};
use iced_layershell::application;
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, Settings, StartMode};
use swayipc::Event;

use crate::app::{BarState, Message, WorkspaceInfo};

#[derive(FromArgs, Debug)]
/// Workspace + gauges display
struct Args {
    /// comma-separated list of gauges to run (clock,date,...)
    #[argh(option, default = "\"clock,date\".to_string()")]
    gauges: String,

    /// list all available gauges and exit
    #[argh(switch)]
    list_gauges: bool,
}

fn app_subscription(_state: &BarState, gauges: &[&str]) -> Subscription<Message> {
    let mut subs = Vec::new();
    subs.push(workspace_subscription());
    for gauge in gauges {
        match *gauge {
            "clock" => subs.push(clock_subscription()),
            "date" => subs.push(date_subscription()),
            other => eprintln!("Unknown gauge '{other}', skipping"),
        }
    }
    Subscription::batch(subs)
}

fn workspace_subscription() -> Subscription<Message> {
    Subscription::run(workspace_stream)
}

fn workspace_stream() -> impl iced::futures::Stream<Item = Message> {
    let (mut sender, receiver) = mpsc::channel(16);

    std::thread::spawn(move || {
        let send_workspaces =
            |sender: &mut mpsc::Sender<Message>| match sway_workspace::fetch_workspaces() {
                Ok(ws) => {
                    let info = ws.into_iter().map(to_workspace_info).collect();
                    let _ = sender.try_send(Message::Workspaces(info));
                }
                Err(err) => eprintln!("Failed to fetch workspaces: {err}"),
            };

        send_workspaces(&mut sender);

        let mut stream = match sway_workspace::subscribe_workspace_events() {
            Ok(stream) => stream,
            Err(err) => {
                eprintln!("Failed to subscribe to workspace events: {err}");
                return;
            }
        };

        for event in &mut stream {
            match event {
                Ok(Event::Workspace(_)) => send_workspaces(&mut sender),
                Ok(_) => {}
                Err(err) => {
                    eprintln!("Workspace event stream error: {err}");
                    break;
                }
            }
        }
    });

    receiver
}

fn clock_subscription() -> Subscription<Message> {
    Subscription::run(|| clock::seconds_stream().map(Message::Gauge))
}

fn date_subscription() -> Subscription<Message> {
    Subscription::run(|| date::day_stream().map(Message::Gauge))
}

fn main() -> Result<(), iced_layershell::Error> {
    let args: Args = argh::from_env();

    if args.list_gauges {
        println!("Available gauges: clock, date");
        return Ok(());
    }

    let gauges: Vec<String> = args
        .gauges
        .split(',')
        .map(str::trim)
        .filter(|s| !s.is_empty())
        .map(|s| s.to_string())
        .collect();

    let settings = Settings {
        layer_settings: LayerShellSettings {
            size: Some((24, 0)), // width fixed to 24px, height chosen by compositor (anchored top+bottom)
            exclusive_zone: 24,
            anchor: Anchor::Top | Anchor::Bottom | Anchor::Left,
            layer: Layer::Overlay,
            margin: (0, 0, 0, 0),
            keyboard_interactivity: KeyboardInteractivity::OnDemand,
            start_mode: StartMode::Active,
            events_transparent: false,
        },
        antialiasing: true,
        ..Settings::default()
    };

    application(BarState::new, BarState::namespace, update, BarState::view)
        .theme(BarState::theme)
        .subscription({
            let gauges = gauges.clone();
            move |state| {
                let gauge_refs: Vec<&str> = gauges.iter().map(|s| s.as_str()).collect();
                app_subscription(state, &gauge_refs)
            }
        })
        .settings(settings)
        .run()
}

fn update(state: &mut BarState, message: Message) -> Task<Message> {
    match message {
        Message::Workspaces(ws) => state.workspaces = ws,
        Message::Clicked(name) => {
            if let Err(err) = sway_workspace::focus_workspace(&name) {
                eprintln!("Failed to focus workspace \"{name}\": {err}");
            }
        }
        Message::Gauge(gauge) => {
            println!("{}: {}", gauge.title, gauge.value);
        }
    }

    Task::none()
}

fn to_workspace_info(ws: swayipc::Workspace) -> WorkspaceInfo {
    let rect = crate::app::Rect {
        x: ws.rect.x,
        y: ws.rect.y,
        width: ws.rect.width,
        height: ws.rect.height,
    };

    WorkspaceInfo {
        id: ws.id,
        num: ws.num,
        name: ws.name,
        layout: ws.layout,
        visible: ws.visible,
        focused: ws.focused,
        urgent: ws.urgent,
        representation: ws.representation,
        orientation: ws.orientation,
        rect,
        output: ws.output,
        focus: ws.focus,
    }
}
