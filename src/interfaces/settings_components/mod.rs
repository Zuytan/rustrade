//! Settings components module

pub mod help_about;
pub mod language_settings;
pub mod risk_settings;
pub mod strategy_settings;

pub use help_about::{render_about_tab, render_help_tab, render_shortcuts_tab};
pub use language_settings::render_language_settings;
pub use risk_settings::render_risk_settings;
pub use strategy_settings::render_strategy_settings;
