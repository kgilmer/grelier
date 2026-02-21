use std::collections::HashMap;

use crate::bar::{BarState, Message, Panel, lerp_color};
use crate::icon::{svg_asset, themed_svg_handle_cached};
use crate::panels::gauges::gauge::{
    GaugeDisplay, GaugeInput, GaugeModel, GaugeNominalColor, GaugeValue, GaugeValueAttention,
};
use crate::settings;
use iced::alignment;
use iced::widget::svg::{self, Svg};
use iced::widget::text;
use iced::widget::{Column, Space, container, mouse_area};
use iced::{Color, Element, Length, Theme, mouse};
use iced_anim::animation_builder::AnimationBuilder;
use iced_anim::transition::Easing;

fn themed_svg_element(
    cache: std::sync::Arc<std::sync::Mutex<std::collections::HashMap<String, svg::Handle>>>,
    handle: svg::Handle,
    start: Color,
    end: Color,
    size: f32,
    fallback_color: Option<Color>,
) -> Element<'static, Message> {
    if let Some(themed_handle) = themed_svg_handle_cached(&cache, &handle, start, end) {
        Svg::new(themed_handle)
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .into()
    } else if let Some(color) = fallback_color {
        Svg::new(handle)
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .style(move |_, _| svg::Style { color: Some(color) })
            .into()
    } else {
        Svg::new(handle)
            .width(Length::Fixed(size))
            .height(Length::Fixed(size))
            .into()
    }
}

fn nominal_color_value(_nominal_color: GaugeNominalColor, theme: &Theme) -> Color {
    theme.extended_palette().secondary.strong.color
}

