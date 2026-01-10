// Menu sizing and rendering for gauge popup dialogs.
// Consumes Settings: grelier.menu_dialog.*.
use crate::gauge::{GaugeMenu, GaugeMenuItem};
use crate::icon::svg_asset;
use crate::settings;
use iced::alignment;
use iced::widget::svg::Svg;
use iced::widget::{Column, Container, Row, Space, Text, button, container};
use iced::{Element, Length, Theme};

const DEFAULT_HEADER_FONT_SIZE: u32 = 14;
const DEFAULT_ITEM_FONT_SIZE: u32 = 12;
const DEFAULT_INDICATOR_SIZE: u32 = 16;
const DEFAULT_BUTTON_PADDING_Y: u32 = 4;
const DEFAULT_HEADER_SPACING: u32 = 4;
const DEFAULT_LIST_SPACING: u32 = 6;
const DEFAULT_HEADER_LIST_SPACING: u32 = 6;
const DEFAULT_CONTAINER_PADDING_Y: u32 = 20;
const DEFAULT_MIN_WIDTH: u32 = 340;
const DEFAULT_MAX_WIDTH: u32 = 840;
const DEFAULT_CHAR_WIDTH: u32 = 7;
const DEFAULT_LABEL_PADDING: u32 = 120;
const DEFAULT_HEADER_BOTTOM_SPACING: u32 = 4;
const DEFAULT_INDICATOR_SPACING: u32 = 10;
const DEFAULT_BUTTON_PADDING_X: u32 = 6;
const DEFAULT_CONTAINER_PADDING_X: u32 = 10;

/// Calculate a reasonable window size for a menu based on item count.
pub fn dialog_dimensions(menu: &GaugeMenu) -> (u32, u32) {
    let min_width =
        settings::settings().get_parsed_or("grelier.menu_dialog.min_width", DEFAULT_MIN_WIDTH);
    let max_width =
        settings::settings().get_parsed_or("grelier.menu_dialog.max_width", DEFAULT_MAX_WIDTH);
    let char_width =
        settings::settings().get_parsed_or("grelier.menu_dialog.char_width", DEFAULT_CHAR_WIDTH);
    let label_padding = settings::settings()
        .get_parsed_or("grelier.menu_dialog.label_padding", DEFAULT_LABEL_PADDING);
    let header_font_size = settings::settings().get_parsed_or(
        "grelier.menu_dialog.header_font_size",
        DEFAULT_HEADER_FONT_SIZE,
    );
    let item_font_size = settings::settings()
        .get_parsed_or("grelier.menu_dialog.item_font_size", DEFAULT_ITEM_FONT_SIZE);
    let indicator_size = settings::settings()
        .get_parsed_or("grelier.menu_dialog.indicator_size", DEFAULT_INDICATOR_SIZE);
    let button_padding_y = settings::settings().get_parsed_or(
        "grelier.menu_dialog.button_padding_y",
        DEFAULT_BUTTON_PADDING_Y,
    );
    let header_spacing = settings::settings()
        .get_parsed_or("grelier.menu_dialog.header_spacing", DEFAULT_HEADER_SPACING);
    let list_spacing = settings::settings()
        .get_parsed_or("grelier.menu_dialog.list_spacing", DEFAULT_LIST_SPACING);
    let header_list_spacing = settings::settings().get_parsed_or(
        "grelier.menu_dialog.header_list_spacing",
        DEFAULT_HEADER_LIST_SPACING,
    );
    let container_padding_y = settings::settings().get_parsed_or(
        "grelier.menu_dialog.container_padding_y",
        DEFAULT_CONTAINER_PADDING_Y,
    );

    let max_label_chars = menu
        .items
        .iter()
        .map(|item| item.label.chars().count() as u32)
        .max()
        .unwrap_or(0);
    // Rough estimate: ~7px per character plus some padding for the checkbox.
    let width = (max_label_chars * char_width + label_padding).clamp(min_width, max_width);

    let rows = menu.items.len().max(1) as u32;
    let header_height = (header_font_size as f32 * 1.2).ceil() as u32 + header_spacing;
    let text_height = (item_font_size as f32 * 1.2).ceil() as u32;
    let row_height = indicator_size.max(text_height) + button_padding_y * 2;
    let list_height = rows * row_height + list_spacing.saturating_mul(rows.saturating_sub(1));
    let height = header_height + header_list_spacing + list_height + container_padding_y;

    (width, height)
}

