//! Comparing folder paths by their parts, the same way on every platform
//! (ADE multi-project PR #18 review B1). The app decides whether two tasks
//! can write the same files from this; it lives here so its Windows rules are
//! tested by every CI run, macOS and Windows alike.

/// A path as components to compare (PR #18 review B1, 3rd round). Pure, so
/// the Windows rules are tested on every platform. Separators are unified,
/// empty and `.` parts dropped, and case folded where the disk ignores it
/// (macOS and Windows volumes usually do: refusing more, never less). On
/// Windows, `\\?\` and `\\?\UNC\` prefixes are removed, so a drive letter or
/// a UNC server and share lead the components the same way in every spelling.
pub fn path_key_components(text: &str, windows: bool, fold_case: bool) -> Vec<String> {
    let mut text = text.to_owned();
    if windows {
        text = text.replace('/', "\\");
        if let Some(rest) = text.strip_prefix(r"\\?\UNC\") { text = format!(r"\\{rest}"); }
        else if let Some(rest) = text.strip_prefix(r"\\?\") { text = rest.to_owned(); }
        else if let Some(rest) = text.strip_prefix(r"\\.\") { text = rest.to_owned(); }
    }
    let separator = if windows { '\\' } else { '/' };
    let mut parts = vec![if windows && text.starts_with(r"\\") { "unc".to_owned() } else if text.starts_with(separator) { "root".to_owned() } else { "relative".to_owned() }];
    for part in text.split(separator) {
        if part.is_empty() || part == "." { continue; }
        parts.push(if fold_case { part.to_lowercase() } else { part.to_owned() });
    }
    parts
}


/// `inner` is `outer` or inside it (both from `path_key_components`).
pub fn components_within(inner: &[String], outer: &[String]) -> bool {
    inner.len() >= outer.len() && inner[..outer.len()] == outer[..]
}

#[cfg(test)]
mod tests {
    use super::*;

    fn within(inner: &str, outer: &str, windows: bool) -> bool {
        let fold = true;
        components_within(&path_key_components(inner, windows, fold), &path_key_components(outer, windows, fold))
    }

    /// Windows paths by components (PR #18 review B1, 3rd round): any case,
    /// either separator, `\\?\` and UNC spellings, drive letters.
    #[test]
    fn windows_paths_compare_by_components_case_prefix_and_separator() {
        assert!(within(r"C:\Users\A\work\task-1", r"c:/users/a/WORK", true));
        assert!(within(r"\\?\C:\Users\A\work", r"C:\Users\A", true));
        assert!(within(r"C:\Users\A\work", r"\\?\c:\users\a\", true));
        assert!(within(r"\\?\UNC\srv\share\x\y", r"\\srv\share\x", true));
        assert!(within(r"\\srv\share\x", r"\\?\UNC\SRV\Share", true));
        assert!(within(r"C:\work\.\a", r"C:\work\a", true));
        assert!(!within(r"C:\work\ab", r"C:\work\a", true), "whole components only");
        assert!(!within(r"D:\work", r"C:\work", true), "another drive");
        assert!(!within(r"\\srv\share2\x", r"\\srv\share", true));
        assert!(!within(r"C:\work", r"\\srv\share\work", true), "a drive is not a share");
        assert!(!within(r"C:\work", r"C:\work\a", true), "the outer is not inside the inner");
    }

    #[test]
    fn unix_paths_compare_by_components() {
        assert!(within("/Users/Someone/Work/a", "/users/someone/work", false));
        assert!(within("/work/a/", "/work/a", false));
        assert!(!within("/work/ab", "/work/a", false));
        assert!(!within("work/a", "/work", false), "relative is not under a root");
    }
}
