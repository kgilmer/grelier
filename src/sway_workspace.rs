use swayipc::{Connection, Error, EventStream, EventType, Workspace};

/// Fetch and sort the current Sway workspaces.
pub fn fetch_workspaces() -> Result<Vec<Workspace>, Error> {
    let mut conn = Connection::new()?;
    let mut workspaces = conn.get_workspaces()?;
    workspaces.sort_by(|a, b| (a.num, &a.name).cmp(&(b.num, &b.name)));
    Ok(workspaces)
}

/// Subscribe to workspace-related events.
pub fn subscribe_workspace_events() -> Result<EventStream, Error> {
    Connection::new()?.subscribe([EventType::Workspace])
}
