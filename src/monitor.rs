use crate::bar::OutputSnapshot;
use crate::sway_workspace;
use log::error;
use std::collections::HashSet;

pub fn normalize_monitor_selection(raw: Option<&str>) -> Result<Option<String>, String> {
    let Some(raw) = raw else {
        return Ok(None);
    };

    let monitor_name = raw.trim();
    if monitor_name.is_empty() {
        return Err("--on-monitor requires a monitor name.".to_string());
    }
    if monitor_name.contains(',') {
        return Err(
            "--on-monitor accepts exactly one monitor name. Use --list-monitors to inspect names."
                .to_string(),
        );
    }
    let monitor_name = monitor_name.to_string();

    let outputs =
        sway_workspace::fetch_outputs().map_err(|err| format!("Failed to query outputs: {err}"))?;
    let known: HashSet<String> = outputs.into_iter().map(|output| output.name).collect();

    if !known.contains(&monitor_name) {
        return Err(format!(
            "Unknown monitor '{}'. Known monitors: {}",
            monitor_name,
            known
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    Ok(Some(monitor_name))
}

pub fn list_monitors() -> Result<(), String> {
    let outputs =
        sway_workspace::fetch_outputs().map_err(|err| format!("Failed to query outputs: {err}"))?;
    if outputs.is_empty() {
        println!("No outputs detected.");
        return Ok(());
    }

    for output in outputs {
        let status = if output.active { "active" } else { "inactive" };
        let make_model = format!("{} {}", output.make, output.model)
            .trim()
            .to_string();
        if make_model.trim().is_empty() {
            println!("{}\t{}", output.name, status);
        } else {
            println!("{}\t{}\t{}", output.name, status, make_model.trim());
        }
    }

    Ok(())
}

pub fn snapshot_outputs() -> Option<Vec<OutputSnapshot>> {
    match sway_workspace::fetch_outputs() {
        Ok(outputs) => Some(
            outputs
                .into_iter()
                .map(|output| OutputSnapshot {
                    name: output.name,
                    active: output.active,
                    rect: (
                        output.rect.x,
                        output.rect.y,
                        output.rect.width,
                        output.rect.height,
                    ),
                })
                .collect(),
        ),
        Err(err) => {
            error!("Failed to query outputs for snapshot: {err}");
            None
        }
    }
}

pub fn has_active_outputs(snapshot: &[OutputSnapshot]) -> bool {
    snapshot.iter().any(|output| output.active)
}

pub fn outputs_equal(a: &[OutputSnapshot], b: &[OutputSnapshot]) -> bool {
    if a.len() != b.len() {
        return false;
    }
    let mut left = a.to_vec();
    let mut right = b.to_vec();
    left.sort_by(|lhs, rhs| lhs.name.cmp(&rhs.name));
    right.sort_by(|lhs, rhs| lhs.name.cmp(&rhs.name));
    left == right
}
