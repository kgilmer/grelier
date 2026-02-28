use crate::bar::{BarState, Message, Panel, app_icon_view};
use crate::panels::panel_registry::{
    PanelActivation, PanelBootstrapConfig, PanelBootstrapContext, PanelSpec,
};
use crate::settings;
use elbey_cache::{FALLBACK_ICON_HANDLE, IconHandle};
use iced::alignment;
use iced::widget::{Column, container, mouse_area};
use iced::{Element, Length, mouse};

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

    let top_apps_section: Element<'_, Message> = container(top_apps)
        .padding([workspace_icon_padding_y, workspace_icon_padding_x])
        .width(Length::Fill)
        .align_x(alignment::Horizontal::Center)
        .into();

    Panel::new(top_apps_section)
}

fn panel_settings() -> &'static [crate::settings::SettingSpec] {
    crate::settings::NO_SETTINGS
}

fn panel_bootstrap(context: PanelBootstrapContext<'_>, out: &mut PanelBootstrapConfig) {
    if context.activation == PanelActivation::Active {
        out.top_apps_count = context
            .settings
            .get_parsed_or("grelier.app.top_apps.count", 6usize);
    }
}

inventory::submit! {
    PanelSpec {
        id: "top_apps",
        description: "Top launched apps panel with clickable app icons.",
        default_enabled: true,
        settings: panel_settings,
        view,
        subscription: None,
        bootstrap: Some(panel_bootstrap),
        validate: None,
    }
}
