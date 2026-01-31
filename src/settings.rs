// Global settings store with parsing helpers and runtime updates persisted to storage.
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{OnceLock, RwLock};

use crate::settings_storage::SettingsStorage;

/// Static settings metadata for defaults and help output.
#[derive(Debug, Clone, Copy)]
pub struct SettingSpec {
    pub key: &'static str,
    pub default: &'static str,
}

pub const NO_SETTINGS: &[SettingSpec] = &[];

/// Base settings shared by the bar regardless of which gauges are enabled.
pub fn base_setting_specs(
    default_gauges: &'static str,
    default_panels: &'static str,
    default_orientation: &'static str,
    default_theme: &'static str,
) -> Vec<SettingSpec> {
    vec![
        SettingSpec {
            key: "grelier.gauges",
            default: default_gauges,
        },
        SettingSpec {
            key: "grelier.panels",
            default: default_panels,
        },
        SettingSpec {
            key: "grelier.bar.orientation",
            default: default_orientation,
        },
        SettingSpec {
            key: "grelier.bar.theme",
            default: default_theme,
        },
        SettingSpec {
            key: "grelier.bar.width",
            default: "28",
        },
        SettingSpec {
            key: "grelier.bar.border.blend",
            default: "true",
        },
        SettingSpec {
            key: "grelier.bar.border.line_width",
            default: "1.0",
        },
        SettingSpec {
            key: "grelier.bar.border.column_width",
            default: "3.0",
        },
        SettingSpec {
            key: "grelier.bar.border.mix_1",
            default: "0.2",
        },
        SettingSpec {
            key: "grelier.bar.border.mix_2",
            default: "0.6",
        },
        SettingSpec {
            key: "grelier.bar.border.mix_3",
            default: "1.0",
        },
        SettingSpec {
            key: "grelier.bar.border.alpha_1",
            default: "0.6",
        },
        SettingSpec {
            key: "grelier.bar.border.alpha_2",
            default: "0.7",
        },
        SettingSpec {
            key: "grelier.bar.border.alpha_3",
            default: "0.9",
        },
        SettingSpec {
            key: "grelier.dialog.header.font_size",
            default: "14",
        },
        SettingSpec {
            key: "grelier.dialog.title_align",
            default: "center",
        },
        SettingSpec {
            key: "grelier.dialog.header.bottom_spacing",
            default: "4",
        },
        SettingSpec {
            key: "grelier.dialog.container.padding_y",
            default: "10",
        },
        SettingSpec {
            key: "grelier.dialog.container.padding_x",
            default: "10",
        },
        SettingSpec {
            key: "grelier.gauge.ui.anchor_offset_icon",
            default: "7.0",
        },
        SettingSpec {
            key: "grelier.app.workspace.padding_x",
            default: "4",
        },
        SettingSpec {
            key: "grelier.app.workspace.padding_y",
            default: "2",
        },
        SettingSpec {
            key: "grelier.app.workspace.spacing",
            default: "2",
        },
        SettingSpec {
            key: "grelier.app.workspace.button_padding_x",
            default: "4",
        },
        SettingSpec {
            key: "grelier.app.workspace.button_padding_y",
            default: "4",
        },
        SettingSpec {
            key: "grelier.app.workspace.corner_radius",
            default: "5.0",
        },
        SettingSpec {
            key: "grelier.app.workspace.label_size",
            default: "14",
        },
        SettingSpec {
            key: "grelier.app.workspace.icon_size",
            default: "22.0",
        },
        SettingSpec {
            key: "grelier.app.workspace.icon_spacing",
            default: "6",
        },
        SettingSpec {
            key: "grelier.app.workspace.icon_padding_x",
            default: "2",
        },
        SettingSpec {
            key: "grelier.app.workspace.icon_padding_y",
            default: "2",
        },
        SettingSpec {
            key: "grelier.app.workspace.app_icons",
            default: "true",
        },
        SettingSpec {
            key: "grelier.app.top_apps.count",
            default: "6",
        },
        SettingSpec {
            key: "grelier.app.top_apps.icon_size",
            default: "20.0",
        },
        SettingSpec {
            key: "grelier.gauge.ui.padding_x",
            default: "2",
        },
        SettingSpec {
            key: "grelier.gauge.ui.padding_y",
            default: "2",
        },
        SettingSpec {
            key: "grelier.gauge.ui.spacing",
            default: "7",
        },
        SettingSpec {
            key: "grelier.gauge.ui.icon_size",
            default: "20.0",
        },
        SettingSpec {
            key: "grelier.gauge.ui.value_icon_size",
            default: "20.0",
        },
        SettingSpec {
            key: "grelier.gauge.ui.icon_value_spacing",
            default: "0.0",
        },
    ]
}

#[derive(Debug)]
pub struct Settings {
    map: RwLock<HashMap<String, String>>,
    storage: SettingsStorage,
}

impl Settings {
    pub fn new(storage: SettingsStorage) -> Self {
        let map = match storage.load() {
            Ok(map) => map,
            Err(err) => {
                eprintln!("Failed to load settings storage: {err}");
                HashMap::new()
            }
        };
        Self {
            map: RwLock::new(map),
            storage,
        }
    }

    pub fn get(&self, key: &str) -> Option<String> {
        self.map
            .read()
            .expect("settings read lock poisoned")
            .get(key)
            .cloned()
    }

