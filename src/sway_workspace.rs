// Sway IPC helpers for workspace state, focus, and subscriptions.
use std::cell::RefCell;

use crate::bar::Message;
use iced::Subscription;
use iced::futures::channel::mpsc;
use swayipc::Event;
use swayipc::{Connection, Error, EventStream, EventType, Node, NodeType, Workspace};

#[cfg(test)]
type SwayConnection = FakeConnection;
#[cfg(not(test))]
type SwayConnection = Connection;

#[derive(Debug, Clone)]
pub struct WorkspaceInfo {
    pub num: i32,
    pub name: String,
    pub focused: bool,
    pub urgent: bool,
    pub rect: Rect,
}

#[derive(Debug, Clone)]
pub struct WorkspaceApps {
    pub name: String,
    pub apps: Vec<WorkspaceApp>,
}

#[derive(Debug, Clone)]
pub struct WorkspaceApp {
    pub app_id: String,
    pub con_id: i64,
}

#[derive(Debug, Clone)]
pub struct Rect {
    pub y: i32,
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

pub fn fetch_workspace_apps() -> Result<Vec<WorkspaceApps>, Error> {
    with_command_conn(|conn| {
        let tree = conn.get_tree()?;
        Ok(workspace_apps(&tree))
    })
}

/// Fetch the current Sway outputs.
pub fn fetch_outputs() -> Result<Vec<swayipc::Output>, Error> {
    with_command_conn(|conn| conn.get_outputs())
}

/// Subscribe to workspace-related events.
pub fn subscribe_workspace_events() -> Result<EventStream, Error> {
    Connection::new()?.subscribe([EventType::Workspace, EventType::Window])
}

/// Focus the workspace with the given name.
pub fn focus_workspace(name: &str) -> Result<(), Error> {
    with_command_conn(|conn| {
        let cmd = format!("workspace \"{}\"", name.replace('"', "\\\""));
        conn.run_command(cmd)?;
        Ok(())
    })
}

/// Focus the container with the given Sway con_id.
pub fn focus_con_id(con_id: i64) -> Result<(), Error> {
    with_command_conn(|conn| {
        let cmd = format!("[con_id={con_id}] focus");
        let _ = conn.run_command(cmd)?;
        Ok(())
    })
}

/// Launch an application using the desktop app id.
pub fn launch_app(app_id: &str) -> Result<(), Error> {
    with_command_conn(|conn| {
        let escaped = app_id.replace('"', "\\\"");
        let cmd = format!("exec gtk-launch \"{escaped}\"");
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
        y: ws.rect.y,
        height: ws.rect.height,
    };

    WorkspaceInfo {
        num: ws.num,
        name: ws.name,
        focused: ws.focused,
        urgent: ws.urgent,
        rect,
    }
}

fn workspace_apps(root: &Node) -> Vec<WorkspaceApps> {
    let mut out = Vec::new();
    collect_workspace_apps(root, &mut out);
    out
}

fn collect_workspace_apps(node: &Node, out: &mut Vec<WorkspaceApps>) {
    if node.node_type == NodeType::Workspace {
        let name = node
            .name
            .clone()
            .or_else(|| node.num.map(|num| num.to_string()))
            .unwrap_or_else(|| "<unnamed>".to_string());
        if name == "__i3_scratch" {
            return;
        }
        let mut apps = Vec::new();
        for child in node.nodes.iter().chain(node.floating_nodes.iter()) {
            collect_app_names(child, &mut apps);
        }
        out.push(WorkspaceApps { name, apps });
    }

    for child in node.nodes.iter().chain(node.floating_nodes.iter()) {
        collect_workspace_apps(child, out);
    }
}

fn collect_app_names(node: &Node, out: &mut Vec<WorkspaceApp>) {
    if let Some(name) = app_name(node) {
        out.push(WorkspaceApp {
            app_id: name,
            con_id: node.id,
        });
    }

    for child in node.nodes.iter().chain(node.floating_nodes.iter()) {
        collect_app_names(child, out);
    }
}

fn app_name(node: &Node) -> Option<String> {
    if let Some(app_id) = &node.app_id {
        return Some(app_id.clone());
    }

    let props = node.window_properties.as_ref()?;
    if let Some(class) = &props.class {
        Some(class.clone())
    } else if let Some(instance) = &props.instance {
        Some(instance.clone())
    } else {
        props.title.clone()
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
                let apps = match fetch_workspace_apps() {
                    Ok(apps) => apps,
                    Err(err) => {
                        eprintln!("Failed to fetch workspace app names: {err}");
                        Vec::new()
                    }
                };
                sender
                    .try_send(Message::Workspaces {
                        workspaces: info,
                        apps,
                    })
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
                Ok(Event::Window(_)) => send_workspaces(&mut sender),
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

    fn get_tree(&mut self) -> Result<swayipc::Node, Error> {
        Ok(empty_node())
    }

    fn get_outputs(&mut self) -> Result<Vec<swayipc::Output>, Error> {
        log_call(self.id, "get_outputs");
        Ok(Vec::new())
    }
}

#[cfg(test)]
fn empty_node() -> swayipc::Node {
    let rect = serde_json::json!({
        "x": 0,
        "y": 0,
        "width": 0,
        "height": 0
    });

    serde_json::from_value(serde_json::json!({
        "id": 0,
        "name": null,
        "type": "root",
        "border": "none",
        "current_border_width": 0,
        "layout": "splith",
        "orientation": "none",
        "percent": null,
        "rect": rect,
        "window_rect": rect,
        "deco_rect": rect,
        "geometry": rect,
        "urgent": false,
        "focused": false,
        "focus": [],
        "floating": null,
        "nodes": [],
        "floating_nodes": [],
        "sticky": false,
        "representation": null,
        "fullscreen_mode": null,
        "scratchpad_state": null,
        "app_id": null,
        "pid": null,
        "window": null,
        "num": null,
        "window_properties": null,
        "marks": [],
        "inhibit_idle": null,
        "idle_inhibitors": null,
        "sandbox_engine": null,
        "sandbox_app_id": null,
        "sandbox_instance_id": null,
        "tag": null,
        "shell": null,
        "foreign_toplevel_identifier": null,
        "visible": null,
        "output": null
    }))
    .expect("empty swayipc node should deserialize")
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
