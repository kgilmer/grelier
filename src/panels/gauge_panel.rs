use std::collections::HashMap;

use crate::bar::{BarState, Message, Panel, lerp_color};
use crate::icon::svg_asset;
use crate::panels::gauges::gauge::{
    GaugeInput, GaugeModel, GaugeNominalColor, GaugeValue, GaugeValueAttention,
};
use crate::settings;
use iced::alignment;
use iced::widget::svg::{self, Svg};
use iced::widget::text;
use iced::widget::{Column, Space, container, mouse_area};
use iced::{Color, Element, Length, Theme, mouse};
use iced_anim::animation_builder::AnimationBuilder;
use iced_anim::transition::Easing;

fn nominal_color_value(nominal_color: GaugeNominalColor, theme: &Theme) -> Color {
    match nominal_color {
        GaugeNominalColor::SecondaryStrong => theme.extended_palette().secondary.strong.color,
        GaugeNominalColor::Primary => theme.palette().primary,
    }
}

fn attention_color(
    attention: GaugeValueAttention,
    nominal_color: GaugeNominalColor,
    theme: &Theme,
) -> Color {
    match attention {
        GaugeValueAttention::Nominal => nominal_color_value(nominal_color, theme),
        GaugeValueAttention::Warning => theme.extended_palette().warning.base.color,
        GaugeValueAttention::Danger => theme.extended_palette().danger.base.color,
    }
}

fn attention_color_at_level(
    level: f32,
    nominal_color: GaugeNominalColor,
    theme: &Theme,
) -> Color {
    let normal = nominal_color_value(nominal_color, theme);
    let warning = theme.extended_palette().warning.base.color;
    let danger = theme.extended_palette().danger.base.color;
    if level <= 1.0 {
        lerp_color(normal, warning, level.clamp(0.0, 1.0))
    } else {
        lerp_color(warning, danger, (level - 1.0).clamp(0.0, 1.0))
    }
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

pub fn ordered_gauges<'a>(gauges: &'a [GaugeModel], gauge_order: &[String]) -> Vec<&'a GaugeModel> {
    let order_index: HashMap<_, _> = gauge_order
        .iter()
        .enumerate()
        .map(|(i, id)| (id.clone(), i))
        .collect();

    let mut ordered: Vec<(usize, &GaugeModel)> = gauges.iter().enumerate().collect();
    ordered.sort_by_key(|(idx, g)| (order_index.get(g.id).copied().unwrap_or(usize::MAX), *idx));
    ordered.into_iter().map(|(_, gauge)| gauge).collect()
}

