//! The notes' folder: a note a Markdown file each (see docs/station-and-notes.md, sections 二 and 六), readable without
//! Glance and synced as the user syncs the folder. What Glance keeps of them besides their words (each one's colour)
//! is in a file of its own beside them (`.glance-notes.json`), so the notes stay plain Markdown.
//!
//! - Read and written whole; written to a file beside it first, which then takes its place (a power cut leaves the old
//!   or the new, never half of one).
//! - Each note as it was last read or written (its length, when it was written, a digest of its bytes): one changed
//!   outside since is never written over. Its words from outside stay where they are, and what was written here goes
//!   to a copy beside it, named as a conflict's copy, so nothing is lost.
//! - Taken away to the Recycle Bin, not deleted.

use serde::{Deserialize, Serialize};
use std::collections::BTreeMap;
use std::hash::{Hash, Hasher};
use std::path::{Path, PathBuf};

/// A note's colour, as its card shows it.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Colour {
    #[default]
    Blue,
    Yellow,
    Green,
    Pink,
    Purple,
    Grey,
}

impl Colour {
    pub const ALL: [Colour; 6] = [Colour::Blue, Colour::Yellow, Colour::Green, Colour::Pink, Colour::Purple, Colour::Grey];
}

/// A note's file as it was last read or written: how long, when written (ms since 1970), and a digest of its bytes.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Default)]
pub struct Stamp {
    pub bytes: u64,
    pub written_ms: u64,
    pub digest: u64,
}

impl Stamp {
    fn of(bytes: &[u8], written_ms: u64) -> Stamp {
        let mut hasher = std::collections::hash_map::DefaultHasher::new();
        bytes.hash(&mut hasher);
        Stamp { bytes: bytes.len() as u64, written_ms, digest: hasher.finish() }
    }
}

/// A note: its file's name in the folder, its words (Markdown, as the file holds them), its colour, and its file as
/// last read or written.
#[derive(Clone, PartialEq, Debug)]
pub struct Note {
    pub name: String,
    pub text: String,
    pub colour: Colour,
    pub stamp: Stamp,
}

impl Note {
    /// Its title: its first line with words, without a heading's marks.
    pub fn title(&self) -> &str {
        self.text.lines().map(|line| line.trim().trim_start_matches('#').trim()).find(|line| !line.is_empty()).unwrap_or("")
    }
}

/// What Glance keeps of the notes besides their words: each one's colour, by its file's name.
#[derive(Clone, Default, Debug, Serialize, Deserialize)]
struct Kept {
    #[serde(default)]
    colours: BTreeMap<String, Colour>,
}

const KEPT: &str = ".glance-notes.json";

/// What became of a note written.
#[derive(Clone, PartialEq, Debug)]
pub enum Written {
    /// Written in its place (its file as it is now).
    Done(Stamp),
    /// Its file had been changed outside since it was read (or deleted): left as it was, and what was written here
    /// put in a copy beside it, by this name.
    Apart(String),
}

/// The notes' folder, and what a conflict's copy of a note is called after its name (see `Written::Apart`).
pub struct Folder {
    pub path: PathBuf,
    pub apart: &'static str,
}

impl Folder {
    /// The notes in it, the last written first.
    pub fn notes(&self) -> Vec<Note> {
        let kept = self.kept();
        let mut notes: Vec<Note> = std::fs::read_dir(&self.path)
            .into_iter()
            .flatten()
            .flatten()
            .filter(|entry| entry.path().extension().is_some_and(|extension| extension.eq_ignore_ascii_case("md")) && entry.file_type().is_ok_and(|kind| kind.is_file()))
            .filter_map(|entry| {
                let name = entry.file_name().to_string_lossy().into_owned();
                let (text, stamp) = self.read(&name)?;
                let colour = kept.colours.get(&name).copied().unwrap_or_default();
                Some(Note { name, text, colour, stamp })
            })
            .collect();
        notes.sort_by(|a, b| b.stamp.written_ms.cmp(&a.stamp.written_ms).then_with(|| a.name.cmp(&b.name)));
        notes
    }

