// App launcher dialog sizing and rendering for top-apps popup.
// Consumes Settings: grelier.dialog.*, grelier.app_launcher_dialog.*.
use std::sync::LazyLock;

use crate::dialog::common::{self, BorderSettings};
use crate::settings;
use elbey_cache::{FALLBACK_ICON_HANDLE, IconHandle};
use iced::alignment;
use iced::widget::Id as TextInputId;
use iced::widget::image::Image;
use iced::widget::svg::Svg;
use iced::widget::{Column, Row, Space, Text, button, container, scrollable, text_input};
use iced::{Element, Length, Theme};

const DEFAULT_TITLE: &str = "Launch Application";
const DEFAULT_MIN_WIDTH: u32 = 340;
const DEFAULT_MAX_WIDTH: u32 = 840;
const DEFAULT_CHAR_WIDTH: u32 = 7;
const DEFAULT_LABEL_PADDING: u32 = 140;
const DEFAULT_HEADER_FONT_SIZE: u32 = 14;
const DEFAULT_ITEM_FONT_SIZE: u32 = 12;
const DEFAULT_FILTER_FONT_SIZE: u32 = 12;
const DEFAULT_ICON_SIZE: u32 = 16;
const DEFAULT_BUTTON_PADDING_Y: u32 = 4;
const DEFAULT_BUTTON_PADDING_X: u32 = 6;
const DEFAULT_LIST_SPACING: u32 = 6;
const DEFAULT_HEADER_LIST_SPACING: u32 = 6;
const DEFAULT_CONTAINER_PADDING_Y: u32 = 10;
const DEFAULT_CONTAINER_PADDING_X: u32 = 10;
const DEFAULT_HEADER_BOTTOM_SPACING: u32 = 4;
const DEFAULT_ICON_SPACING: u32 = 10;
const DEFAULT_FILTER_HINT: &str = "Filter applications...";
const DEFAULT_MAX_ROWS: u32 = 12;

static FILTER_INPUT_ID: LazyLock<TextInputId> =
    LazyLock::new(|| TextInputId::new("top_apps_launcher_filter"));

#[derive(Debug, Clone)]
pub struct LauncherAppItem {
    pub appid: String,
    pub title: String,
    pub lower_title: String,
    pub exec_count: usize,
    pub icon_handle: IconHandle,
}

#[derive(Debug, Clone)]
pub struct AppLauncherDialog {
    pub title: String,
    pub filter: String,
    pub items: Vec<LauncherAppItem>,
    pub selected_index: Option<usize>,
}

impl AppLauncherDialog {
    pub fn new(items: Vec<LauncherAppItem>) -> Self {
        Self {
            title: DEFAULT_TITLE.to_string(),
            filter: String::new(),
            items,
            selected_index: None,
        }
    }

    pub fn filtered_items(&self) -> Vec<&LauncherAppItem> {
        let query = self.filter.trim().to_ascii_lowercase();
        if query.is_empty() {
            return self.items.iter().collect();
        }

        self.items
            .iter()
            .filter(|item| item.lower_title.contains(&query))
            .collect()
    }

    pub fn clear_selection(&mut self) {
        self.selected_index = None;
    }

    pub fn select_first(&mut self) {
        let filtered_len = self.filtered_items().len();
        if filtered_len > 0 {
            self.selected_index = Some(0);
        } else {
            self.selected_index = None;
        }
    }

    pub fn move_selection(&mut self, delta: i32) {
        let filtered_len = self.filtered_items().len();
        if filtered_len == 0 {
            self.selected_index = None;
            return;
        }

        let current = self.selected_index.unwrap_or(0) as i32;
        let next = (current + delta).clamp(0, filtered_len.saturating_sub(1) as i32) as usize;
        self.selected_index = Some(next);
    }

    pub fn selected_appid(&self) -> Option<String> {
        let filtered = self.filtered_items();
        let index = self.selected_index?;
        filtered.get(index).map(|item| item.appid.clone())
    }
}

pub fn filter_input_id() -> TextInputId {
    FILTER_INPUT_ID.clone()
}

