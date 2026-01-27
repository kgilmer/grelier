// Info dialog sizing and rendering for gauge popup dialogs.
// Consumes Settings: grelier.dialog.*, grelier.info_dialog.*.
use crate::dialog_settings;
use crate::settings;
use iced::alignment;
use iced::font::Weight;
use iced::widget::{Column, Container, Row, Space, Stack, Text, container, rule, text};
use iced::{Color, Element, Font, Length, Theme};

const DEFAULT_HEADER_FONT_SIZE: u32 = 14;
const DEFAULT_BODY_FONT_SIZE: u32 = 12;
const DEFAULT_CHAR_WIDTH: u32 = 6;
const DEFAULT_MAX_CHARS_PER_LINE: u32 = 60;
const DEFAULT_MIN_WIDTH: u32 = 0;
const DEFAULT_MAX_WIDTH: u32 = 840;
const DEFAULT_HEADER_SPACING: u32 = 4;
const DEFAULT_HEADER_BOTTOM_SPACING: u32 = 4;
const DEFAULT_LINE_SPACING: u32 = 6;
const DEFAULT_CONTAINER_PADDING_Y: u32 = 10;
const DEFAULT_CONTAINER_PADDING_X: u32 = 10;
const DEFAULT_BOTTOM_PADDING_EXTRA: u32 = 4;

struct InfoDialogSettings {
    min_width: u32,
    max_width: u32,
    char_width: u32,
    max_chars_per_line: u32,
    header_font_size: u32,
    body_font_size: u32,
    header_spacing: u32,
    header_bottom_spacing: u32,
    line_spacing: u32,
    container_padding_y: u32,
    container_padding_x: u32,
    bottom_padding_extra: u32,
}

impl InfoDialogSettings {
    fn load() -> Self {
        let settings = settings::settings();
        Self {
            min_width: settings.get_parsed_or("grelier.info_dialog.min_width", DEFAULT_MIN_WIDTH),
            max_width: settings.get_parsed_or("grelier.info_dialog.max_width", DEFAULT_MAX_WIDTH),
            char_width: settings
                .get_parsed_or("grelier.info_dialog.char_width", DEFAULT_CHAR_WIDTH),
            max_chars_per_line: settings.get_parsed_or(
                "grelier.info_dialog.max_chars_per_line",
                DEFAULT_MAX_CHARS_PER_LINE,
            ),
            header_font_size: settings.get_parsed_or(
                "grelier.dialog.header_font_size",
                DEFAULT_HEADER_FONT_SIZE,
            ),
            body_font_size: settings
                .get_parsed_or("grelier.info_dialog.body_font_size", DEFAULT_BODY_FONT_SIZE),
            header_spacing: settings
                .get_parsed_or("grelier.info_dialog.header_spacing", DEFAULT_HEADER_SPACING),
            header_bottom_spacing: settings.get_parsed_or(
                "grelier.dialog.header_bottom_spacing",
                DEFAULT_HEADER_BOTTOM_SPACING,
            ),
            line_spacing: settings
                .get_parsed_or("grelier.info_dialog.line_spacing", DEFAULT_LINE_SPACING),
            container_padding_y: settings.get_parsed_or(
                "grelier.dialog.container_padding_y",
                DEFAULT_CONTAINER_PADDING_Y,
            ),
            container_padding_x: settings.get_parsed_or(
                "grelier.dialog.container_padding_x",
                DEFAULT_CONTAINER_PADDING_X,
            ),
            bottom_padding_extra: settings.get_parsed_or(
                "grelier.info_dialog.bottom_padding_extra",
                DEFAULT_BOTTOM_PADDING_EXTRA,
            ),
        }
    }
}

struct BorderSettings {
    blend: bool,
    line_width: f32,
    column_width: f32,
    mix_1: f32,
    mix_2: f32,
    mix_3: f32,
    alpha_1: f32,
    alpha_2: f32,
    alpha_3: f32,
}