pub fn menu_view<'a, Message: Clone + 'a>(
    menu: &'a GaugeMenu,
    on_select: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    let checked_icon = svg_asset("option-checked.svg");
    let empty_icon = svg_asset("option-empty.svg");
    let header_font_size = settings::settings().get_parsed_or(
        "grelier.menu_dialog.header_font_size",
        DEFAULT_HEADER_FONT_SIZE,
    );
    let item_font_size = settings::settings()
        .get_parsed_or("grelier.menu_dialog.item_font_size", DEFAULT_ITEM_FONT_SIZE);
    let indicator_size = settings::settings()
        .get_parsed_or("grelier.menu_dialog.indicator_size", DEFAULT_INDICATOR_SIZE);
    let button_padding_y = settings::settings().get_parsed_or(
        "grelier.menu_dialog.button_padding_y",
        DEFAULT_BUTTON_PADDING_Y,
    );
    let list_spacing = settings::settings()
        .get_parsed_or("grelier.menu_dialog.list_spacing", DEFAULT_LIST_SPACING);
    let header_list_spacing = settings::settings().get_parsed_or(
        "grelier.menu_dialog.header_list_spacing",
        DEFAULT_HEADER_LIST_SPACING,
    );
    let container_padding_y = settings::settings().get_parsed_or(
        "grelier.menu_dialog.container_padding_y",
        DEFAULT_CONTAINER_PADDING_Y,
    );
    let header_bottom_spacing = settings::settings().get_parsed_or(
        "grelier.menu_dialog.header_bottom_spacing",
        DEFAULT_HEADER_BOTTOM_SPACING,
    );
    let indicator_spacing = settings::settings().get_parsed_or(
        "grelier.menu_dialog.indicator_spacing",
        DEFAULT_INDICATOR_SPACING,
    );
    let button_padding_x = settings::settings().get_parsed_or(
        "grelier.menu_dialog.button_padding_x",
        DEFAULT_BUTTON_PADDING_X,
    );
    let container_padding_x = settings::settings().get_parsed_or(
        "grelier.menu_dialog.container_padding_x",
        DEFAULT_CONTAINER_PADDING_X,
    );

    let header = Column::new()
        .width(Length::Fill)
        .push(
            Text::new(menu.title.clone())
                .size(header_font_size)
                .width(Length::Fill)
                .align_x(alignment::Horizontal::Left),
        )
        .push(Space::new().height(Length::Fixed(header_bottom_spacing as f32)));

    let mut list = Column::new().width(Length::Fill);

    for GaugeMenuItem {
        id,
        label,
        selected,
    } in &menu.items
    {
        let indicator = Svg::new(if *selected {
            checked_icon.clone()
        } else {
            empty_icon.clone()
        })
        .width(Length::Fixed(indicator_size as f32))
        .height(Length::Fixed(indicator_size as f32));
        let row = Row::new()
            .align_y(alignment::Vertical::Center)
            .spacing(indicator_spacing)
            .push(indicator)
            .push(
                Text::new(label.clone())
                    .width(Length::Fill)
                    .size(item_font_size),
            );

        let item_id = id.clone();
        list = list.push(
            button(row)
                .padding([button_padding_y as u16, button_padding_x as u16])
                .width(Length::Fill)
                .on_press(on_select(item_id)),
        );
    }

    Container::new(
        Column::new()
            .width(Length::Fill)
            .height(Length::Fill)
            .spacing(header_list_spacing)
            .push(header)
            .push(list.spacing(list_spacing)),
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
