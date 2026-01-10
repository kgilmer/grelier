// Load/save settings in an Xresources-style file under the grelier config directory.
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct SettingsStorage {
    path: PathBuf,
}

impl SettingsStorage {
    pub fn new(path: PathBuf) -> Self {
        Self { path }
    }

    pub fn default_path() -> PathBuf {
        let mut path = match std::env::var_os("HOME") {
            Some(home) => PathBuf::from(home),
            None => PathBuf::from("."),
        };
        path.push(".config");
        path.push("grelier");
        path.push("Settings.xresources");
        path
    }

    pub fn load(&self) -> Result<HashMap<String, String>, String> {
        let file = match fs::File::open(&self.path) {
            Ok(file) => file,
            Err(err) if err.kind() == std::io::ErrorKind::NotFound => {
                return Ok(HashMap::new());
            }
            Err(err) => {
                return Err(format!(
                    "unable to open settings storage {}: {err}",
                    self.path.display()
                ));
            }
        };
        let reader = BufReader::new(file);
        let mut map = HashMap::new();

        for (index, line) in reader.lines().enumerate() {
            let line = line.map_err(|err| {
                format!(
                    "unable to read settings storage {}: {err}",
                    self.path.display()
                )
            })?;
            if let Some((key, value)) = parse_line(&line, index + 1)? {
                map.insert(key, value);
            }
        }

        Ok(map)
    }

    pub fn save(&self, map: &HashMap<String, String>) -> Result<(), String> {
        if let Some(parent) = self.path.parent() {
            fs::create_dir_all(parent).map_err(|err| {
                format!(
                    "unable to create settings directory {}: {err}",
                    parent.display()
                )
            })?;
        }

        let mut file = fs::File::create(&self.path).map_err(|err| {
            format!(
                "unable to open settings storage {}: {err}",
                self.path.display()
            )
        })?;

        let mut keys: Vec<&String> = map.keys().collect();
        keys.sort();
        for key in keys {
            if let Some(value) = map.get(key) {
                writeln!(file, "{key}: {value}").map_err(|err| {
                    format!(
                        "unable to write settings storage {}: {err}",
                        self.path.display()
                    )
                })?;
            }
        }

        Ok(())
    }
}

fn parse_line(line: &str, line_number: usize) -> Result<Option<(String, String)>, String> {
    let trimmed = line.trim();
    if trimmed.is_empty() || trimmed.starts_with('!') || trimmed.starts_with('#') {
        return Ok(None);
    }

    let sep_index = trimmed
        .find([':', '='])
        .ok_or_else(|| format!("line {line_number}: missing ':' or '=' separator"))?;
    let (key, value) = trimmed.split_at(sep_index);
    let key = key.trim();
    let value = value[1..].trim();

    if key.is_empty() {
        return Err(format!("line {line_number}: empty key"));
    }

    Ok(Some((key.to_string(), value.to_string())))
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::fs;

    fn temp_storage(name: &str) -> (SettingsStorage, PathBuf) {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "grelier_settings_test_{}_{}",
            name,
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create temp test dir");
        let mut file_path = dir.clone();
        file_path.push("Settings.xresources");
        (SettingsStorage::new(file_path), dir)
    }

    #[test]
    fn load_missing_file_returns_empty_map() {
        let (storage, dir) = temp_storage("missing");
        let _ = fs::remove_file(storage.path.clone());

        let map = storage.load().expect("load missing file");
        assert!(map.is_empty());

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn save_sorts_by_key() {
        let (storage, dir) = temp_storage("sorted");
        let mut map = HashMap::new();
        map.insert("grelier.zeta".to_string(), "last".to_string());
        map.insert("grelier.alpha".to_string(), "first".to_string());

        storage.save(&map).expect("save settings");
        let contents = fs::read_to_string(storage.path).expect("read settings storage");

        assert_eq!(contents, "grelier.alpha: first\ngrelier.zeta: last\n");

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn load_parses_basic_lines() {
        let (storage, dir) = temp_storage("parse");
        let data = "\n! comment\n# comment\nkey.one: value\nkey.two=other\n";
        fs::write(&storage.path, data).expect("write settings storage");

        let map = storage.load().expect("load parsed settings");
        assert_eq!(map.get("key.one"), Some(&"value".to_string()));
        assert_eq!(map.get("key.two"), Some(&"other".to_string()));

        let _ = fs::remove_dir_all(dir);
    }
}
