use crate::app::Message;
use crate::gauge::{GaugeModel, SettingSpec};
use crate::settings::Settings;
use iced::Subscription;
use iced::futures::StreamExt;
use std::sync::OnceLock;

/// Boxed gauge stream used by the registry.
pub type GaugeStream = Box<dyn iced::futures::Stream<Item = GaugeModel> + Send + Unpin>;
pub type GaugeMessageStream = iced::futures::stream::Map<GaugeStream, fn(GaugeModel) -> Message>;
pub type GaugeValidator = fn(&Settings) -> Result<(), String>;

/// Static metadata for a gauge implementation.
pub struct GaugeSpec {
    pub id: &'static str,
    pub label: &'static str,
    pub description: &'static str,
    pub default_enabled: bool,
    pub settings: fn() -> &'static [SettingSpec],
    pub stream: fn() -> GaugeStream,
    pub validate: Option<GaugeValidator>,
}

inventory::collect!(GaugeSpec);

pub fn all() -> impl Iterator<Item = &'static GaugeSpec> {
    inventory::iter::<GaugeSpec>.into_iter()
}

pub fn find(id: &str) -> Option<&'static GaugeSpec> {
    inventory::iter::<GaugeSpec>
        .into_iter()
        .find(|spec| spec.id == id)
}

/// Build the default gauges list based on registry metadata.
pub fn default_gauges() -> &'static str {
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

pub fn subscription_for(spec: &GaugeSpec) -> Subscription<Message> {
    Subscription::run_with(spec.id, gauge_message_stream_by_id)
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

fn gauge_message_stream_by_id(id: &&str) -> GaugeMessageStream {
    let spec = find(id).expect("gauge spec registered");
    (spec.stream)().map(Message::Gauge)
}
