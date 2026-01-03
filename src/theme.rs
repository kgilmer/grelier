use std::sync::Arc;

use iced::{
    Color, Theme,
    theme::{Custom, Palette},
};

pub const DEFAULT_THEME: Theme = Theme::Nord;

pub fn parse_them(name: &str) -> Option<Theme> {
    match name {
        "CatppuccinFrappe" => Some(Theme::CatppuccinFrappe),
        "CatppuccinLatte" => Some(Theme::CatppuccinLatte),
        "CatppuccinMacchiato" => Some(Theme::CatppuccinMacchiato),
        "CatppuccinMocha" => Some(Theme::CatppuccinMocha),
        "Dark" => Some(Theme::Dark),
        "Dracula" => Some(Theme::Dracula),
        "Ferra" => Some(Theme::Ferra),
        "GruvboxDark" => Some(Theme::GruvboxDark),
        "GruvboxLight" => Some(Theme::GruvboxLight),
        "KanagawaDragon" => Some(Theme::KanagawaDragon),
        "KanagawaLotus" => Some(Theme::KanagawaLotus),
        "KanagawaWave" => Some(Theme::KanagawaWave),
        "Light" => Some(Theme::Light),
        "Moonfly" => Some(Theme::Moonfly),
        "Nightfly" => Some(Theme::Nightfly),
        "Nord" => Some(Theme::Nord),
        "Oxocarbon" => Some(Theme::Oxocarbon),
        "TokyoNight" => Some(Theme::TokyoNight),
        "TokyoNightLight" => Some(Theme::TokyoNightLight),
        "TokyoNightStorm" => Some(Theme::TokyoNightStorm),
        "AyuMirage" => Some(Theme::Custom(Arc::new(Custom::new(
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