fn nominal_gradient_colors(_nominal_color: GaugeNominalColor, theme: &Theme) -> (Color, Color) {
    let palette = theme.extended_palette();
    (palette.secondary.weak.color, palette.secondary.strong.color)
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

fn attention_color_at_level(level: f32, nominal_color: GaugeNominalColor, theme: &Theme) -> Color {
    let normal = nominal_color_value(nominal_color, theme);
    let warning = theme.extended_palette().warning.base.color;
    let danger = theme.extended_palette().danger.base.color;
    if level <= 1.0 {
        lerp_color(normal, warning, level.clamp(0.0, 1.0))
    } else {
        lerp_color(warning, danger, (level - 1.0).clamp(0.0, 1.0))
    }
}

fn attention_gradient_colors_at_level(
    level: f32,
    nominal_color: GaugeNominalColor,
    theme: &Theme,
) -> (Color, Color) {
    let palette = theme.extended_palette();
    let (normal_weak, normal_strong) = nominal_gradient_colors(nominal_color, theme);
    let warning_weak = palette.warning.weak.color;
    let warning_strong = palette.warning.strong.color;
    let danger_weak = palette.danger.weak.color;
    let danger_strong = palette.danger.strong.color;

    if level <= 1.0 {
        let t = level.clamp(0.0, 1.0);
        (
            lerp_color(normal_weak, warning_weak, t),
            lerp_color(normal_strong, warning_strong, t),
        )
    } else {
        let t = (level - 1.0).clamp(0.0, 1.0);
        (
            lerp_color(warning_weak, danger_weak, t),
            lerp_color(warning_strong, danger_strong, t),
        )
    }
}

fn quantize_attention_level(level: f32) -> f32 {
    if level < 0.5 {
        0.0
    } else if level < 1.5 {
        1.0
    } else {
        2.0
    }
}

fn attention_level(attention: GaugeValueAttention) -> f32 {
    match attention {
        GaugeValueAttention::Nominal => 0.0,
        GaugeValueAttention::Warning => 1.0,
        GaugeValueAttention::Danger => 2.0,
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
    let bar_theme = state.bar_theme.clone();
    let svg_cache = state.themed_svg_cache.clone();

    let ordered = ordered_gauges(&state.gauges, &state.gauge_order);
    let ratio_inner_full_icon = svg_asset("ratio-inner-full.svg");

    let gauges = ordered.into_iter().fold(
        Column::new()
            .padding([gauge_padding_y, gauge_padding_x])
            .spacing(gauge_spacing)
            .width(Length::Fill)
            .align_x(alignment::Horizontal::Center),
        |col, gauge| {
            let icon_attention = GaugeValueAttention::Nominal;
            let nominal_color = gauge
                .nominal_color
                .unwrap_or(GaugeNominalColor::SecondaryStrong);
            let bar_theme = bar_theme.clone();
            let svg_cache = svg_cache.clone();
            let show_value = !matches!(&gauge.display, GaugeDisplay::Empty);
            let dialog_open = state
                .dialog_windows
                .values()
                .any(|window| window.gauge_id == gauge.id);

            let mut gauge_column = Column::new()
                .align_x(alignment::Horizontal::Center)
                .width(Length::Fill);

            if let Some(icon) = &gauge.icon {
                let attention = icon_attention;
                let icon_handle = icon.clone();
                let bar_theme = bar_theme.clone();
                let svg_cache = svg_cache.clone();
                let icon_box: Element<'_, Message> =
                    AnimationBuilder::new(if dialog_open { 1.0 } else { 0.0 }, move |t| {
                        let icon_view: Element<'_, Message> = {
                            let theme = &bar_theme;
                            let (base_start, base_end) =
                                nominal_gradient_colors(GaugeNominalColor::SecondaryStrong, theme);
                            let base_fallback = attention_color(
                                attention,
                                GaugeNominalColor::SecondaryStrong,
                                theme,
                            );
                            let selected_foreground = theme.palette().background;
                            let start = lerp_color(base_start, selected_foreground, t);
                            let end = lerp_color(base_end, selected_foreground, t);
                            let fallback = lerp_color(base_fallback, selected_foreground, t);
                            themed_svg_element(
                                svg_cache.clone(),
                                icon_handle.clone(),
                                start,
                                end,
                                gauge_icon_size,
                                Some(fallback),
                            )
                        };

                        container(icon_view)
                            .width(Length::Fixed(gauge_icon_size))
                            .height(Length::Fixed(gauge_icon_size))
                            .style(move |theme: &Theme| {
                                let target = theme.palette().primary;
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
                gauge_column = gauge_column.push(centered_icon).push(if show_value {
                    Space::new().height(Length::Fixed(gauge_icon_value_spacing))
                } else {
                    Space::new().height(Length::Fixed(0.0))
                });
            }

            let centered_value: Option<Element<'_, Message>> = if show_value {
                let value: Element<'_, Message> = match &gauge.display {
                    GaugeDisplay::Value {
                        value: GaugeValue::Text(value),
                        attention,
                    } => {
                        let attention_level = attention_level(*attention);
                        let value = value.clone();
                        AnimationBuilder::new(attention_level, move |level| {
                            text::Text::new(value.clone())
                                .width(Length::Fill)
                                .align_x(text::Alignment::Center)
                                .style(move |theme: &Theme| text::Style {
                                    color: Some(attention_color_at_level(
                                        level,
                                        nominal_color,
                                        theme,
                                    )),
                                })
                                .into()
                        })
                        .animation(Easing::EASE_IN_OUT.very_quick())
                        .into()
                    }
                    GaugeDisplay::Value {
                        value: GaugeValue::Svg(handle),
                        attention,
                    } => {
                        let attention_level = attention_level(*attention);
                        let handle = handle.clone();
                        let bar_theme = bar_theme.clone();
                        let svg_cache = svg_cache.clone();
                        AnimationBuilder::new(attention_level, move |level| {
                            let theme = &bar_theme;
                            let quantized = quantize_attention_level(level);
                            let (start, end) =
                                attention_gradient_colors_at_level(quantized, nominal_color, theme);
                            let fallback =
                                attention_color_at_level(quantized, nominal_color, theme);
                            themed_svg_element(
                                svg_cache.clone(),
                                handle.clone(),
                                start,
                                end,
                                gauge_value_icon_size,
                                Some(fallback),
                            )
                        })
                        .animation(Easing::EASE_IN_OUT.very_quick())
                        .into()
                    }
                    GaugeDisplay::Error => {
                        let attention_level = 2.0;
                        let ratio_inner_full_icon = ratio_inner_full_icon.clone();
                        let bar_theme = bar_theme.clone();
                        let svg_cache = svg_cache.clone();
                        AnimationBuilder::new(attention_level, move |level| {
                            let theme = &bar_theme;
                            let quantized = quantize_attention_level(level);
                            let (start, end) =
                                attention_gradient_colors_at_level(quantized, nominal_color, theme);
                            let fallback =
                                attention_color_at_level(quantized, nominal_color, theme);
                            themed_svg_element(
                                svg_cache.clone(),
                                ratio_inner_full_icon.clone(),
                                start,
                                end,
                                gauge_value_icon_size,
                                Some(fallback),
                            )
                        })
                        .animation(Easing::EASE_IN_OUT.very_quick())
                        .into()
                    }
                    GaugeDisplay::Empty => Space::new().into(),
                };
                Some(
                    container(value)
                        .width(Length::Fill)
                        .align_x(alignment::Horizontal::Center)
                        .into(),
                )
            } else {
                None
            };

            let gauge_id = gauge.id.to_string();
            let gauge_element: Element<'_, Message> = mouse_area({
                let mut column = gauge_column.align_x(alignment::Horizontal::Center);
                if let Some(value) = centered_value {
                    column = column.push(value);
                }
                column.width(Length::Fill)
            })
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

#[cfg(test)]
mod gradient_tests {
    use super::*;

    fn assert_color_close(a: Color, b: Color, eps: f32) {
        assert!((a.r - b.r).abs() <= eps, "r {} != {}", a.r, b.r);
        assert!((a.g - b.g).abs() <= eps, "g {} != {}", a.g, b.g);
        assert!((a.b - b.b).abs() <= eps, "b {} != {}", a.b, b.b);
        assert!((a.a - b.a).abs() <= eps, "a {} != {}", a.a, b.a);
    }

    #[test]
    fn quantize_attention_levels() {
        assert_eq!(quantize_attention_level(0.0), 0.0);
        assert_eq!(quantize_attention_level(0.49), 0.0);
        assert_eq!(quantize_attention_level(0.5), 1.0);
        assert_eq!(quantize_attention_level(1.49), 1.0);
        assert_eq!(quantize_attention_level(1.5), 2.0);
        assert_eq!(quantize_attention_level(3.0), 2.0);
    }

    #[test]
    fn attention_gradient_colors_follow_segments() {
        let theme = Theme::Nord;
        let palette = theme.extended_palette();

        let (start0, end0) =
            attention_gradient_colors_at_level(0.0, GaugeNominalColor::SecondaryStrong, &theme);
        assert_color_close(start0, palette.secondary.weak.color, 1e-5);
        assert_color_close(end0, palette.secondary.strong.color, 1e-5);

        let (start1, end1) =
            attention_gradient_colors_at_level(1.0, GaugeNominalColor::SecondaryStrong, &theme);
        assert_color_close(start1, palette.warning.weak.color, 1e-5);
        assert_color_close(end1, palette.warning.strong.color, 1e-5);

        let (start2, end2) =
            attention_gradient_colors_at_level(2.0, GaugeNominalColor::SecondaryStrong, &theme);
        assert_color_close(start2, palette.danger.weak.color, 1e-5);
        assert_color_close(end2, palette.danger.strong.color, 1e-5);
    }
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
            display: GaugeDisplay::Value {
                value: GaugeValue::Text(id.to_string()),
                attention: GaugeValueAttention::Nominal,
            },
            nominal_color: None,
            on_click: None,
            menu: None,
            action_dialog: None,
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
