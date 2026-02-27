use crate::bar::{BarState, Message, Panel, app_icon_view, lerp_color};
use crate::panels::panel_registry::{
    PanelActivation, PanelBootstrapConfig, PanelBootstrapContext, PanelSpec,
    PanelSubscriptionContext,
};
use crate::settings;
use crate::sway_workspace::WorkspaceInfo;
use elbey_cache::FALLBACK_ICON_HANDLE;
use iced::alignment;
use iced::border;
use iced::font::Weight;
use iced::gradient::Linear;
use iced::widget::text;
use iced::widget::{Column, Text, button, container, mouse_area};
use iced::{Border, Degrees, Element, Font, Gradient, Length, Theme, mouse};
use iced_anim::animation_builder::AnimationBuilder;
use iced_anim::transition::Easing;

fn workspace_gradient(start: iced::Color, end: iced::Color) -> Gradient {
    Gradient::Linear(
        Linear::new(Degrees(180.0))
            .add_stop(0.0, start)
            .add_stop(1.0, end),
    )
}

fn workspace_gradient_colors(
    focus: f32,
    urgent: f32,
    is_previous: bool,
    palette: &iced::theme::palette::Extended,
) -> (iced::Color, iced::Color) {
    let (mut start, mut end) = if focus <= 0.0 && urgent <= 0.0 {
        if is_previous {
            (palette.primary.weak.color, palette.primary.strong.color)
        } else {
            (
                palette.background.weak.color,
                palette.background.strong.color,
            )
        }
    } else {
        (
            workspace_color(
                focus,
                urgent,
                palette.background.weak.color,
                palette.primary.weak.color,
                palette.success.weak.color,
            ),
            workspace_color(
                focus,
                urgent,
                palette.background.strong.color,
                palette.primary.strong.color,
                palette.success.strong.color,
            ),
        )
    };

    if is_previous {
        let fade = 0.6;
        start = lerp_color(start, palette.background.strong.color, fade);
        end = lerp_color(end, palette.background.strong.color, fade);
    }

    (start, end)
}

