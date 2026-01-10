use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::path::Path;

/// Language metadata loaded from JSON
#[derive(Debug, Clone, PartialEq, Eq, Hash, Serialize, Deserialize)]
pub struct LanguageInfo {
    pub code: String,
    pub name: String,
    pub flag: String,
    pub native_name: String,
}

/// Translation data loaded from JSON
#[derive(Debug, Clone, Deserialize)]
pub struct TranslationData {
    pub language: LanguageInfo,
    pub ui: HashMap<String, String>,
    pub help_categories: HashMap<String, String>,
    pub help_topics: Vec<HelpTopicData>,
}

#[derive(Debug, Clone, Deserialize)]
pub struct HelpTopicData {
    pub id: String,
    pub category: String,
    pub title: String,
    pub abbreviation: Option<String>,
    pub full_name: String,
    pub description: String,
    pub example: Option<String>,
}

/// Internationalization service with dynamic language loading
pub struct I18nService {
    current_language: String,
    translations: HashMap<String, TranslationData>,
    available_languages: Vec<LanguageInfo>,
}

impl I18nService {
    /// Create a new I18nService that auto-discovers all translation files
    pub fn new() -> Self {
        let mut translations = HashMap::new();
        let mut available_languages = Vec::new();

        // Auto-discover all .json files in translations directory
        let translations_dir = Path::new("translations");
        if let Ok(entries) = std::fs::read_dir(translations_dir) {
            for entry in entries.flatten() {
                let path = entry.path();
                if path.extension().and_then(|s| s.to_str()) == Some("json")
                    && let Ok(json_content) = std::fs::read_to_string(&path)
                    && let Ok(data) = serde_json::from_str::<TranslationData>(&json_content)
                {
                    let lang_code = data.language.code.clone();
                    available_languages.push(data.language.clone());
                    translations.insert(lang_code, data);
                }
            }
        }

        // Sort languages by code for consistency
        available_languages.sort_by(|a, b| a.code.cmp(&b.code));

        // Default to first available language, or "fr" if specified
        let default_lang = available_languages
            .iter()
            .find(|l| l.code == "fr")
            .or_else(|| available_languages.first())
            .map(|l| l.code.clone())
            .unwrap_or_else(|| "en".to_string());

        Self {
            current_language: default_lang,
            translations,
            available_languages,
        }
    }

    /// Get list of all available languages
    pub fn available_languages(&self) -> &[LanguageInfo] {
        &self.available_languages
    }

    /// Get current language info
    pub fn current_language_info(&self) -> Option<&LanguageInfo> {
        self.available_languages
            .iter()
            .find(|l| l.code == self.current_language)
    }

    /// Set current language by code
    pub fn set_language(&mut self, language_code: &str) -> bool {
        if self.translations.contains_key(language_code) {
            self.current_language = language_code.to_string();
            true
        } else {
            false
        }
    }

    /// Get current language code
    pub fn current_language_code(&self) -> &str {
        &self.current_language
    }

    /// Translate a UI key
    pub fn t<'a>(&'a self, key: &'a str) -> &'a str {
        self.translations
            .get(&self.current_language)
            .and_then(|data| data.ui.get(key))
            .map(|s| s.as_str())
            .unwrap_or(key)
    }

    /// Get category name
    pub fn category_name<'a>(&'a self, category_key: &'a str) -> &'a str {
        self.translations
            .get(&self.current_language)
            .and_then(|data| data.help_categories.get(category_key))
            .map(|s| s.as_str())
            .unwrap_or(category_key)
    }

    /// Translate with format parameters
    /// Usage: i18n.tf("pnl_label", &[("sign", "+"), ("amount", "10.50"), ("percent", "5.2")])
    /// Template in JSON: "P&L: {sign}{amount} ({sign}{percent}%)"
    pub fn tf(&self, key: &str, params: &[(&str, &str)]) -> String {
        let template = self.t(key);
        let mut result = template.to_string();

        for (placeholder, value) in params {
            let placeholder_pattern = format!("{{{}}}", placeholder);
            result = result.replace(&placeholder_pattern, value);
        }

        result
    }

    /// Get all help topics for current language
    pub fn help_topics(&self) -> Vec<&HelpTopicData> {
        self.translations
            .get(&self.current_language)
            .map(|data| data.help_topics.iter().collect())
            .unwrap_or_default()
    }

    /// Search help topics
    pub fn search_help(&self, query: &str) -> Vec<&HelpTopicData> {
        let query_lower = query.to_lowercase();
        self.help_topics()
            .into_iter()
            .filter(|topic| {
                topic.title.to_lowercase().contains(&query_lower)
                    || topic.full_name.to_lowercase().contains(&query_lower)
                    || topic.description.to_lowercase().contains(&query_lower)
                    || topic
                        .abbreviation
                        .as_ref()
                        .map(|a| a.to_lowercase().contains(&query_lower))
                        .unwrap_or(false)
            })
            .collect()
    }

    /// Get topics by category
    pub fn topics_by_category(&self, category: &str) -> Vec<&HelpTopicData> {
        self.help_topics()
            .into_iter()
            .filter(|topic| topic.category == category)
            .collect()
    }
}

impl Default for I18nService {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_auto_discovery() {
        let i18n = I18nService::new();
        // Should auto-discover at least French and English
        assert!(!i18n.available_languages().is_empty());
    }

    #[test]
    fn test_language_switching() {
        let mut i18n = I18nService::new();

        if i18n.available_languages().len() >= 2 {
            let first_lang = i18n.available_languages()[0].code.clone();
            let second_lang = i18n.available_languages()[1].code.clone();

            assert!(i18n.set_language(&first_lang));
            assert_eq!(i18n.current_language_code(), &first_lang);

            assert!(i18n.set_language(&second_lang));
            assert_eq!(i18n.current_language_code(), &second_lang);
        }
    }

    #[test]
    fn test_translation_loading() {
        let i18n = I18nService::new();
        // Should load help topics
        assert!(!i18n.help_topics().is_empty());
    }
}