struct AppLauncherDialogSettings {
    min_width: u32,
    max_width: u32,
    char_width: u32,
    label_padding: u32,
    header_font_size: u32,
    item_font_size: u32,
    filter_font_size: u32,
    icon_size: u32,
    button_padding_y: u32,
    button_padding_x: u32,
    list_spacing: u32,
    header_list_spacing: u32,
    container_padding_y: u32,
    container_padding_x: u32,
    header_bottom_spacing: u32,
    icon_spacing: u32,
    max_rows: u32,
    filter_hint: String,
}

impl AppLauncherDialogSettings {
    fn load() -> Self {
        let settings = settings::settings();
        Self {
            min_width: settings
                .get_parsed_or("grelier.app_launcher_dialog.min_width", DEFAULT_MIN_WIDTH),
            max_width: settings
                .get_parsed_or("grelier.app_launcher_dialog.max_width", DEFAULT_MAX_WIDTH),
            char_width: settings
                .get_parsed_or("grelier.app_launcher_dialog.char_width", DEFAULT_CHAR_WIDTH),
            label_padding: settings.get_parsed_or(
                "grelier.app_launcher_dialog.label_padding",
                DEFAULT_LABEL_PADDING,
            ),
            header_font_size: settings
                .get_parsed_or("grelier.dialog.header.font_size", DEFAULT_HEADER_FONT_SIZE),
            item_font_size: settings.get_parsed_or(
                "grelier.app_launcher_dialog.item_font_size",
                DEFAULT_ITEM_FONT_SIZE,
            ),
            filter_font_size: settings.get_parsed_or(
                "grelier.app_launcher_dialog.filter_font_size",
                DEFAULT_FILTER_FONT_SIZE,
            ),
            icon_size: settings
                .get_parsed_or("grelier.app_launcher_dialog.icon_size", DEFAULT_ICON_SIZE),
            button_padding_y: settings.get_parsed_or(
                "grelier.app_launcher_dialog.button_padding_y",
                DEFAULT_BUTTON_PADDING_Y,
            ),
            button_padding_x: settings.get_parsed_or(
                "grelier.app_launcher_dialog.button_padding_x",
                DEFAULT_BUTTON_PADDING_X,
            ),
            list_spacing: settings.get_parsed_or(
                "grelier.app_launcher_dialog.list_spacing",
                DEFAULT_LIST_SPACING,
            ),
            header_list_spacing: settings.get_parsed_or(
                "grelier.app_launcher_dialog.header_list_spacing",
                DEFAULT_HEADER_LIST_SPACING,
            ),
            container_padding_y: settings.get_parsed_or(
                "grelier.dialog.container.padding_y",
                DEFAULT_CONTAINER_PADDING_Y,
            ),
            container_padding_x: settings.get_parsed_or(
                "grelier.dialog.container.padding_x",
                DEFAULT_CONTAINER_PADDING_X,
            ),
            header_bottom_spacing: settings.get_parsed_or(
                "grelier.dialog.header.bottom_spacing",
                DEFAULT_HEADER_BOTTOM_SPACING,
            ),
            icon_spacing: settings.get_parsed_or(
                "grelier.app_launcher_dialog.icon_spacing",
                DEFAULT_ICON_SPACING,
            ),
            max_rows: settings
                .get_parsed_or("grelier.app_launcher_dialog.max_rows", DEFAULT_MAX_ROWS),
            filter_hint: settings.get_or(
                "grelier.app_launcher_dialog.filter_hint",
                DEFAULT_FILTER_HINT,
            ),
        }
    }
}

pub fn dialog_dimensions(dialog: &AppLauncherDialog) -> (u32, u32) {
    let cfg = AppLauncherDialogSettings::load();
    let max_label_chars = dialog
        .items
        .iter()
        .map(|item| item.title.chars().count() as u32)
        .max()
        .unwrap_or(0);
    let width =
        (max_label_chars * cfg.char_width + cfg.label_padding).clamp(cfg.min_width, cfg.max_width);

    let row_height = cfg
        .icon_size
        .max(cfg.item_font_size)
        .saturating_add(cfg.button_padding_y.saturating_mul(2));
    let rows = dialog.filtered_items().len().max(1) as u32;
    let visible_rows = rows.min(cfg.max_rows.max(1));
    let list_height = visible_rows.saturating_mul(row_height)
        + cfg
            .list_spacing
            .saturating_mul(visible_rows.saturating_sub(1));
    let filter_height = cfg
        .filter_font_size
        .saturating_add(cfg.button_padding_y.saturating_mul(2))
        .saturating_add(8);
    let header_height = cfg.header_font_size + cfg.header_bottom_spacing;
    let height = header_height
        + cfg.header_list_spacing
        + filter_height
        + cfg.header_list_spacing
        + list_height
        + cfg.container_padding_y.saturating_mul(2);

    (width, height)
}

