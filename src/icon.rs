// SVG asset helpers and quantity icon selection for gauges.
use iced::{Color, widget::svg};
use iced_core::svg::Data;
use std::collections::HashMap;
use std::path::Path;

/// Absolute path to the bundled asset directory (e.g. SVG icons).
pub const ASSETS_DIR: &str = concat!(env!("CARGO_MANIFEST_DIR"), "/assets");

/// Return (and cache) a themed SVG handle with gradient stops replaced by colors.
pub fn themed_svg_handle_cached(
    cache: &std::sync::Arc<std::sync::Mutex<HashMap<String, svg::Handle>>>,
    handle: &svg::Handle,
    start: Color,
    end: Color,
) -> Option<svg::Handle> {
    let start_hex = color_to_hex(start);
    let end_hex = color_to_hex(end);
    let key = format!("{}:{start_hex}:{end_hex}", handle.id());
    if let Ok(map) = cache.lock()
        && let Some(existing) = map.get(&key)
    {
        return Some(existing.clone());
    }

    let template = svg_template_from_handle(handle)?;
    let svg_data = svg_with_gradient_stops(&template, &start_hex, &end_hex);
    let themed = svg::Handle::from_memory(svg_data);
    if let Ok(mut map) = cache.lock() {
        map.insert(key, themed.clone());
    }
    Some(themed)
}

fn svg_template_from_handle(handle: &svg::Handle) -> Option<String> {
    match handle.data() {
        Data::Path(path) => std::fs::read_to_string(path).ok(),
        Data::Bytes(bytes) => std::str::from_utf8(bytes).ok().map(|s| s.to_string()),
    }
}

fn svg_with_gradient_stops(template: &str, start_hex: &str, end_hex: &str) -> Vec<u8> {
    template
        .replacen(
            "stop-color=\"currentColor\"",
            &format!("stop-color=\"{start_hex}\""),
            1,
        )
        .replacen(
            "stop-color=\"currentColor\"",
            &format!("stop-color=\"{end_hex}\""),
            1,
        )
        .replace("stroke=\"currentColor\"", &format!("stroke=\"{end_hex}\""))
        .replace("fill=\"currentColor\"", &format!("fill=\"{end_hex}\""))
        .replace("stop-opacity=\"0.7\"", "stop-opacity=\"1\"")
        .into_bytes()
}

fn color_to_hex(color: Color) -> String {
    let r = (color.r.clamp(0.0, 1.0) * 255.0).round() as u8;
    let g = (color.g.clamp(0.0, 1.0) * 255.0).round() as u8;
    let b = (color.b.clamp(0.0, 1.0) * 255.0).round() as u8;
    format!("#{:02X}{:02X}{:02X}", r, g, b)
}

/// Build an `iced` SVG handle for a file under `assets/`.
pub fn svg_asset(name: &str) -> svg::Handle {
    let path = Path::new(ASSETS_DIR).join(name);
    svg::Handle::from_path(path)
}

#[cfg(test)]
mod svg_tests {
    use super::*;
    use std::sync::{Arc, Mutex};

    #[test]
    fn themed_svg_handle_is_cached_by_color_and_id() {
        let cache: Arc<Mutex<HashMap<String, svg::Handle>>> = Arc::new(Mutex::new(HashMap::new()));
        let svg_src = r#"
            <svg xmlns="http://www.w3.org/2000/svg">
                <linearGradient id="g">
                    <stop stop-color="currentColor" stop-opacity="0.7"/>
                    <stop stop-color="currentColor"/>
                </linearGradient>
                <rect stroke="currentColor" fill="currentColor"/>
            </svg>
        "#;
        let handle = svg::Handle::from_memory(svg_src.as_bytes());

        let start = Color::from_rgb(0.1, 0.2, 0.3);
        let end = Color::from_rgb(0.7, 0.6, 0.5);
        let themed_a = themed_svg_handle_cached(&cache, &handle, start, end);
        assert!(themed_a.is_some());
        let cache_len_after_first = cache.lock().unwrap().len();
        assert_eq!(cache_len_after_first, 1);

        let themed_b = themed_svg_handle_cached(&cache, &handle, start, end);
        assert!(themed_b.is_some());
        let cache_len_after_second = cache.lock().unwrap().len();
        assert_eq!(cache_len_after_second, 1);

        let themed_c =
            themed_svg_handle_cached(&cache, &handle, start, Color::from_rgb(0.0, 0.0, 0.0));
        assert!(themed_c.is_some());
        let cache_len_after_third = cache.lock().unwrap().len();
        assert_eq!(cache_len_after_third, 2);
    }

    #[test]
    fn svg_gradient_replacement_updates_stops_and_styling() {
        let template = r#"
            <svg xmlns="http://www.w3.org/2000/svg">
                <linearGradient id="g">
                    <stop stop-color="currentColor" stop-opacity="0.7"/>
                    <stop stop-color="currentColor"/>
                    <stop stop-color="currentColor"/>
                </linearGradient>
                <rect stroke="currentColor" fill="currentColor"/>
            </svg>
        "#;
        let data = svg_with_gradient_stops(template, "#112233", "#AABBCC");
        let output = String::from_utf8(data).unwrap();

        assert!(output.contains("stop-color=\"#112233\""));
        assert!(output.contains("stop-color=\"#AABBCC\""));
        assert_eq!(
            output.matches("stop-color=\"currentColor\"").count(),
            1,
            "only the first two stop-color entries should be replaced"
        );
        assert!(output.contains("stroke=\"#AABBCC\""));
        assert!(output.contains("fill=\"#AABBCC\""));
        assert!(output.contains("stop-opacity=\"1\""));
    }
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
