use std::collections::HashMap;
use std::convert::TryInto;

use crate::gauge::{GaugeClickTarget, GaugeModel, GaugeValue, GaugeValueAttention};
use crate::sway_workspace::WorkspaceInfo;
use iced::alignment;
use iced::border;
use iced::font::Weight;
use iced::widget::svg::{self, Svg};
use iced::widget::text;
use iced::widget::{Column, Space, Text, button, container, mouse_area};
use iced::{Border, Color, Element, Font, Length, Theme, mouse};
use iced_anim::animation_builder::AnimationBuilder;
use iced_anim::transition::Easing;
use iced_layershell::actions::LayershellCustomActionWithId;

#[derive(Debug, Clone)]
pub enum Message {
    Workspaces(Vec<WorkspaceInfo>),
    WorkspaceClicked(String),
    Gauge(GaugeModel),
    GaugeClicked {
        id: String,
        target: GaugeClickTarget,
        button: mouse::Button,
    },
}

impl TryInto<LayershellCustomActionWithId> for Message {
    type Error = Message;

    fn try_into(self) -> Result<LayershellCustomActionWithId, Message> {
        match self {
            // All messages stay within the app; none translate to layer-shell actions.
            Message::Workspaces(_)
            | Message::WorkspaceClicked(_)
            | Message::Gauge(_)
            | Message::GaugeClicked { .. } => Err(self),
        }
    }
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

#[derive(Clone)]
pub struct BarState {
    pub workspaces: Vec<WorkspaceInfo>,
    pub gauges: Vec<GaugeModel>,
    pub gauge_order: Vec<String>,
}

impl BarState {
    pub fn new() -> Self {
        Self {
            workspaces: Vec::new(),
            gauges: Vec::new(),
            gauge_order: Vec::new(),
        }
    }

    pub fn with_workspaces(workspaces: Vec<WorkspaceInfo>) -> Self {
        Self {
            workspaces,
            gauges: Vec::new(),
            gauge_order: Vec::new(),
        }
    }

    pub fn with_gauge_order(gauge_order: Vec<String>) -> Self {
        Self {
            workspaces: Vec::new(),
            gauges: Vec::new(),
            gauge_order,
        }
    }

    pub fn namespace() -> String {
        env!("CARGO_PKG_NAME").to_string()
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

    pub fn view(&self) -> Element<'_, Message> {
        let workspaces =
            self.workspaces
                .iter()
                .fold(Column::new().padding([4, 2]).spacing(2), |col, ws| {
                    let ws_name = ws.name.clone();
                    let ws_num = ws.num;
                    let (focus_level, urgent_level) = workspace_levels(ws);

                    let animated_workspace = AnimationBuilder::new(
                        (focus_level, urgent_level),
                        move |(focus, urgent)| {
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
                                .padding([4, 4])
                                .width(Length::Fill)
                                .style(move |theme: &Theme| {
                                    let palette = theme.extended_palette();

                                    let background_color = workspace_color(
                                        focus,
                                        urgent,
                                        palette.background.base.color,
                                        palette.primary.base.color,
                                        palette.danger.base.color,
                                    );
                                    let text_color = if urgent > 0.0 || focus > 0.0 {
                                        palette.background.base.color
                                    } else {
                                        theme.palette().text
                                    };

                                    container::Style {
                                        background: Some(background_color.into()),
                                        border: Border::default()
                                            .color(palette.warning.base.color)
                                            .width(3.0)
                                            .rounded(border::Radius::new(5.0)),
                                        text_color: Some(text_color),
                                        ..container::Style::default()
                                    }
                                });

                            button(content)
                                .padding(0)
                                .width(Length::Fill)
                                .on_press(Message::WorkspaceClicked(name))
                                .into()
                        },
                    )
                    .animation(Easing::EASE_IN_OUT.quick());

                    col.push(animated_workspace)
                });

        let ordered_gauges = self.ordered_gauges();

        let gauges = ordered_gauges.into_iter().fold(
            Column::new()
                .padding([2, 2])
                .spacing(22)
                .width(Length::Fill)
                .align_x(alignment::Horizontal::Center),
            |col, gauge| {
                let mut gauge_column = Column::new()
                    .align_x(alignment::Horizontal::Center)
                    .width(Length::Fill);

                if let Some(icon) = &gauge.icon {
                    let icon_view = Svg::new(icon.clone())
                        .width(Length::Fixed(14.0))
                        .height(Length::Fixed(14.0))
                        .style({
                            let attention = gauge.attention;
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
                    let centered_icon: Element<'_, Message> = mouse_area(centered_icon)
                        .on_press(Message::GaugeClicked {
                            id: gauge.id.to_string(),
                            target: GaugeClickTarget::Icon,
                            button: mouse::Button::Left,
                        })
                        .on_right_press(Message::GaugeClicked {
                            id: gauge.id.to_string(),
                            target: GaugeClickTarget::Icon,
                            button: mouse::Button::Right,
                        })
                        .on_middle_press(Message::GaugeClicked {
                            id: gauge.id.to_string(),
                            target: GaugeClickTarget::Icon,
                            button: mouse::Button::Middle,
                        })
                        .interaction(mouse::Interaction::Pointer)
                        .into();
                    gauge_column = gauge_column
                        .push(centered_icon)
                        .push(Space::new().height(Length::Fixed(3.0)));
                }

                let value: Element<'_, Message> = match &gauge.value {
                    GaugeValue::Text(value) => {
                        let attention = gauge.attention;
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
                    GaugeValue::Svg(handle) => Svg::new(handle.clone())
                        .width(Length::Fixed(20.0))
                        .height(Length::Fixed(20.0))
                        .style({
                            let attention = gauge.attention;
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
                let centered_value: Element<'_, Message> = mouse_area(centered_value)
                    .on_press(Message::GaugeClicked {
                        id: gauge.id.to_string(),
                        target: GaugeClickTarget::Value,
                        button: mouse::Button::Left,
                    })
                    .on_right_press(Message::GaugeClicked {
                        id: gauge.id.to_string(),
                        target: GaugeClickTarget::Value,
                        button: mouse::Button::Right,
                    })
                    .on_middle_press(Message::GaugeClicked {
                        id: gauge.id.to_string(),
                        target: GaugeClickTarget::Value,
                        button: mouse::Button::Middle,
                    })
                    .interaction(mouse::Interaction::Pointer)
                    .into();

                col.push(gauge_column.push(centered_value))
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

        mouse_area(filled)
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
            value: GaugeValue::Text(id.to_string()),
            attention: GaugeValueAttention::Nominal,
            on_click: None,
        }
    }

    #[test]
    fn orders_gauges_by_config_then_appends_rest() {
        let state = BarState {
            workspaces: Vec::new(),
            gauges: vec![gauge("cpu"), gauge("ram"), gauge("disk")],
            gauge_order: vec!["ram".into(), "clock".into(), "cpu".into()],
        };

        let ordered_ids: Vec<_> = state.ordered_gauges().into_iter().map(|g| g.id).collect();

        assert_eq!(ordered_ids, vec!["ram", "cpu", "disk"]);
    }
}