pub fn view<'a>(state: &'a BarState) -> Panel<'a> {
    let settings = settings::settings();
    let gauge_padding_x = settings.get_parsed_or("grelier.gauge.ui.padding_x", 2u16);
    let gauge_padding_y = settings.get_parsed_or("grelier.gauge.ui.padding_y", 2u16);
    let gauge_spacing = settings
        .get_parsed("grelier.gauge.spacing")
        .unwrap_or_else(|| settings.get_parsed_or("grelier.gauge.ui.spacing", 14u32));
    let gauge_icon_size = settings.get_parsed_or("grelier.gauge.ui.icon_size", 20.0);
    let gauge_value_icon_size = settings.get_parsed_or("grelier.gauge.ui.value_icon_size", 20.0);
    let gauge_icon_value_spacing =
        settings.get_parsed_or("grelier.gauge.ui.icon_value_spacing", 0.0);

    let ordered = ordered_gauges(&state.gauges, &state.gauge_order);
    let ratio_inner_full_icon = svg_asset("ratio-inner-full.svg");

    let gauges = ordered.into_iter().fold(
        Column::new()
            .padding([gauge_padding_y, gauge_padding_x])
            .spacing(gauge_spacing)
            .width(Length::Fill)
            .align_x(alignment::Horizontal::Center),
        |col, gauge| {
            let gauge_attention = gauge.attention;
            let icon_attention = GaugeValueAttention::Nominal;
            let nominal_color =
                gauge
                    .nominal_color
                    .unwrap_or(GaugeNominalColor::SecondaryStrong);

            let mut gauge_column = Column::new()
                .align_x(alignment::Horizontal::Center)
                .width(Length::Fill);

            if let Some(icon) = &gauge.icon {
                let dialog_open = state
                    .dialog_windows
                    .values()
                    .any(|window| window.gauge_id == gauge.id);
                let attention = icon_attention;
                let icon_handle = icon.clone();
                let icon_box: Element<'_, Message> =
                    AnimationBuilder::new(if dialog_open { 1.0 } else { 0.0 }, move |t| {
                        let icon_view = Svg::new(icon_handle.clone())
                            .width(Length::Fixed(gauge_icon_size))
                            .height(Length::Fixed(gauge_icon_size))
                            .style(move |theme: &Theme, _status| {
                                let normal = attention_color(
                                    attention,
                                    GaugeNominalColor::SecondaryStrong,
                                    theme,
                                );
                                let inverted = theme.palette().background;
                                svg::Style {
                                    color: Some(lerp_color(normal, inverted, t)),
                                }
                            });

                        container(icon_view)
                            .width(Length::Fixed(gauge_icon_size))
                            .height(Length::Fixed(gauge_icon_size))
                            .style(move |theme: &Theme| {
                                let target = nominal_color_value(
                                    GaugeNominalColor::SecondaryStrong,
                                    theme,
                                );
                                let transparent = Color { a: 0.0, ..target };
                                container::Style {
                                    background: Some(lerp_color(transparent, target, t).into()),
                                    ..container::Style::default()
                                }
                            })
                            .into()
                    })
                    .animation(Easing::EASE_IN_OUT.very_quick())
                    .into();
                let centered_icon: Element<'_, Message> = container(icon_box)
                    .width(Length::Fill)
                    .align_x(alignment::Horizontal::Center)
                    .into();
                gauge_column = gauge_column
                    .push(centered_icon)
                    .push(Space::new().height(Length::Fixed(gauge_icon_value_spacing)));
            }

            let value: Element<'_, Message> = match &gauge.value {
                Some(GaugeValue::Text(value)) => {
                    let attention_level = match gauge_attention {
                        GaugeValueAttention::Nominal => 0.0,
                        GaugeValueAttention::Warning => 1.0,
                        GaugeValueAttention::Danger => 2.0,
                    };
                    let value = value.clone();
                    AnimationBuilder::new(attention_level, move |level| {
                        text::Text::new(value.clone())
                            .width(Length::Fill)
                            .align_x(text::Alignment::Center)
                            .style(move |theme: &Theme| text::Style {
                                color: Some(attention_color_at_level(level, nominal_color, theme)),
                            })
                            .into()
                    })
                    .animation(Easing::EASE_IN_OUT.very_quick())
                    .into()
                }
                Some(GaugeValue::Svg(handle)) => {
                    let attention_level = match gauge_attention {
                        GaugeValueAttention::Nominal => 0.0,
                        GaugeValueAttention::Warning => 1.0,
                        GaugeValueAttention::Danger => 2.0,
                    };
                    let handle = handle.clone();
                    AnimationBuilder::new(attention_level, move |level| {
                        Svg::new(handle.clone())
                            .width(Length::Fixed(gauge_value_icon_size))
                            .height(Length::Fixed(gauge_value_icon_size))
                            .style(move |theme: &Theme, _status| svg::Style {
                                color: Some(attention_color_at_level(level, nominal_color, theme)),
                            })
                            .into()
                    })
                    .animation(Easing::EASE_IN_OUT.very_quick())
                    .into()
                }
                None => {
                    let attention_level = 2.0;
                    let ratio_inner_full_icon = ratio_inner_full_icon.clone();
                    AnimationBuilder::new(attention_level, move |level| {
                        Svg::new(ratio_inner_full_icon.clone())
                            .width(Length::Fixed(gauge_value_icon_size))
                            .height(Length::Fixed(gauge_value_icon_size))
                            .style(move |theme: &Theme, _status| svg::Style {
                                color: Some(attention_color_at_level(level, nominal_color, theme)),
                            })
                            .into()
                    })
                    .animation(Easing::EASE_IN_OUT.very_quick())
                    .into()
                }
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
                input: GaugeInput::Button(mouse::Button::Left),
            })
            .on_right_press(Message::GaugeClicked {
                id: gauge_id.clone(),
                input: GaugeInput::Button(mouse::Button::Right),
            })
            .on_middle_press(Message::GaugeClicked {
                id: gauge_id.clone(),
                input: GaugeInput::Button(mouse::Button::Middle),
            })
            .on_scroll(move |delta| Message::GaugeClicked {
                id: gauge_id.clone(),
                input: scroll_input(delta).unwrap_or(GaugeInput::ScrollUp),
            })
            .interaction(mouse::Interaction::Pointer)
            .into();

            col.push(gauge_element)
        },
    );

    Panel::new(gauges)
}

pub fn anchor_y(state: &BarState) -> Option<i32> {
    let p = state.last_cursor?;
    // Align to top of icon for the gauge regardless of click location.
    // Icon is 14px tall with no padding; value sits below with a 3px spacer.
    let icon_offset =
        settings::settings().get_parsed_or("grelier.gauge.ui.anchor_offset_icon", 7.0);
    Some((p.y - icon_offset).round() as i32)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn gauge(id: &'static str) -> GaugeModel {
        GaugeModel {
            id,
            icon: None,
            value: Some(GaugeValue::Text(id.to_string())),
            attention: GaugeValueAttention::Nominal,
            nominal_color: None,
            on_click: None,
            menu: None,
            info: None,
        }
    }

    #[test]
    fn orders_gauges_by_config_then_appends_rest() {
        let gauges = vec![gauge("cpu"), gauge("ram"), gauge("disk")];
        let gauge_order = vec!["ram".into(), "clock".into(), "cpu".into()];

        let ordered_ids: Vec<_> = ordered_gauges(&gauges, &gauge_order)
            .into_iter()
            .map(|g| g.id)
            .collect();

        assert_eq!(ordered_ids, vec!["ram", "cpu", "disk"]);
    }
}
