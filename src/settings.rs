// Global settings store with parsing helpers and runtime updates persisted to storage.
use std::collections::HashMap;
use std::str::FromStr;
use std::sync::{OnceLock, RwLock};

use crate::gauge::SettingSpec;
use crate::settings_storage::SettingsStorage;

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

    for pair in split_settings_pairs(trimmed)? {
        let pair = pair.trim();
        if pair.is_empty() {
            continue;
        }
        let sep_index = pair
            .find(['=', ':'])
            .ok_or_else(|| format!("missing '=' or ':' in setting '{pair}'"))?;
        let (key, value) = pair.split_at(sep_index);
        let value = &value[1..];
        if key.is_empty() {
            return Err(format!("missing key in setting '{pair}'"));
        }
        if key.chars().any(|c| c.is_whitespace()) {
            return Err(format!("setting key '{key}' cannot contain whitespace"));
        }
        let key = key.to_string();
        if map.contains_key(&key) {
            return Err(format!("duplicate setting key '{key}'"));
        }
        let parsed_value = parse_value(value.trim())?;
        map.insert(key, parsed_value);
    }

    Ok(map)
}

fn split_settings_pairs(input: &str) -> Result<Vec<String>, String> {
    let mut parts = Vec::new();
    let mut current = String::new();
    let mut in_quotes = false;
    let mut escaped = false;
    let chars: Vec<char> = input.chars().collect();

    for (idx, ch) in chars.iter().copied().enumerate() {
        if escaped {
            current.push(ch);
            escaped = false;
            continue;
        }
        match ch {
            '\\' => {
                escaped = true;
            }
            '"' => {
                in_quotes = !in_quotes;
                current.push(ch);
            }
            ',' if !in_quotes => {
                // Only treat as a separator if the remainder contains another key/value separator.
                let rest = &chars[idx + 1..];
                let has_more_pairs = rest.iter().any(|c| *c == '=' || *c == ':');
                if has_more_pairs {
                    parts.push(current);
                    current = String::new();
                } else {
                    current.push(ch);
                }
            }
            _ => current.push(ch),
        }
    }

    if in_quotes {
        return Err("unterminated quote in settings string".to_string());
    }
    if escaped {
        return Err("dangling escape at end of settings string".to_string());
    }

    parts.push(current);
    Ok(parts)
}

fn parse_value(raw: &str) -> Result<String, String> {
    let trimmed = raw.trim();
    if trimmed.starts_with('"') && trimmed.ends_with('"') && trimmed.len() >= 2 {
        let inner = &trimmed[1..trimmed.len() - 1];
        return Ok(unescape_quoted(inner));
    }
    Ok(trimmed.to_string())
}

fn unescape_quoted(value: &str) -> String {
    let mut result = String::new();
    let mut escaped = false;
    for ch in value.chars() {
        if escaped {
            result.push(ch);
            escaped = false;
            continue;
        }
        if ch == '\\' {
            escaped = true;
        } else {
            result.push(ch);
        }
    }
    if escaped {
        result.push('\\');
    }
    result
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
        path.push("Settings.xresources");
        SettingsStorage::new(path)
    }

    #[test]
    fn parse_settings_rejects_missing_separator() {
        let err = parse_settings_arg("grelier.theme").unwrap_err();
        assert!(err.contains("missing '=' or ':'"));
    }

    #[test]
    fn parse_settings_rejects_empty_key() {
        let err = parse_settings_arg("=value").unwrap_err();
        assert!(err.contains("missing key"));
    }

    #[test]
    fn parse_settings_rejects_whitespace_key() {
        let err = parse_settings_arg("grelier.theme name=Light").unwrap_err();
        assert!(err.contains("cannot contain whitespace"));
    }

    #[test]
    fn parse_settings_rejects_duplicate_key() {
        let err = parse_settings_arg("grelier.theme=Dark,grelier.theme=Light").unwrap_err();
        assert!(err.contains("duplicate setting key"));
    }

    #[test]
    fn parse_settings_accepts_empty_string() {
        let map = parse_settings_arg("").expect("empty settings should parse");
        assert!(map.is_empty());
    }

    #[test]
    fn parse_settings_accepts_quoted_value_with_commas() {
        let map =
            parse_settings_arg("grelier.gauges:\"test_gauge,clock\"").expect("parse quoted comma");
        assert_eq!(
            map.get("grelier.gauges").cloned(),
            Some("test_gauge,clock".to_string())
        );
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
    fn parse_settings_handles_multiple_pairs() {
        let map = parse_settings_arg("grelier.gauges:\"test_gauge,clock\",grelier.theme=Light")
            .expect("parse two pairs");
        assert_eq!(
            map.get("grelier.gauges").cloned(),
            Some("test_gauge,clock".to_string())
        );
        assert_eq!(map.get("grelier.theme").cloned(), Some("Light".to_string()));
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
