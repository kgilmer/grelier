pub mod apps;
pub mod bar;
pub mod monitor;
pub mod runtime_dispatch;
pub mod settings;
pub mod settings_storage;
pub mod sway_workspace;
pub mod theme;

mod dialog;
mod icon;
mod panels;

pub mod gauges {
    pub use crate::panels::gauges::gauge;
    pub use crate::panels::gauges::gauge_registry;
    pub use crate::panels::gauges::gauge_work_manager;
}
