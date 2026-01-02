use swayipc::Connection;

fn main() -> Result<(), Box<dyn std::error::Error>> {
    let mut conn = match Connection::new() {
        Ok(conn) => conn,
        Err(err) => {
            eprintln!("Failed to connect to sway IPC: {err}");
            return Ok(());
        }
    };

    let mut workspaces = match conn.get_workspaces() {
        Ok(list) => list,
        Err(err) => {
            eprintln!("Failed to fetch workspaces: {err}");
            return Ok(());
        }
    };

    if workspaces.is_empty() {
        println!("No workspaces reported.");
        return Ok(());
    }

    workspaces.sort_by(|a, b| (a.num, &a.name).cmp(&(b.num, &b.name)));

    println!("Workspaces:");
    for ws in workspaces {
        let mut state = Vec::new();
        if ws.focused {
            state.push("focused");
        }
        if ws.visible {
            state.push("visible");
        }
        if ws.urgent {
            state.push("urgent");
        }
        let state_str = if state.is_empty() { "idle".to_string() } else { state.join(", ") };
        println!(
            "- {} (num: {}, id: {}, output: {}, state: {}, layout: {})",
            ws.name,
            ws.num,
            ws.id,
            ws.output,
            state_str,
            ws.layout
        );
    }

    Ok(())
}
