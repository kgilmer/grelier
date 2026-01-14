// Bar application state, update handling, and view composition for workspaces and gauges.
// Consumes Settings: grelier.bar.width, grelier.bar.border_*, grelier.app.*, grelier.ws.*.
use std::collections::{HashMap, HashSet};
use std::convert::TryInto;

use crate::gauge::{
    GaugeClickTarget, GaugeInput, GaugeMenu, GaugeModel, GaugeValue, GaugeValueAttention,
};
use crate::icon::svg_asset;
use crate::info_dialog::{dialog_dimensions as info_dialog_dimensions, info_view, InfoDialog};
use crate::menu_dialog::{dialog_dimensions as menu_dialog_dimensions, menu_view};
use crate::settings;
use crate::sway_workspace::WorkspaceInfo;
use iced::alignment;
use iced::border;
use iced::font::Weight;
use iced::widget::svg::{self, Svg};
use iced::widget::text;
use iced::widget::{Column, Row, Space, Stack, Text, button, container, mouse_area, rule};
use iced::{Border, Color, Element, Font, Length, Task, Theme, mouse, window};
use iced_anim::animation_builder::AnimationBuilder;
use iced_anim::transition::Easing;
use iced_layershell::actions::IcedNewPopupSettings;
use iced_layershell::to_layer_message;

#[to_layer_message(multi)]
#[derive(Debug, Clone)]
pub enum Message {
    Workspaces(Vec<WorkspaceInfo>),
    WorkspaceClicked(String),
    BackgroundClicked,
    Gauge(GaugeModel),
    GaugeClicked {
        id: String,
        target: GaugeClickTarget,
        input: GaugeInput,
    },
    MenuItemSelected {
        window: iced::window::Id,
        gauge_id: String,
        item_id: String,
    },
    MenuDismissed(iced::window::Id),
    WindowClosed(iced::window::Id),
    IcedEvent(iced::Event),
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
                "Invalid orientation '{other}', expected 'left' or 'right'"
            )),
        }
    }
}

fn workspace_color(
    focus_level: f32,
    urgent_level: f32,
    normal: Color,
    focused: Color,
    urgent: Color,
) -> Color {
    let focus_blend = lerp_color(normal, focused, focus_level);
    // Urgent overlays focused; higher priority means this mix wins if present.
    lerp_color(focus_blend, urgent, urgent_level)
}

fn workspace_levels(ws: &WorkspaceInfo) -> (f32, f32) {
    (
        if ws.focused { 1.0 } else { 0.0 },
        if ws.urgent { 1.0 } else { 0.0 },
    )
}

fn scroll_input(delta: mouse::ScrollDelta) -> Option<GaugeInput> {
    match delta {
        mouse::ScrollDelta::Lines { x: _, y } | mouse::ScrollDelta::Pixels { x: _, y } => {
            if y > 0.0 {
                Some(GaugeInput::ScrollUp)
            } else if y < 0.0 {
                Some(GaugeInput::ScrollDown)
            } else {
                None
            }
        }
    }
}

#[derive(Clone, Default)]
pub struct BarState {
    pub workspaces: Vec<WorkspaceInfo>,
    pub gauges: Vec<GaugeModel>,
    pub gauge_order: Vec<String>,
    pub current_workspace: Option<String>,
    pub previous_workspace: Option<String>,
    pub dialog_windows: HashMap<window::Id, GaugeDialogWindow>,
    pub last_cursor: Option<iced::Point>,
    pub closing_dialogs: HashSet<window::Id>,
    pub gauge_dialog_anchor: HashMap<String, i32>,
}

#[derive(Clone)]
pub enum GaugeDialog {
    Menu(GaugeMenu),
    Info(InfoDialog),
}

#[derive(Clone)]
pub struct GaugeDialogWindow {
    pub gauge_id: String,
    pub dialog: GaugeDialog,
}

impl BarState {
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_workspaces(workspaces: Vec<WorkspaceInfo>) -> Self {
        Self {
            workspaces,
            ..Self::default()
        }
    }

