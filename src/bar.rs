// Bar application state, update handling, and view composition for panels.
// Consumes Settings: grelier.bar.width, grelier.bar.border.*.
use std::collections::{HashMap, HashSet};
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::dialog::action::{action_view, dialog_dimensions as action_dialog_dimensions};
use crate::dialog::app_launcher::{AppLauncherDialog, launcher_view};
use crate::dialog::info::{InfoDialog, dialog_dimensions as info_dialog_dimensions, info_view};
use crate::dialog::menu::{dialog_dimensions as menu_dialog_dimensions, menu_view};
use crate::panels::gauges::gauge::{GaugeActionDialog, GaugeInput, GaugeMenu, GaugeModel};
use crate::settings;
use crate::sway_workspace::{WorkspaceApps, WorkspaceInfo};
use elbey_cache::{AppDescriptor, FALLBACK_ICON_HANDLE, IconHandle};
use iced::alignment;
use iced::widget::image::Image;
use iced::widget::svg::Svg;
use iced::widget::{Column, Row, Space, Stack, container, mouse_area, rule};
use iced::{Color, Element, Length, Task, Theme, mouse, window};
use iced_layershell::actions::IcedNewPopupSettings;
use iced_layershell::reexport::{
    Anchor, KeyboardInteractivity, Layer, NewLayerShellSettings, OutputOption,
};
use iced_layershell::to_layer_message;

const CLICK_FILTER_WINDOW: Duration = Duration::from_millis(250);
/// Default panel ordering when no explicit panel list is set.
pub const DEFAULT_PANELS: &str = "workspaces,top_apps,gauges";

/// Application-level messages for the bar, panels, and dialogs.
#[to_layer_message(multi)]
#[derive(Debug, Clone)]
pub enum Message {
    Workspaces {
        workspaces: Vec<WorkspaceInfo>,
        apps: Vec<WorkspaceApps>,
    },
    WorkspaceClicked(String),
    WorkspaceAppClicked {
        con_id: i64,
        app_id: String,
    },
    TopAppClicked {
        app_id: String,
    },
    TopAppsLauncherClicked,
    TopAppsLauncherShortcut,
    TopAppsLauncherFilterClicked,
    TopAppsLauncherFilterChanged(String),
    TopAppsLauncherItemSelected(String),
    BackgroundClicked,
    Gauge(GaugeModel),
    GaugeClicked {
        id: String,
        input: GaugeInput,
    },
    MenuItemSelected {
        window: iced::window::Id,
        gauge_id: String,
        item_id: String,
    },
    ActionItemSelected {
        window: iced::window::Id,
        gauge_id: String,
        item_id: String,
    },
    MenuItemHoverEnter {
        window: iced::window::Id,
        item_id: String,
    },
    MenuItemHoverExit {
        window: iced::window::Id,
        item_id: String,
    },
    WindowFocusChanged {
        focused: bool,
    },
    WindowOpened(iced::window::Id),
    WindowEvent(iced::window::Id, iced::window::Event),
    MenuDismissed(iced::window::Id),
    WindowClosed(iced::window::Id),
    CacheRefreshed(Result<(Vec<AppDescriptor>, Vec<AppDescriptor>), String>),
    OutputChanged,
    IcedEvent(iced::Event),
}

/// Supported panel types that can be ordered in the bar.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum PanelKind {
    Workspaces,
    TopApps,
    Gauges,
}

impl PanelKind {
    pub fn parse(value: &str) -> Option<Self> {
        match value.trim().to_ascii_lowercase().as_str() {
            "workspaces" => Some(PanelKind::Workspaces),
            "top_apps" => Some(PanelKind::TopApps),
            "gauges" => Some(PanelKind::Gauges),
            _ => None,
        }
    }

    pub fn as_str(&self) -> &'static str {
        match self {
            PanelKind::Workspaces => "workspaces",
            PanelKind::TopApps => "top_apps",
            PanelKind::Gauges => "gauges",
        }
    }
}

