// Menu sizing and rendering for gauge popup dialogs.
// Consumes Settings: grelier.dialog.*, grelier.menu_dialog.*.
use crate::dialog_settings;
use crate::gauge::{GaugeMenu, GaugeMenuItem};
use crate::icon::svg_asset;
use crate::settings;
use iced::alignment;
use iced::font::Weight;
use iced::widget::svg::{self, Svg};
use iced::widget::text::LineHeight;
use iced::widget::{
    Column, Container, Row, Space, Stack, Text, button, container, mouse_area, rule, text,
};
use iced::{Color, Element, Font, Length, Pixels, Theme};

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
        "grelier.dialog.header_font_size",
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
    let header_bottom_spacing = settings::settings().get_parsed_or(
        "grelier.dialog.header_bottom_spacing",
        DEFAULT_HEADER_BOTTOM_SPACING,
    );
    let list_spacing = settings::settings()
        .get_parsed_or("grelier.menu_dialog.list_spacing", DEFAULT_LIST_SPACING);
    let header_list_spacing = settings::settings().get_parsed_or(
        "grelier.menu_dialog.header_list_spacing",
        DEFAULT_HEADER_LIST_SPACING,
    );
    let container_padding_y = settings::settings().get_parsed_or(
        "grelier.dialog.container_padding_y",
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
    let header_line_height = LineHeight::default()
        .to_absolute(Pixels(header_font_size as f32))
        .0;
    let item_line_height = LineHeight::default()
        .to_absolute(Pixels(item_font_size as f32))
        .0;
    let header_height = header_line_height.ceil() as u32 + header_bottom_spacing;
    let text_height = item_line_height.ceil() as u32;
    let row_height = indicator_size.max(text_height) + button_padding_y * 2;
    let list_height = rows * row_height + list_spacing.saturating_mul(rows.saturating_sub(1));
    let height =
        header_height + header_list_spacing + list_height + container_padding_y.saturating_mul(2);

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
    let checked_icon = svg_asset("option-checked.svg");
    let empty_icon = svg_asset("option-empty.svg");
    let header_font_size = settings::settings().get_parsed_or(
        "grelier.dialog.header_font_size",
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
        "grelier.dialog.container_padding_y",
        DEFAULT_CONTAINER_PADDING_Y,
    );
    let header_bottom_spacing = settings::settings().get_parsed_or(
        "grelier.dialog.header_bottom_spacing",
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
        "grelier.dialog.container_padding_x",
        DEFAULT_CONTAINER_PADDING_X,
    );

    let header = Column::new()
        .width(Length::Fill)
        .push(
            Container::new(
                Text::new(menu.title.clone())
                    .size(header_font_size)
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
        .push(Space::new().height(Length::Fixed(header_bottom_spacing as f32)));

    let mut list = Column::new().width(Length::Fill);

    for GaugeMenuItem {
        id,
        label,
        selected,
    } in &menu.items
    {
        let is_hovered = hovered_item.is_some_and(|hovered| hovered == id.as_str());
        let indicator = Svg::new(if *selected {
            checked_icon.clone()
        } else {
            empty_icon.clone()
        })
        .width(Length::Fixed(indicator_size as f32))
        .height(Length::Fixed(indicator_size as f32))
        .style({
            let is_hovered = is_hovered;
            move |theme: &Theme, status| {
                let palette = theme.extended_palette();
                let hovered = is_hovered || matches!(status, svg::Status::Hovered);
                let color = if hovered {
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
            .spacing(indicator_spacing)
            .push(container(indicator))
            .push(
                Text::new(label.clone())
                    .width(Length::Shrink)
                    .size(item_font_size),
            )
            .push(Space::new().width(Length::Fill));

        let item_id = id.clone();
        let row_button = button(row)
            .padding([button_padding_y as u16, button_padding_x as u16])
            .width(Length::Fill)
            .style(|theme: &Theme, status| {
                let highlight = theme.extended_palette().primary.weak.color;
                let background = match status {
                    button::Status::Hovered | button::Status::Pressed => {
                        Some(highlight.into())
                    }
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

    let content = Container::new(
        Column::new()
            .width(Length::Fill)
            .height(Length::Fill)
            .spacing(header_list_spacing)
            .push(header)
            .push(list.spacing(list_spacing)),
    )
    .padding([container_padding_y as u16, container_padding_x as u16])
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
