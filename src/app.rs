use std::convert::TryInto;

use crate::gauge::GaugeModel;
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
                                let background_color = if urgent {
                                    palette.danger.base.color
                                } else if focused {
                                    palette.primary.base.color
                                } else {
                                    palette.background.base.color
                                };

                                container::Style {
                                    background: Some(background_color.into()),
                                    text_color: Some(palette.background.base.text),
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

        let gauges =
            self.gauges
                .iter()
                .fold(Column::new().padding([4, 2]).spacing(2), |col, gauge| {
                    let text = match &gauge.title {
                        Some(title) => format!("{title}: {}", gauge.value),
                        None => gauge.value.clone(),
                    };
                    col.push(Text::new(text))
                });

        let layout = Column::new()
            .width(Length::Fill)
            .height(Length::Fill)
            .push(workspaces)
            .push(Space::new().height(Length::Fill))
            .push(gauges);

        let filled = container(layout).width(Length::Fill).height(Length::Fill);

        mouse_area(filled)
            .interaction(mouse::Interaction::Pointer)
            .into()
    }

    pub fn theme(&self) -> Option<Theme> {
        Some(Theme::Dark)
    }
}

#[derive(Debug, Clone)]
pub struct WorkspaceInfo {
    pub id: i64,
    pub num: i32,
    pub name: String,
    pub layout: String,
    pub visible: bool,
    pub focused: bool,
    pub urgent: bool,
    pub representation: Option<String>,
    pub orientation: String,
    pub rect: Rect,
    pub output: String,
    pub focus: Vec<i64>,
}

#[derive(Debug, Clone)]
pub struct Rect {
    pub x: i32,
    pub y: i32,
    pub width: i32,
    pub height: i32,
}