pub const PANEL_KINDS: &[PanelKind] =
    &[PanelKind::Workspaces, PanelKind::TopApps, PanelKind::Gauges];

pub(crate) fn close_window_task(window: window::Id) -> Task<Message> {
    let callback = iced_layershell::actions::ActionCallback::new(|_region| {});
    Task::batch([
        Task::done(Message::SetInputRegion {
            id: window,
            callback,
        }),
        Task::done(Message::RemoveWindow(window)),
    ])
}

pub fn list_panels() {
    for kind in PANEL_KINDS {
        println!("{}", kind.as_str());
    }
}

/// Parse a comma-delimited panel list into a de-duplicated order.
pub fn panel_order_from_setting(setting: &str) -> Vec<PanelKind> {
    let mut ordered = Vec::new();
    let mut seen = HashSet::new();
    for raw in setting.split(',') {
        let Some(kind) = PanelKind::parse(raw) else {
            continue;
        };
        if seen.insert(kind) {
            ordered.push(kind);
        }
    }
    ordered
}

/// Simple container wrapper for a panel element.
pub struct Panel<'a> {
    content: Element<'a, Message>,
}

impl<'a> Panel<'a> {
    pub fn new(content: impl Into<Element<'a, Message>>) -> Self {
        Self {
            content: content.into(),
        }
    }

    pub fn view(self) -> Element<'a, Message> {
        container(self.content).width(Length::Fill).into()
    }
}

pub(crate) fn lerp_color(from: Color, to: Color, t: f32) -> Color {
    let t = t.clamp(0.0, 1.0);
    Color {
        r: from.r + (to.r - from.r) * t,
        g: from.g + (to.g - from.g) * t,
        b: from.b + (to.b - from.b) * t,
        a: from.a + (to.a - from.a) * t,
    }
}

pub(crate) fn app_icon_view(handle: &IconHandle, size: f32) -> Element<'_, Message> {
    match handle {
        IconHandle::Raster(handle) => Image::new(handle.clone())
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .into(),
        IconHandle::Vector(handle) => Svg::new(handle.clone())
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .into(),
        IconHandle::NotLoaded => match &*FALLBACK_ICON_HANDLE {
            IconHandle::Raster(handle) => Image::new(handle.clone())
                .width(Length::Fixed(size))
                .height(Length::Fixed(size))
                .into(),
            IconHandle::Vector(handle) => Svg::new(handle.clone())
                .width(Length::Fixed(size))
                .height(Length::Fixed(size))
                .into(),
            IconHandle::NotLoaded => container(Space::new())
                .width(Length::Fixed(size))
                .height(Length::Fixed(size))
                .into(),
        },
    }
}

/// Bar placement on the screen edge.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Orientation {
    #[default]
    Left,
    Right,
}

impl std::str::FromStr for Orientation {
    type Err = String;

    fn from_str(s: &str) -> Result<Self, Self::Err> {
        match s.to_ascii_lowercase().as_str() {
            "left" => Ok(Orientation::Left),
            "right" => Ok(Orientation::Right),
            other => Err(format!(
                "Invalid orientation '{other}', expected 'left' or 'right'",
            )),
        }
    }
}

/// Runtime state for the bar, including panels, dialogs, and cache.
pub struct BarState {
    pub workspaces: Vec<WorkspaceInfo>,
    pub workspace_apps: HashMap<String, Vec<crate::sway_workspace::WorkspaceApp>>,
    pub top_apps: Vec<AppDescriptor>,
    pub app_catalog: Vec<AppDescriptor>,
    pub app_icons: AppIconCache,
    pub gauges: Vec<GaugeModel>,
    pub gauge_order: Vec<String>,
    pub bar_theme: Theme,
    pub themed_svg_cache: Arc<Mutex<HashMap<String, iced::widget::svg::Handle>>>,
    pub current_workspace: Option<String>,
    pub previous_workspace: Option<String>,
    pub dialog_windows: HashMap<window::Id, GaugeDialogWindow>,
    pub launcher_window: Option<window::Id>,
    pub launcher_window_opened: bool,
    pub launcher: Option<AppLauncherDialog>,
    pub last_cursor: Option<iced::Point>,
    pub closing_dialogs: HashSet<window::Id>,
    pub gauge_dialog_anchor: HashMap<String, i32>,
    pub primary_window: Option<window::Id>,
    pub pending_primary_window: bool,
    pub bar_windows: HashSet<window::Id>,
    pub last_click_at: Option<Instant>,
    pub last_dialog_opened_at: Option<Instant>,
    pub last_output_change_at: Option<Instant>,
    pub last_bar_window_opened_at: Option<Instant>,
    pub last_outputs: Option<Vec<OutputSnapshot>>,
}