    /// The note `name`'s words and its file as it is now; none where it is gone (or not to be read).
    pub fn read(&self, name: &str) -> Option<(String, Stamp)> {
        let path = self.path.join(name);
        let bytes = std::fs::read(&path).ok()?;
        let written = written_ms(&path)?;
        // (Words not in UTF-8, as another editor may have saved them: read as near as they go.)
        let text = String::from_utf8(bytes.clone()).unwrap_or_else(|_| String::from_utf8_lossy(&bytes).into_owned());
        Some((text.strip_prefix('\u{FEFF}').map(str::to_string).unwrap_or(text), Stamp::of(&bytes, written)))
    }

    /// The note `name`'s file as it is now, if it is there.
    pub fn stamp(&self, name: &str) -> Option<Stamp> {
        let path = self.path.join(name);
        Some(Stamp::of(&std::fs::read(&path).ok()?, written_ms(&path)?))
    }

    /// `text` written as the note `name`, its file having been `was` as last read or written here (none: a note new
    /// here, whose file must not be there yet): in its place, unless it was changed outside since (see `Written`).
    pub fn write(&self, name: &str, text: &str, was: Option<Stamp>) -> std::io::Result<Written> {
        std::fs::create_dir_all(&self.path)?;
        let now = self.stamp(name);
        let changed_outside = match (was, now) {
            (Some(was), Some(now)) => was.bytes != now.bytes || was.digest != now.digest,
            // A note new here whose name was taken meanwhile; or one deleted outside since (not made again where it
            // was, as if it had never been deleted: what was written here kept apart).
            (None, Some(_)) | (Some(_), None) => true,
            (None, None) => false,
        };
        if changed_outside {
            let stem = Path::new(name).file_stem().map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
            let apart = self.free_name(&format!("{stem}{}.md", self.apart));
            self.replace(&apart, text)?;
            return Ok(Written::Apart(apart));
        }
        self.replace(name, text)?;
        Ok(Written::Done(self.stamp(name).unwrap_or_default()))
    }

    /// `text` in the file `name`: written beside it first, then put in its place.
    fn replace(&self, name: &str, text: &str) -> std::io::Result<()> {
        let path = self.path.join(name);
        let beside = self.path.join(format!(".{name}.glance-writing"));
        std::fs::write(&beside, text.as_bytes())?;
        std::fs::rename(&beside, &path).inspect_err(|_| {
            let _ = std::fs::remove_file(&beside);
        })
    }

