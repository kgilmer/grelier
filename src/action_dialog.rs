// Action dialog sizing and rendering for gauge popup dialogs.
// Consumes Settings: grelier.dialog.*, grelier.action_dialog.*.
use crate::panels::gauges::gauge::{GaugeActionDialog, GaugeActionItem};
use crate::settings;
use iced::alignment;
use iced::widget::svg::{self, Svg};
use iced::widget::{Column, Container, Row, Stack, button, container, rule};
use iced::{Color, Element, Length, Theme};

const DEFAULT_ICON_SIZE: u32 = 20;
const DEFAULT_BUTTON_PADDING_Y: u32 = 2;
const DEFAULT_BUTTON_PADDING_X: u32 = 2;
const DEFAULT_ITEM_SPACING_X: u32 = 8;
const DEFAULT_BORDER_PADDING_Y: u32 = 4;
const DEFAULT_BORDER_PADDING_X: u32 = 2;
const DEFAULT_MIN_WIDTH: u32 = 0;
const DEFAULT_MAX_WIDTH: u32 = 4096;

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

fn lerp_color(from: Color, to: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color {
        r: from.r + (to.r - from.r) * t,
        g: from.g + (to.g - from.g) * t,
        b: from.b + (to.b - from.b) * t,
        a: from.a + (to.a - from.a) * t,
    }
}

/// Calculate a reasonable window size for an action dialog based on button count.
pub fn dialog_dimensions(dialog: &GaugeActionDialog) -> (u32, u32) {
    let settings = settings::settings();
    let min_width =
        settings.get_parsed_or("grelier.action_dialog.min_width", DEFAULT_MIN_WIDTH);
    let max_width =
        settings.get_parsed_or("grelier.action_dialog.max_width", DEFAULT_MAX_WIDTH);
    let border_column_width: f32 =
        settings.get_parsed_or("grelier.bar.border.column_width", 3.0);
    let icon_size =
        settings.get_parsed_or("grelier.action_dialog.icon_size", DEFAULT_ICON_SIZE);
    let button_padding_y =
        settings.get_parsed_or("grelier.action_dialog.button_padding_y", DEFAULT_BUTTON_PADDING_Y);
    let button_padding_x =
        settings.get_parsed_or("grelier.action_dialog.button_padding_x", DEFAULT_BUTTON_PADDING_X);
    let item_spacing_x =
        settings.get_parsed_or("grelier.action_dialog.item_spacing_x", DEFAULT_ITEM_SPACING_X);
    let border_padding_y =
        settings.get_parsed_or("grelier.action_dialog.border_padding_y", DEFAULT_BORDER_PADDING_Y);
    let border_padding_x =
        settings.get_parsed_or("grelier.action_dialog.border_padding_x", DEFAULT_BORDER_PADDING_X);

    let button_height = icon_size + button_padding_y * 2;
    let height = button_height
        + border_padding_y.saturating_mul(2)
        + (border_column_width * 2.0_f32).ceil() as u32;

    let button_width = icon_size + button_padding_x * 2;
    let buttons = dialog.items.len().max(1) as u32;
    let buttons_width =
        buttons * button_width + item_spacing_x.saturating_mul(buttons.saturating_sub(1));
    let width = (buttons_width
        + border_padding_x * 2
        + (border_column_width * 2.0_f32).ceil() as u32)
        .clamp(min_width, max_width);

    (width, height)
}

pub fn action_view<'a, Message: Clone + 'a>(
    dialog: &'a GaugeActionDialog,
    on_select: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    let settings = settings::settings();
    let border_settings = BorderSettings::load();
    let icon_size =
        settings.get_parsed_or("grelier.action_dialog.icon_size", DEFAULT_ICON_SIZE);
    let button_padding_y =
        settings.get_parsed_or("grelier.action_dialog.button_padding_y", DEFAULT_BUTTON_PADDING_Y);
    let button_padding_x =
        settings.get_parsed_or("grelier.action_dialog.button_padding_x", DEFAULT_BUTTON_PADDING_X);
    let item_spacing_x =
        settings.get_parsed_or("grelier.action_dialog.item_spacing_x", DEFAULT_ITEM_SPACING_X);
    let border_padding_y =
        settings.get_parsed_or("grelier.action_dialog.border_padding_y", DEFAULT_BORDER_PADDING_Y);
    let border_padding_x =
        settings.get_parsed_or("grelier.action_dialog.border_padding_x", DEFAULT_BORDER_PADDING_X);

    let mut buttons = Row::new()
        .width(Length::Shrink)
        .align_y(alignment::Vertical::Center)
        .spacing(item_spacing_x);

    for GaugeActionItem { id, icon } in &dialog.items {
        let item_id = id.clone();
        let icon = Svg::new(icon.clone())
            .width(Length::Fixed(icon_size as f32))
            .height(Length::Fixed(icon_size as f32))
            .style(|theme: &Theme, status| {
                let palette = theme.extended_palette();
                let hovered = matches!(status, svg::Status::Hovered);
                let color = if hovered {
                    palette.primary.weak.text
                } else {
                    palette.primary.weak.color
                };
                svg::Style { color: Some(color) }
            });
        let button = button(Container::new(icon))
            .padding([button_padding_y as u16, button_padding_x as u16])
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
            .on_press(on_select(item_id));
        buttons = buttons.push(button);
    }

    let content = Container::new(
        container(buttons)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(alignment::Horizontal::Center)
            .align_y(alignment::Vertical::Center),
    )
    .padding([border_padding_y as u16, border_padding_x as u16])
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

    let top_border = container(border_column())
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(alignment::Vertical::Top);

    let bottom_border = container(border_column())
        .width(Length::Fill)
        .height(Length::Fill)
        .align_y(alignment::Vertical::Bottom);

    let left_border = container(border_row())
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(alignment::Horizontal::Left);

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
        .push(left_border)
        .push(right_border)
        .into()
}
