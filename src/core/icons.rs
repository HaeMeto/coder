//! File icons used by the Files panel.

const FILE: &str = "\u{ea7b}";
const FILE_CODE: &str = "\u{eae9}";
const FILE_TEXT: &str = "\u{ec5e}";
const FILE_MEDIA: &str = "\u{eaea}";
const FILE_PDF: &str = "\u{eaeb}";
const FILE_ARCHIVE: &str = "\u{eaef}";
const FOLDER: &str = "\u{ea83}";
const FOLDER_OPEN: &str = "\u{eaf7}";
const DOCKER: &str = "\u{e7b0}";

/// Returns a one-cell file or folder icon. ASCII mode uses plain letters so
/// tree names and the right-pinned directory actions remain aligned.
pub fn file(name: &str, is_dir: bool, opened: bool, ascii: bool) -> &'static str {
    if ascii {
        if is_dir {
            return "d";
        }
        return match kind(name) {
            FileIconKind::Code | FileIconKind::Docker => "c",
            FileIconKind::Text => "t",
            FileIconKind::Media => "m",
            FileIconKind::Pdf => "p",
            FileIconKind::Archive => "z",
            FileIconKind::Generic => "f",
        };
    }
    if is_dir {
        return if opened { FOLDER_OPEN } else { FOLDER };
    }
    match kind(name) {
        FileIconKind::Code => FILE_CODE,
        FileIconKind::Text => FILE_TEXT,
        FileIconKind::Media => FILE_MEDIA,
        FileIconKind::Pdf => FILE_PDF,
        FileIconKind::Archive => FILE_ARCHIVE,
        FileIconKind::Docker => DOCKER,
        FileIconKind::Generic => FILE,
    }
}

#[derive(Clone, Copy)]
enum FileIconKind {
    Code,
    Text,
    Media,
    Pdf,
    Archive,
    Docker,
    Generic,
}

fn kind(name: &str) -> FileIconKind {
    let lower = name.to_ascii_lowercase();
    if matches!(name, "Dockerfile" | ".dockerignore")
        || matches!(lower.as_str(), "docker-compose.yml" | "docker-compose.yaml")
    {
        return FileIconKind::Docker;
    }
    match lower.rsplit('.').next().unwrap_or_default() {
        "rs" | "py" | "js" | "jsx" | "ts" | "tsx" | "go" | "java" | "c" | "h" | "cpp" | "hpp"
        | "cs" | "rb" | "php" | "swift" | "kt" | "kts" | "zig" | "lua" | "sh" | "fish" | "vim" => {
            FileIconKind::Code
        }
        "md" | "markdown" | "txt" | "rst" | "adoc" | "log" => FileIconKind::Text,
        "png" | "jpg" | "jpeg" | "gif" | "svg" | "webp" | "bmp" | "ico" => FileIconKind::Media,
        "pdf" => FileIconKind::Pdf,
        "zip" | "tar" | "gz" | "bz2" | "xz" | "7z" => FileIconKind::Archive,
        _ => FileIconKind::Generic,
    }
}
