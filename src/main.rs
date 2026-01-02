#![allow(dead_code)] // workspace handling will be re-enabled later
mod app;
mod clock;
mod gauge;
mod date;
mod sway_workspace;

use iced::Subscription;
use iced::Task;
use iced::futures::{channel::mpsc, StreamExt};
use iced_layershell::application;
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, Settings, StartMode};
use swayipc::Event;

use crate::app::{BarState, Message, WorkspaceInfo};

fn app_subscription(state: &BarState) -> Subscription<Message> {
    Subscription::batch(vec![
        workspace_subscription(state),
        clock_subscription(),
        date_subscription(),
    ])
}

fn workspace_subscription(_state: &BarState) -> Subscription<Message> {
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
    Subscription::run(|| clock::seconds_stream().map(Message::Second))
}

fn date_subscription() -> Subscription<Message> {
    Subscription::run(|| date::day_stream().map(Message::Day))
}

fn main() -> Result<(), iced_layershell::Error> {
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
        .subscription(app_subscription)
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
        Message::Second(gauge) => {
            println!("{}: {}", gauge.title, gauge.value);
        }
        Message::Day(gauge) => {
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