impl Default for BarState {
    fn default() -> Self {
        Self {
            workspaces: Vec::new(),
            workspace_apps: HashMap::new(),
            top_apps: Vec::new(),
            app_catalog: Vec::new(),
            app_icons: AppIconCache::default(),
            gauges: Vec::new(),
            gauge_order: Vec::new(),
            bar_theme: Theme::Nord,
            themed_svg_cache: Arc::new(Mutex::new(HashMap::new())),
            current_workspace: None,
            previous_workspace: None,
            dialog_windows: HashMap::new(),
            launcher_window: None,
            launcher_window_opened: false,
            launcher: None,
            last_cursor: None,
            closing_dialogs: HashSet::new(),
            gauge_dialog_anchor: HashMap::new(),
            primary_window: None,
            pending_primary_window: false,
            bar_windows: HashSet::new(),
            last_click_at: None,
            last_dialog_opened_at: None,
            last_output_change_at: None,
            last_bar_window_opened_at: None,
            last_outputs: None,
        }
    }
}

#[derive(Clone, Debug, PartialEq, Eq)]
pub struct OutputSnapshot {
    pub name: String,
    pub active: bool,
    pub rect: (i32, i32, i32, i32),
}

/// Lookup cache for app icon handles by app id or title.
#[derive(Clone, Default)]
pub struct AppIconCache {
    by_appid: HashMap<String, IconHandle>,
    by_lower_title: HashMap<String, IconHandle>,
    by_icon_name: HashMap<String, IconHandle>,
}

impl AppIconCache {
    pub fn from_app_descriptors_ref(apps: &[elbey_cache::AppDescriptor]) -> Self {
        let mut cache = AppIconCache::default();
        for app in apps {
            cache
                .by_appid
                .insert(app.appid.clone(), app.icon_handle.clone());
            cache
                .by_lower_title
                .insert(app.lower_title.clone(), app.icon_handle.clone());
            if let Some(icon_name) = app.icon_name.as_ref() {
                cache
                    .by_icon_name
                    .insert(icon_name.to_string(), app.icon_handle.clone());
            }
        }
        cache
    }

    pub fn icon_for(&self, app_id: &str) -> Option<&IconHandle> {
        let lower = app_id.to_ascii_lowercase();
        self.by_appid
            .get(app_id)
            .or_else(|| self.by_appid.get(&lower))
            .or_else(|| self.by_lower_title.get(&lower))
            .or_else(|| self.by_icon_name.get(app_id))
            .or_else(|| self.by_icon_name.get(&lower))
    }
}

/// Dialog payload associated with a gauge.
#[derive(Clone)]
pub enum GaugeDialog {
    Menu(GaugeMenu),
    Action(GaugeActionDialog),
    Info(InfoDialog),
}

/// Tracking info for an open gauge dialog window.
#[derive(Clone)]
pub struct GaugeDialogWindow {
    pub gauge_id: String,
    pub dialog: GaugeDialog,
    pub hovered_item: Option<String>,
}

impl BarState {
    fn dialog_offset_x() -> i32 {
        settings::settings().get_parsed_or("grelier.bar.width", 28u32) as i32
    }

