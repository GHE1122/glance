//! What Glance asks of the system it runs on, each system answered its own
//! way: the user's language, whether their apps are dark, their accent.

#[cfg(windows)]
mod windows;
#[cfg(windows)]
pub use self::windows::*;

#[cfg(not(windows))]
mod unix;
#[cfg(not(windows))]
pub use self::unix::*;

/// `text` as a file's name may hold it on Windows: what a name may not hold left out (and spaces and dots where it
/// ends), at most `most` characters; empty where nothing is left, or what is left names a device (CON, NUL, COM1, a
/// name such as "con.txt" too).
pub fn file_name_of(text: &str, most: usize) -> String {
    let kept: String = text.chars().filter(|c| !r#"\/:*?"<>|"#.contains(*c) && !c.is_control()).take(most).collect();
    let kept = kept.trim().trim_end_matches(['.', ' ']);
    let device = kept.split('.').next().unwrap_or_default().trim_end().to_uppercase();
    let numbered = |name: &str| device.strip_prefix(name).is_some_and(|number| matches!(number, "0" | "1" | "2" | "3" | "4" | "5" | "6" | "7" | "8" | "9" | "¹" | "²" | "³"));
    if matches!(device.as_str(), "CON" | "PRN" | "AUX" | "NUL" | "CONIN$" | "CONOUT$") || numbered("COM") || numbered("LPT") {
        return String::new();
    }
    kept.to_string()
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_name_holds_what_windows_lets_it() {
        assert_eq!(super::file_name_of("  a/b:c?.txt. . ", 60), "abc?.txt".replace('?', ""));
        assert_eq!(super::file_name_of("报告\u{7}「终稿」", 60), "报告「终稿」");
        assert_eq!(super::file_name_of("abcdef", 3), "abc");
        for device in ["CON", "nul.txt", "Com1.tar.gz", "lpt²", "aux .md", "CONIN$"] {
            assert_eq!(super::file_name_of(device, 60), "", "{device}");
        }
        for name in ["console.txt", "COM10", "con-notes.md", "LPT"] {
            assert_eq!(super::file_name_of(name, 60), name);
        }
        assert_eq!(super::file_name_of("...", 60), "");
    }
}
