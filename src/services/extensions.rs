//! Language extensions: declarative TOML manifests that map a language to an
//! LSP server plus optional standalone formatter/linter commands.
//!
//! Each extension lives at `~/.config/coder/extensions/<name>/extension.toml`
//! (or `$CODER_EXTENSIONS/<name>/extension.toml`). The user installs the actual
//! binaries (rust-analyzer, pyright, black, ruff); the editor only orchestrates
//! them. A few built-in defaults ship so the feature works with zero config.

use std::collections::HashMap;
use std::path::{Path, PathBuf};

use serde::Deserialize;

/// One installed extension: a name and the languages it supports.
#[derive(Debug, Clone, Deserialize)]
pub struct ExtensionManifest {
    pub name: String,
    #[serde(default)]
    pub languages: Vec<LanguageDef>,
}

/// A single language's capabilities within an extension.
#[derive(Debug, Clone, Deserialize)]
pub struct LanguageDef {
    /// LSP language identifier (e.g. "rust", "python").
    pub id: String,
    /// File extensions (without the dot) this language claims.
    #[serde(default)]
    pub extensions: Vec<String>,
    /// Language server to launch (stdio JSON-RPC).
    pub lsp: Option<ServerSpec>,
    /// Standalone formatter: stdin -> stdout (used when the LSP can't format).
    pub formatter: Option<ToolSpec>,
    /// Standalone linter: stdin -> stdout diagnostics (for LSP-less languages).
    pub linter: Option<ToolSpec>,
}

/// A language-server command.
#[derive(Debug, Clone, Deserialize)]
pub struct ServerSpec {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
    /// Extra environment variables passed to the server process.
    #[serde(default)]
    pub env: Vec<(String, String)>,
}

/// A standalone command-line tool that reads stdin and writes stdout.
#[derive(Debug, Clone, Deserialize)]
pub struct ToolSpec {
    pub command: String,
    #[serde(default)]
    pub args: Vec<String>,
}

/// All loaded extensions plus a fast extension -> language lookup.
#[derive(Debug, Clone, Default)]
pub struct ExtensionRegistry {
    manifests: Vec<ExtensionManifest>,
    /// Maps a file extension to `(manifest index, language index)`.
    by_ext: HashMap<String, (usize, usize)>,
}

impl ExtensionRegistry {
    /// The installed extensions, for the Extensions panel.
    pub fn manifests(&self) -> &[ExtensionManifest] {
        &self.manifests
    }

    /// Resolves the language for a file by its extension.
    pub fn language_for_path(&self, path: &Path) -> Option<&LanguageDef> {
        let ext = path.extension().and_then(|e| e.to_str())?;
        let &(m, l) = self.by_ext.get(ext)?;
        Some(&self.manifests[m].languages[l])
    }

    /// Builds the extension -> language index from the current manifests. A later
    /// manifest (user config) wins over an earlier one (built-in) for the same
    /// extension, since user extensions are appended after the built-ins.
    fn reindex(&mut self) {
        self.by_ext.clear();
        for (m, manifest) in self.manifests.iter().enumerate() {
            for (l, lang) in manifest.languages.iter().enumerate() {
                for ext in &lang.extensions {
                    self.by_ext.insert(ext.clone(), (m, l));
                }
            }
        }
    }
}

/// Directory holding user extensions: `$CODER_EXTENSIONS` override, else
/// `~/.config/coder/extensions`.
fn extensions_dir() -> Option<PathBuf> {
    if let Ok(p) = std::env::var("CODER_EXTENSIONS") {
        return Some(PathBuf::from(p));
    }
    let home = std::env::var("HOME").ok()?;
    Some(PathBuf::from(home).join(".config/coder/extensions"))
}

/// Parses a single `extension.toml`, returning `None` (never panicking) on any
/// read or parse error so one bad extension can't take down the editor.
fn load_manifest(path: &Path) -> Option<ExtensionManifest> {
    let text = std::fs::read_to_string(path).ok()?;
    toml::from_str(&text).ok()
}