    pub fn with_gauge_order_and_icons(
        gauge_order: Vec<String>,
        app_icons: AppIconCache,
        app_catalog: Vec<AppDescriptor>,
        top_apps: Vec<AppDescriptor>,
    ) -> Self {
        Self {
            gauge_order,
            top_apps,
            app_catalog,
            app_icons,
            ..Self::default()
        }
    }

    pub fn namespace() -> String {
        env!("CARGO_PKG_NAME").to_string()
    }

    pub fn open_menu(
        &mut self,
        gauge_id: &str,
        menu: GaugeMenu,
        anchor_y: Option<i32>,
    ) -> Task<Message> {
        let (width, height) = menu_dialog_dimensions(&menu);
        self.open_dialog_window(gauge_id, GaugeDialog::Menu(menu), anchor_y, (width, height))
    }

    pub fn open_action_dialog(
        &mut self,
        gauge_id: &str,
        dialog: GaugeActionDialog,
        anchor_y: Option<i32>,
    ) -> Task<Message> {
        let (width, height) = action_dialog_dimensions(&dialog);
        self.open_dialog_window(
            gauge_id,
            GaugeDialog::Action(dialog),
            anchor_y,
            (width, height),
        )
    }

    pub fn open_info_dialog(
        &mut self,
        gauge_id: &str,
        dialog: InfoDialog,
        anchor_y: Option<i32>,
    ) -> Task<Message> {
        let (width, height) = info_dialog_dimensions(&dialog);
        self.open_dialog_window(
            gauge_id,
            GaugeDialog::Info(dialog),
            anchor_y,
            (width, height),
        )
    }

    fn open_dialog_window(
        &mut self,
        gauge_id: &str,
        dialog: GaugeDialog,
        anchor_y: Option<i32>,
        size: (u32, u32),
    ) -> Task<Message> {
        let mut tasks = vec![self.close_dialogs()];

        let (width, height) = size;
        let bar_width = Self::dialog_offset_x();
        let anchor_y = anchor_y
            .or_else(|| self.gauge_dialog_anchor.get(gauge_id).copied())
            .or_else(|| self.last_cursor.map(|p| p.y as i32))
            .unwrap_or_default();
        // Use workspace bounds to keep the popup within the visible screen height.
        let screen_height = self
            .workspaces
            .iter()
            .map(|ws| ws.rect.y + ws.rect.height)
            .max()
            .unwrap_or(height as i32);
        let max_top = (screen_height - height as i32).max(0);
        // Center the popup around the anchor and keep it on-screen vertically.
        let position_y = anchor_y.saturating_sub(height as i32 / 2).clamp(0, max_top);

        let settings = IcedNewPopupSettings {
            size: (width, height),
            position: (bar_width, position_y),
        };
        let (window, task) = Message::popup_open(settings);
        self.gauge_dialog_anchor
            .insert(gauge_id.to_string(), anchor_y);
        self.dialog_windows.insert(
            window,
            GaugeDialogWindow {
                gauge_id: gauge_id.to_string(),
                dialog,
                hovered_item: None,
            },
        );
        self.last_dialog_opened_at = Some(Instant::now());
        tasks.push(task);

        Task::batch(tasks)
    }

    pub fn close_dialogs(&mut self) -> Task<Message> {
        let ids: Vec<window::Id> = self.dialog_windows.drain().map(|(id, _)| id).collect();
        self.closing_dialogs.extend(&ids);
        Task::batch(ids.into_iter().map(close_window_task))
    }

