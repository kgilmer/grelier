// Theme parsing and custom palette definitions for the UI.
use std::sync::Arc;

use iced::{
    Color, Theme,
    theme::{Custom, Palette},
};

use crate::settings::Settings;

pub const DEFAULT_THEME: Theme = Theme::Nord;
pub const VALID_THEME_NAMES: &[&str] = &[
    "CatppuccinFrappe",
    "CatppuccinLatte",
    "CatppuccinMacchiato",
    "CatppuccinMocha",
    "Dark",
    "Dracula",
    "Ferra",
    "GruvboxDark",
    "GruvboxLight",
    "KanagawaDragon",
    "KanagawaLotus",
    "KanagawaWave",
    "Light",
    "Moonfly",
    "Nightfly",
    "Nord",
    "Oxocarbon",
    "TokyoNight",
    "TokyoNightLight",
    "TokyoNightStorm",
    "AyuMirage",
    "Custom",
];

pub const CUSTOM_THEME_NAME: &str = "Custom";
pub const CUSTOM_THEME_SETTING_KEYS: [&str; 6] = [
    "grelier.bar.theme.background",
    "grelier.bar.theme.text",
    "grelier.bar.theme.primary",
    "grelier.bar.theme.success",
    "grelier.bar.theme.warning",
    "grelier.bar.theme.danger",
];

pub fn list_themes() {
    for name in VALID_THEME_NAMES {
        println!("{name}");
    }
}

pub fn is_custom_theme_name(name: &str) -> bool {
    name.trim().eq_ignore_ascii_case(CUSTOM_THEME_NAME)
}

pub fn custom_theme_from_settings(settings: &Settings) -> Result<Theme, String> {
    let mut missing = Vec::new();
    let mut values: [Option<String>; 6] = [None, None, None, None, None, None];

    for (index, key) in CUSTOM_THEME_SETTING_KEYS.iter().enumerate() {
        match settings.get(key) {
            Some(value) if !value.trim().is_empty() => values[index] = Some(value),
            _ => missing.push(*key),
        }
    }

    if !missing.is_empty() {
        return Err(format!(
            "Custom theme requires settings for: {}",
            missing.join(", ")
        ));
    }

    let background = parse_color_setting(
        CUSTOM_THEME_SETTING_KEYS[0],
        values[0].as_deref().unwrap_or(""),
    )?;
    let text = parse_color_setting(
        CUSTOM_THEME_SETTING_KEYS[1],
        values[1].as_deref().unwrap_or(""),
    )?;
    let primary = parse_color_setting(
        CUSTOM_THEME_SETTING_KEYS[2],
        values[2].as_deref().unwrap_or(""),
    )?;
    let success = parse_color_setting(
        CUSTOM_THEME_SETTING_KEYS[3],
        values[3].as_deref().unwrap_or(""),
    )?;
    let warning = parse_color_setting(
        CUSTOM_THEME_SETTING_KEYS[4],
        values[4].as_deref().unwrap_or(""),
    )?;
    let danger = parse_color_setting(
        CUSTOM_THEME_SETTING_KEYS[5],
        values[5].as_deref().unwrap_or(""),
    )?;

    Ok(Theme::Custom(Arc::new(Custom::new(
        CUSTOM_THEME_NAME.to_string(),
        Palette {
            background,
            text,
            primary,
            success,
            warning,
            danger,
        },
    ))))
}

pub fn parse_them(name: &str) -> Option<Theme> {
    match name.trim().to_ascii_lowercase().as_str() {
        "catppuccinfrappe" => Some(Theme::CatppuccinFrappe),
        "catppuccinlatte" => Some(Theme::CatppuccinLatte),
        "catppuccinmacchiato" => Some(Theme::CatppuccinMacchiato),
        "catppuccinmocha" => Some(Theme::CatppuccinMocha),
        "dark" => Some(Theme::Dark),
        "dracula" => Some(Theme::Dracula),
        "ferra" => Some(Theme::Ferra),
        "gruvboxdark" => Some(Theme::GruvboxDark),
        "gruvboxlight" => Some(Theme::GruvboxLight),
        "kanagawadragon" => Some(Theme::KanagawaDragon),
        "kanagawalotus" => Some(Theme::KanagawaLotus),
        "kanagawawave" => Some(Theme::KanagawaWave),
        "light" => Some(Theme::Light),
        "moonfly" => Some(Theme::Moonfly),
        "nightfly" => Some(Theme::Nightfly),
        "nord" => Some(Theme::Nord),
        "oxocarbon" => Some(Theme::Oxocarbon),
        "tokyonight" => Some(Theme::TokyoNight),
        "tokyonightlight" => Some(Theme::TokyoNightLight),
        "tokyonightstorm" => Some(Theme::TokyoNightStorm),
        "ayumirage" => Some(Theme::Custom(Arc::new(Custom::new(
            "AyuMirage".to_string(),
            Palette {
                background: Color::from_rgb8(0x1F, 0x24, 0x30),
                text: Color::from_rgb8(0xCB, 0xCC, 0xC6),
                primary: Color::from_rgb8(0xFF, 0xCC, 0x66),
                success: Color::from_rgb8(0xBA, 0xE6, 0x7E),
                warning: Color::from_rgb8(0xFF, 0xD1, 0x73),
                danger: Color::from_rgb8(0xF2, 0x87, 0x79),
            },
        )))),
        _ => None,
    }
}

