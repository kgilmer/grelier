use iced::widget::svg;
use std::path::Path;

/// Absolute path to the bundled asset directory (e.g. SVG icons).
pub const ASSETS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");

/// Build an `iced` SVG handle for a file under `assets/`.
pub fn svg_asset(name: &str) -> svg::Handle {
    let path = Path::new(ASSETS_DIR).join(name);
    svg::Handle::from_path(path)
}

#[derive(Debug, Clone, Copy)]
pub enum QuantityStyle {
    Grid,
    Pie,
}

impl QuantityStyle {
    pub fn toggle(self) -> Self {
        match self {
            QuantityStyle::Grid => QuantityStyle::Pie,
            QuantityStyle::Pie => QuantityStyle::Grid,
        }
    }

    pub fn parse_setting(key: &str, value: &str) -> Self {
        match value {
            "grid" => QuantityStyle::Grid,
            "pie" => QuantityStyle::Pie,
            other => panic!(
                "Invalid setting '{key}': expected grid or pie, got '{other}'"
            ),
        }
    }

    pub fn as_setting_value(self) -> &'static str {
        match self {
            QuantityStyle::Grid => "grid",
            QuantityStyle::Pie => "pie",
        }
    }
}

impl Default for QuantityStyle {
    fn default() -> Self {
        QuantityStyle::Grid
    }
}

/// Returns the appropriate handle to the SVG representing the quantity `value`.
/// `value` must be a number between 0 and 1.  0 indicates "no quantity" and 1 indicates "full quantity".
/// grid-0.svg - grid-9.svg and pie-0.svg - pie-8.svg are the icons returned.
pub fn icon_quantity(style: QuantityStyle, value: f32) -> svg::Handle {
    let (prefix, max_index) = match style {
        QuantityStyle::Grid => ("grid", 9usize),
        QuantityStyle::Pie => ("pie", 8usize),
    };

    let clamped = value.clamp(0.0, 1.0);
    let icon_name = if clamped <= 0.0 {
        format!("{prefix}-0.svg")
    } else if clamped >= 1.0 {
        format!("{prefix}-{max_index}.svg")
    } else {
        // Use half-up rounding to avoid skipping icons (ties-to-even would skip grid-5).
        let half_up = (clamped * max_index as f32 + 0.5).floor() as usize;
        let index = half_up.min(max_index);
        format!("{prefix}-{index}.svg")
    };

    svg_asset(&icon_name)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn quantity_grid_uses_grid_icons_at_extremes() {
        let zero = icon_quantity(QuantityStyle::Grid, 0.0);
        let full = icon_quantity(QuantityStyle::Grid, 1.0);
        assert_eq!(zero, svg_asset("grid-0.svg"));
        assert_eq!(full, svg_asset("grid-9.svg"));
    }

    #[test]
    fn quantity_grid_midpoints_cover_range() {
        // Check a mid value that should land near the middle of the 0-9 grid set.
        let mid = icon_quantity(QuantityStyle::Grid, 0.5);
        assert_eq!(mid, svg_asset("grid-5.svg"));
        // And a lower-mid value that should map to grid-4.
        let lower_mid = icon_quantity(QuantityStyle::Grid, 0.45);
        assert_eq!(lower_mid, svg_asset("grid-4.svg"));
    }

    #[test]
    fn quantity_pie_uses_pie_icons_at_extremes() {
        let zero = icon_quantity(QuantityStyle::Pie, 0.0);
        let full = icon_quantity(QuantityStyle::Pie, 1.0);
        assert_eq!(zero, svg_asset("pie-0.svg"));
        assert_eq!(full, svg_asset("pie-8.svg"));
    }
}
