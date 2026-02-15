use crate::bar::OutputSnapshot;
use crate::sway_workspace;
use log::error;
use std::collections::HashSet;
use std::ffi::OsString;
use std::path::Path;
use std::process::Command;

pub fn parse_monitor_list(raw: &str) -> Vec<String> {
    raw.split(',')
        .map(str::trim)
        .filter(|name| !name.is_empty())
        .map(|name| name.to_string())
        .collect()
}

pub fn normalize_monitor_selection(raw: Option<&str>) -> Result<Vec<String>, String> {
    let Some(raw) = raw else {
        return Ok(Vec::new());
    };

    let mut monitor_names = parse_monitor_list(raw);
    if monitor_names.is_empty() {
        return Err("--on-monitors requires at least one monitor name.".to_string());
    }

    let outputs =
        sway_workspace::fetch_outputs().map_err(|err| format!("Failed to query outputs: {err}"))?;
    let known: HashSet<String> = outputs.into_iter().map(|output| output.name).collect();

    monitor_names.retain(|name| !name.is_empty());
    let mut seen = HashSet::new();
    let mut unique = Vec::new();
    for name in monitor_names {
        if seen.insert(name.clone()) {
            unique.push(name);
        }
    }

    let unknown: Vec<String> = unique
        .iter()
        .filter(|name| !known.contains(*name))
        .cloned()
        .collect();
    if !unknown.is_empty() {
        return Err(format!(
            "Unknown monitor(s): {}. Known monitors: {}",
            unknown.join(", "),
            known
                .iter()
                .map(String::as_str)
                .collect::<Vec<_>>()
                .join(", ")
        ));
    }

    Ok(unique)
}

pub fn spawn_per_monitor(
    exe: &Path,
    forward_args: &[OsString],
    monitor_names: &[String],
) -> Result<(), String> {
    for name in monitor_names {
        let mut cmd = Command::new(exe);
        cmd.args(forward_args);
        cmd.arg(format!("--on-monitors={name}"));
        cmd.spawn()
            .map_err(|err| format!("Failed to launch for monitor '{name}': {err}"))?;
    }
    Ok(())
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
