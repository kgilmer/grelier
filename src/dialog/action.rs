// Action dialog sizing and rendering for gauge popup dialogs.
// Consumes Settings: grelier.dialog.*, grelier.action_dialog.*.
use crate::dialog::common::{self, BorderSettings};
use crate::panels::gauges::gauge::{GaugeActionDialog, GaugeActionItem};
use crate::settings;
use iced::alignment;
use iced::widget::svg::{self, Svg};
use iced::widget::{Container, Row, button, container};
use iced::{Element, Length, Theme};

const DEFAULT_ICON_SIZE: u32 = 20;
const DEFAULT_BUTTON_PADDING_Y: u32 = 2;
const DEFAULT_BUTTON_PADDING_X: u32 = 2;
const DEFAULT_ITEM_SPACING_X: u32 = 8;
const DEFAULT_BORDER_PADDING_Y: u32 = 4;
const DEFAULT_BORDER_PADDING_X: u32 = 2;
const DEFAULT_MIN_WIDTH: u32 = 0;
const DEFAULT_MAX_WIDTH: u32 = 4096;

struct ActionDialogSettings {
    min_width: u32,
    max_width: u32,
    icon_size: u32,
    button_padding_y: u32,
    button_padding_x: u32,
    item_spacing_x: u32,
    border_padding_y: u32,
    border_padding_x: u32,
}

impl ActionDialogSettings {
    fn load() -> Self {
        let settings = settings::settings();
        Self {
            min_width: settings.get_parsed_or("grelier.action_dialog.min_width", DEFAULT_MIN_WIDTH),
            max_width: settings.get_parsed_or("grelier.action_dialog.max_width", DEFAULT_MAX_WIDTH),
            icon_size: settings.get_parsed_or("grelier.action_dialog.icon_size", DEFAULT_ICON_SIZE),
            button_padding_y: settings.get_parsed_or(
                "grelier.action_dialog.button_padding_y",
                DEFAULT_BUTTON_PADDING_Y,
            ),
            button_padding_x: settings.get_parsed_or(
                "grelier.action_dialog.button_padding_x",
                DEFAULT_BUTTON_PADDING_X,
            ),
            item_spacing_x: settings.get_parsed_or(
                "grelier.action_dialog.item_spacing_x",
                DEFAULT_ITEM_SPACING_X,
            ),
            border_padding_y: settings.get_parsed_or(
                "grelier.action_dialog.border_padding_y",
                DEFAULT_BORDER_PADDING_Y,
            ),
            border_padding_x: settings.get_parsed_or(
                "grelier.action_dialog.border_padding_x",
                DEFAULT_BORDER_PADDING_X,
            ),
        }
    }
}

/// Calculate a reasonable window size for an action dialog based on button count.
pub fn dialog_dimensions(dialog: &GaugeActionDialog) -> (u32, u32) {
    let cfg = ActionDialogSettings::load();

    let button_height = cfg.icon_size + cfg.button_padding_y * 2;
    let height = button_height + cfg.border_padding_y.saturating_mul(2);

    let button_width = cfg.icon_size + cfg.button_padding_x * 2;
    let buttons = dialog.items.len().max(1) as u32;
    let buttons_width =
        buttons * button_width + cfg.item_spacing_x.saturating_mul(buttons.saturating_sub(1));
    let width = (buttons_width + cfg.border_padding_x * 2).clamp(cfg.min_width, cfg.max_width);

    (width, height)
}

pub fn action_view<'a, Message: Clone + 'a>(
    dialog: &'a GaugeActionDialog,
    on_select: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    let cfg = ActionDialogSettings::load();
    let border_settings = BorderSettings::load();

    let mut buttons = Row::new()
        .width(Length::Shrink)
        .align_y(alignment::Vertical::Center)
        .spacing(cfg.item_spacing_x);

    for GaugeActionItem { id, icon } in &dialog.items {
        let item_id = id.clone();
        let icon = Svg::new(icon.clone())
            .width(Length::Fixed(cfg.icon_size as f32))
            .height(Length::Fixed(cfg.icon_size as f32))
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
            .padding([cfg.button_padding_y as u16, cfg.button_padding_x as u16])
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

    let content = common::dialog_surface(
        container(buttons)
            .width(Length::Fill)
            .height(Length::Fill)
            .align_x(alignment::Horizontal::Center)
            .align_y(alignment::Vertical::Center),
        cfg.border_padding_y as u16,
        cfg.border_padding_x as u16,
    );

    common::stack_with_border(content, border_settings, common::popup_border_sides())
}
