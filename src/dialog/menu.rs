// Menu sizing and rendering for gauge popup dialogs.
// Consumes Settings: grelier.dialog.*, grelier.menu_dialog.*.
use crate::dialog::common::{self, BorderSettings};
use crate::icon::svg_asset;
use crate::panels::gauges::gauge::{GaugeMenu, GaugeMenuItem};
use crate::settings;
use iced::alignment;
use iced::widget::svg::{self, Svg};
use iced::widget::text::LineHeight;
use iced::widget::{Column, Row, Space, Text, button, container, mouse_area};
use iced::{Element, Length, Pixels, Theme};

const DEFAULT_HEADER_FONT_SIZE: u32 = 14;
const DEFAULT_ITEM_FONT_SIZE: u32 = 12;
const DEFAULT_INDICATOR_SIZE: u32 = 16;
const DEFAULT_BUTTON_PADDING_Y: u32 = 4;
const DEFAULT_LIST_SPACING: u32 = 6;
const DEFAULT_HEADER_LIST_SPACING: u32 = 6;
const DEFAULT_CONTAINER_PADDING_Y: u32 = 10;
const DEFAULT_MIN_WIDTH: u32 = 340;
const DEFAULT_MAX_WIDTH: u32 = 840;
const DEFAULT_CHAR_WIDTH: u32 = 7;
const DEFAULT_LABEL_PADDING: u32 = 120;
const DEFAULT_HEADER_BOTTOM_SPACING: u32 = 4;
const DEFAULT_INDICATOR_SPACING: u32 = 10;
const DEFAULT_BUTTON_PADDING_X: u32 = 6;
const DEFAULT_CONTAINER_PADDING_X: u32 = 10;

struct MenuDialogSettings {
    min_width: u32,
    max_width: u32,
    char_width: u32,
    label_padding: u32,
    header_font_size: u32,
    item_font_size: u32,
    indicator_size: u32,
    button_padding_y: u32,
    list_spacing: u32,
    header_list_spacing: u32,
    container_padding_y: u32,
    header_bottom_spacing: u32,
    indicator_spacing: u32,
    button_padding_x: u32,
    container_padding_x: u32,
}

impl MenuDialogSettings {
    fn load() -> Self {
        let settings = settings::settings();
        Self {
            min_width: settings.get_parsed_or("grelier.menu_dialog.min_width", DEFAULT_MIN_WIDTH),
            max_width: settings.get_parsed_or("grelier.menu_dialog.max_width", DEFAULT_MAX_WIDTH),
            char_width: settings
                .get_parsed_or("grelier.menu_dialog.char_width", DEFAULT_CHAR_WIDTH),
            label_padding: settings
                .get_parsed_or("grelier.menu_dialog.label_padding", DEFAULT_LABEL_PADDING),
            header_font_size: settings
                .get_parsed_or("grelier.dialog.header.font_size", DEFAULT_HEADER_FONT_SIZE),
            item_font_size: settings
                .get_parsed_or("grelier.menu_dialog.item_font_size", DEFAULT_ITEM_FONT_SIZE),
            indicator_size: settings
                .get_parsed_or("grelier.menu_dialog.indicator_size", DEFAULT_INDICATOR_SIZE),
            button_padding_y: settings.get_parsed_or(
                "grelier.menu_dialog.button_padding_y",
                DEFAULT_BUTTON_PADDING_Y,
            ),
            list_spacing: settings
                .get_parsed_or("grelier.menu_dialog.list_spacing", DEFAULT_LIST_SPACING),
            header_list_spacing: settings.get_parsed_or(
                "grelier.menu_dialog.header_list_spacing",
                DEFAULT_HEADER_LIST_SPACING,
            ),
            container_padding_y: settings.get_parsed_or(
                "grelier.dialog.container.padding_y",
                DEFAULT_CONTAINER_PADDING_Y,
            ),
            header_bottom_spacing: settings.get_parsed_or(
                "grelier.dialog.header.bottom_spacing",
                DEFAULT_HEADER_BOTTOM_SPACING,
            ),
            indicator_spacing: settings.get_parsed_or(
                "grelier.menu_dialog.indicator_spacing",
                DEFAULT_INDICATOR_SPACING,
            ),
            button_padding_x: settings.get_parsed_or(
                "grelier.menu_dialog.button_padding_x",
                DEFAULT_BUTTON_PADDING_X,
            ),
            container_padding_x: settings.get_parsed_or(
                "grelier.dialog.container.padding_x",
                DEFAULT_CONTAINER_PADDING_X,
            ),
        }
    }
}

