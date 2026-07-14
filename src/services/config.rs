//! Persisted user preferences (selected theme + editor settings) in a TOML file.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

use crate::core::highlight::DEFAULT_THEME;

/// User preferences serialized to `~/.config/coder/config.toml`.
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    /// Selected theme name.
    pub theme: String,
    /// Run the enabled format actions when saving.
    pub format_on_save: bool,
    /// Strip trailing whitespace on save.
    pub trim_trailing_whitespace: bool,
    /// Ensure a single final newline on save.
    pub insert_final_newline: bool,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            theme: DEFAULT_THEME.to_string(),
            format_on_save: false,
            trim_trailing_whitespace: true,
            insert_final_newline: true,
        }
    }
}

/// Path of the config file: `$CODER_CONFIG` override, else `~/.config/coder/config.toml`.
fn config_path() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("CODER_CONFIG") {
        return Some(PathBuf::from(p));
    }
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".config/coder/config.toml"))
}

/// Loads the config, falling back to defaults when missing or unparseable.
pub fn load() -> Config {
    let Some(path) = config_path() else {
        return Config::default();
    };
    match std::fs::read_to_string(&path) {
        Ok(text) => toml::from_str(&text).unwrap_or_default(),
        Err(_) => Config::default(),
    }
}

/// Writes the config to disk (creating the parent directory). Errors are ignored.
pub fn save(config: &Config) {
    let Some(path) = config_path() else {
        return;
    };
    if let Some(dir) = path.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    if let Ok(text) = toml::to_string_pretty(config) {
        let _ = std::fs::write(&path, text);
    }
}
