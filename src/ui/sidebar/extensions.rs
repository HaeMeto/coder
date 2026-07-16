//! Extensions panel: lists installed language extensions and their capabilities.

use ratatui::Frame;
use ratatui::layout::Rect;
use ratatui::style::Style;
use ratatui::text::{Line, Span};
use ratatui::widgets::Paragraph;

use crate::app::model::Model;

pub(super) fn render(frame: &mut Frame, area: Rect, model: &Model) {
    let th = &model.theme;
    let mut lines: Vec<Line> = Vec::new();

    let manifests = model.extensions.manifests();
    if manifests.is_empty() {
        lines.push(Line::from(Span::styled(
            " No extensions installed.",
            Style::new().fg(th.fg_dim),
        )));
    } else {
        for manifest in manifests {
            lines.push(Line::from(Span::styled(
                format!(" {}", manifest.name),
                Style::new().fg(th.fg),
            )));
            for lang in &manifest.languages {
                // Declared capabilities for this language.
                let mut caps: Vec<&str> = Vec::new();
                if lang.lsp.is_some() {
                    caps.push("lsp");
                }
                if lang.formatter.is_some() {
                    caps.push("fmt");
                }
                if lang.linter.is_some() {
                    caps.push("lint");
                }
                let caps = if caps.is_empty() {
                    "—".to_string()
                } else {
                    caps.join(" · ")
                };
                lines.push(Line::from(vec![
                    Span::styled(format!("   {} ", lang.id), Style::new().fg(th.fg_dim)),
                    Span::styled(caps, Style::new().fg(th.accent)),
                ]));
            }
        }
    }

    // Running / starting language servers.
    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " LANGUAGE SERVERS",
        Style::new().fg(th.fg_dim),
    )));
    let mut langs: Vec<&String> = model
        .lsp
        .sessions
        .keys()
        .chain(model.lsp.starting.iter())
        .collect();
    langs.sort();
    langs.dedup();
    if langs.is_empty() {
        lines.push(Line::from(Span::styled(
            "   none running",
            Style::new().fg(th.fg_dim),
        )));
    } else {
        for lang in langs {
            // Status glyph: running (initialized), starting, or connecting.
            let (glyph, color) = if model.lsp.initialized.contains(lang) {
                ("●", th.git_added)
            } else if model.lsp.sessions.contains_key(lang) {
                ("◐", th.git_modified)
            } else {
                ("○", th.fg_dim)
            };
            // Total diagnostics reported for files of this language.
            let diag_count: usize = model
                .diagnostics
                .iter()
                .filter(|(p, _)| {
                    model
                        .extensions
                        .language_for_path(p)
                        .map(|l| l.id.as_str() == lang.as_str())
                        .unwrap_or(false)
                })
                .map(|(_, d)| d.len())
                .sum();
            let mut spans = vec![
                Span::styled(format!("   {glyph} "), Style::new().fg(color)),
                Span::styled(lang.to_string(), Style::new().fg(th.fg)),
            ];
            if diag_count > 0 {
                spans.push(Span::styled(
                    format!("  {diag_count} diag"),
                    Style::new().fg(th.fg_dim),
                ));
            }
            lines.push(Line::from(spans));
        }
    }

    lines.push(Line::from(""));
    lines.push(Line::from(Span::styled(
        " ~/.config/coder/extensions/<name>/extension.toml",
        Style::new().fg(th.fg_dim),
    )));

    let p = Paragraph::new(lines).style(Style::new().bg(th.bg_alt));
    frame.render_widget(p, area);
}
