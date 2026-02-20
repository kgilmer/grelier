use iced::alignment;
use iced::font::Weight;
use iced::widget::{Column, Row, Stack, Text, container, rule, text};
use iced::{Color, Element, Font, Length, Theme};

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
            log::warn!(
                "Invalid setting 'grelier.dialog.title_align': '{}'. Expected left|center|right.",
                raw
            );
            alignment::Horizontal::Center
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BorderSettings {
    pub blend: bool,
    pub line_width: f32,
    pub column_width: f32,
    pub mix_1: f32,
    pub mix_2: f32,
    pub mix_3: f32,
    pub alpha_1: f32,
    pub alpha_2: f32,
    pub alpha_3: f32,
}

impl BorderSettings {
    pub fn load() -> Self {
        let settings = settings::settings();
        Self {
            blend: settings.get_bool_or("grelier.bar.border.blend", true),
            line_width: settings.get_parsed_or("grelier.bar.border.line_width", 1.0),
            column_width: settings.get_parsed_or("grelier.bar.border.column_width", 3.0),
            mix_1: settings.get_parsed_or("grelier.bar.border.mix_1", 0.2),
            mix_2: settings.get_parsed_or("grelier.bar.border.mix_2", 0.6),
            mix_3: settings.get_parsed_or("grelier.bar.border.mix_3", 1.0),
            alpha_1: settings.get_parsed_or("grelier.bar.border.alpha_1", 0.6),
            alpha_2: settings.get_parsed_or("grelier.bar.border.alpha_2", 0.7),
            alpha_3: settings.get_parsed_or("grelier.bar.border.alpha_3", 0.9),
        }
    }
}

#[derive(Debug, Clone, Copy)]
pub struct BorderSides {
    pub top: bool,
    pub top_reversed: bool,
    pub bottom: bool,
    pub left: bool,
    pub right: bool,
}

pub fn popup_border_sides() -> BorderSides {
    BorderSides {
        top: true,
        top_reversed: true,
        bottom: true,
        left: false,
        right: true,
    }
}

fn lerp_color(from: Color, to: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color {
        r: from.r + (to.r - from.r) * t,
        g: from.g + (to.g - from.g) * t,
        b: from.b + (to.b - from.b) * t,
        a: from.a + (to.a - from.a) * t,
    }
}

fn border_style(theme: &Theme, settings: BorderSettings, mix: f32, alpha: f32) -> rule::Style {
    let background = theme.palette().background;
    let blended = if settings.blend && mix != 0.0 {
        lerp_color(background, Color::BLACK, mix)
    } else {
        background
    };
    rule::Style {
        color: Color {
            a: alpha,
            ..blended
        },
        radius: 0.0.into(),
        fill_mode: rule::FillMode::Full,
        snap: true,
    }
}

fn border_row<'a, Message: 'a>(settings: BorderSettings) -> Row<'a, Message> {
    Row::new()
        .spacing(0)
        .push(
            rule::vertical(settings.line_width).style(move |theme| {
                border_style(theme, settings, settings.mix_1, settings.alpha_1)
            }),
        )
        .push(
            rule::vertical(settings.line_width).style(move |theme| {
                border_style(theme, settings, settings.mix_2, settings.alpha_2)
            }),
        )
        .push(
            rule::vertical(settings.line_width).style(move |theme| {
                border_style(theme, settings, settings.mix_3, settings.alpha_3)
            }),
        )
        .width(Length::Fixed(settings.column_width))
        .height(Length::Fill)
}

fn border_column<'a, Message: 'a>(settings: BorderSettings, reversed: bool) -> Column<'a, Message> {
    let (mix_1, mix_2, mix_3, alpha_1, alpha_2, alpha_3) = if reversed {
        (
            settings.mix_3,
            settings.mix_2,
            settings.mix_1,
            settings.alpha_3,
            settings.alpha_2,
            settings.alpha_1,
        )
    } else {
        (
            settings.mix_1,
            settings.mix_2,
            settings.mix_3,
            settings.alpha_1,
            settings.alpha_2,
            settings.alpha_3,
        )
    };

    Column::new()
        .spacing(0)
        .push(
            rule::horizontal(settings.line_width)
                .style(move |theme| border_style(theme, settings, mix_1, alpha_1)),
        )
        .push(
            rule::horizontal(settings.line_width)
                .style(move |theme| border_style(theme, settings, mix_2, alpha_2)),
        )
        .push(
            rule::horizontal(settings.line_width)
                .style(move |theme| border_style(theme, settings, mix_3, alpha_3)),
        )
        .width(Length::Fill)
        .height(Length::Fixed(settings.column_width))
}

pub fn stack_with_border<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    settings: BorderSettings,
    sides: BorderSides,
) -> Element<'a, Message> {
    let mut stack = Stack::new()
        .width(Length::Fill)
        .height(Length::Fill)
        .push(content);

    if sides.top {
        stack = stack.push(
            container(border_column(settings, sides.top_reversed))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_y(alignment::Vertical::Top),
        );
    }

    if sides.bottom {
        stack = stack.push(
            container(border_column(settings, false))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_y(alignment::Vertical::Bottom),
        );
    }

    if sides.left {
        stack = stack.push(
            container(border_row(settings))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(alignment::Horizontal::Left),
        );
    }

    if sides.right {
        stack = stack.push(
            container(border_row(settings))
                .width(Length::Fill)
                .height(Length::Fill)
                .align_x(alignment::Horizontal::Right),
        );
    }

    stack.into()
}

pub fn dialog_title<'a, Message: 'a>(title: &'a str, font_size: u32) -> Element<'a, Message> {
    container(
        Text::new(title)
            .size(font_size)
            .width(Length::Fill)
            .align_x(title_alignment())
            .style(|theme: &Theme| text::Style {
                color: Some(theme.extended_palette().background.base.color),
            })
            .font(Font {
                weight: Weight::Bold,
                ..Font::DEFAULT
            }),
    )
    .padding([0, 6])
    .width(Length::Fill)
    .style(|theme: &Theme| container::Style {
        background: Some(theme.extended_palette().primary.base.color.into()),
        ..container::Style::default()
    })
    .into()
}

pub fn dialog_surface<'a, Message: 'a>(
    content: impl Into<Element<'a, Message>>,
    padding_y: u16,
    padding_x: u16,
) -> Element<'a, Message> {
    container(content)
        .padding([padding_y, padding_x])
        .width(Length::Fill)
        .height(Length::Fill)
        .style(|theme: &Theme| container::Style {
            background: Some(theme.extended_palette().background.base.color.into()),
            ..container::Style::default()
        })
        .into()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn popup_border_sides_match_standard_dialog_profile() {
        let sides = popup_border_sides();
        assert!(sides.top);
        assert!(sides.top_reversed);
        assert!(sides.bottom);
        assert!(!sides.left);
        assert!(sides.right);
    }
}
