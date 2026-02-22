// Gauge models, menus, and interaction payloads.
use iced::mouse;
use iced::widget::svg;
use std::fmt;
use std::sync::Arc;
use std::time::Instant;

use crate::dialog::info::InfoDialog;

/// Severity level used when rendering gauge values.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum GaugeValueAttention {
    #[default]
    Nominal,
    Warning,
    Danger,
}

/// Renderable content for a gauge value.
#[derive(Debug, Clone)]
pub enum GaugeValue {
    Text(String),
    Svg(svg::Handle),
}

/// What a gauge should display for its value area.
#[derive(Debug, Clone)]
pub enum GaugeDisplay {
    Value {
        value: GaugeValue,
        attention: GaugeValueAttention,
    },
    Empty,
    Error,
}

/// One selectable entry in a gauge menu.
#[derive(Debug, Clone)]
pub struct GaugeMenuItem {
    pub id: String,
    pub label: String,
    pub selected: bool,
}

/// Callback invoked when a gauge menu item is selected.
pub type MenuSelectAction = Arc<dyn Fn(String) + Send + Sync>;
/// Callback invoked when a gauge action item is selected.
pub type ActionSelectAction = MenuSelectAction;

/// Context menu model shown for a gauge.
#[derive(Clone)]
pub struct GaugeMenu {
    pub title: String,
    pub items: Vec<GaugeMenuItem>,
    pub on_select: Option<MenuSelectAction>,
}

/// One action entry shown in a gauge action dialog.
#[derive(Debug, Clone)]
pub struct GaugeActionItem {
    pub id: String,
    pub icon: svg::Handle,
}

/// Action dialog model shown for a gauge.
#[derive(Clone)]
pub struct GaugeActionDialog {
    pub title: String,
    pub items: Vec<GaugeActionItem>,
    pub on_select: Option<ActionSelectAction>,
}

/// Full render/update model for a single gauge instance.
#[derive(Clone)]
pub struct GaugeModel {
    /// Stable gauge id used for routing, replacement, and click dispatch.
    pub id: &'static str,
    /// Icon rendered at the top of the gauge.
    pub icon: svg::Handle,
    /// Value/error content shown in the gauge value area.
    pub display: GaugeDisplay,
    /// Optional pointer-input callback for click/scroll interactions.
    pub on_click: Option<GaugeClickAction>,
    /// Optional context menu shown on right-click.
    pub menu: Option<GaugeMenu>,
    /// Optional action dialog shown on right-click when configured.
    pub action_dialog: Option<GaugeActionDialog>,
    /// Optional info dialog content shown for this gauge.
    pub info: Option<InfoDialog>,
}

impl fmt::Debug for GaugeModel {
    fn fmt(&self, f: &mut fmt::Formatter<'_>) -> fmt::Result {
        f.debug_struct("GaugeModel")
            .field("id", &self.id)
            .field("icon", &self.icon)
            .field("display", &self.display)
            .field(
                "menu",
                &self
                    .menu
                    .as_ref()
                    .map(|menu| menu.title.as_str())
                    .unwrap_or("<none>"),
            )
            .field(
                "action_dialog",
                &self
                    .action_dialog
                    .as_ref()
                    .map(|dialog| dialog.title.as_str())
                    .unwrap_or("<none>"),
            )
            .field(
                "info",
                &self
                    .info
                    .as_ref()
                    .map(|dialog| dialog.title.as_str())
                    .unwrap_or("<none>"),
            )
            .finish_non_exhaustive()
    }
}

/// Supported user input events for a gauge.
#[derive(Debug, Clone, Copy)]
pub enum GaugeInput {
    Button(mouse::Button),
    ScrollUp,
    ScrollDown,
}

/// Click/scroll payload delivered to gauge click handlers.
#[derive(Debug, Clone, Copy)]
pub struct GaugeClick {
    pub input: GaugeInput,
}

/// Callback invoked when a gauge receives pointer input.
pub type GaugeClickAction = Arc<dyn Fn(GaugeClick) + Send + Sync>;

/// Callback used by gauges to request immediate scheduling by the work manager.
///
/// Gauges call this after local input/state changes (for example from click handlers)
/// when they want `run_once` invoked before the next deadline.
pub type GaugeReadyNotify = Arc<dyn Fn(&'static str) + Send + Sync>;

/// Runtime contract implemented by every gauge.
///
/// A gauge is a stateful worker that decides when it wants to run next and can emit
/// a new `GaugeModel` for rendering.
pub trait Gauge: Send + 'static {
    /// Stable gauge id. Must match `GaugeSpec::id`.
    fn id(&self) -> &'static str;

    /// Inject the callback used to request immediate scheduling.
    ///
    /// Most gauges can keep the default implementation. Gauges with click/menu callbacks
    /// should store the callback and trigger it after queuing local commands.
    fn bind_ready_notify(&mut self, _notify: GaugeReadyNotify) {}

    /// Next time this gauge should be run by the scheduler.
    ///
    /// The scheduler will not run the gauge before this deadline unless it is explicitly
    /// marked ready via `GaugeReadyNotify`.
    fn next_deadline(&self) -> Instant;

    /// Execute one unit of gauge work.
    ///
    /// Return `Some(GaugeModel)` when the UI should be updated, or `None` to keep the
    /// previously rendered model.
    fn run_once(&mut self, now: Instant) -> Option<GaugeModel>;
}