impl BorderSettings {
    fn load() -> Self {
        let settings = settings::settings();
        Self {
            blend: settings.get_bool_or("grelier.bar.border_blend", true),
            line_width: settings.get_parsed_or("grelier.bar.border_line_width", 1.0),
            column_width: settings.get_parsed_or("grelier.bar.border_column_width", 3.0),
            mix_1: settings.get_parsed_or("grelier.bar.border_mix_1", 0.2),
            mix_2: settings.get_parsed_or("grelier.bar.border_mix_2", 0.6),
            mix_3: settings.get_parsed_or("grelier.bar.border_mix_3", 1.0),
            alpha_1: settings.get_parsed_or("grelier.bar.border_alpha_1", 0.6),
            alpha_2: settings.get_parsed_or("grelier.bar.border_alpha_2", 0.7),
            alpha_3: settings.get_parsed_or("grelier.bar.border_alpha_3", 0.9),
        }
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

#[derive(Debug, Clone)]
pub struct InfoDialog {
    pub title: String,
    pub lines: Vec<String>,
}

/// Calculate a reasonable window size for an info dialog based on line count and length.
pub fn dialog_dimensions(dialog: &InfoDialog) -> (u32, u32) {
    let dialog_cfg = InfoDialogSettings::load();
    let mut char_width = dialog_cfg.char_width;
    let estimated_char_width = ((dialog_cfg
        .header_font_size
        .max(dialog_cfg.body_font_size) as f32)
        * 0.6)
        .ceil() as u32;
    if char_width < estimated_char_width {
        char_width = estimated_char_width;
    }

    let max_line_chars = dialog
        .lines
        .iter()
        .map(|line| line.chars().count() as u32)
        .chain(std::iter::once(dialog.title.chars().count() as u32))
        .max()
        .unwrap_or(0);
    let target_chars = max_line_chars
        .min(dialog_cfg.max_chars_per_line.max(1))
        .max(1);
    let width = ((target_chars + 2) * char_width + dialog_cfg.container_padding_x * 2)
        .clamp(dialog_cfg.min_width, dialog_cfg.max_width);

    let header_rows = (dialog.title.chars().count() as u32)
        .max(1)
        .div_ceil(target_chars);
    let rows: u32 = dialog
        .lines
        .iter()
        .map(|line| {
            let len = (line.chars().count() as u32).max(1);
            len.div_ceil(target_chars)
        })
        .sum::<u32>()
        .max(1);
    let header_height = header_rows * (dialog_cfg.header_font_size as f32 * 1.2).ceil() as u32
        + dialog_cfg.header_spacing;
    let line_height = (dialog_cfg.body_font_size as f32 * 1.2).ceil() as u32;
    let body_height = rows * line_height
        + dialog_cfg
            .line_spacing
            .saturating_mul(dialog.lines.len().saturating_sub(1) as u32);
    let safety_height = (dialog_cfg.body_font_size as f32 * 0.6).ceil() as u32;
    let height = header_height
        + dialog_cfg.header_bottom_spacing
        + body_height
        + dialog_cfg.container_padding_y * 2
        + dialog_cfg.bottom_padding_extra
        + safety_height;

    (width, height)
}

pub fn info_view<'a, Message: 'a>(dialog: &'a InfoDialog) -> Element<'a, Message> {
    let dialog_cfg = InfoDialogSettings::load();
    let border_settings = BorderSettings::load();

    let header = Column::new()
        .width(Length::Fill)
        .spacing(dialog_cfg.header_spacing)
        .push(
            Container::new(
                Text::new(dialog.title.clone())
                    .size(dialog_cfg.header_font_size)
                    .width(Length::Fill)
                    .align_x(dialog_settings::title_alignment())
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
            }),
        )
        .push(Space::new().height(Length::Fixed(dialog_cfg.header_bottom_spacing as f32)));

    let lines = dialog.lines.iter().fold(
        Column::new()
            .width(Length::Fill)
            .spacing(dialog_cfg.line_spacing),
        |col, line| {
            col.push(
                Text::new(line.clone())
                    .size(dialog_cfg.body_font_size)
                    .width(Length::Fill),
            )
        },
    );

    let content = Container::new(
        Column::new()
            .width(Length::Fill)
            .height(Length::Fill)
            .spacing(dialog_cfg.header_spacing)
            .push(header)
            .push(lines)
            .push(Space::new().height(Length::Fixed(dialog_cfg.bottom_padding_extra as f32))),
    )
    .padding([
        dialog_cfg.container_padding_y as u16,
        dialog_cfg.container_padding_x as u16,
    ])
    .width(Length::Fill)
    .height(Length::Fill)
    .style(|theme: &Theme| container::Style {
        background: Some(theme.extended_palette().background.base.color.into()),
        ..container::Style::default()
    });

    let line_style = |mix: f32, alpha: f32| {
        move |theme: &Theme| {
            let background = theme.palette().background;
            let blended = if border_settings.blend && mix != 0.0 {
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
    };

    let border_column = || {
        Column::new()
            .spacing(0)
            .push(
                rule::horizontal(border_settings.line_width)
                    .style(line_style(border_settings.mix_1, border_settings.alpha_1)),
            )
            .push(
                rule::horizontal(border_settings.line_width)
                    .style(line_style(border_settings.mix_2, border_settings.alpha_2)),
            )
            .push(
                rule::horizontal(border_settings.line_width)
                    .style(line_style(border_settings.mix_3, border_settings.alpha_3)),
            )
            .width(Length::Fill)
            .height(Length::Fixed(border_settings.column_width))
    };

    let border_column_reversed = || {
        Column::new()
            .spacing(0)
            .push(
                rule::horizontal(border_settings.line_width)
                    .style(line_style(border_settings.mix_3, border_settings.alpha_3)),
            )
            .push(
                rule::horizontal(border_settings.line_width)
                    .style(line_style(border_settings.mix_2, border_settings.alpha_2)),
            )
            .push(
                rule::horizontal(border_settings.line_width)
                    .style(line_style(border_settings.mix_1, border_settings.alpha_1)),
            )
            .width(Length::Fill)
            .height(Length::Fixed(border_settings.column_width))
    };

    let border_row = || {
        Row::new()
            .spacing(0)
            .push(
                rule::vertical(border_settings.line_width)
                    .style(line_style(border_settings.mix_1, border_settings.alpha_1)),
            )
            .push(
                rule::vertical(border_settings.line_width)
                    .style(line_style(border_settings.mix_2, border_settings.alpha_2)),
            )
            .push(
                rule::vertical(border_settings.line_width)
                    .style(line_style(border_settings.mix_3, border_settings.alpha_3)),
            )
            .width(Length::Fixed(border_settings.column_width))
            .height(Length::Fill)
    };

    let top_border = container(border_column_reversed())
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(alignment::Vertical::Top);

    let bottom_border = container(border_column())
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(alignment::Vertical::Bottom);

    let right_border = container(border_row())
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(alignment::Horizontal::Right);

    Stack::new()
        .width(Length::Fill)
        .height(Length::Fill)
        .push(content)
        .push(top_border)
        .push(bottom_border)
        .push(right_border)
        .into()
}
