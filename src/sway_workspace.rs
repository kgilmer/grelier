use std::cell::{RefCell, RefMut};

use swayipc::{Connection, Error, EventStream, EventType, Workspace};

thread_local! {
    static COMMAND_CONN: RefCell<Option<Connection>> = RefCell::new(None);
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
        let _ = conn.run_command(cmd)?;
        Ok(())
    })
}

fn with_command_conn<R>(f: impl FnOnce(&mut Connection) -> Result<R, Error>) -> Result<R, Error> {
    COMMAND_CONN.with(|cell| {
        if cell.borrow().is_none() {
            *cell.borrow_mut() = Some(Connection::new()?);
        }

        // Connection is guaranteed to exist after initialization above.
        let mut_ref = cell.borrow_mut();
        // RefCell borrow is short-lived; reborrow to satisfy the borrow checker.
        let mut mut_ref = RefMut::map(mut_ref, |opt| opt.as_mut().unwrap());
        f(&mut *mut_ref)
    })
}
