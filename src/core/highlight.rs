//! syntect-based syntax highlighting; results are cached per buffer version.

use std::io::Cursor;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

use ratatui::style::Color;
use syntect::easy::HighlightLines;
use syntect::highlighting::{Color as SynColor, Style as SynStyle, ThemeSet};
use syntect::parsing::{SyntaxDefinition, SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

use crate::core::theme::Theme;

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();

/// Sublime-syntax definitions compiled into the binary for languages syntect
/// doesn't bundle (e.g. TOML), so highlighting works without external files.
static EMBEDDED_SYNTAXES: &[&str] =
    &[include_str!("../../assets/syntaxes/TOML.sublime-syntax")];

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(|| {
        // `_newlines` because `highlight_line` is fed lines that keep their `\n`.
        let mut builder = SyntaxSet::load_defaults_newlines().into_builder();
        for src in EMBEDDED_SYNTAXES {
            if let Ok(def) = SyntaxDefinition::load_from_str(src, true, None) {
                builder.add(def);
            }
        }
        // Extra user/asset syntaxes (best-effort, silently skipped if absent).
        for dir in syntax_dirs() {
            let _ = builder.add_from_folder(&dir, true);
        }
        builder.build()
    })
}

/// Folders searched for extra `.sublime-syntax` files: user config, env override, repo assets.
fn syntax_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(home).join(".config/coder/syntaxes"));
    }
    if let Ok(d) = std::env::var("CODER_SYNTAXES_DIR") {
        dirs.push(PathBuf::from(d));
    }
    dirs.push(PathBuf::from("assets/syntaxes"));
    dirs
}

fn theme_set() -> &'static ThemeSet {
    THEME_SET.get_or_init(|| {
        let mut ts = ThemeSet::load_defaults();
        // Popular full `.tmTheme` files compiled into the binary. These are the
        // real Sublime Text themes (complete scope coverage), not approximations,
        // so every listed theme is genuinely syntect-compatible.
        for (name, src) in EMBEDDED_THEMES {
            if let Ok(mut theme) = ThemeSet::load_from_reader(&mut Cursor::new(src.as_bytes())) {
                // Key the picker entry off our chosen display name, not the file's
                // internal one (e.g. Gruvbox ships as "gruvbox (Dark) (Medium)").
                theme.name = Some((*name).to_string());
                ts.themes.insert((*name).to_string(), theme);
            }
        }
        // Extra user `.tmTheme` files in config folders (silently skipped if absent).
        for dir in theme_dirs() {
            let _ = ts.add_from_folder(&dir);
        }
        ts
    })
}

/// Folders searched for .tmTheme files: user config, env override, repo assets.
fn theme_dirs() -> Vec<PathBuf> {
    let mut dirs = Vec::new();
    if let Ok(home) = std::env::var("HOME") {
        dirs.push(PathBuf::from(home).join(".config/coder/themes"));
    }
    if let Ok(d) = std::env::var("CODER_THEMES_DIR") {
        dirs.push(PathBuf::from(d));
    }
    dirs.push(PathBuf::from("assets/themes"));
    dirs
}

/// Popular full Sublime Text `.tmTheme` files compiled into the binary, as
/// `(display name, XML source)`. Unlike a hand-rolled palette these carry the
/// theme's complete scope rules, so they highlight every token kind the real
/// Sublime/bat versions do. Only genuinely syntect-loadable themes belong here
/// — the picker must never list a theme that can't render (see `theme_set`).
///
/// `.tmTheme` (TextMate/Sublime XML plist) is the only theme format syntect
/// understands; the newer `.sublime-color-scheme` JSON is not supported, so
/// these are sourced from projects that still ship the XML form (bat's set).
static EMBEDDED_THEMES: &[(&str, &str)] = &[
    ("Dracula", include_str!("../../assets/themes/Dracula.tmTheme")),
    ("Nord", include_str!("../../assets/themes/Nord.tmTheme")),
    ("Monokai Extended", include_str!("../../assets/themes/Monokai Extended.tmTheme")),
    ("One Dark", include_str!("../../assets/themes/One Dark.tmTheme")),
    ("Gruvbox Dark", include_str!("../../assets/themes/Gruvbox Dark.tmTheme")),
    ("Gruvbox Light", include_str!("../../assets/themes/Gruvbox Light.tmTheme")),
    ("Catppuccin Mocha", include_str!("../../assets/themes/Catppuccin Mocha.tmTheme")),
    ("Catppuccin Latte", include_str!("../../assets/themes/Catppuccin Latte.tmTheme")),
];