    pub fn open_top_apps_launcher(
        &mut self,
        launcher: AppLauncherDialog,
        size: (u32, u32),
    ) -> Task<Message> {
        let mut tasks = vec![self.close_dialogs(), self.close_top_apps_launcher()];
        let (width, height) = size;
        let orientation_raw = settings::settings().get_or("grelier.bar.orientation", "left");
        let orientation = orientation_raw
            .parse::<Orientation>()
            .unwrap_or(Orientation::Left);
        let screen_height = self
            .workspaces
            .iter()
            .map(|ws| ws.rect.y + ws.rect.height)
            .max()
            .unwrap_or(height as i32);
        let anchor_y = self
            .last_cursor
            .map(|p| p.y as i32)
            .unwrap_or(screen_height / 2);
        let max_top = (screen_height - height as i32).max(0);
        let position_y = anchor_y.saturating_sub(height as i32 / 2).clamp(0, max_top);

        let (anchor, margin) = match orientation {
            // Popup dialogs are positioned relative to the bar surface (x = bar width),
            // while this launcher is a top-level layer-shell surface positioned relative
            // to the output. Using horizontal margin 0 keeps it aligned with other dialogs.
            Orientation::Left => (Anchor::Left | Anchor::Top, (position_y, 0, 0, 0)),
            Orientation::Right => (Anchor::Right | Anchor::Top, (position_y, 0, 0, 0)),
        };
        let window = window::Id::unique();
        let layer_settings = NewLayerShellSettings {
            size: Some((width, height)),
            layer: Layer::Top,
            anchor,
            exclusive_zone: Some(0),
            margin: Some(margin),
            // Launcher needs reliable keyboard input even when opened from a global
            // compositor binding (e.g. Super+L), so request explicit keyboard ownership.
            keyboard_interactivity: KeyboardInteractivity::Exclusive,
            output_option: OutputOption::None,
            events_transparent: false,
            namespace: Some(Self::namespace()),
        };
        let task = Task::done(Message::NewLayerShell {
            settings: layer_settings,
            id: window,
        });
        self.launcher_window = Some(window);
        self.launcher_window_opened = false;
        self.launcher = Some(launcher);
        self.last_dialog_opened_at = Some(Instant::now());
        tasks.push(task);
        Task::batch(tasks)
    }

    pub fn close_top_apps_launcher(&mut self) -> Task<Message> {
        let Some(window) = self.launcher_window.take() else {
            return Task::none();
        };
        self.launcher_window_opened = false;
        self.launcher = None;
        self.closing_dialogs.insert(window);
        close_window_task(window)
    }

    pub fn has_open_overlays(&self) -> bool {
        !self.dialog_windows.is_empty() || self.launcher_window.is_some()
    }

    pub fn close_overlays(&mut self) -> Task<Message> {
        Task::batch([self.close_dialogs(), self.close_top_apps_launcher()])
    }

    pub fn allow_click(&mut self) -> bool {
        self.allow_click_at(Instant::now())
    }

    pub(crate) fn allow_click_at(&mut self, now: Instant) -> bool {
        let too_soon_since_click = self
            .last_click_at
            .is_some_and(|last| now.saturating_duration_since(last) < CLICK_FILTER_WINDOW);
        let too_soon_since_dialog = self
            .last_dialog_opened_at
            .is_some_and(|last| now.saturating_duration_since(last) < CLICK_FILTER_WINDOW);

        if too_soon_since_click || too_soon_since_dialog {
            return false;
        }

        self.last_click_at = Some(now);
        true
    }

