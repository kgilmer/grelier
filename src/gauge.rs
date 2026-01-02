use std::borrow::Cow;

#[derive(Debug, Clone)]
pub struct GaugeModel {
    pub title: Cow<'static, str>,
    pub value: String,
}
