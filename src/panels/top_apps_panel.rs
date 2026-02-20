use crate::bar::{BarState, Message, Panel, app_icon_view, lerp_color};
use crate::icon::{svg_asset, themed_svg_handle_cached};
use crate::settings;
use elbey_cache::{FALLBACK_ICON_HANDLE, IconHandle};
use iced::alignment;
use iced::widget::svg::{self, Svg};
use iced::widget::{Column, container, mouse_area};
use iced::{Color, Element, Length, Theme, mouse};
use iced_anim::animation_builder::AnimationBuilder;
use iced_anim::transition::Easing;

pub fn view<'a>(state: &'a BarState) -> Panel<'a> {
    let settings = settings::settings();
    let top_apps_icon_size = settings.get_parsed_or("grelier.app.top_apps.icon_size", 20.0);
    let workspace_icon_spacing = settings
        .get_parsed_or("grelier.app.workspace.icon_spacing", 6u32)
        .max(2);
    let workspace_icon_padding_x =
        settings.get_parsed_or("grelier.app.workspace.icon_padding_x", 2u16);
    let workspace_icon_padding_y =
        settings.get_parsed_or("grelier.app.workspace.icon_padding_y", 2u16);

    let top_apps = state.top_apps.iter().fold(
        Column::new()
            .spacing(workspace_icon_spacing)
            .align_x(alignment::Horizontal::Center)
            .width(Length::Fill),
        |col, app| {
            let app_id = app.appid.clone();
            let handle = match &app.icon_handle {
                IconHandle::NotLoaded => state
                    .app_icons
                    .icon_for(&app_id)
                    .unwrap_or(&FALLBACK_ICON_HANDLE),
                handle => handle,
            };
            let icon = mouse_area(app_icon_view(handle, top_apps_icon_size))
                .on_press(Message::TopAppClicked { app_id })
                .interaction(mouse::Interaction::Pointer);
            col.push(icon)
        },
    );
    let launch_icon = svg_asset("launch.svg");
    let bar_theme = state.bar_theme.clone();
    let svg_cache = state.themed_svg_cache.clone();
    let launcher_open = state.launcher_window.is_some();
    let launcher_icon: Element<'_, Message> =
        AnimationBuilder::new(if launcher_open { 1.0 } else { 0.0 }, move |t| {
            let theme = &bar_theme;
            let palette = theme.extended_palette();
            let base_start = palette.secondary.weak.color;
            let base_end = palette.secondary.strong.color;
            let selected_foreground = theme.palette().background;
            let start = lerp_color(base_start, selected_foreground, t);
            let end = lerp_color(base_end, selected_foreground, t);
            let fallback = lerp_color(base_end, selected_foreground, t);
            let icon = if let Some(themed) =
                themed_svg_handle_cached(&svg_cache, &launch_icon, start, end)
            {
                Svg::new(themed)
            } else {
                Svg::new(launch_icon.clone()).style(move |_, _| svg::Style {
                    color: Some(fallback),
                })
            };

            let icon: Element<'_, Message> = icon
                .width(Length::Fixed(top_apps_icon_size))
                .height(Length::Fixed(top_apps_icon_size))
                .into();

            let target = theme.palette().primary;
            let transparent = Color { a: 0.0, ..target };
            container(icon)
                .width(Length::Fixed(top_apps_icon_size))
                .height(Length::Fixed(top_apps_icon_size))
                .style(move |_theme: &Theme| container::Style {
                    background: Some(lerp_color(transparent, target, t).into()),
                    ..container::Style::default()
                })
                .into()
        })
        .animation(Easing::EASE_IN_OUT.very_quick())
        .into();
    let launcher_button = container(
        mouse_area(launcher_icon)
            .on_press(Message::TopAppsLauncherClicked)
            .interaction(mouse::Interaction::Pointer),
    )
    .padding([workspace_icon_padding_y, workspace_icon_padding_x])
    .width(Length::Shrink);
    let top_apps = top_apps.push(launcher_button);

    let top_apps_section: Element<'_, Message> = container(top_apps)
        .padding([workspace_icon_padding_y, workspace_icon_padding_x])
        .width(Length::Fill)
        .align_x(alignment::Horizontal::Center)
        .into();

    Panel::new(top_apps_section)
}