    pub fn view<'a>(&'a self, window: window::Id) -> Element<'a, Message> {
        let settings = settings::settings();
        let border_blend = settings.get_bool_or("grelier.bar.border.blend", true);
        let border_line_width = settings.get_parsed_or("grelier.bar.border.line_width", 1.0);
        let border_column_width = settings.get_parsed_or("grelier.bar.border.column_width", 3.0);
        let border_mix_1 = settings.get_parsed_or("grelier.bar.border.mix_1", 0.2);
        let border_mix_2 = settings.get_parsed_or("grelier.bar.border.mix_2", 0.6);
        let border_mix_3 = settings.get_parsed_or("grelier.bar.border.mix_3", 1.0);
        let border_alpha_1 = settings.get_parsed_or("grelier.bar.border.alpha_1", 0.6);
        let border_alpha_2 = settings.get_parsed_or("grelier.bar.border.alpha_2", 0.7);
        let border_alpha_3 = settings.get_parsed_or("grelier.bar.border.alpha_3", 0.9);

        if self.launcher_window.is_some_and(|id| id == window) {
            return self
                .launcher
                .as_ref()
                .map(|dialog| {
                    launcher_view(
                        dialog,
                        || Message::TopAppsLauncherFilterClicked,
                        Message::TopAppsLauncherFilterChanged,
                        Message::TopAppsLauncherItemSelected,
                    )
                })
                .unwrap_or_else(|| container(Space::new()).into());
        }

        if let Some(dialog_window) = self.dialog_windows.get(&window) {
            let gauge_id = dialog_window.gauge_id.clone();
            let window_id = window;
            return match &dialog_window.dialog {
                GaugeDialog::Menu(menu) => menu_view(
                    menu,
                    dialog_window.hovered_item.as_deref(),
                    move |item_id| Message::MenuItemSelected {
                        window: window_id,
                        gauge_id: gauge_id.clone(),
                        item_id,
                    },
                    move |item_id| Message::MenuItemHoverEnter {
                        window: window_id,
                        item_id,
                    },
                    move |item_id| Message::MenuItemHoverExit {
                        window: window_id,
                        item_id,
                    },
                ),
                GaugeDialog::Action(dialog) => {
                    action_view(dialog, move |item_id| Message::ActionItemSelected {
                        window: window_id,
                        gauge_id: gauge_id.clone(),
                        item_id,
                    })
                }
                GaugeDialog::Info(dialog) => info_view(dialog),
            };
        }
        if self.closing_dialogs.contains(&window) {
            return container(Space::new()).into();
        }

        let mut panel_order =
            panel_order_from_setting(&settings.get_or("grelier.panels", DEFAULT_PANELS));
        if panel_order.is_empty() {
            panel_order = panel_order_from_setting(DEFAULT_PANELS);
        }

        let mut layout = Column::new().width(Length::Fill).height(Length::Fill);
        let mut iter = panel_order.iter().peekable();
        while let Some(panel) = iter.next() {
            let panel = match panel {
                PanelKind::Workspaces => crate::panels::ws_panel::view(self),
                PanelKind::TopApps => crate::panels::top_apps_panel::view(self),
                PanelKind::Gauges => crate::panels::gauge_panel::view(self),
            };
            layout = layout.push(panel.view());
            if iter.peek().is_some() {
                layout = layout.push(Space::new().height(Length::Fill));
            }
        }

        let filled = container(layout)
            .width(Length::Fill)
            .height(Length::Fill)
            .style(|theme: &Theme| container::Style {
                background: Some(theme.palette().background.into()),
                ..container::Style::default()
            });

        let border = container({
            let line = |mix: f32, alpha: f32| {
                rule::vertical(border_line_width).style(move |theme: &Theme| {
                    let background = theme.palette().background;
                    let blended = if border_blend && mix != 0.0 {
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
                })
            };
            let line1 = line(border_mix_1, border_alpha_1);
            let line2 = line(border_mix_2, border_alpha_2);
            let line3 = line(border_mix_3, border_alpha_3);

            Row::new()
                .spacing(0)
                .push(line1)
                .push(line2)
                .push(line3)
                .width(Length::Fixed(border_column_width))
                .height(Length::Fill)
        })
        .width(Length::Fill)
        .height(Length::Fill)
        .align_x(alignment::Horizontal::Right);

        let layered = Stack::new()
            .width(Length::Fill)
            .height(Length::Fill)
            .push(filled)
            .push(border);

        mouse_area(layered)
            .on_press(Message::BackgroundClicked)
            .on_right_press(Message::BackgroundClicked)
            .interaction(mouse::Interaction::None)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn panel_order_filters_duplicates() {
        let order = panel_order_from_setting("gauges,workspaces,gauges,top_apps");
        let labels: Vec<_> = order.iter().map(PanelKind::as_str).collect();
        assert_eq!(labels, vec!["gauges", "workspaces", "top_apps"]);
    }
}