    /// A name for a new note, from its title (`title`, the words a name may not hold left out), or else from today's
    /// date (`fallback`), not taken by another file.
    pub fn name_for(&self, title: &str, fallback: &str) -> String {
        let cleaned: String = title.chars().filter(|c| !r#"\/:*?"<>|"#.contains(*c) && !c.is_control()).collect::<String>().trim().trim_end_matches('.').chars().take(60).collect();
        let stem = if cleaned.is_empty() { fallback.to_string() } else { cleaned };
        self.free_name(&format!("{stem}.md"))
    }

    /// `name`, or with a number after its stem where a file has it already.
    fn free_name(&self, name: &str) -> String {
        let path = Path::new(name);
        let stem = path.file_stem().map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
        let extension = path.extension().map_or_else(String::new, |extension| format!(".{}", extension.to_string_lossy()));
        let mut candidate = name.to_string();
        let mut more = 2;
        while self.path.join(&candidate).exists() {
            candidate = format!("{stem} {more}{extension}");
            more += 1;
        }
        candidate
    }

    /// The note `name`'s colour kept as `colour`.
    pub fn colour(&self, name: &str, colour: Colour) -> std::io::Result<()> {
        let mut kept = self.kept();
        kept.colours.insert(name.to_string(), colour);
        self.keep(&kept)
    }

    /// The note `name` taken to the Recycle Bin (its colour forgotten); whether it was.
    pub fn take_away(&self, name: &str) -> bool {
        let taken = recycle(&self.path.join(name));
        if taken {
            let mut kept = self.kept();
            if kept.colours.remove(name).is_some() {
                let _ = self.keep(&kept);
            }
        }
        taken
    }

    fn kept(&self) -> Kept {
        std::fs::read(self.path.join(KEPT)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default()
    }

    fn keep(&self, kept: &Kept) -> std::io::Result<()> {
        std::fs::create_dir_all(&self.path)?;
        self.replace(KEPT, &serde_json::to_string_pretty(kept).unwrap_or_default())
    }
}

/// When the file at `path` was last written (ms since 1970).
fn written_ms(path: &Path) -> Option<u64> {
    let modified = std::fs::metadata(path).ok()?.modified().ok()?;
    Some(modified.duration_since(std::time::UNIX_EPOCH).ok()?.as_millis() as u64)
}

/// The file at `path` moved to the Recycle Bin; whether it was.
#[cfg(windows)]
fn recycle(path: &Path) -> bool {
    use windows::Win32::UI::Shell::{SHFileOperationW, FOF_ALLOWUNDO, FOF_NOCONFIRMATION, FOF_NOERRORUI, FOF_SILENT, FO_DELETE, SHFILEOPSTRUCTW};
    // (Its path ended by two nulls, as the call reads a list of them.)
    use std::os::windows::ffi::OsStrExt;
    let mut from: Vec<u16> = path.as_os_str().encode_wide().collect();
    from.extend([0, 0]);
    let mut operation = SHFILEOPSTRUCTW { wFunc: FO_DELETE, pFrom: windows::core::PCWSTR(from.as_ptr()), fFlags: (FOF_ALLOWUNDO | FOF_NOCONFIRMATION | FOF_NOERRORUI | FOF_SILENT).0 as u16, ..Default::default() };
    unsafe { SHFileOperationW(&mut operation) == 0 && !operation.fAnyOperationsAborted.as_bool() }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn folder(test: &str) -> Folder {
        let path = std::env::temp_dir().join(format!("glance-notes-{}-{test}", std::process::id()));
        let _ = std::fs::remove_dir_all(&path);
        Folder { path, apart: "（冲突副本）" }
    }

    /// A note written, read back as written, its title its first line with words; a new note named by its title, a
    /// second one of that title numbered.
    #[test]
    fn writes_and_reads_notes() {
        let folder = folder("write");
        let name = folder.name_for("本周: 要做的事?", "便签");
        assert_eq!(name, "本周 要做的事.md");
        assert_eq!(folder.write(&name, "# 本周: 要做的事?\n\n- [ ] 发布\n", None).unwrap(), Written::Done(folder.stamp(&name).unwrap()));
        assert_eq!(folder.name_for("本周: 要做的事?", "便签"), "本周 要做的事 2.md");
        let notes = folder.notes();
        assert_eq!((notes.len(), notes[0].title(), notes[0].colour), (1, "本周: 要做的事?", Colour::Blue));
        folder.colour(&name, Colour::Green).unwrap();
        assert_eq!(folder.notes()[0].colour, Colour::Green);
        // (No file of its own writing left beside it.)
        assert_eq!(std::fs::read_dir(&folder.path).unwrap().count(), 2);
        let _ = std::fs::remove_dir_all(&folder.path);
    }

    /// A note changed outside since it was read is not written over: its words from outside stay, and what was
    /// written here goes to a conflict's copy beside it.
    #[test]
    fn never_writes_over_what_changed_outside() {
        let folder = folder("conflict");
        let Written::Done(stamp) = folder.write("a.md", "here", None).unwrap() else { panic!("written") };
        std::fs::write(folder.path.join("a.md"), "outside, longer").unwrap();
        let Written::Apart(apart) = folder.write("a.md", "here again", Some(stamp)).unwrap() else { panic!("put apart") };
        assert_eq!(folder.read("a.md").unwrap().0, "outside, longer");
        assert_eq!(folder.read(&apart).unwrap().0, "here again");
        // A note new here whose name was taken meanwhile: put apart too.
        assert!(matches!(folder.write("a.md", "new", None).unwrap(), Written::Apart(_)));
        // One deleted outside since it was read: not made again in its place, what was written kept apart.
        let Written::Done(stamp) = folder.write("b.md", "b", None).unwrap() else { panic!("written") };
        std::fs::remove_file(folder.path.join("b.md")).unwrap();
        assert!(matches!(folder.write("b.md", "b again", Some(stamp)).unwrap(), Written::Apart(_)));
        assert!(!folder.path.join("b.md").exists());
        let _ = std::fs::remove_dir_all(&folder.path);
    }
}
