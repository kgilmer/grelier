use iced::alignment;

use crate::settings;

/// Default alignment when the dialog title alignment setting is missing/invalid.
const DEFAULT_TITLE_ALIGN: &str = "center";

/// Resolve the horizontal alignment for dialog titles from settings.
///
/// Falls back to the default and logs when the setting value is invalid.
pub fn title_alignment() -> alignment::Horizontal {
    let raw = settings::settings().get_or("grelier.dialog.title_align", DEFAULT_TITLE_ALIGN);
    let value = raw.trim().to_lowercase();
    match value.as_str() {
        "left" => alignment::Horizontal::Left,
        "center" => alignment::Horizontal::Center,
        "right" => alignment::Horizontal::Right,
        _ => {
            eprintln!(
                "Invalid setting 'grelier.dialog.title_align': '{}'. Expected left|center|right.",
                raw
            );
            alignment::Horizontal::Center
        }
    }
}
