// Theme parsing and custom palette definitions for the UI.
use std::sync::Arc;

use iced::{
    Color, Theme,
    theme::{Custom, Palette},
};

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
];

pub fn list_themes() {
    for name in VALID_THEME_NAMES {
        println!("{name}");
    }
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
