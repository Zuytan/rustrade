// Re-export i18n types for convenience
pub use super::service::{HelpTopicData, I18nService, LanguageInfo};

/// Help content categories (used as keys in translation files)
pub const CATEGORY_ABBREVIATIONS: &str = "abbreviations";
pub const CATEGORY_STRATEGIES: &str = "strategies";
pub const CATEGORY_INDICATORS: &str = "indicators";
pub const CATEGORY_RISK_MANAGEMENT: &str = "risk_management";
pub const CATEGORY_ORDER_TYPES: &str = "order_types";

/// All help categories
pub fn all_categories() -> Vec<&'static str> {
    vec![
        CATEGORY_ABBREVIATIONS,
        CATEGORY_STRATEGIES,
        CATEGORY_INDICATORS,
        CATEGORY_RISK_MANAGEMENT,
        CATEGORY_ORDER_TYPES,
    ]
}