    pub fn with_gauge_order(gauge_order: Vec<String>) -> Self {
        Self {
            gauge_order,
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
        self.open_dialog_window(
            gauge_id,
            GaugeDialog::Menu(menu),
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
        let bar_width = settings::settings().get_parsed_or("grelier.bar.width", 28u32) as i32;
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
        let mut position_y = anchor_y.saturating_sub(height as i32 / 2);
        if position_y < 0 {
            position_y = 0;
        }
        if position_y > max_top {
            position_y = max_top;
        }

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
            },
        );
        tasks.push(task);

        Task::batch(tasks)
    }

    pub fn close_dialogs(&mut self) -> Task<Message> {
        let ids: Vec<window::Id> = self.dialog_windows.keys().copied().collect();
        self.dialog_windows.clear();
        for id in &ids {
            self.closing_dialogs.insert(*id);
        }
        Task::batch(ids.into_iter().map(Message::RemoveWindow).map(Task::done))
    }

    pub fn gauge_anchor_y(&self, target: GaugeClickTarget) -> Option<i32> {
        let p = self.last_cursor?;
        // Align to top of icon for the gauge regardless of click location.
        // Icon is 14px tall with no padding; value sits below with a 3px spacer.
        let icon_offset =
            settings::settings().get_parsed_or("grelier.app.gauge_anchor_offset_icon", 7.0);
        let value_offset =
            settings::settings().get_parsed_or("grelier.app.gauge_anchor_offset_value", 28.0);
        let offset = match target {
            GaugeClickTarget::Icon => icon_offset, // half of icon size to reach top
            GaugeClickTarget::Value => value_offset, // approx icon+spacer+half text line
        };
        Some((p.y - offset).round() as i32)
    }

    pub fn update_workspace_focus(&mut self, workspaces: &[WorkspaceInfo]) {
        let workspace_count = workspaces.len();

        // Drop the previous reference if it no longer exists or there's nothing to highlight.
        if workspace_count <= 1
            || self
                .previous_workspace
                .as_ref()
                .is_some_and(|prev| !workspaces.iter().any(|ws| ws.name == *prev))
        {
            self.previous_workspace = None;
        }

        let focused_workspace = workspaces
            .iter()
            .find(|ws| ws.focused)
            .map(|ws| ws.name.clone());

        match focused_workspace {
            Some(ref focused) if Some(focused.as_str()) != self.current_workspace.as_deref() => {
                if workspace_count > 1 {
                    if let Some(current) = self.current_workspace.take() {
                        self.previous_workspace = Some(current);
                    }
                } else {
                    self.previous_workspace = None;
                }

                self.current_workspace = Some(focused.clone());
            }
            Some(_) => {}
            None => self.current_workspace = None,
        }
    }

    fn ordered_gauges(&self) -> Vec<&GaugeModel> {
        let order_index: HashMap<_, _> = self
            .gauge_order
            .iter()
            .enumerate()
            .map(|(i, id)| (id.clone(), i))
            .collect();

        let mut ordered: Vec<(usize, &GaugeModel)> = self.gauges.iter().enumerate().collect();
        ordered
            .sort_by_key(|(idx, g)| (order_index.get(g.id).copied().unwrap_or(usize::MAX), *idx));
        ordered.into_iter().map(|(_, gauge)| gauge).collect()
    }

