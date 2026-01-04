use std::convert::TryInto;

use crate::gauge::{GaugeModel, GaugeValue, GaugeValueAttention};
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
    Clicked(String),
    Gauge(GaugeModel),
}

impl TryInto<LayershellCustomActionWithId> for Message {
    type Error = Message;

    fn try_into(self) -> Result<LayershellCustomActionWithId, Message> {
        match self {
            // All messages stay within the app; none translate to layer-shell actions.
            Message::Workspaces(_) | Message::Clicked(_) | Message::Gauge(_) => Err(self),
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
}

impl BarState {
    pub fn new() -> Self {
        Self {
            workspaces: Vec::new(),
            gauges: Vec::new(),
        }
    }

    pub fn with_workspaces(workspaces: Vec<WorkspaceInfo>) -> Self {
        Self {
            workspaces,
            gauges: Vec::new(),
        }
    }

    pub fn namespace() -> String {
        env!("CARGO_PKG_NAME").to_string()
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
                                .on_press(Message::Clicked(name))
                                .into()
                        },
                    )
                    .animation(Easing::EASE_IN_OUT.quick());

                    col.push(animated_workspace)
                });

        let gauges = self.gauges.iter().fold(
            Column::new()
                .padding([4, 2])
                .spacing(8)
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
                    let centered_icon = container(icon_view)
                        .width(Length::Fill)
                        .align_x(alignment::Horizontal::Center);
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

                let centered_value = container(value)
                    .width(Length::Fill)
                    .align_x(alignment::Horizontal::Center);

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