fn workspace_color(
    focus_level: f32,
    urgent_level: f32,
    normal: iced::Color,
    focused: iced::Color,
    urgent: iced::Color,
) -> iced::Color {
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

pub fn update_workspace_focus(state: &mut BarState, workspaces: &[WorkspaceInfo]) {
    let workspace_count = workspaces.len();

    // Drop the previous reference if it no longer exists or there's nothing to highlight.
    if workspace_count <= 1
        || state
            .previous_workspace
            .as_ref()
            .is_some_and(|prev| !workspaces.iter().any(|ws| ws.name == *prev))
    {
        state.previous_workspace = None;
    }

    let focused_workspace = workspaces
        .iter()
        .find(|ws| ws.focused)
        .map(|ws| ws.name.clone());

    match focused_workspace {
        Some(focused) if Some(focused.as_str()) != state.current_workspace.as_deref() => {
            if workspace_count > 1 {
                if let Some(current) = state.current_workspace.take() {
                    state.previous_workspace = Some(current);
                }
            } else {
                state.previous_workspace = None;
            }

            state.current_workspace = Some(focused);
        }
        Some(_) => {}
        None => state.current_workspace = None,
    }
}

pub fn view<'a>(state: &'a BarState) -> Panel<'a> {
    let settings = settings::settings();
    let workspace_padding_x = settings.get_parsed_or("grelier.app.workspace.padding_x", 4u16);
    let workspace_padding_y = settings.get_parsed_or("grelier.app.workspace.padding_y", 2u16);
    let workspace_spacing = settings.get_parsed_or("grelier.ws.spacing", 2u32);
    let workspace_button_padding_x =
        settings.get_parsed_or("grelier.app.workspace.button_padding_x", 4u16);
    let workspace_button_padding_y =
        settings.get_parsed_or("grelier.app.workspace.button_padding_y", 4u16);
    let workspace_corner_radius = settings.get_parsed_or("grelier.ws.corner_radius", 5.0_f32);
    let workspace_transitions = settings.get_bool_or("grelier.ws.transitions", false);
    let workspace_label_size = settings.get_parsed_or("grelier.app.workspace.label_size", 14u32);
    let workspace_icon_size = settings.get_parsed_or("grelier.app.workspace.icon_size", 22.0);
    let workspace_icon_spacing = settings
        .get_parsed_or("grelier.app.workspace.icon_spacing", 6u32)
        .max(2);
    let workspace_icon_padding_x =
        settings.get_parsed_or("grelier.app.workspace.icon_padding_x", 2u16);
    let workspace_icon_padding_y =
        settings.get_parsed_or("grelier.app.workspace.icon_padding_y", 2u16);
    let workspace_app_icons = settings.get_bool_or("grelier.app.workspace.app_icons", true);

    let previous_workspace = state.previous_workspace.as_deref();
    let highlight_previous = previous_workspace.is_some() && state.workspaces.len() > 1;

    let workspaces = state.workspaces.iter().fold(
        Column::new()
            .padding([workspace_padding_y, workspace_padding_x])
            .spacing(workspace_spacing),
        |col, ws| {
            let ws_name = ws.name.clone();
            let ws_num = ws.num;
            let ws_apps = state
                .workspace_apps
                .get(&ws_name)
                .map(|apps| apps.as_slice())
                .unwrap_or(&[]);
            let (focus_level, urgent_level) = workspace_levels(ws);
            let is_previous =
                highlight_previous && !ws.focused && previous_workspace == Some(ws.name.as_str());

            let build_workspace = move |focus: f32, urgent: f32| -> Element<'_, Message> {
                let name = ws_name.clone();
                let mut label = Text::new(ws_num.to_string())
                    .size(workspace_label_size)
                    .width(Length::Fill)
                    .align_x(text::Alignment::Center);
                if focus > 0.0 {
                    label = label.font(Font {
                        weight: Weight::Bold,
                        ..Font::DEFAULT
                    });
                }

                let mut icons_column = Column::new()
                    .spacing(workspace_icon_spacing)
                    .align_x(alignment::Horizontal::Center);
                if workspace_app_icons {
                    for app in ws_apps {
                        let handle = state
                            .app_icons
                            .icon_for(&app.app_id)
                            .unwrap_or(&FALLBACK_ICON_HANDLE);
                        let app_id = app.app_id.clone();
                        let con_id = app.con_id;
                        let icon = mouse_area(app_icon_view(handle, workspace_icon_size))
                            .on_press(Message::WorkspaceAppClicked { con_id, app_id })
                            .interaction(mouse::Interaction::Pointer);
                        icons_column = icons_column.push(icon);
                    }
                }

                let label_content = container(label)
                    .padding([workspace_button_padding_y, workspace_button_padding_x])
                    .width(Length::Fill)
                    .style(move |theme: &Theme| {
                        let palette = theme.extended_palette();
                        let (gradient_start, gradient_end) =
                            workspace_gradient_colors(focus, urgent, is_previous, palette);
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
                        let border =
                            Border::default().rounded(border::Radius::new(workspace_corner_radius));

                        container::Style {
                            background: Some(
                                workspace_gradient(gradient_start, gradient_end).into(),
                            ),
                            border,
                            text_color: Some(text_color),
                            ..container::Style::default()
                        }
                    });

                let label_button: Element<'_, Message> = button(label_content)
                    .style(|theme: &Theme, _status| button::Style {
                        background: None,
                        text_color: theme.palette().text,
                        ..button::Style::default()
                    })
                    .padding(0)
                    .width(Length::Fill)
                    .on_press(Message::WorkspaceClicked(name))
                    .into();

                let mut layout = Column::new()
                    .spacing(2)
                    .align_x(alignment::Horizontal::Center)
                    .push(label_button);

                if workspace_app_icons && !ws_apps.is_empty() {
                    let icons_container = container(icons_column)
                        .padding([workspace_icon_padding_y, workspace_icon_padding_x])
                        .width(Length::Fill)
                        .align_x(alignment::Horizontal::Center)
                        .style(move |theme: &Theme| container::Style {
                            background: Some(theme.palette().background.into()),
                            border: Border::default()
                                .rounded(border::Radius::new(workspace_corner_radius)),
                            ..container::Style::default()
                        });
                    layout = layout.push(icons_container);
                }

                layout.into()
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

    Panel::new(workspaces)
}

fn panel_settings() -> &'static [crate::settings::SettingSpec] {
    crate::settings::NO_SETTINGS
}

fn panel_subscription(context: PanelSubscriptionContext<'_>) -> Option<iced::Subscription<Message>> {
    Some(if context.activation == PanelActivation::Active {
        crate::sway_workspace::workspace_subscription()
    } else {
        crate::sway_workspace::output_subscription()
    })
}