pub fn launcher_view<'a, Message: Clone + 'a>(
    dialog: &'a AppLauncherDialog,
    on_filter_click: impl Fn() -> Message + 'a,
    on_filter_input: impl Fn(String) -> Message + 'a,
    on_select: impl Fn(String) -> Message + 'a,
) -> Element<'a, Message> {
    let border_settings = BorderSettings::load();
    let cfg = AppLauncherDialogSettings::load();

    let header = Column::new()
        .width(Length::Fill)
        .push(common::dialog_title(
            dialog.title.as_str(),
            cfg.header_font_size,
        ))
        .push(Space::new().height(Length::Fixed(cfg.header_bottom_spacing as f32)));

    let filter_box = iced::widget::mouse_area(
        text_input(cfg.filter_hint.as_str(), dialog.filter.as_str())
            .id(filter_input_id())
            .on_input(on_filter_input)
            .size(cfg.filter_font_size)
            .padding([cfg.button_padding_y as u16, cfg.button_padding_x as u16])
            .width(Length::Fill),
    )
    .on_press(on_filter_click());

    let filtered = dialog.filtered_items();
    let mut list = Column::new().width(Length::Fill).spacing(cfg.list_spacing);

    if filtered.is_empty() {
        list = list.push(
            container(Text::new("No matches").size(cfg.item_font_size))
                .width(Length::Fill)
                .padding([cfg.button_padding_y as u16, cfg.button_padding_x as u16]),
        );
    } else {
        for (index, item) in filtered.into_iter().enumerate() {
            let is_selected = dialog
                .selected_index
                .is_some_and(|selected| selected == index);
            let icon_handle = match &item.icon_handle {
                IconHandle::NotLoaded => &*FALLBACK_ICON_HANDLE,
                handle => handle,
            };
            let icon: Element<'_, Message> = match icon_handle {
                IconHandle::Raster(handle) => Image::new(handle.clone())
                    .width(Length::Fixed(cfg.icon_size as f32))
                    .height(Length::Fixed(cfg.icon_size as f32))
                    .into(),
                IconHandle::Vector(handle) => Svg::new(handle.clone())
                    .width(Length::Fixed(cfg.icon_size as f32))
                    .height(Length::Fixed(cfg.icon_size as f32))
                    .into(),
                IconHandle::NotLoaded => container(Space::new())
                    .width(Length::Fixed(cfg.icon_size as f32))
                    .height(Length::Fixed(cfg.icon_size as f32))
                    .into(),
            };

            let row = Row::new()
                .width(Length::Fill)
                .align_y(alignment::Vertical::Center)
                .spacing(cfg.icon_spacing)
                .push(container(icon))
                .push(
                    Text::new(item.title.as_str())
                        .width(Length::Shrink)
                        .size(cfg.item_font_size),
                )
                .push(Space::new().width(Length::Fill));
            let app_id = item.appid.clone();
            let row_button = button(row)
                .padding([cfg.button_padding_y as u16, cfg.button_padding_x as u16])
                .width(Length::Fill)
                .style(move |theme: &Theme, status| {
                    let highlight = theme.extended_palette().primary.weak.color;
                    let background = match status {
                        button::Status::Hovered | button::Status::Pressed => Some(highlight.into()),
                        button::Status::Active | button::Status::Disabled => {
                            if is_selected {
                                Some(highlight.into())
                            } else {
                                None
                            }
                        }
                    };
                    button::Style {
                        background,
                        text_color: theme.palette().text,
                        ..button::Style::default()
                    }
                })
                .on_press(on_select(app_id));
            list = list.push(row_button);
        }
    }

    let content = common::dialog_surface(
        Column::new()
            .width(Length::Fill)
            .height(Length::Fill)
            .spacing(cfg.header_list_spacing)
            .push(header)
            .push(filter_box)
            .push(scrollable(list).width(Length::Fill).height(Length::Fill)),
        cfg.container_padding_y as u16,
        cfg.container_padding_x as u16,
    );

    common::stack_with_border(content, border_settings, common::popup_border_sides())
}
