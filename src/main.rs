#![allow(dead_code)] // workspace handling will be re-enabled later
mod app;
mod sway_workspace;

use iced::futures::channel::mpsc;
use iced::Subscription;
use iced_layershell::application;
use iced_layershell::reexport::{Anchor, KeyboardInteractivity, Layer};
use iced_layershell::settings::{LayerShellSettings, Settings, StartMode};
use swayipc::Event;

use crate::app::{Message, NumberStrip};

fn workspace_subscription(_state: &NumberStrip) -> Subscription<Message> {
    Subscription::run(workspace_stream)
}

fn workspace_stream() -> impl iced::futures::Stream<Item = Message> {
    let (mut sender, receiver) = mpsc::channel(16);

    std::thread::spawn(move || {
        let send_workspaces = |sender: &mut mpsc::Sender<Message>| {
            if let Ok(ws) = sway_workspace::fetch_workspaces() {
                let ids = ws.into_iter().map(|w| w.num).collect();
                let _ = sender.try_send(Message::Workspaces(ids));
            }
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

    application(NumberStrip::new, NumberStrip::namespace, NumberStrip::update, NumberStrip::view)
        .theme(NumberStrip::theme)
        .subscription(workspace_subscription)
        .settings(settings)
        .run()
}
