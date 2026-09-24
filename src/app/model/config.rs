//! Persisted preferences: theme selection and editor settings.

use crate::core::highlight;

use super::Model;

impl Model {
    /// Applies the theme at the given index: UI palette + syntax theme for all tabs.
    pub fn apply_theme(&mut self, idx: usize) {
        let Some(name) = self.sidebar.themes.names.get(idx).cloned() else {
            return;
        };
        self.sidebar.themes.selected = idx;
        self.theme = highlight::theme_for(&name);
        // The worker re-highlights with the new theme (carried in the next job);
        // invalidation drops the old colors and forces a fresh submission.
        self.invalidate_highlight();
    }

    /// Applies persisted preferences: selected theme + editor settings.
    pub fn apply_config(&mut self, config: &crate::services::config::Config) {
        if let Some(idx) = self
            .sidebar
            .themes
            .names
            .iter()
            .position(|n| n == &config.theme)
        {
            self.apply_theme(idx);
        }
        let s = &mut self.sidebar.settings;
        s.format_on_save = config.format_on_save;
        s.format_on_paste = config.format_on_paste;
        s.trim_trailing_whitespace = config.trim_trailing_whitespace;
        s.insert_final_newline = config.insert_final_newline;
        s.inline_diagnostics = config.inline_diagnostics;
        // CODER_ASCII still wins when set (a quick one-off override); otherwise
        // the persisted Settings-panel toggle governs.
        if std::env::var("CODER_ASCII").is_err() {
            self.ascii_icons = config.ascii_icons;
        }
        self.extensions =
            crate::services::extensions::ExtensionRegistry::from_config(&config.languages);
    }

    /// Snapshot of the current preferences, for persisting to disk.
    pub fn config_snapshot(&self) -> crate::services::config::Config {
        let s = &self.sidebar.settings;
        crate::services::config::Config {
            theme: self.current_theme_name().to_string(),
            format_on_save: s.format_on_save,
            format_on_paste: s.format_on_paste,
            trim_trailing_whitespace: s.trim_trailing_whitespace,
            insert_final_newline: s.insert_final_newline,
            inline_diagnostics: s.inline_diagnostics,
            ascii_icons: self.ascii_icons,
            languages: self.extensions.to_language_configs(),
        }
    }

    /// Name of the currently selected theme.
    pub fn current_theme_name(&self) -> &str {
        let t = &self.sidebar.themes;
        t.names
            .get(t.selected)
            .map(String::as_str)
            .unwrap_or(highlight::DEFAULT_THEME)
    }
}
