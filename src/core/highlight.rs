//! syntect-based syntax highlighting; results are cached per buffer version.

use std::path::{Path, PathBuf};
use std::str::FromStr;
use std::sync::OnceLock;

use ratatui::style::Color;
use syntect::easy::HighlightLines;
use syntect::highlighting::{
    Color as SynColor, ScopeSelectors, Style as SynStyle, StyleModifier, Theme as SynTheme,
    ThemeItem, ThemeSet, ThemeSettings,
};
use syntect::parsing::{SyntaxReference, SyntaxSet};
use syntect::util::LinesWithEndings;

use crate::core::theme::Theme;

static SYNTAX_SET: OnceLock<SyntaxSet> = OnceLock::new();
static THEME_SET: OnceLock<ThemeSet> = OnceLock::new();

fn syntax_set() -> &'static SyntaxSet {
    SYNTAX_SET.get_or_init(SyntaxSet::load_defaults_newlines)
}

fn theme_set() -> &'static ThemeSet {
    THEME_SET.get_or_init(|| {
        let mut ts = ThemeSet::load_defaults();
        // Programmatically embedded popular themes.
        for p in PALETTES {
            ts.themes.insert(p.name.to_string(), build_theme(p));
        }
        // .tmTheme files in user folders (silently skipped if absent).
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

/// Embedded theme palette: background, foreground, selection + basic syntax scope colors (0xRRGGBB).
struct Palette {
    name: &'static str,
    bg: u32,
    fg: u32,
    sel: u32,
    comment: u32,
    string: u32,
    keyword: u32,
    func: u32,
    constant: u32,
    type_: u32,
}

#[rustfmt::skip]
static PALETTES: &[Palette] = &[
    Palette { name: "Dracula",          bg: 0x282a36, fg: 0xf8f8f2, sel: 0x44475a, comment: 0x6272a4, string: 0xf1fa8c, keyword: 0xff79c6, func: 0x50fa7b, constant: 0xbd93f9, type_: 0x8be9fd },
    Palette { name: "Gruvbox Dark",     bg: 0x282828, fg: 0xebdbb2, sel: 0x3c3836, comment: 0x928374, string: 0xb8bb26, keyword: 0xfb4934, func: 0xb8bb26, constant: 0xd3869b, type_: 0xfabd2f },
    Palette { name: "Gruvbox Light",    bg: 0xfbf1c7, fg: 0x3c3836, sel: 0xebdbb2, comment: 0x928374, string: 0x79740e, keyword: 0x9d0006, func: 0x79740e, constant: 0x8f3f71, type_: 0xb57614 },
    Palette { name: "Nord",             bg: 0x2e3440, fg: 0xd8dee9, sel: 0x434c5e, comment: 0x616e88, string: 0xa3be8c, keyword: 0x81a1c1, func: 0x88c0d0, constant: 0xb48ead, type_: 0x8fbcbb },
    Palette { name: "One Dark",         bg: 0x282c34, fg: 0xabb2bf, sel: 0x3e4451, comment: 0x5c6370, string: 0x98c379, keyword: 0xc678dd, func: 0x61afef, constant: 0xd19a66, type_: 0xe5c07b },
    Palette { name: "Monokai",          bg: 0x272822, fg: 0xf8f8f2, sel: 0x49483e, comment: 0x75715e, string: 0xe6db74, keyword: 0xf92672, func: 0xa6e22e, constant: 0xae81ff, type_: 0x66d9ef },
    Palette { name: "Tokyo Night",      bg: 0x1a1b26, fg: 0xc0caf5, sel: 0x283457, comment: 0x565f89, string: 0x9ece6a, keyword: 0xbb9af7, func: 0x7aa2f7, constant: 0xff9e64, type_: 0x2ac3de },
    Palette { name: "Catppuccin Mocha", bg: 0x1e1e2e, fg: 0xcdd6f4, sel: 0x313244, comment: 0x6c7086, string: 0xa6e3a1, keyword: 0xcba6f7, func: 0x89b4fa, constant: 0xfab387, type_: 0xf9e2af },
];

fn hexc(v: u32) -> SynColor {
    SynColor {
        r: (v >> 16) as u8,
        g: (v >> 8) as u8,
        b: v as u8,
        a: 0xFF,
    }
}

fn scope_item(selector: &str, color: u32) -> ThemeItem {
    ThemeItem {
        scope: ScopeSelectors::from_str(selector).unwrap_or_default(),
        style: StyleModifier {
            foreground: Some(hexc(color)),
            background: None,
            font_style: None,
        },
    }
}

/// Builds a syntect theme from a palette (settings + basic scope rules).
fn build_theme(p: &Palette) -> SynTheme {
    let settings = ThemeSettings {
        foreground: Some(hexc(p.fg)),
        background: Some(hexc(p.bg)),
        caret: Some(hexc(p.keyword)),
        selection: Some(hexc(p.sel)),
        line_highlight: Some(hexc(p.sel)),
        gutter_foreground: Some(hexc(p.comment)),
        ..Default::default()
    };
    let scopes = vec![
        scope_item("comment", p.comment),
        scope_item("string, string.quoted", p.string),
        scope_item(
            "constant.numeric, constant.language, constant.character, constant",
            p.constant,
        ),
        scope_item("keyword, storage.modifier, keyword.control", p.keyword),
        scope_item(
            "entity.name.function, support.function, meta.function-call",
            p.func,
        ),
        scope_item(
            "entity.name.type, entity.name.class, support.type, support.class, storage.type",
            p.type_,
        ),
        scope_item("entity.name.tag", p.keyword),
        scope_item("entity.other.attribute-name", p.type_),
    ];
    SynTheme {
        name: Some(p.name.to_string()),
        author: Some("coder".to_string()),
        settings,
        scopes,
    }
}

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
    Theme {
        bg,
        bg_alt: shade(bg, if dark { 1.25 } else { 0.94 }),
        fg,
        fg_dim: mix(fg, bg, 0.45),
        accent,
        selection: s.selection.map(conv).unwrap_or(def.selection),
        activity_bg: shade(bg, if dark { 1.45 } else { 0.90 }),
        statusbar_bg: accent,
        statusbar_fg: if luma(accent) < 128.0 {
            Color::Rgb(255, 255, 255)
        } else {
            Color::Rgb(0, 0, 0)
        },
        tab_active_bg: bg,
        tab_inactive_bg: shade(bg, if dark { 1.25 } else { 0.94 }),
        border: shade(bg, if dark { 1.9 } else { 0.82 }),
        line_number: s.gutter_foreground.map(conv).unwrap_or(def.line_number),
        cursor_line: s
            .line_highlight
            .map(conv)
            .unwrap_or_else(|| shade(bg, if dark { 1.2 } else { 0.95 })),
        git_added: def.git_added,
        git_modified: def.git_modified,
        git_deleted: def.git_deleted,
        git_untracked: def.git_untracked,
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
        let theme = &theme_set().themes[&self.theme_name];
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
        // syntect defaults (7) + embedded palettes.
        assert!(names.len() >= 7 + PALETTES.len());
        for p in PALETTES {
            assert!(names.iter().any(|n| n == p.name), "missing theme: {}", p.name);
        }
        assert!(names.iter().any(|n| n == DEFAULT_THEME));
    }

    #[test]
    fn builtin_theme_derives_palette() {
        // The derived UI palette should differ from the default (settings are read).
        let t = theme_for("Dracula");
        assert_eq!(t.bg, Color::Rgb(0x28, 0x2a, 0x36));
    }
}
