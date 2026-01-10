// Sway IPC helpers for workspace state, focus, and subscriptions.
use std::cell::RefCell;

use iced::Subscription;
use iced::futures::channel::mpsc;
use swayipc::Event;
use swayipc::{Connection, Error, EventStream, EventType, Workspace};

#[cfg(test)]
type SwayConnection = FakeConnection;
#[cfg(not(test))]
type SwayConnection = Connection;

#[derive(Debug, Clone)]
pub struct WorkspaceInfo {
    pub id: i64,
    pub num: i32,
    pub name: String,
    pub layout: String,
    pub visible: bool,
    pub focused: bool,
    pub urgent: bool,
    pub representation: Option<String>,
    pub orientation: String,
    pub rect: Rect,
    pub output: String,
    pub focus: Vec<i64>,
}

#[derive(Debug, Clone)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}

thread_local! {
    static COMMAND_CONN: RefCell<Option<SwayConnection>> = const { RefCell::new(None) };
}

/// Fetch and sort the current Sway workspaces.
pub fn fetch_workspaces() -> Result<Vec<Workspace>, Error> {
    with_command_conn(|conn| {
        let mut workspaces = conn.get_workspaces()?;
        workspaces.sort_by(|a, b| (a.num, &a.name).cmp(&(b.num, &b.name)));
        Ok(workspaces)
    })
}

/// Subscribe to workspace-related events.
pub fn subscribe_workspace_events() -> Result<EventStream, Error> {
    Connection::new()?.subscribe([EventType::Workspace])
}

/// Focus the workspace with the given name.
pub fn focus_workspace(name: &str) -> Result<(), Error> {
    with_command_conn(|conn| {
        let cmd = format!("workspace \"{}\"", name.replace('"', "\\\""));
        conn.run_command(cmd)?;
        Ok(())
    })
}

fn with_command_conn<R>(
    f: impl FnOnce(&mut SwayConnection) -> Result<R, Error>,
) -> Result<R, Error> {
    COMMAND_CONN.with(|cell| {
        if cell.borrow().is_none() {
            *cell.borrow_mut() = Some(SwayConnection::new()?);
        }

        // Connection is guaranteed to exist after initialization above.
        let mut conn_ref = cell.borrow_mut();
        let conn = conn_ref.as_mut().expect("connection initialized");
        f(conn)
    })
}

fn to_workspace_info(ws: swayipc::Workspace) -> WorkspaceInfo {
    let rect = Rect {
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

pub fn workspace_subscription() -> Subscription<Message> {
    Subscription::run(workspace_stream)
}

fn workspace_stream() -> impl iced::futures::Stream<Item = Message> {
    let (mut sender, receiver) = mpsc::channel(16);

    std::thread::spawn(move || {
        let send_workspaces = |sender: &mut mpsc::Sender<Message>| match fetch_workspaces() {
            Ok(ws) => {
                let info = ws.into_iter().map(to_workspace_info).collect();
                sender
                    .try_send(Message::Workspaces(info))
                    .expect("failed to send workspace info");
            }
            Err(err) => eprintln!("Failed to fetch workspaces: {err}"),
        };

        send_workspaces(&mut sender);

        let mut stream = match subscribe_workspace_events() {
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

#[cfg(test)]
use std::sync::atomic::{AtomicUsize, Ordering};
#[cfg(test)]
use std::sync::{Mutex, OnceLock};

use crate::app::Message;

#[cfg(test)]
static LOG: OnceLock<Mutex<Vec<(usize, &'static str)>>> = OnceLock::new();

#[cfg(test)]
fn log_call(id: usize, name: &'static str) {
    let log = LOG.get_or_init(|| Mutex::new(Vec::new()));
    log.lock().unwrap().push((id, name));
}

#[cfg(test)]
fn take_log() -> Vec<(usize, &'static str)> {
    let log = LOG.get_or_init(|| Mutex::new(Vec::new()));
    let mut lock = log.lock().unwrap();
    let out = lock.clone();
    lock.clear();
    out
}

#[cfg(test)]
#[derive(Debug)]
struct FakeConnection {
    id: usize,
}

#[cfg(test)]
impl FakeConnection {
    fn new() -> Result<Self, Error> {
        static NEXT: AtomicUsize = AtomicUsize::new(0);
        Ok(Self {
            id: NEXT.fetch_add(1, Ordering::SeqCst),
        })
    }

    fn get_workspaces(&mut self) -> Result<Vec<Workspace>, Error> {
        log_call(self.id, "get_workspaces");
        Ok(Vec::new())
    }

    fn run_command<T: AsRef<str>>(&mut self, _payload: T) -> Result<Vec<Result<(), Error>>, Error> {
        log_call(self.id, "run_command");
        Ok(vec![Ok(())])
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn reuses_single_connection_for_fetch_and_focus() {
        // Ensure clean log before starting.
        let _ = take_log();

        fetch_workspaces().expect("fetch succeeds");
        focus_workspace("1").expect("focus succeeds");

        let log = take_log();
        assert!(
            !log.is_empty(),
            "expected calls to be recorded; got empty log"
        );

        let ids: Vec<usize> = log.iter().map(|(id, _)| *id).collect();
        assert!(
            ids.windows(2).all(|w| w[0] == w[1]),
            "expected same connection id, got {ids:?}"
        );

        let calls: Vec<&str> = log.iter().map(|(_, name)| *name).collect();
        assert!(
            calls.contains(&"get_workspaces") && calls.contains(&"run_command"),
            "expected both fetch and focus calls; got {calls:?}"
        );
    }
}
