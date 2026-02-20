#![cfg_attr(not(feature = "gauges"), allow(dead_code))]

use crate::bar::Message;
use crate::panels::gauges::gauge::GaugeModel;
use crate::settings::{SettingSpec, Settings};
use iced::Subscription;

/// Boxed gauge stream used by the registry.
pub type GaugeStream = Box<dyn iced::futures::Stream<Item = GaugeModel> + Send + Unpin>;
#[cfg(feature = "gauges")]
pub type GaugeMessageStream = iced::futures::stream::Map<GaugeStream, fn(GaugeModel) -> Message>;
pub type GaugeValidator = fn(&Settings) -> Result<(), String>;

/// Static metadata for a gauge implementation.
pub struct GaugeSpec {
    pub id: &'static str,
    pub description: &'static str,
    pub default_enabled: bool,
    pub settings: fn() -> &'static [SettingSpec],
    pub stream: fn() -> GaugeStream,
    pub validate: Option<GaugeValidator>,
}

#[cfg(feature = "gauges")]
inventory::collect!(GaugeSpec);

#[cfg(feature = "gauges")]
pub fn all() -> impl Iterator<Item = &'static GaugeSpec> {
    inventory::iter::<GaugeSpec>.into_iter()
}

#[cfg(not(feature = "gauges"))]
pub fn all() -> std::iter::Empty<&'static GaugeSpec> {
    std::iter::empty()
}

#[cfg(feature = "gauges")]
pub fn find(id: &str) -> Option<&'static GaugeSpec> {
    inventory::iter::<GaugeSpec>
        .into_iter()
        .find(|spec| spec.id == id)
}

#[cfg(not(feature = "gauges"))]
pub fn find(_id: &str) -> Option<&'static GaugeSpec> {
    None
}

/// Build the default gauges list based on registry metadata.
#[cfg(feature = "gauges")]
pub fn default_gauges() -> &'static str {
    use std::sync::OnceLock;

    static DEFAULT_GAUGES: OnceLock<&'static str> = OnceLock::new();
    DEFAULT_GAUGES.get_or_init(|| {
        let mut ids: Vec<&'static str> = all()
            .filter(|spec| spec.default_enabled)
            .map(|spec| spec.id)
            .collect();
        ids.sort();
        let joined = ids.join(",");
        Box::leak(joined.into_boxed_str())
    })
}

#[cfg(not(feature = "gauges"))]
pub fn default_gauges() -> &'static str {
    ""
}

#[cfg(feature = "gauges")]
pub fn subscription_for(spec: &GaugeSpec) -> Subscription<Message> {
    Subscription::run_with(spec.id, gauge_message_stream_by_id)
}

#[cfg(not(feature = "gauges"))]
pub fn subscription_for(_spec: &GaugeSpec) -> Subscription<Message> {
    Subscription::none()
}

pub fn collect_settings(base: &[SettingSpec]) -> Vec<SettingSpec> {
    let mut specs = base.to_vec();
    for spec in all() {
        specs.extend_from_slice((spec.settings)());
    }
    specs
}

pub fn list_settings(base: &[SettingSpec]) {
    for spec in base {
        println!("{}:{}", spec.key, spec.default);
    }
    let mut gauges: Vec<&'static GaugeSpec> = all().collect();
    gauges.sort_by_key(|spec| spec.id);
    for gauge in gauges {
        for spec in (gauge.settings)() {
            println!("{}:{}", spec.key, spec.default);
        }
    }
}

pub fn list_gauges() {
    let mut gauges: Vec<&'static GaugeSpec> = all().collect();
    gauges.sort_by_key(|spec| spec.id);
    for gauge in gauges {
        println!("{}: {}", gauge.id, gauge.description);
    }
}

pub fn validate_settings(settings: &Settings) -> Result<(), String> {
    for spec in all() {
        if let Some(validate) = spec.validate {
            validate(settings).map_err(|err| format!("Gauge '{}': {err}", spec.id))?;
        }
    }
    Ok(())
}

#[cfg(feature = "gauges")]
fn gauge_message_stream_by_id(id: &&str) -> GaugeMessageStream {
    use iced::futures::StreamExt;

    let spec = find(id).expect("gauge spec registered");
    (spec.stream)().map(Message::Gauge)
}