/// Default syntect theme used at application startup.
pub const DEFAULT_THEME: &str = "base16-eighties.dark";

/// Names of the loaded syntect themes (alphabetical; BTreeMap order).
pub fn theme_names() -> Vec<String> {
    theme_set().themes.keys().cloned().collect()
}

/// Derives the UI palette from a syntect theme's `settings` field.
/// Fields without a counterpart (such as git colors) come from `Theme::default()`.
pub fn theme_for(name: &str) -> Theme {
    let def = Theme::default();
    let Some(t) = theme_set().themes.get(name) else {
        return def;
    };
    let s = &t.settings;
    let bg = s.background.map(conv).unwrap_or(def.bg);
    let fg = s.foreground.map(conv).unwrap_or(def.fg);
    let dark = luma(bg) < 128.0;
    let accent = s.caret.or(s.find_highlight).map(conv).unwrap_or(def.accent);
    // Status bar: a touch darker than the accent on dark themes, a touch lighter on light ones.
    let statusbar_bg = shade(accent, if dark { 0.8 } else { 1.2 });
    Theme {
        bg,
        bg_alt: shade(bg, if dark { 1.25 } else { 0.94 }),
        fg,
        fg_dim: mix(fg, bg, 0.45),
        accent,
        selection: s.selection.map(conv).unwrap_or(def.selection),
        activity_bg: shade(bg, if dark { 1.45 } else { 0.90 }),
        statusbar_bg,
        statusbar_fg: if luma(statusbar_bg) < 128.0 {
            Color::Rgb(255, 255, 255)
        } else {
            Color::Rgb(0, 0, 0)
        },
        tab_active_bg: bg,
        tab_inactive_bg: shade(bg, if dark { 1.25 } else { 0.94 }),
        border: shade(bg, if dark { 1.9 } else { 0.82 }),
        line_number: s.gutter_foreground.map(conv).unwrap_or(def.line_number),
        git_added: def.git_added,
        git_modified: def.git_modified,
        git_deleted: def.git_deleted,
        git_untracked: def.git_untracked,
        // Subtle change backgrounds: mostly the editor bg with a hint of the git color.
        diff_add_bg: mix(def.git_added, bg, 0.82),
        diff_del_bg: mix(def.git_deleted, bg, 0.82),
    }
}

fn conv(c: SynColor) -> Color {
    Color::Rgb(c.r, c.g, c.b)
}

fn rgb_parts(c: Color) -> (u8, u8, u8) {
    if let Color::Rgb(r, g, b) = c {
        (r, g, b)
    } else {
        (128, 128, 128)
    }
}

fn luma(c: Color) -> f32 {
    let (r, g, b) = rgb_parts(c);
    0.299 * r as f32 + 0.587 * g as f32 + 0.114 * b as f32
}

/// Lightens (>1) or darkens (<1) the color by the given factor.
fn shade(c: Color, f: f32) -> Color {
    let (r, g, b) = rgb_parts(c);
    let ap = |v: u8| (v as f32 * f).clamp(0.0, 255.0) as u8;
    Color::Rgb(ap(r), ap(g), ap(b))
}

/// Mixes color `a` toward `b` by the ratio `t`.
fn mix(a: Color, b: Color, t: f32) -> Color {
    let (ar, ag, ab) = rgb_parts(a);
    let (br, bg, bb) = rgb_parts(b);
    let m = |x: u8, y: u8| (x as f32 * (1.0 - t) + y as f32 * t) as u8;
    Color::Rgb(m(ar, br), m(ag, bg), m(ab, bb))
}

/// The highlighted pieces of a line: (color, text).
pub type HlLine = Vec<(Color, String)>;

pub struct Highlighter {
    /// Syntax name for the file this highlighter belongs to.
    syntax_name: String,
    theme_name: String,
    /// Which buffer version the cache was produced for.
    cached_version: Option<u64>,
    cache: Vec<HlLine>,
}

impl Highlighter {
    pub fn for_path(path: Option<&Path>) -> Self {
        let ss = syntax_set();
        let syntax = path
            .and_then(|p| p.extension().and_then(|e| e.to_str()))
            .and_then(|ext| ss.find_syntax_by_extension(ext))
            .or_else(|| {
                path.and_then(|p| p.file_name().and_then(|n| n.to_str()))
                    .and_then(|name| ss.find_syntax_by_token(name))
            })
            .unwrap_or_else(|| ss.find_syntax_plain_text());
        Highlighter {
            syntax_name: syntax.name.clone(),
            theme_name: DEFAULT_THEME.to_string(),
            cached_version: None,
            cache: Vec::new(),
        }
    }

