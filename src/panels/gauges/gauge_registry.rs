use crate::panels::gauges::gauge::Gauge;
use crate::settings::{SettingSpec, Settings};
use std::sync::OnceLock;
use std::time::Instant;

pub type GaugeValidator = fn(&Settings) -> Result<(), String>;
/// Factory used to create a runtime gauge instance.
///
/// The `Instant` argument is the scheduler start time and should be used to seed
/// initial deadlines/state when needed.
pub type GaugeFactory = fn(Instant) -> Box<dyn Gauge>;

/// Static metadata for a gauge implementation.
pub struct GaugeSpec {
    /// Stable gauge id used in settings (`grelier.gauges`) and model routing.
    pub id: &'static str,
    /// Human-readable description shown in `--list-gauges`.
    pub description: &'static str,
    /// Whether the gauge is enabled in the default gauge set.
    pub default_enabled: bool,
    /// Gauge-specific settings spec entries.
    pub settings: fn() -> &'static [SettingSpec],
    /// Runtime gauge constructor.
    pub create: GaugeFactory,
    /// Optional gauge-specific settings validator.
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

/// Construct a gauge runtime by id.
pub fn create_gauge(id: &str, now: Instant) -> Option<Box<dyn Gauge>> {
    find(id).map(|spec| (spec.create)(now))
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
