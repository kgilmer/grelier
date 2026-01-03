use std::convert::TryInto;

use crate::gauge::{GaugeModel, GaugeValue, GaugeValueAttention};
use crate::sway_workspace::WorkspaceInfo;
use iced::alignment;
use iced::widget::svg::{self, Svg};
use iced::widget::text;
use iced::widget::{Column, Space, Text, button, container, mouse_area};
use iced::{Element, Length, Theme, mouse};
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
                .fold(Column::new().padding([4, 2]).spacing(4), |col, ws| {
                    // Buttons show the workspace num; background indicates focus/urgency.
                    let styled = container(Text::new(ws.num.to_string()))
                        .padding([2, 4])
                        .style({
                            let focused = ws.focused;
                            let urgent = ws.urgent;
                            move |theme: &Theme| {
                                let palette = theme.extended_palette();
                                let (background_color, text_color) = if urgent {
                                    (palette.danger.base.color, palette.danger.base.text)
                                } else if focused {
                                    (palette.primary.base.color, palette.primary.base.text)
                                } else {
                                    (palette.background.base.color, palette.background.base.text)
                                };

                                container::Style {
                                    background: Some(background_color.into()),
                                    text_color: Some(text_color),
                                    ..container::Style::default()
                                }
                            }
                        });

                    col.push(
                        button(styled)
                            .padding(0)
                            .on_press(Message::Clicked(ws.name.clone())),
                    )
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