fn panel_bootstrap(context: PanelBootstrapContext<'_>, out: &mut PanelBootstrapConfig) {
    if context.activation == PanelActivation::Active {
        out.workspace_app_icons = context
            .settings
            .get_bool_or("grelier.app.workspace.app_icons", true);
    }
}

inventory::submit! {
    PanelSpec {
        id: "workspaces",
        description: "Workspace list with focus state and optional workspace app icons.",
        default_enabled: true,
        settings: panel_settings,
        view,
        subscription: Some(panel_subscription),
        bootstrap: Some(panel_bootstrap),
        validate: None,
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn assert_color_close(a: iced::Color, b: iced::Color, eps: f32) {
        assert!((a.r - b.r).abs() <= eps, "r {} != {}", a.r, b.r);
        assert!((a.g - b.g).abs() <= eps, "g {} != {}", a.g, b.g);
        assert!((a.b - b.b).abs() <= eps, "b {} != {}", a.b, b.b);
        assert!((a.a - b.a).abs() <= eps, "a {} != {}", a.a, b.a);
    }

    fn workspace(num: i32, focused: bool) -> WorkspaceInfo {
        WorkspaceInfo {
            num,
            name: num.to_string(),
            focused,
            urgent: false,
            rect: crate::sway_workspace::Rect { y: 0, height: 0 },
        }
    }

    #[test]
    fn tracks_previous_workspace_when_focus_changes() {
        let mut state = BarState::default();

        update_workspace_focus(&mut state, &[workspace(1, true)]);
        assert_eq!(
            state.current_workspace.as_deref(),
            Some("1"),
            "initial focus should be recorded",
        );
        assert!(
            state.previous_workspace.is_none(),
            "no previous on first focus",
        );

        update_workspace_focus(&mut state, &[workspace(1, false), workspace(2, true)]);
        assert_eq!(
            state.previous_workspace.as_deref(),
            Some("1"),
            "prior workspace should be tracked",
        );
        assert_eq!(
            state.current_workspace.as_deref(),
            Some("2"),
            "new focus should replace current",
        );
    }

    #[test]
    fn clears_previous_when_unavailable_or_single_workspace() {
        let mut state = BarState::default();

        update_workspace_focus(&mut state, &[workspace(1, true), workspace(2, false)]);
        update_workspace_focus(&mut state, &[workspace(1, false), workspace(2, true)]);
        assert_eq!(
            state.previous_workspace.as_deref(),
            Some("1"),
            "previous workspace should be set when multiple exist",
        );

        update_workspace_focus(&mut state, &[workspace(2, true)]);
        assert!(
            state.previous_workspace.is_none(),
            "previous should clear when only one workspace remains",
        );

        update_workspace_focus(&mut state, &[workspace(3, true)]);
        assert!(
            state.previous_workspace.is_none(),
            "previous remains cleared without a prior focus to track",
        );
    }

    #[test]
    fn workspace_gradient_colors_handle_previous_fade() {
        let theme = Theme::Nord;
        let palette = theme.extended_palette();

        let (start_inactive, end_inactive) = workspace_gradient_colors(0.0, 0.0, false, palette);
        assert_color_close(start_inactive, palette.background.weak.color, 1e-5);
        assert_color_close(end_inactive, palette.background.strong.color, 1e-5);

        let fade = 0.6;
        let expected_prev_start = lerp_color(
            palette.primary.weak.color,
            palette.background.strong.color,
            fade,
        );
        let expected_prev_end = lerp_color(
            palette.primary.strong.color,
            palette.background.strong.color,
            fade,
        );
        let (prev_start, prev_end) = workspace_gradient_colors(0.0, 0.0, true, palette);
        assert_color_close(prev_start, expected_prev_start, 1e-5);
        assert_color_close(prev_end, expected_prev_end, 1e-5);

        let focused_start = workspace_color(
            1.0,
            0.0,
            palette.background.weak.color,
            palette.primary.weak.color,
            palette.success.weak.color,
        );
        let focused_end = workspace_color(
            1.0,
            0.0,
            palette.background.strong.color,
            palette.primary.strong.color,
            palette.success.strong.color,
        );
        let (prev_focus_start, prev_focus_end) = workspace_gradient_colors(1.0, 0.0, true, palette);
        assert_color_close(
            prev_focus_start,
            lerp_color(focused_start, palette.background.strong.color, fade),
            1e-5,
        );
        assert_color_close(
            prev_focus_end,
            lerp_color(focused_end, palette.background.strong.color, fade),
            1e-5,
        );
    }
}