/// Loads every extension: built-in defaults first, then user manifests (which
/// override built-ins for a shared extension via `reindex`'s last-wins rule).
pub fn load_all() -> ExtensionRegistry {
    let mut manifests = builtins();
    if let Some(dir) = extensions_dir()
        && let Ok(entries) = std::fs::read_dir(&dir)
    {
        let mut dirs: Vec<PathBuf> = entries
            .flatten()
            .map(|e| e.path())
            .filter(|p| p.is_dir())
            .collect();
        dirs.sort();
        for d in dirs {
            if let Some(m) = load_manifest(&d.join("extension.toml")) {
                manifests.push(m);
            }
        }
    }
    let mut registry = ExtensionRegistry {
        manifests,
        by_ext: HashMap::new(),
    };
    registry.reindex();
    registry
}

/// Built-in language defaults so common languages work with zero user config.
/// The user still needs the actual binaries on PATH.
fn builtins() -> Vec<ExtensionManifest> {
    vec![
        ExtensionManifest {
            name: "rust".to_string(),
            languages: vec![LanguageDef {
                id: "rust".to_string(),
                extensions: vec!["rs".to_string()],
                lsp: Some(ServerSpec {
                    command: "rust-analyzer".to_string(),
                    args: vec![],
                    env: vec![],
                }),
                formatter: Some(ToolSpec {
                    command: "rustfmt".to_string(),
                    args: vec!["--edition".to_string(), "2021".to_string()],
                }),
                linter: None,
            }],
        },
        ExtensionManifest {
            name: "python".to_string(),
            languages: vec![LanguageDef {
                id: "python".to_string(),
                extensions: vec!["py".to_string(), "pyi".to_string()],
                lsp: Some(ServerSpec {
                    command: "pyright-langserver".to_string(),
                    args: vec!["--stdio".to_string()],
                    env: vec![],
                }),
                formatter: Some(ToolSpec {
                    command: "black".to_string(),
                    args: vec!["-".to_string(), "-q".to_string()],
                }),
                linter: None,
            }],
        },
    ]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtins_resolve_by_extension() {
        let mut reg = ExtensionRegistry {
            manifests: builtins(),
            by_ext: HashMap::new(),
        };
        reg.reindex();
        assert_eq!(
            reg.language_for_path(Path::new("main.rs")).map(|l| &l.id[..]),
            Some("rust")
        );
        assert_eq!(
            reg.language_for_path(Path::new("a/b/app.py"))
                .map(|l| &l.id[..]),
            Some("python")
        );
        assert!(reg.language_for_path(Path::new("notes.txt")).is_none());
        assert_eq!(
            reg.language_for_path(Path::new("main.rs"))
                .and_then(|l| l.lsp.as_ref())
                .map(|s| &s.command[..]),
            Some("rust-analyzer")
        );
    }

    #[test]
    fn manifest_parses_from_toml() {
        let src = r#"
            name = "go"
            [[languages]]
            id = "go"
            extensions = ["go"]
            [languages.lsp]
            command = "gopls"
            [languages.formatter]
            command = "gofmt"
        "#;
        let m: ExtensionManifest = toml::from_str(src).unwrap();
        assert_eq!(m.name, "go");
        assert_eq!(m.languages[0].extensions, ["go"]);
        assert_eq!(m.languages[0].lsp.as_ref().unwrap().command, "gopls");
        assert!(m.languages[0].linter.is_none());
    }

    #[test]
    fn user_manifest_overrides_builtin_extension() {
        let mut manifests = builtins();
        manifests.push(ExtensionManifest {
            name: "custom-rust".to_string(),
            languages: vec![LanguageDef {
                id: "rust".to_string(),
                extensions: vec!["rs".to_string()],
                lsp: Some(ServerSpec {
                    command: "my-analyzer".to_string(),
                    args: vec![],
                    env: vec![],
                }),
                formatter: None,
                linter: None,
            }],
        });
        let mut reg = ExtensionRegistry {
            manifests,
            by_ext: HashMap::new(),
        };
        reg.reindex();
        // Last (user) manifest wins for the .rs extension.
        assert_eq!(
            reg.language_for_path(Path::new("x.rs"))
                .and_then(|l| l.lsp.as_ref())
                .map(|s| &s.command[..]),
            Some("my-analyzer")
        );
    }
}
