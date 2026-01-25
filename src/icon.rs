// SVG asset helpers and quantity icon selection for gauges.
use iced::widget::svg;
use std::path::Path;

/// Absolute path to the bundled asset directory (e.g. SVG icons).
pub const ASSETS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");

/// Build an `iced` SVG handle for a file under `assets/`.
pub fn svg_asset(name: &str) -> svg::Handle {
    let path = Path::new(ASSETS_DIR).join(name);
    svg::Handle::from_path(path)
}

/// Returns the appropriate handle to the SVG representing the quantity `value`.
/// `value` must be a number between 0 and 1.  0 indicates "no quantity" and 1 indicates "full quantity".
/// ratio-0.svg through ratio-7.svg are the icons returned.
pub fn icon_quantity(value: f32) -> svg::Handle {
    let clamped = value.clamp(0.0, 1.0);
    let index = (clamped * 7.0).round() as u8;
    let icon_name = format!("ratio-{index}.svg");

    svg_asset(&icon_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantity_bar_uses_ratio_icons_across_range() {
        let zero = icon_quantity(0.0);
        let low = icon_quantity(0.25);
        let mid = icon_quantity(0.5);
        let high = icon_quantity(0.85);
        let full = icon_quantity(1.0);
        assert_eq!(zero, svg_asset("ratio-0.svg"));
        assert_eq!(low, svg_asset("ratio-2.svg"));
        assert_eq!(mid, svg_asset("ratio-4.svg"));
        assert_eq!(high, svg_asset("ratio-6.svg"));
        assert_eq!(full, svg_asset("ratio-7.svg"));
    }
}
