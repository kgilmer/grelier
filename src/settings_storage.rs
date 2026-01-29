// Load/save settings in an Xresources-style file under the grelier config directory.
// The filename includes the grelier version (Settings-<version>.xresources).
use std::collections::HashMap;
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::PathBuf;

#[derive(Clone, Debug)]
pub struct SettingsStorage {
    path: PathBuf,
}

fn settings_filename() -> String {
    format!("Settings-{}.xresources", env!("CARGO_PKG_VERSION"))
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
        path.push(settings_filename());
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
        let mut pending = String::new();
        let mut pending_line = 0usize;
        let mut continuation = false;

        for (index, line) in reader.lines().enumerate() {
            let line = line.map_err(|err| {
                format!(
                    "unable to read settings storage {}: {err}",
                    self.path.display()
                )
            })?;
            let line_number = index + 1;
            if !continuation && pending.is_empty() {
                let trimmed = line.trim();
                if trimmed.is_empty() || trimmed.starts_with('!') || trimmed.starts_with('#') {
                    continue;
                }
                pending_line = line_number;
            } else if pending.is_empty() {
                pending_line = line_number;
            }

            let mut fragment = line;
            if continuation {
                fragment = fragment.trim_start().to_string();
            }

            let (segment, has_continuation) = split_continuation(&fragment);
            pending.push_str(&segment);
            continuation = has_continuation;

            if continuation {
                continue;
            }

            if let Some((key, value)) = parse_line(&pending, pending_line)? {
                map.insert(key, value);
            }
            pending.clear();
        }

        if continuation {
            return Err(format!(
                "line {pending_line}: trailing line continuation without content"
            ));
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

    let value = unescape_value(value);

    Ok(Some((key.to_string(), value)))
}

fn split_continuation(line: &str) -> (String, bool) {
    let trimmed = line.trim_end();
    let mut trailing_backslashes = 0usize;
    for ch in trimmed.chars().rev() {
        if ch == '\\' {
            trailing_backslashes += 1;
        } else {
            break;
        }
    }

    if trailing_backslashes % 2 == 1 {
        let cutoff = trimmed.len() - 1;
        (trimmed[..cutoff].to_string(), true)
    } else {
        (line.to_string(), false)
    }
}

fn unescape_value(value: &str) -> String {
    let mut output = String::new();
    let mut chars = value.chars().peekable();

    while let Some(ch) = chars.next() {
        if ch != '\\' {
            output.push(ch);
            continue;
        }

        match chars.next() {
            Some('n') => output.push('\n'),
            Some('t') => output.push('\t'),
            Some('r') => output.push('\r'),
            Some('b') => output.push('\x08'),
            Some('f') => output.push('\x0c'),
            Some('\\') => output.push('\\'),
            Some('x') => {
                let mut digits = String::new();
                for _ in 0..2 {
                    if let Some(next) = chars.peek().copied() {
                        if next.is_ascii_hexdigit() {
                            digits.push(next);
                            chars.next();
                        } else {
                            break;
                        }
                    }
                }
                if digits.is_empty() {
                    output.push('x');
                } else if let Ok(value) = u8::from_str_radix(&digits, 16) {
                    output.push(value as char);
                }
            }
            Some(c @ '0'..='7') => {
                let mut digits = String::new();
                digits.push(c);
                for _ in 0..2 {
                    if let Some(next @ '0'..='7') = chars.peek().copied() {
                        digits.push(next);
                        chars.next();
                    } else {
                        break;
                    }
                }
                if let Ok(value) = u8::from_str_radix(&digits, 8) {
                    output.push(value as char);
                }
            }
            Some(other) => output.push(other),
            None => output.push('\\'),
        }
    }

    output
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
        file_path.push(settings_filename());
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

    #[test]
    fn load_parses_line_continuations() {
        let (storage, dir) = temp_storage("continuation");
        let data = "key.one: hello\\\n  world\nkey.two: stay\n";
        fs::write(&storage.path, data).expect("write settings storage");

        let map = storage.load().expect("load continued settings");
        assert_eq!(map.get("key.one"), Some(&"helloworld".to_string()));
        assert_eq!(map.get("key.two"), Some(&"stay".to_string()));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn load_unescapes_values() {
        let (storage, dir) = temp_storage("unescape");
        let data = "key.one: line\\nwrap\nkey.two: tab\\tvalue\nkey.three: \\101\n";
        fs::write(&storage.path, data).expect("write settings storage");

        let map = storage.load().expect("load escaped settings");
        assert_eq!(map.get("key.one"), Some(&"line\nwrap".to_string()));
        assert_eq!(map.get("key.two"), Some(&"tab\tvalue".to_string()));
        assert_eq!(map.get("key.three"), Some(&"A".to_string()));

        let _ = fs::remove_dir_all(dir);
    }
}
