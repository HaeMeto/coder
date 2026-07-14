//! VSCode Dark+ style color palette.

use ratatui::style::Color;

pub struct Theme {
    pub bg: Color,
    pub bg_alt: Color,
    pub fg: Color,
    pub fg_dim: Color,
    pub accent: Color,
    pub selection: Color,
    pub activity_bg: Color,
    pub statusbar_bg: Color,
    pub statusbar_fg: Color,
    pub tab_active_bg: Color,
    pub tab_inactive_bg: Color,
    pub border: Color,
    pub line_number: Color,
    pub cursor_line: Color,
    pub git_added: Color,
    pub git_modified: Color,
    pub git_deleted: Color,
    pub git_untracked: Color,
}

impl Default for Theme {
    fn default() -> Self {
        Theme {
            bg: Color::Rgb(30, 30, 30),
            bg_alt: Color::Rgb(37, 37, 38),
            fg: Color::Rgb(212, 212, 212),
            fg_dim: Color::Rgb(133, 133, 133),
            accent: Color::Rgb(0, 122, 204),
            selection: Color::Rgb(38, 79, 120),
            activity_bg: Color::Rgb(51, 51, 51),
            statusbar_bg: Color::Rgb(0, 122, 204),
            statusbar_fg: Color::Rgb(255, 255, 255),
            tab_active_bg: Color::Rgb(30, 30, 30),
            tab_inactive_bg: Color::Rgb(45, 45, 45),
            border: Color::Rgb(64, 64, 64),
            line_number: Color::Rgb(133, 133, 133),
            cursor_line: Color::Rgb(40, 40, 40),
            git_added: Color::Rgb(129, 184, 139),
            git_modified: Color::Rgb(226, 192, 141),
            git_deleted: Color::Rgb(199, 118, 117),
            git_untracked: Color::Rgb(115, 201, 145),
        }
    }
}
