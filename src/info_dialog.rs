// Info dialog sizing and rendering for gauge popup dialogs.
// Consumes Settings: grelier.info_dialog.*.
use crate::settings;
use iced::alignment;
use iced::widget::{Column, Container, Space, Text, container};
use iced::{Element, Length, Theme};

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

#[derive(Debug, Clone)]
pub struct InfoDialog {
    pub title: String,
    pub lines: Vec<String>,
}

/// Calculate a reasonable window size for an info dialog based on line count and length.
pub fn dialog_dimensions(dialog: &InfoDialog) -> (u32, u32) {
    let min_width =
        settings::settings().get_parsed_or("grelier.info_dialog.min_width", DEFAULT_MIN_WIDTH);
    let max_width =
        settings::settings().get_parsed_or("grelier.info_dialog.max_width", DEFAULT_MAX_WIDTH);
    let char_width =
        settings::settings().get_parsed_or("grelier.info_dialog.char_width", DEFAULT_CHAR_WIDTH);
    let max_chars_per_line = settings::settings().get_parsed_or(
        "grelier.info_dialog.max_chars_per_line",
        DEFAULT_MAX_CHARS_PER_LINE,
    );
    let header_font_size = settings::settings().get_parsed_or(
        "grelier.info_dialog.header_font_size",
        DEFAULT_HEADER_FONT_SIZE,
    );
    let body_font_size = settings::settings()
        .get_parsed_or("grelier.info_dialog.body_font_size", DEFAULT_BODY_FONT_SIZE);
    let header_spacing = settings::settings()
        .get_parsed_or("grelier.info_dialog.header_spacing", DEFAULT_HEADER_SPACING);
    let header_bottom_spacing = settings::settings().get_parsed_or(
        "grelier.info_dialog.header_bottom_spacing",
        DEFAULT_HEADER_BOTTOM_SPACING,
    );
    let line_spacing = settings::settings()
        .get_parsed_or("grelier.info_dialog.line_spacing", DEFAULT_LINE_SPACING);
    let container_padding_y = settings::settings()
        .get_parsed_or("grelier.info_dialog.container_padding_y", DEFAULT_CONTAINER_PADDING_Y);
    let container_padding_x = settings::settings()
        .get_parsed_or("grelier.info_dialog.container_padding_x", DEFAULT_CONTAINER_PADDING_X);
    let bottom_padding_extra = settings::settings().get_parsed_or(
        "grelier.info_dialog.bottom_padding_extra",
        DEFAULT_BOTTOM_PADDING_EXTRA,
    );

    let max_line_chars = dialog
        .lines
        .iter()
        .map(|line| line.chars().count() as u32)
        .chain(std::iter::once(dialog.title.chars().count() as u32))
        .max()
        .unwrap_or(0);
    let target_chars = max_line_chars
        .min(max_chars_per_line.max(1))
        .max(1);
    let width =
        (target_chars * char_width + container_padding_x * 2).clamp(min_width, max_width);

    let header_rows = ((dialog.title.chars().count() as u32).max(1) + target_chars - 1)
        / target_chars;
    let rows: u32 = dialog
        .lines
        .iter()
        .map(|line| {
            let len = (line.chars().count() as u32).max(1);
            (len + target_chars - 1) / target_chars
        })
        .sum::<u32>()
        .max(1);
    let header_height =
        header_rows * (header_font_size as f32 * 1.2).ceil() as u32 + header_spacing;
    let line_height = (body_font_size as f32 * 1.2).ceil() as u32;
    let body_height = rows * line_height
        + line_spacing.saturating_mul(dialog.lines.len().saturating_sub(1) as u32);
    let height = header_height
        + header_bottom_spacing
        + body_height
        + container_padding_y * 2
        + bottom_padding_extra;

    (width, height)
}

pub fn info_view<'a, Message: 'a>(dialog: &'a InfoDialog) -> Element<'a, Message> {
    let header_font_size = settings::settings().get_parsed_or(
        "grelier.info_dialog.header_font_size",
        DEFAULT_HEADER_FONT_SIZE,
    );
    let body_font_size = settings::settings()
        .get_parsed_or("grelier.info_dialog.body_font_size", DEFAULT_BODY_FONT_SIZE);
    let header_spacing = settings::settings()
        .get_parsed_or("grelier.info_dialog.header_spacing", DEFAULT_HEADER_SPACING);
    let header_bottom_spacing = settings::settings().get_parsed_or(
        "grelier.info_dialog.header_bottom_spacing",
        DEFAULT_HEADER_BOTTOM_SPACING,
    );
    let line_spacing = settings::settings()
        .get_parsed_or("grelier.info_dialog.line_spacing", DEFAULT_LINE_SPACING);
    let container_padding_y = settings::settings().get_parsed_or(
        "grelier.info_dialog.container_padding_y",
        DEFAULT_CONTAINER_PADDING_Y,
    );
    let container_padding_x = settings::settings()
        .get_parsed_or("grelier.info_dialog.container_padding_x", DEFAULT_CONTAINER_PADDING_X);
    let bottom_padding_extra = settings::settings().get_parsed_or(
        "grelier.info_dialog.bottom_padding_extra",
        DEFAULT_BOTTOM_PADDING_EXTRA,
    );

    let header = Column::new()
        .width(Length::Fill)
        .spacing(header_spacing)
        .push(
            Text::new(dialog.title.clone())
                .size(header_font_size)
                .width(Length::Fill)
                .align_x(alignment::Horizontal::Left),
        )
        .push(Space::new().height(Length::Fixed(header_bottom_spacing as f32)));

    let lines = dialog.lines.iter().fold(
        Column::new().width(Length::Fill).spacing(line_spacing),
        |col, line| {
            col.push(
                Text::new(line.clone())
                    .size(body_font_size)
                    .width(Length::Fill),
            )
        },
    );

    Container::new(
        Column::new()
            .width(Length::Fill)
            .height(Length::Fill)
            .spacing(header_spacing)
            .push(header)
            .push(lines)
            .push(Space::new().height(Length::Fixed(bottom_padding_extra as f32))),
    )
    .padding([container_padding_y as u16, container_padding_x as u16])
    .width(Length::Fill)
    .height(Length::Shrink)
    .style(|theme: &Theme| container::Style {
        background: Some(theme.extended_palette().background.base.color.into()),
        ..container::Style::default()
    })
    .into()
}
