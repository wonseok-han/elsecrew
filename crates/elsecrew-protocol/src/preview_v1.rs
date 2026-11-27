//! Result preview (ADE 1차-나 F12, `docs/proposals/m5-ade-layout-plan.md` 2장):
//! the read-only contract the app uses to show a task workspace's static web
//! output in an isolated view. Nothing here writes; the sidecar only resolves
//! the task's own workspace and reads allow-listed files from it.
//!
//! The same rules live in `src/m5/contracts/preview-v1.ts`; keep them equal.

/// Optional ready suffix, after `provider.install`, in this order.
/// `preview.buildPlan` (2026-11-27) is read-only too: what a build needs.
pub const PREVIEW_METHODS_V1: &[&str] = &["preview.resolve", "preview.readAsset", "preview.buildPlan", "preview.buildReserve", "preview.buildRelease"];

/// One file at most (the same bound as `file.read`).
pub const PREVIEW_ASSET_MAX_BYTES_V1: u64 = 5 * 1024 * 1024;
/// Raw bytes per `preview.readAsset` answer. Base64 of this plus the JSON
/// frame stays under the 1 MiB control line.
pub const PREVIEW_ASSET_CHUNK_BYTES_V1: u64 = 512 * 1024;
/// A path longer than this, or with more segments, is refused.
pub const PREVIEW_PATH_MAX_BYTES_V1: usize = 1024;
pub const PREVIEW_PATH_MAX_SEGMENTS_V1: usize = 32;

/// Entry candidates, in order, when no entry file is named.
pub const PREVIEW_ENTRY_CANDIDATES_V1: &[&str] = &[
    "index.html",
    "public/index.html",
    "dist/index.html",
    "build/index.html",
    "docs/index.html",
];

/// Every other extension is not found. `wasm` and `pdf` stay out in 1차.
pub const PREVIEW_EXTENSIONS_V1: &[&str] = &[
    "html", "htm", "css", "js", "mjs", "json", "map", "txt", "md", "svg", "png", "jpg", "jpeg",
    "gif", "webp", "avif", "ico", "bmp", "woff", "woff2", "ttf", "otf", "mp3", "wav", "ogg", "mp4",
    "webm",
];

/// An entry file must be a page.
pub const PREVIEW_ENTRY_EXTENSIONS_V1: &[&str] = &["html", "htm"];

/// Why a path is refused before any file is opened. The reason is a fixed
/// label for diagnostics, never shown to the page.
pub fn preview_path_refusal_v1(path: &str) -> Option<&'static str> {
    if path.is_empty() {
        return Some("empty");
    }
    if path.len() > PREVIEW_PATH_MAX_BYTES_V1 {
        return Some("tooLong");
    }
    if path.starts_with('/') || path.contains('\\') || path.contains(':') {
        return Some("notRelative");
    }
    if path.chars().any(|c| c.is_control()) {
        return Some("control");
    }
    // Decoding happens once, in the app. A second level of encoding is never
    // a real file name the preview needs.
    let lower = path.to_ascii_lowercase();
    if ["%2e", "%2f", "%5c", "%00"].iter().any(|encoded| lower.contains(encoded)) {
        return Some("encoded");
    }
    let segments: Vec<&str> = path.split('/').collect();
    if segments.len() > PREVIEW_PATH_MAX_SEGMENTS_V1 {
        return Some("tooDeep");
    }
    for segment in &segments {
        if segment.is_empty() || *segment == "." || *segment == ".." {
            return Some("traversal");
        }
        // `.git`, `.env`, `.ssh`, `.secret/…`: every dot segment.
        if segment.starts_with('.') {
            return Some("hidden");
        }
    }
    if preview_extension_v1(path).is_none() {
        return Some("extension");
    }
    None
}

/// The allow-listed, lowercase extension of a path's last segment.
pub fn preview_extension_v1(path: &str) -> Option<&'static str> {
    let name = path.rsplit('/').next()?;
    let (stem, extension) = name.rsplit_once('.')?;
    if stem.is_empty() {
        return None;
    }
    let lower = extension.to_ascii_lowercase();
    PREVIEW_EXTENSIONS_V1.iter().copied().find(|allowed| *allowed == lower)
}

pub fn preview_entry_refusal_v1(path: &str) -> Option<&'static str> {
    if let Some(reason) = preview_path_refusal_v1(path) {
        return Some(reason);
    }
    match preview_extension_v1(path) {
        Some(extension) if PREVIEW_ENTRY_EXTENSIONS_V1.contains(&extension) => None,
        _ => Some("notPage"),
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn accepts_plain_relative_assets() {
        for path in [
            "index.html",
            "css/app.css",
            "assets/Logo.PNG",
            "a b/c d.js",
            "fonts/x.woff2",
            "deep/a/b/c/d.json",
            "100%.png",
        ] {
            assert_eq!(preview_path_refusal_v1(path), None, "{path}");
        }
    }

    #[test]
    fn refuses_escapes_hidden_and_unknown_types() {
        let cases = [
            ("", "empty"),
            ("/etc/hosts.txt", "notRelative"),
            ("C:/x.txt", "notRelative"),
            ("a\\b.txt", "notRelative"),
            ("a\u{0}b.txt", "control"),
            ("a\nb.txt", "control"),
            ("../secret.txt", "traversal"),
            ("a/../../b.txt", "traversal"),
            ("a//b.txt", "traversal"),
            ("./a.txt", "traversal"),
            ("a/", "traversal"),
            ("%2e%2e/a.txt", "encoded"),
            ("a%2Fb.txt", "encoded"),
            ("a%5cb.txt", "encoded"),
            ("a%00.txt", "encoded"),
            (".env", "hidden"),
            (".git/HEAD", "hidden"),
            (".git/config", "hidden"),
            ("a/.secret/x.js", "hidden"),
            (".ssh/id_rsa", "hidden"),
            ("key.pem", "extension"),
            ("server.key", "extension"),
            ("prod.env", "extension"),
            ("app.wasm", "extension"),
            ("doc.pdf", "extension"),
            ("Makefile", "extension"),
            ("noext.", "extension"),
        ];
        for (path, reason) in cases {
            assert_eq!(preview_path_refusal_v1(path), Some(reason), "{path:?}");
        }
        assert_eq!(preview_path_refusal_v1(&format!("{}x.txt", "a/".repeat(40))), Some("tooDeep"));
        assert_eq!(preview_path_refusal_v1(&format!("{}.txt", "a".repeat(1100))), Some("tooLong"));
    }

    #[test]
    fn entries_must_be_pages() {
        assert_eq!(preview_entry_refusal_v1("index.html"), None);
        assert_eq!(preview_entry_refusal_v1("docs/Index.HTM"), None);
        assert_eq!(preview_entry_refusal_v1("app.js"), Some("notPage"));
        assert_eq!(preview_entry_refusal_v1(".hidden/index.html"), Some("hidden"));
        for candidate in PREVIEW_ENTRY_CANDIDATES_V1 {
            assert_eq!(preview_entry_refusal_v1(candidate), None);
        }
    }

    #[test]
    fn a_chunk_fits_one_control_line() {
        let encoded = (PREVIEW_ASSET_CHUNK_BYTES_V1 as usize).div_ceil(3) * 4;
        assert!(encoded + 64 * 1024 < crate::MAX_CONTROL_LINE_BYTES);
    }
}
