use iced::widget::svg;
use std::path::Path;

/// Absolute path to the bundled asset directory (e.g. SVG icons).
pub const ASSETS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");

/// Build an `iced` SVG handle for a file under `assets/`.
pub fn svg_asset(name: &str) -> svg::Handle {
    let path = Path::new(ASSETS_DIR).join(name);
    svg::Handle::from_path(path)
}
