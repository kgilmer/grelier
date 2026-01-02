use std::convert::TryInto;

use iced::widget::{Column, Text};
use iced::{Element, Theme};
use iced_layershell::actions::LayershellCustomActionWithId;

#[derive(Debug)]
pub enum Message {
    Workspaces(Vec<i32>),
}

impl TryInto<LayershellCustomActionWithId> for Message {
    type Error = Message;

    fn try_into(self) -> Result<LayershellCustomActionWithId, Message> {
        match self {
            Message::Workspaces(_) => Err(self),
        }
    }
}

pub struct NumberStrip {
    pub entries: Vec<i32>,
}

impl NumberStrip {
    pub fn new() -> Self {
        Self { entries: Vec::new() }
    }

    pub fn namespace() -> String {
        env!("CARGO_PKG_NAME").to_string()
    }

    pub fn update(&mut self, message: Message) {
        match message {
            Message::Workspaces(ids) => {
                self.entries = ids;
            }
        }
    }

    pub fn view(&self) -> Element<'_, Message> {
        self.entries
            .iter()
            .fold(Column::new().padding([4, 2]).spacing(4), |col, n| {
                col.push(Text::new(n.to_string()))
            })
            .into()
    }

    pub fn theme(&self) -> Option<Theme> {
        Some(Theme::Dark)
    }
}