    pub fn get_or(&self, key: &str, default: &str) -> String {
        self.get(key).unwrap_or_else(|| default.to_string())
    }

    pub fn get_parsed<T: FromStr>(&self, key: &str) -> Option<T> {
        let value = self.get(key)?;
        Some(parse_or_exit::<T>(key, &value))
    }

    pub fn get_parsed_or<T: FromStr>(&self, key: &str, default: T) -> T {
        self.get_parsed(key).unwrap_or(default)
    }

    pub fn get_bool(&self, key: &str) -> Option<bool> {
        let value = self.get(key)?;
        Some(parse_or_exit::<bool>(key, &value))
    }

    pub fn get_bool_or(&self, key: &str, default: bool) -> bool {
        self.get_bool(key).unwrap_or(default)
    }

    pub fn update(&self, key: &str, value: &str) {
        let mut map = self.map.write().expect("settings write lock poisoned");
        if map.get(key).is_some_and(|current| current == value) {
            return;
        }
        map.insert(key.to_string(), value.to_string());
        let storage = self.storage.clone();
        let snapshot = map.clone();
        drop(map);
        if let Err(err) = storage.save(&snapshot) {
            eprintln!("Failed to save settings storage: {err}");
        }
    }

    pub fn ensure_defaults(&self, specs: &[SettingSpec]) {
        let mut map = self.map.write().expect("settings write lock poisoned");
        for spec in specs {
            map.entry(spec.key.to_string())
                .or_insert_with(|| spec.default.to_string());
        }
        let storage = self.storage.clone();
        let snapshot = map.clone();
        drop(map);
        if let Err(err) = storage.save(&snapshot) {
            eprintln!("Failed to save settings storage: {err}");
        }
    }
}

static SETTINGS: OnceLock<Settings> = OnceLock::new();

pub fn init_settings(settings: Settings) -> &'static Settings {
    SETTINGS
        .set(settings)
        .expect("settings initialized more than once");
    SETTINGS.get().expect("settings just initialized")
}

pub fn settings() -> &'static Settings {
    SETTINGS.get().expect("settings not initialized")
}

pub fn parse_settings_arg(arg: &str) -> Result<HashMap<String, String>, String> {
    let mut map = HashMap::new();
    let trimmed = arg.trim();
    if trimmed.is_empty() {
        return Ok(map);
    }
    let sep_index = trimmed
        .find(['=', ':'])
        .ok_or_else(|| format!("missing '=' or ':' in setting '{trimmed}'"))?;
    let (key, value) = trimmed.split_at(sep_index);
    let value = &value[1..];
    if key.is_empty() {
        return Err(format!("missing key in setting '{trimmed}'"));
    }
    if key.chars().any(|c| c.is_whitespace()) {
        return Err(format!("setting key '{key}' cannot contain whitespace"));
    }
    map.insert(key.to_string(), value.trim().to_string());

    Ok(map)
}

fn parse_or_exit<T: FromStr>(key: &str, value: &str) -> T {
    value.parse::<T>().unwrap_or_else(|_| {
        panic!(
            "Invalid setting '{key}': cannot parse '{value}' as {}",
            std::any::type_name::<T>()
        )
    })
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::panic;

    fn temp_storage_path(name: &str) -> SettingsStorage {
        let mut path = std::env::temp_dir();
        path.push(format!("grelier_settings_test_{}", name));
        path.push(format!("Settings-{}.xresources", env!("CARGO_PKG_VERSION")));
        SettingsStorage::new(path)
    }

    #[test]
    fn parse_settings_rejects_missing_separator() {
        let err = parse_settings_arg("grelier.bar.theme").unwrap_err();
        assert!(err.contains("missing '=' or ':'"));
    }

    #[test]
    fn parse_settings_rejects_empty_key() {
        let err = parse_settings_arg("=value").unwrap_err();
        assert!(err.contains("missing key"));
    }

    #[test]
    fn parse_settings_rejects_whitespace_key() {
        let err = parse_settings_arg("grelier.bar.theme name=Light").unwrap_err();
        assert!(err.contains("cannot contain whitespace"));
    }

    #[test]
    fn parse_settings_accepts_empty_string() {
        let map = parse_settings_arg("").expect("empty settings should parse");
        assert!(map.is_empty());
    }

    #[test]
    fn parse_settings_accepts_unquoted_value_with_commas() {
        let map = parse_settings_arg("grelier.gauges:test_gauge,clock")
            .expect("parse unquoted comma value");
        assert_eq!(
            map.get("grelier.gauges").cloned(),
            Some("test_gauge,clock".to_string())
        );
    }

    #[test]
    fn parse_settings_keeps_commas_in_value() {
        let map = parse_settings_arg("grelier.bar.theme=Dark,grelier.bar.theme=Light")
            .expect("parse comma-containing value");
        assert_eq!(
            map.get("grelier.bar.theme").cloned(),
            Some("Dark,grelier.bar.theme=Light".to_string())
        );
    }

    #[test]
    fn get_parsed_panics_on_invalid_value() {
        let storage = temp_storage_path("parse_invalid");
        let mut map = HashMap::new();
        map.insert("grelier.window.width".to_string(), "nope".to_string());
        storage.save(&map).expect("save settings storage");
        let settings = Settings::new(storage);

        let result = panic::catch_unwind(|| {
            let _ = settings.get_parsed::<u32>("grelier.window.width");
        });

        assert!(result.is_err());
    }
}