    /// Changes the syntax theme and invalidates the cache
    /// (the next `highlight()` recomputes with the new theme).
    pub fn set_theme(&mut self, name: &str) {
        if self.theme_name != name {
            self.theme_name = name.to_string();
            self.cached_version = None;
        }
    }

    /// Drops the cached highlight so the next `highlight()` recomputes. Needed
    /// when the buffer's content is replaced without advancing its version
    /// (e.g. a disk reload rebuilds the buffer back to version 0).
    pub fn invalidate(&mut self) {
        self.cached_version = None;
    }

    fn syntax(&self) -> &'static SyntaxReference {
        let ss = syntax_set();
        ss.find_syntax_by_name(&self.syntax_name)
            .unwrap_or_else(|| ss.find_syntax_plain_text())
    }

    /// Highlights the entire text line by line; returns the cache if the version is unchanged.
    pub fn highlight(&mut self, text: &str, version: u64) -> &[HlLine] {
        if self.cached_version == Some(version) {
            return &self.cache;
        }
        let ss = syntax_set();
        let themes = &theme_set().themes;
        // Fall back to the default theme if the configured name is unknown (e.g.
        // a stale name in a hand-edited config) — indexing a missing key panics.
        let theme = themes
            .get(&self.theme_name)
            .unwrap_or_else(|| &themes[DEFAULT_THEME]);
        let mut h = HighlightLines::new(self.syntax(), theme);
        let mut out: Vec<HlLine> = Vec::new();
        for line in LinesWithEndings::from(text) {
            let ranges = h.highlight_line(line, ss).unwrap_or_default();
            let mut hl_line: HlLine = Vec::with_capacity(ranges.len());
            for (style, piece) in ranges {
                let piece = piece.trim_end_matches(['\n', '\r']);
                if piece.is_empty() {
                    continue;
                }
                hl_line.push((syn_to_color(style), piece.to_string()));
            }
            out.push(hl_line);
        }
        // `LinesWithEndings` does not emit the final empty line; align with ropey.
        if text.ends_with('\n') || text.is_empty() {
            out.push(Vec::new());
        }
        self.cache = out;
        self.cached_version = Some(version);
        &self.cache
    }
}

fn syn_to_color(style: SynStyle) -> Color {
    Color::Rgb(style.foreground.r, style.foreground.g, style.foreground.b)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn builtin_themes_listed() {
        let names = theme_names();
        // syntect defaults (7) + embedded full .tmTheme files.
        assert!(names.len() >= 7 + EMBEDDED_THEMES.len());
        for (name, _) in EMBEDDED_THEMES {
            assert!(names.iter().any(|n| n == name), "missing theme: {name}");
        }
        assert!(names.iter().any(|n| n == DEFAULT_THEME));
    }

    #[test]
    fn every_embedded_theme_loads() {
        // Guards the "no incompatible theme in the list" contract: each bundled
        // file must actually parse into a syntect theme, or it must not ship.
        let ts = theme_set();
        for (name, _) in EMBEDDED_THEMES {
            let theme = ts.themes.get(*name).unwrap_or_else(|| panic!("did not load: {name}"));
            assert!(theme.settings.background.is_some(), "no background: {name}");
        }
    }

    #[test]
    fn builtin_theme_derives_palette() {
        // The derived UI palette should differ from the default (settings are read).
        let t = theme_for("Dracula");
        assert_eq!(t.bg, Color::Rgb(0x28, 0x2a, 0x36));
    }

    #[test]
    fn toml_files_resolve_to_toml_syntax() {
        let hl = Highlighter::for_path(Some(Path::new("config.toml")));
        assert_eq!(hl.syntax_name, "TOML");
    }

    #[test]
    fn toml_line_highlights_multiple_scopes() {
        let mut hl = Highlighter::for_path(Some(Path::new("config.toml")));
        // A key, a string and a comment should come out as distinct colors, not
        // one flat run of plain-text foreground.
        let lines = hl.highlight("theme = \"dark\" # note\n", 0);
        let colors: std::collections::HashSet<_> =
            lines[0].iter().map(|(c, _)| *c).collect();
        assert!(colors.len() >= 3, "expected varied coloring, got {colors:?}");
    }
}