    pub fn view<'a>(&'a self, window: window::Id) -> Element<'a, Message> {
        let settings = settings::settings();
        let workspace_padding_x = settings.get_parsed_or("grelier.app.workspace_padding_x", 4u16);
        let workspace_padding_y = settings.get_parsed_or("grelier.app.workspace_padding_y", 2u16);
        let workspace_spacing = settings.get_parsed_or("grelier.ws.spacing", 2u32);
        let workspace_button_padding_x =
            settings.get_parsed_or("grelier.app.workspace_button_padding_x", 4u16);
        let workspace_button_padding_y =
            settings.get_parsed_or("grelier.app.workspace_button_padding_y", 4u16);
        let workspace_corner_radius = settings.get_parsed_or("grelier.ws.corner_radius", 5.0_f32);
        let workspace_transitions = settings.get_bool_or("grelier.ws.transitions", true);
        let gauge_padding_x = settings.get_parsed_or("grelier.app.gauge_padding_x", 2u16);
        let gauge_padding_y = settings.get_parsed_or("grelier.app.gauge_padding_y", 2u16);
        let gauge_spacing = settings
            .get_parsed("grelier.gauge.spacing")
            .unwrap_or_else(|| settings.get_parsed_or("grelier.app.gauge_spacing", 18u32));
        let gauge_icon_size = settings.get_parsed_or("grelier.app.gauge_icon_size", 17.0);
        let gauge_value_icon_size =
            settings.get_parsed_or("grelier.app.gauge_value_icon_size", 20.0);
        let gauge_icon_value_spacing =
            settings.get_parsed_or("grelier.app.gauge_icon_value_spacing", 3.0);
        let border_blend = settings.get_bool_or("grelier.bar.border_blend", true);
        let border_line_width = settings.get_parsed_or("grelier.bar.border_line_width", 1.0);
        let border_column_width = settings.get_parsed_or("grelier.bar.border_column_width", 3.0);
        let border_mix_1 = settings.get_parsed_or("grelier.bar.border_mix_1", 0.2);
        let border_mix_2 = settings.get_parsed_or("grelier.bar.border_mix_2", 0.6);
        let border_mix_3 = settings.get_parsed_or("grelier.bar.border_mix_3", 1.0);
        let border_alpha_1 = settings.get_parsed_or("grelier.bar.border_alpha_1", 0.9);
        let border_alpha_2 = settings.get_parsed_or("grelier.bar.border_alpha_2", 0.7);
        let border_alpha_3 = settings.get_parsed_or("grelier.bar.border_alpha_3", 0.9);

        if let Some(dialog_window) = self.dialog_windows.get(&window) {
            let gauge_id = dialog_window.gauge_id.clone();
            let window_id = window;
            return match &dialog_window.dialog {
                GaugeDialog::Menu(menu) => menu_view(menu, move |item_id| {
                    Message::MenuItemSelected {
                        window: window_id,
                        gauge_id: gauge_id.clone(),
                        item_id,
                    }
                }),
                GaugeDialog::Info(dialog) => info_view(dialog),
            };
        }
        if self.closing_dialogs.contains(&window) {
            return container(Space::new()).into();
        }

        let previous_workspace = self.previous_workspace.as_deref();
        let highlight_previous = previous_workspace.is_some() && self.workspaces.len() > 1;

        let workspaces = self.workspaces.iter().fold(
            Column::new()
                .padding([workspace_padding_y, workspace_padding_x])
                .spacing(workspace_spacing),
            |col, ws| {
                let ws_name = ws.name.clone();
                let ws_num = ws.num;
                let (focus_level, urgent_level) = workspace_levels(ws);
                let is_previous = highlight_previous
                    && !ws.focused
                    && previous_workspace == Some(ws.name.as_str());

                let build_workspace = move |focus: f32, urgent: f32| -> Element<'_, Message> {
                    let name = ws_name.clone();
                    let mut label = Text::new(ws_num.to_string())
                        .width(Length::Fill)
                        .align_x(text::Alignment::Center);
                    if focus > 0.0 {
                        label = label.font(Font {
                            weight: Weight::Bold,
                            ..Font::DEFAULT
                        });
                    }

                    let content = container(label)
                        .padding([workspace_button_padding_y, workspace_button_padding_x])
                        .width(Length::Fill)
                        .style(move |theme: &Theme| {
                            let palette = theme.extended_palette();
                            let is_inactive = focus <= 0.0 && urgent <= 0.0;

                            let background_color = if is_inactive {
                                if is_previous {
                                    palette.primary.weak.color
                                } else {
                                    palette.background.strong.color
                                }
                            } else {
                                workspace_color(
                                    focus,
                                    urgent,
                                    palette.background.base.color,
                                    palette.primary.base.color,
                                    palette.success.base.color,
                                )
                            };
                            let text_color = if is_previous {
                                palette.background.base.color
                            } else {
                                let emphasis = focus.max(urgent);
                                lerp_color(
                                    theme.palette().text,
                                    palette.background.base.color,
                                    emphasis,
                                )
                            };
                            let border = Border::default()
                                .rounded(border::Radius::new(workspace_corner_radius));

                            container::Style {
                                background: Some(background_color.into()),
                                border,
                                text_color: Some(text_color),
                                ..container::Style::default()
                            }
                        });

                    button(content)
                        .style(|theme: &Theme, _status| button::Style {
                            background: None,
                            text_color: theme.palette().text,
                            ..button::Style::default()
                        })
                        .padding(0)
                        .width(Length::Fill)
                        .on_press(Message::WorkspaceClicked(name))
                        .into()
                };

                let workspace: Element<'_, Message> = if workspace_transitions {
                    AnimationBuilder::new((focus_level, urgent_level), move |(focus, urgent)| {
                        build_workspace(focus, urgent)
                    })
                    .animation(Easing::EASE_IN_OUT.very_quick())
                    .into()
                } else {
                    build_workspace(focus_level, urgent_level)
                };

                col.push(workspace)
            },
        );

        let ordered_gauges = self.ordered_gauges();
        let error_icon = svg_asset("error.svg");

        let gauges = ordered_gauges.into_iter().fold(
            Column::new()
                .padding([gauge_padding_y, gauge_padding_x])
                .spacing(gauge_spacing)
                .width(Length::Fill)
                .align_x(alignment::Horizontal::Center),
            |col, gauge| {
                let gauge_attention = if gauge.value.is_some() {
                    gauge.attention
                } else {
                    GaugeValueAttention::Danger
                };

                let mut gauge_column = Column::new()
                    .align_x(alignment::Horizontal::Center)
                    .width(Length::Fill);

                if let Some(icon) = &gauge.icon {
                    let icon_view = Svg::new(icon.clone())
                        .width(Length::Fixed(gauge_icon_size))
                        .height(Length::Fixed(gauge_icon_size))
                        .style({
                            let attention = gauge_attention;
                            move |theme: &Theme, _status| svg::Style {
                                color: Some(match attention {
                                    GaugeValueAttention::Nominal => theme.palette().text,
                                    GaugeValueAttention::Warning => {
                                        theme.extended_palette().warning.base.color
                                    }
                                    GaugeValueAttention::Danger => {
                                        theme.extended_palette().danger.base.color
                                    }
                                }),
                            }
                        });
                    let centered_icon: Element<'_, Message> = container(icon_view)
                        .width(Length::Fill)
                        .align_x(alignment::Horizontal::Center)
                        .into();
                    gauge_column = gauge_column
                        .push(centered_icon)
                        .push(Space::new().height(Length::Fixed(gauge_icon_value_spacing)));
                }

                let value: Element<'_, Message> = match &gauge.value {
                    Some(GaugeValue::Text(value)) => {
                        let attention = gauge_attention;
                        Text::new(value.clone())
                            .width(Length::Fill)
                            .align_x(text::Alignment::Center)
                            .style(move |theme: &Theme| text::Style {
                                color: Some(match attention {
                                    GaugeValueAttention::Nominal => theme.palette().text,
                                    GaugeValueAttention::Warning => {
                                        theme.extended_palette().warning.base.color
                                    }
                                    GaugeValueAttention::Danger => {
                                        theme.extended_palette().danger.base.color
                                    }
                                }),
                            })
                            .into()
                    }
                    Some(GaugeValue::Svg(handle)) => Svg::new(handle.clone())
                        .width(Length::Fixed(gauge_value_icon_size))
                        .height(Length::Fixed(gauge_value_icon_size))
                        .style({
                            let attention = gauge_attention;
                            move |theme: &Theme, _status| svg::Style {
                                color: Some(match attention {
                                    GaugeValueAttention::Nominal => theme.palette().text,
                                    GaugeValueAttention::Warning => {
                                        theme.extended_palette().warning.base.color
                                    }
                                    GaugeValueAttention::Danger => {
                                        theme.extended_palette().danger.base.color
                                    }
                                }),
                            }
                        })
                        .into(),
                    None => Svg::new(error_icon.clone())
                        .width(Length::Fixed(gauge_value_icon_size))
                        .height(Length::Fixed(gauge_value_icon_size))
                        .style({
                            let attention = GaugeValueAttention::Danger;
                            move |theme: &Theme, _status| svg::Style {
                                color: Some(match attention {
                                    GaugeValueAttention::Nominal => theme.palette().text,
                                    GaugeValueAttention::Warning => {
                                        theme.extended_palette().warning.base.color
                                    }
                                    GaugeValueAttention::Danger => {
                                        theme.extended_palette().danger.base.color
                                    }
                                }),
                            }
                        })
                        .into(),
                };

                let centered_value: Element<'_, Message> = container(value)
                    .width(Length::Fill)
                    .align_x(alignment::Horizontal::Center)
                    .into();

                let gauge_id = gauge.id.to_string();
                let gauge_element: Element<'_, Message> = mouse_area(
                    gauge_column
                        .push(centered_value)
                        .align_x(alignment::Horizontal::Center)
                        .width(Length::Fill),
                )
                .on_press(Message::GaugeClicked {
                    id: gauge_id.clone(),
                    target: GaugeClickTarget::Icon,
                    input: GaugeInput::Button(mouse::Button::Left),
                })
                .on_right_press(Message::GaugeClicked {
                    id: gauge_id.clone(),
                    target: GaugeClickTarget::Icon,
                    input: GaugeInput::Button(mouse::Button::Right),
                })
                .on_middle_press(Message::GaugeClicked {
                    id: gauge_id.clone(),
                    target: GaugeClickTarget::Icon,
                    input: GaugeInput::Button(mouse::Button::Middle),
                })
                .on_scroll(move |delta| Message::GaugeClicked {
                    id: gauge_id.clone(),
                    target: GaugeClickTarget::Icon,
                    input: scroll_input(delta).unwrap_or(GaugeInput::ScrollUp),
                })
                .interaction(mouse::Interaction::Pointer)
                .into();

                col.push(gauge_element)
            },
        );

        let layout = Column::new()
            .width(Length::Fill)
            .height(Length::Fill)
            .push(workspaces)
            .push(Space::new().height(Length::Fill))
            .push(gauges);

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
            .interaction(mouse::Interaction::Pointer)
            .into()
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::gauge::GaugeValue;

    fn gauge(id: &'static str) -> GaugeModel {
        GaugeModel {
            id,
            icon: None,
            value: Some(GaugeValue::Text(id.to_string())),
            attention: GaugeValueAttention::Nominal,
            on_click: None,
            menu: None,
            info: None,
        }
    }

    fn workspace(num: i32, focused: bool) -> WorkspaceInfo {
        WorkspaceInfo {
            id: num as i64,
            num,
            name: num.to_string(),
            layout: String::new(),
            visible: focused,
            focused,
            urgent: false,
            representation: None,
            orientation: String::new(),
            rect: crate::sway_workspace::Rect {
                x: 0,
                y: 0,
                width: 0,
                height: 0,
            },
            output: String::new(),
            focus: Vec::new(),
        }
    }

    #[test]
    fn orders_gauges_by_config_then_appends_rest() {
        let state = BarState {
            gauges: vec![gauge("cpu"), gauge("ram"), gauge("disk")],
            gauge_order: vec!["ram".into(), "clock".into(), "cpu".into()],
            ..BarState::default()
        };

        let ordered_ids: Vec<_> = state.ordered_gauges().into_iter().map(|g| g.id).collect();

        assert_eq!(ordered_ids, vec!["ram", "cpu", "disk"]);
    }

    #[test]
    fn tracks_previous_workspace_when_focus_changes() {
        let mut state = BarState::default();

        state.update_workspace_focus(&[workspace(1, true)]);
        assert_eq!(
            state.current_workspace.as_deref(),
            Some("1"),
            "initial focus should be recorded"
        );
        assert!(
            state.previous_workspace.is_none(),
            "no previous on first focus"
        );

        state.update_workspace_focus(&[workspace(1, false), workspace(2, true)]);
        assert_eq!(
            state.previous_workspace.as_deref(),
            Some("1"),
            "prior workspace should be tracked"
        );
        assert_eq!(
            state.current_workspace.as_deref(),
            Some("2"),
            "new focus should replace current"
        );
    }

    #[test]
    fn clears_previous_when_unavailable_or_single_workspace() {
        let mut state = BarState::default();

        state.update_workspace_focus(&[workspace(1, true), workspace(2, false)]);
        state.update_workspace_focus(&[workspace(1, false), workspace(2, true)]);
        assert_eq!(
            state.previous_workspace.as_deref(),
            Some("1"),
            "previous workspace should be set when multiple exist"
        );

        state.update_workspace_focus(&[workspace(2, true)]);
        assert!(
            state.previous_workspace.is_none(),
            "previous should clear when only one workspace remains"
        );

        state.update_workspace_focus(&[workspace(3, true)]);
        assert!(
            state.previous_workspace.is_none(),
            "previous remains cleared without a prior focus to track"
        );
    }
}
