use crate::gauge::{GaugeMenu, GaugeMenuItem};
use crate::icon::svg_asset;
use iced::alignment;
use iced::widget::{Column, Container, Row, Space, Text, button, container};
use iced::widget::svg::Svg;
use iced::{Element, Length, Theme};

const HEADER_FONT_SIZE: u32 = 14;
const ITEM_FONT_SIZE: u32 = 12;
const INDICATOR_SIZE: u32 = 16;
const BUTTON_PADDING_Y: u32 = 4;
const HEADER_SPACING: u32 = 4;
const LIST_SPACING: u32 = 6;
const HEADER_LIST_SPACING: u32 = 6;
const CONTAINER_PADDING_Y: u32 = 20;

/// Calculate a reasonable window size for a menu based on item count.
pub fn dialog_dimensions(menu: &GaugeMenu) -> (u32, u32) {
    const MIN_WIDTH: u32 = 340;
    const MAX_WIDTH: u32 = 840;

    let max_label_chars = menu
        .items
        .iter()
        .map(|item| item.label.chars().count() as u32)
        .max()
        .unwrap_or(0);
    // Rough estimate: ~7px per character plus some padding for the checkbox.
    let width = (max_label_chars * 7 + 120).clamp(MIN_WIDTH, MAX_WIDTH);

    let rows = menu.items.len().max(1) as u32;
    let header_height = (HEADER_FONT_SIZE as f32 * 1.2).ceil() as u32 + HEADER_SPACING;
    let text_height = (ITEM_FONT_SIZE as f32 * 1.2).ceil() as u32;
    let row_height = INDICATOR_SIZE.max(text_height) + BUTTON_PADDING_Y * 2;
    let list_height = rows * row_height + LIST_SPACING.saturating_mul(rows.saturating_sub(1));
    let height = header_height + HEADER_LIST_SPACING + list_height + CONTAINER_PADDING_Y;

    (width, height)
}

pub fn menu_view<'a, Message: Clone + 'a>(
    menu: &'a GaugeMenu,
    on_select: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    let checked_icon = svg_asset("option-checked.svg");
    let empty_icon = svg_asset("option-empty.svg");

    let header = Column::new()
        .width(Length::Fill)
        .push(
            Text::new(menu.title.clone())
                .size(HEADER_FONT_SIZE)
                .width(Length::Fill)
                .align_x(alignment::Horizontal::Left),
        )
        .push(Space::new().height(Length::Fixed(4.0)));

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
        .width(Length::Fixed(16.0))
        .height(Length::Fixed(16.0));
        let row = Row::new()
            .align_y(alignment::Vertical::Center)
            .spacing(10)
            .push(indicator)
            .push(Text::new(label.clone()).width(Length::Fill).size(ITEM_FONT_SIZE));

        let item_id = id.clone();
        list = list.push(
            button(row)
                .padding([4, 6])
                .width(Length::Fill)
                .on_press(on_select(item_id)),
        );
    }

    Container::new(
        Column::new()
            .width(Length::Fill)
            .height(Length::Fill)
            .spacing(6)
            .push(header)
            .push(list.spacing(6)),
    )
    .padding([10, 10])
    .width(Length::Fill)
    .height(Length::Shrink)
    .style(|theme: &Theme| container::Style {
        background: Some(theme.extended_palette().background.base.color.into()),
        ..container::Style::default()
    })
    .into()
}
