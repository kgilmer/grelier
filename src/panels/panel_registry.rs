use crate::bar::{BarState, Message, Panel};
use crate::settings::{SettingSpec, Settings};
use iced::Subscription;
use std::collections::HashSet;
use std::sync::OnceLock;

pub type PanelValidator = fn(&Settings) -> Result<(), String>;
pub type PanelView = for<'a> fn(&'a BarState) -> Panel<'a>;
pub type PanelSubscriptionFactory =
    for<'a> fn(PanelSubscriptionContext<'a>) -> Option<Subscription<Message>>;
pub type PanelBootstrapFactory = for<'a> fn(PanelBootstrapContext<'a>, &mut PanelBootstrapConfig);

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum PanelActivation {
    Active,
    Inactive,
}

#[derive(Clone, Copy)]
pub struct PanelSubscriptionContext<'a> {
    pub activation: PanelActivation,
    pub gauges: &'a [String],
}

#[derive(Clone, Copy)]
pub struct PanelBootstrapContext<'a> {
    pub activation: PanelActivation,
    pub settings: &'a Settings,
}

#[derive(Debug, Clone, Default)]
pub struct PanelBootstrapConfig {
    pub workspace_app_icons: bool,
    pub top_apps_count: usize,
}

/// Static metadata and hooks for one panel implementation.
pub struct PanelSpec {
    /// Stable panel id used in `grelier.panels`.
    pub id: &'static str,
    /// Human-readable description shown in `--list-panels`.
    pub description: &'static str,
    /// Whether this panel is enabled by default.
    pub default_enabled: bool,
    /// Panel-specific settings metadata.
    pub settings: fn() -> &'static [SettingSpec],
    /// Panel render function.
    pub view: PanelView,
    /// Optional panel-owned subscription provider.
    pub subscription: Option<PanelSubscriptionFactory>,
    /// Optional panel-owned startup/bootstrap contribution.
    pub bootstrap: Option<PanelBootstrapFactory>,
    /// Optional panel settings validator.
    pub validate: Option<PanelValidator>,
}

inventory::collect!(PanelSpec);

pub fn all() -> impl Iterator<Item = &'static PanelSpec> {
    inventory::iter::<PanelSpec>.into_iter()
}

pub fn find(id: &str) -> Option<&'static PanelSpec> {
    inventory::iter::<PanelSpec>
        .into_iter()
        .find(|spec| spec.id == id)
}

pub fn panel_order_from_setting(setting: &str) -> Vec<&'static str> {
    let mut ordered = Vec::new();
    let mut seen = HashSet::new();
    for raw in setting.split(',') {
        let id = raw.trim();
        if id.is_empty() {
            continue;
        }
        let Some(spec) = find(id) else {
            continue;
        };
        if seen.insert(spec.id) {
            ordered.push(spec.id);
        }
    }
    ordered
}

pub fn collect_settings(base: &[SettingSpec]) -> Vec<SettingSpec> {
    let mut specs = base.to_vec();
    let mut panels: Vec<&'static PanelSpec> = all().collect();
    panels.sort_by_key(|spec| spec.id);
    for panel in panels {
        specs.extend_from_slice((panel.settings)());
    }
    specs
}

pub fn validate_settings(settings: &Settings) -> Result<(), String> {
    for panel in all() {
        if let Some(validate) = panel.validate {
            validate(settings).map_err(|err| format!("Panel '{}': {err}", panel.id))?;
        }
    }
    Ok(())
}

pub fn list_panels() {
    let mut panels: Vec<&'static PanelSpec> = all().collect();
    panels.sort_by_key(|spec| spec.id);
    for panel in panels {
        println!("{}: {}", panel.id, panel.description);
    }
}

pub fn default_panels() -> &'static str {
    static DEFAULT_PANELS: OnceLock<&'static str> = OnceLock::new();
    DEFAULT_PANELS.get_or_init(|| {
        let mut ids: Vec<&'static str> = all()
            .filter(|spec| spec.default_enabled)
            .map(|spec| spec.id)
            .collect();
        ids.sort();
        let joined = ids.join(",");
        Box::leak(joined.into_boxed_str())
    })
}

pub fn subscriptions_for_setting(setting: &str, gauges: &[String]) -> Vec<Subscription<Message>> {
    let active: HashSet<&'static str> = panel_order_from_setting(setting).into_iter().collect();
    let mut subs = Vec::new();
    let mut panels: Vec<&'static PanelSpec> = all().collect();
    panels.sort_by_key(|spec| spec.id);
    for panel in panels {
        let Some(factory) = panel.subscription else {
            continue;
        };
        let activation = if active.contains(panel.id) {
            PanelActivation::Active
        } else {
            PanelActivation::Inactive
        };
        if let Some(sub) = factory(PanelSubscriptionContext { activation, gauges }) {
            subs.push(sub);
        }
    }
    subs
}

pub fn bootstrap_for_setting(setting: &str, settings: &Settings) -> PanelBootstrapConfig {
    let active: HashSet<&'static str> = panel_order_from_setting(setting).into_iter().collect();
    let mut config = PanelBootstrapConfig::default();
    let mut panels: Vec<&'static PanelSpec> = all().collect();
    panels.sort_by_key(|spec| spec.id);
    for panel in panels {
        let Some(bootstrap) = panel.bootstrap else {
            continue;
        };
        let activation = if active.contains(panel.id) {
            PanelActivation::Active
        } else {
            PanelActivation::Inactive
        };
        bootstrap(
            PanelBootstrapContext {
                activation,
                settings,
            },
            &mut config,
        );
    }
    config
}