fn parse_color_setting(key: &str, value: &str) -> Result<Color, String> {
    parse_hex_color(value).map_err(|err| format!("Invalid setting '{key}': {err}"))
}

fn parse_hex_color(value: &str) -> Result<Color, String> {
    let trimmed = value.trim();
    if trimmed.is_empty() {
        return Err("empty color value".to_string());
    }

    let hex = trimmed
        .strip_prefix('#')
        .or_else(|| trimmed.strip_prefix("0x"))
        .unwrap_or(trimmed);

    if hex.len() != 6 || !hex.chars().all(|c| c.is_ascii_hexdigit()) {
        return Err(format!(
            "expected hex color in RRGGBB format, got '{value}'"
        ));
    }

    let r = u8::from_str_radix(&hex[0..2], 16).map_err(|_| "invalid red channel".to_string())?;
    let g = u8::from_str_radix(&hex[2..4], 16).map_err(|_| "invalid green channel".to_string())?;
    let b = u8::from_str_radix(&hex[4..6], 16).map_err(|_| "invalid blue channel".to_string())?;

    Ok(Color::from_rgb8(r, g, b))
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::settings::Settings;
    use crate::settings_storage::SettingsStorage;
    use std::collections::HashMap;
    use std::fs;
    use std::path::PathBuf;

    fn temp_storage_path(name: &str) -> (SettingsStorage, PathBuf) {
        let mut dir = std::env::temp_dir();
        dir.push(format!(
            "grelier_theme_test_{}_{}",
            name,
            std::process::id()
        ));
        fs::create_dir_all(&dir).expect("create temp dir");
        let mut file_path = dir.clone();
        file_path.push(format!("Settings-{}.xresources", env!("CARGO_PKG_VERSION")));
        (SettingsStorage::new(file_path), dir)
    }

    fn build_settings(map: HashMap<String, String>, name: &str) -> (Settings, PathBuf) {
        let (storage, dir) = temp_storage_path(name);
        storage.save(&map).expect("save settings storage");
        (Settings::new(storage), dir)
    }

    #[test]
    fn custom_theme_requires_all_settings() {
        let mut map = HashMap::new();
        map.insert(
            "grelier.bar.theme.background".to_string(),
            "112233".to_string(),
        );
        let (settings, dir) = build_settings(map, "missing");

        let err = custom_theme_from_settings(&settings).unwrap_err();
        assert!(err.contains("grelier.bar.theme.text"));

        let _ = fs::remove_dir_all(dir);
    }

    #[test]
    fn custom_theme_accepts_valid_hex_colors() {
        let mut map = HashMap::new();
        map.insert(
            "grelier.bar.theme.background".to_string(),
            "#112233".to_string(),
        );
        map.insert("grelier.bar.theme.text".to_string(), "445566".to_string());
        map.insert(
            "grelier.bar.theme.primary".to_string(),
            "0x778899".to_string(),
        );
        map.insert(
            "grelier.bar.theme.success".to_string(),
            "AABBCC".to_string(),
        );
        map.insert(
            "grelier.bar.theme.warning".to_string(),
            "DDEEFF".to_string(),
        );
        map.insert("grelier.bar.theme.danger".to_string(), "010203".to_string());
        let (settings, dir) = build_settings(map, "valid");

        let theme = custom_theme_from_settings(&settings).expect("valid custom theme");
        match theme {
            Theme::Custom(_) => {}
            other => panic!("expected custom theme, got {other:?}"),
        }

        let _ = fs::remove_dir_all(dir);
    }
}
