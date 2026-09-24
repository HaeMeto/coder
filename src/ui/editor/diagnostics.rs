//! Severity glyphs, colors and ranking for LSP / linter diagnostics.

use ratatui::style::Color;

use crate::core::theme::Theme;
use crate::services::lsp::Severity;

/// Lower rank = more severe (Error wins over Warning wins over Info/Hint).
pub(super) fn severity_rank(sev: Severity) -> u8 {
    match sev {
        Severity::Error => 0,
        Severity::Warning => 1,
        Severity::Info => 2,
        Severity::Hint => 3,
    }
}

/// The glyph marking a diagnostic of the given severity (codicon, or ASCII when
/// `CODER_ASCII` is set).
pub fn severity_icon(sev: Severity, ascii: bool) -> &'static str {
    if ascii {
        match sev {
            Severity::Error => "E",
            Severity::Warning => "W",
            Severity::Info => "i",
            Severity::Hint => "h",
        }
    } else {
        match sev {
            Severity::Error => "\u{ea87}",   // error
            Severity::Warning => "\u{ea6c}", // warning
            Severity::Info => "\u{ea74}",    // info
            Severity::Hint => "\u{ea61}",    // lightbulb
        }
    }
}

/// The color used to mark a diagnostic of the given severity.
pub fn severity_color(th: &Theme, sev: Severity) -> Color {
    match sev {
        Severity::Error => th.git_deleted,
        Severity::Warning => th.git_modified,
        Severity::Info | Severity::Hint => th.accent,
    }
}
