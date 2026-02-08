// Power actions gauge with icon-only display.
use crate::icon::svg_asset;
use crate::panels::gauges::gauge::{
    ActionSelectAction, GaugeActionDialog, GaugeActionItem, GaugeClickAction, GaugeDisplay,
    fixed_interval,
};
use crate::panels::gauges::gauge_registry::{GaugeSpec, GaugeStream};
use crate::settings::SettingSpec;
use iced::futures::StreamExt;
use std::sync::Arc;
use std::time::Duration;

fn power_stream() -> impl iced::futures::Stream<Item = crate::panels::gauges::gauge::GaugeModel> {
    let on_select: ActionSelectAction = Arc::new(|item: String| {
        println!("{item}");
    });
    let action_dialog = GaugeActionDialog {
        title: "Power".to_string(),
        items: vec![
            GaugeActionItem {
                id: "shutdown.svg".to_string(),
                icon: svg_asset("shutdown.svg"),
            },
            GaugeActionItem {
                id: "reboot.svg".to_string(),
                icon: svg_asset("reboot.svg"),
            },
            GaugeActionItem {
                id: "sleep.svg".to_string(),
                icon: svg_asset("sleep.svg"),
            },
        ],
        on_select: Some(on_select),
    };
    let on_click: GaugeClickAction = Arc::new(|_click| {});

    fixed_interval(
        "power",
        Some(svg_asset("on-off.svg")),
        || Duration::from_secs(60),
        || Some(GaugeDisplay::Empty),
        Some(on_click),
    )
    .map(move |mut model| {
        model.hide_value = true;
        model.action_dialog = Some(action_dialog.clone());
        model
    })
}

pub fn settings() -> &'static [SettingSpec] {
    const SETTINGS: &[SettingSpec] = &[];
    SETTINGS
}

fn stream() -> GaugeStream {
    Box::new(power_stream())
}

inventory::submit! {
    GaugeSpec {
        id: "power",
        description: "Power actions gauge (shutdown, reboot, sleep).",
        default_enabled: false,
        settings,
        stream,
        validate: None,
    }
}