/// Calculate a reasonable window size for a menu based on item count.
pub fn dialog_dimensions(menu: &GaugeMenu) -> (u32, u32) {
    let cfg = MenuDialogSettings::load();

    let max_label_chars = menu
        .items
        .iter()
        .map(|item| item.label.chars().count() as u32)
        .max()
        .unwrap_or(0);
    // Rough estimate: ~7px per character plus some padding for the checkbox.
    let width =
        (max_label_chars * cfg.char_width + cfg.label_padding).clamp(cfg.min_width, cfg.max_width);

    let rows = menu.items.len().max(1) as u32;
    let header_line_height = LineHeight::default()
        .to_absolute(Pixels(cfg.header_font_size as f32))
        .0;
    let item_line_height = LineHeight::default()
        .to_absolute(Pixels(cfg.item_font_size as f32))
        .0;
    let header_height = header_line_height.ceil() as u32 + cfg.header_bottom_spacing;
    let text_height = item_line_height.ceil() as u32;
    let row_height = cfg.indicator_size.max(text_height) + cfg.button_padding_y * 2;
    let list_height = rows * row_height + cfg.list_spacing.saturating_mul(rows.saturating_sub(1));
    let height = header_height
        + cfg.header_list_spacing
        + list_height
        + cfg.container_padding_y.saturating_mul(2);

    (width, height)
}

pub fn menu_view<'a, Message: Clone + 'a>(
    menu: &'a GaugeMenu,
    hovered_item: Option<&'a str>,
    on_select: impl Fn(String) -> Message + 'a,
    on_hover_enter: impl Fn(String) -> Message + 'a,
    on_hover_exit: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    let border_settings = BorderSettings::load();
    let cfg = MenuDialogSettings::load();
    let checked_icon = svg_asset("option-checked.svg");
    let empty_icon = svg_asset("option-empty.svg");

    let header = Column::new()
        .width(Length::Fill)
        .push(common::dialog_title(
            menu.title.as_str(),
            cfg.header_font_size,
        ))
        .push(Space::new().height(Length::Fixed(cfg.header_bottom_spacing as f32)));

    let mut list = Column::new().width(Length::Fill);

    for GaugeMenuItem {
        id,
        label,
        selected,
    } in &menu.items
    {
        let is_hovered = hovered_item.is_some_and(|hovered| hovered == id.as_str());
        let is_selected = *selected;
        let indicator = Svg::new(if is_selected {
            checked_icon.clone()
        } else {
            empty_icon.clone()
        })
        .width(Length::Fixed(cfg.indicator_size as f32))
        .height(Length::Fixed(cfg.indicator_size as f32))
        .style({
            move |theme: &Theme, status| {
                let palette = theme.extended_palette();
                let hovered = is_hovered || matches!(status, svg::Status::Hovered);
                let color = if is_selected {
                    palette.secondary.strong.color
                } else if hovered {
                    palette.primary.weak.text
                } else {
                    palette.primary.weak.color
                };

                svg::Style { color: Some(color) }
            }
        });
        let row = Row::new()
            .width(Length::Fill)
            .align_y(alignment::Vertical::Center)
            .spacing(cfg.indicator_spacing)
            .push(container(indicator))
            .push(
                Text::new(label.as_str())
                    .width(Length::Shrink)
                    .size(cfg.item_font_size),
            )
            .push(Space::new().width(Length::Fill));

        let item_id = id.clone();
        let row_button = button(row)
            .padding([cfg.button_padding_y as u16, cfg.button_padding_x as u16])
            .width(Length::Fill)
            .style(|theme: &Theme, status| {
                let highlight = theme.extended_palette().primary.weak.color;
                let background = match status {
                    button::Status::Hovered | button::Status::Pressed => Some(highlight.into()),
                    button::Status::Active | button::Status::Disabled => None,
                };

                button::Style {
                    background,
                    text_color: theme.palette().text,
                    ..button::Style::default()
                }
            })
            .on_press(on_select(item_id.clone()));
        list = list.push(
            mouse_area(row_button)
                .on_enter(on_hover_enter(item_id.clone()))
                .on_exit(on_hover_exit(item_id)),
        );
    }

    let content = common::dialog_surface(
        Column::new()
            .width(Length::Fill)
            .height(Length::Fill)
            .spacing(cfg.header_list_spacing)
            .push(header)
            .push(list.spacing(cfg.list_spacing)),
        cfg.container_padding_y as u16,
        cfg.container_padding_x as u16,
    );

    common::stack_with_border(content, border_settings, common::popup_border_sides())
}
