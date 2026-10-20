//! Notes: thoughts, to-dos and things to remember, a Markdown file each in a folder of the user's (see
//! docs/station-and-notes.md, section 二): `Documents\Glance 便签` unless another is chosen. Listed here as the folder
//! holds them, read again as it changes (by Glance, or by another program, or by a sync); written by the page they
//! are edited on (see `ui::beside`), as `store` says.

pub mod markdown;
pub mod store;

use std::path::PathBuf;
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

pub use store::{Colour, Folder, Note, Written};

/// How often the folder is looked for while neither it nor any folder above it is there (its drive gone): nothing
/// to be told of by then.
const LOOKED_FOR_EVERY: Duration = Duration::from_secs(5);

static SHOWN: Mutex<Option<Arc<Vec<Note>>>> = Mutex::new(None);
static TOLD: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
/// What the thread that follows the folder waits on besides the folder: set, it reads it again (see `read_again`).
static AGAIN: OnceLock<Event> = OnceLock::new();

/// An event the system keeps (set by one thread, waited on by another).
struct Event(windows::Win32::Foundation::HANDLE);
// (A handle names it: set and waited on from any thread.)
unsafe impl Send for Event {}
unsafe impl Sync for Event {}

fn again() -> &'static Event {
    AGAIN.get_or_init(|| Event(unsafe { windows::Win32::System::Threading::CreateEventW(None, false, false, None) }.expect("an event")))
}

/// The notes, the last written first (none till first read).
pub fn shown() -> Arc<Vec<Note>> {
    SHOWN.lock().unwrap().clone().unwrap_or_default()
}

/// For pictures: as if the folder held these.
#[cfg(all(test, feature = "studio"))]
pub fn pretend(notes: Vec<Note>) {
    *SHOWN.lock().unwrap() = Some(Arc::new(notes));
}

/// The notes' folder as the settings have it (none chosen yet: the one in the language shown, kept as chosen once the
/// notes are first read, so it stays as the language changes), and what a conflict's copy is called in it.
pub fn folder() -> Folder {
    let chosen = crate::app().settings.lock().unwrap().notes_folder.clone();
    folder_at(chosen.unwrap_or_else(|| default_folder(crate::update::language())))
}

/// The notes' folder at `path`, a conflict's copy called as the language shown says.
pub fn folder_at(path: PathBuf) -> Folder {
    Folder { path, apart: crate::update::language().pick(APART[0], APART[1]) }
}

/// What a conflict's copy of a note is called after its name, in each language (see `store::Written::Apart`).
const APART: [&str; 2] = ["（冲突副本）", " (conflicted copy)"];

/// Whether the note `name` is a conflict's copy (named so in either language, numbered or not).
pub fn is_apart(name: &str) -> bool {
    let stem = std::path::Path::new(name).file_stem().map_or_else(String::new, |stem| stem.to_string_lossy().into_owned());
    // (A second copy of the same note is numbered after its name: "（冲突副本） 2".)
    let stem = stem.trim_end_matches(|c: char| c.is_ascii_digit()).trim_end();
    APART.iter().any(|apart| stem.ends_with(apart.trim_start()))
}

/// `Documents\Glance 便签` (`Glance Notes` in English).
#[cfg(windows)]
fn default_folder(lang: crate::ui::text::Lang) -> PathBuf {
    use windows::Win32::UI::Shell::{FOLDERID_Documents, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    let documents = unsafe {
        SHGetKnownFolderPath(&FOLDERID_Documents, KF_FLAG_DEFAULT, None).ok().and_then(|found| {
            let path = found.to_string().ok().map(PathBuf::from);
            windows::Win32::System::Com::CoTaskMemFree(Some(found.0 as *const _));
            path
        })
    };
    documents.unwrap_or_default().join(lang.pick("Glance 便签", "Glance Notes"))
}

/// How the notes show, as the settings keep it (read as the panel is drawn: kept beside the notes, not locked for).
pub fn view() -> crate::settings::NotesView {
    *VIEW.lock().unwrap()
}

/// How the notes show, taken from the settings as they change; the folder they are in followed as it changes.
pub fn follow_settings(settings: &crate::settings::Settings) {
    *VIEW.lock().unwrap() = settings.notes_view;
    let mut chosen = CHOSEN.lock().unwrap();
    if *chosen != settings.notes_folder {
        chosen.clone_from(&settings.notes_folder);
        read_again();
    }
}

/// The notes' folder as the settings last had it (see `follow_settings`).
static CHOSEN: Mutex<Option<PathBuf>> = Mutex::new(None);

static VIEW: Mutex<crate::settings::NotesView> = Mutex::new(crate::settings::NotesView::Deck);

/// The folder read again at once (written here, or another chosen: what shows of it follows at once).
pub fn read_again() {
    unsafe {
        let _ = windows::Win32::System::Threading::SetEvent(again().0);
    }
}

/// Starts following the folder on a thread of its own; `told` is called whenever the notes changed.
pub fn start(told: impl Fn() + Send + Sync + 'static) {
    if TOLD.set(Box::new(told)).is_err() {
        return;
    }
    std::thread::Builder::new().name("glance: notes".into()).spawn(follow).expect("a thread started");
}

fn follow() {
    // The folder in the language shown kept as chosen, where none is yet.
    if crate::app().settings.lock().unwrap().notes_folder.is_none() {
        let path = default_folder(crate::update::language());
        crate::app().change(move |settings| {
            if settings.notes_folder.is_none() {
                settings.notes_folder = Some(path);
            }
        });
    }
    // What the folder held as last read: each note's name and file.
    let mut held: Vec<(String, store::Stamp, Colour)> = Vec::new();
    // The folder followed (or, while it is not there, the nearest above it that is, for it to come).
    let mut watched: Option<Watch> = None;
    loop {
        let folder = folder();
        // Followed before it is read: what changes while it is read is told after.
        let nearest = folder.path.ancestors().find(|path| path.is_dir()).map(std::path::Path::to_path_buf);
        if watched.as_ref().map(|watch| &watch.path) != nearest.as_ref() {
            watched = nearest.and_then(|path| Watch::start(path, &folder.path));
        }
        let notes = folder.notes();
        let now: Vec<(String, store::Stamp, Colour)> = notes.iter().map(|note| (note.name.clone(), note.stamp, note.colour)).collect();
        if now != held || SHOWN.lock().unwrap().is_none() {
            held = now;
            *SHOWN.lock().unwrap() = Some(Arc::new(notes));
            if let Some(told) = TOLD.get() {
                told();
            }
        }
        // Read again as the folder changes (by Glance, another program, a sync), or as Glance asks.
        if let Some(watch) = &watched {
            if !watch.wait() {
                watched = None;
            }
        } else {
            unsafe {
                let _ = windows::Win32::System::Threading::WaitForSingleObject(again().0, LOOKED_FOR_EVERY.as_millis() as u32);
            }
        }
    }
}

/// What the system tells of a folder's changes: the notes' folder's (a note come, gone, renamed or written), or one
/// above it while it is not there (a folder come in it, which may be it, or on the way to it).
struct Watch {
    path: PathBuf,
    changed: windows::Win32::Foundation::HANDLE,
}

impl Watch {
    /// `path` followed, as the notes' folder `notes` (or, another, as one above it).
    fn start(path: PathBuf, notes: &std::path::Path) -> Option<Watch> {
        use windows::Win32::Storage::FileSystem::{FindFirstChangeNotificationW, FILE_NOTIFY_CHANGE_DIR_NAME, FILE_NOTIFY_CHANGE_FILE_NAME, FILE_NOTIFY_CHANGE_LAST_WRITE, FILE_NOTIFY_CHANGE_SIZE};
        let what = if path == notes { FILE_NOTIFY_CHANGE_FILE_NAME | FILE_NOTIFY_CHANGE_LAST_WRITE | FILE_NOTIFY_CHANGE_SIZE | FILE_NOTIFY_CHANGE_DIR_NAME } else { FILE_NOTIFY_CHANGE_DIR_NAME };
        let wide = windows::core::HSTRING::from(path.as_os_str());
        let changed = unsafe { FindFirstChangeNotificationW(&wide, false, what) }.ok()?;
        Some(Watch { path, changed })
    }

    /// Till the folder changes, or Glance asks for it to be read again; whether it is still followed (followed again
    /// before it is read, see `follow`: one gone, even if another is made in its place, is not).
    fn wait(&self) -> bool {
        use windows::Win32::Foundation::WAIT_OBJECT_0;
        use windows::Win32::System::Threading::{WaitForMultipleObjects, INFINITE};
        let came = unsafe { WaitForMultipleObjects(&[again().0, self.changed], false, INFINITE) };
        came.0 != WAIT_OBJECT_0.0 + 1 || unsafe { windows::Win32::Storage::FileSystem::FindNextChangeNotification(self.changed) }.is_ok()
    }
}

impl Drop for Watch {
    fn drop(&mut self) {
        let _ = unsafe { windows::Win32::Storage::FileSystem::FindCloseChangeNotification(self.changed) };
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A note written into the folder followed, and a folder come where none was (the notes' folder made, followed
    /// meanwhile through the one above it), are told of as they come, not looked for every so often (the folder was
    /// read whole every second, its thread woken twenty times a second).
    #[test]
    fn the_folder_tells_of_what_changes_in_it() {
        let root = std::env::temp_dir().join(format!("glance-notes-watch-{}", std::process::id()));
        let notes = root.join("Glance Notes");
        let _ = std::fs::remove_dir_all(&root);
        std::fs::create_dir_all(&root).unwrap();
        let told_within = |watch: &Watch, change: Box<dyn FnOnce() + Send>| {
            let changing = std::thread::spawn(move || {
                std::thread::sleep(Duration::from_millis(150));
                change();
            });
            let began = std::time::Instant::now();
            assert!(watch.wait(), "followed still");
            changing.join().unwrap();
            began.elapsed()
        };
        // Not there: the folder above it followed, for it to come.
        let above = Watch::start(root.clone(), &notes).expect("followed");
        let made = told_within(&above, Box::new({
            let notes = notes.clone();
            move || std::fs::create_dir(&notes).unwrap()
        }));
        // Come: followed itself, a note written into it told of.
        let watch = Watch::start(notes.clone(), &notes).expect("followed");
        let written = told_within(&watch, Box::new({
            let notes = notes.clone();
            move || std::fs::write(notes.join("a.md"), "a note").unwrap()
        }));
        let _ = std::fs::remove_dir_all(&root);
        assert!(made >= Duration::from_millis(100) && made < Duration::from_secs(2), "{made:?}");
        assert!(written >= Duration::from_millis(100) && written < Duration::from_secs(2), "{written:?}");
    }

    /// A conflict's copy known by its name, in either language, a second one numbered; a note merely about conflicts
    /// not.
    #[test]
    fn knows_a_conflicts_copy() {
        assert!(is_apart("周会（冲突副本）.md"));
        assert!(is_apart("周会（冲突副本） 2.md"));
        assert!(is_apart("Meeting notes (conflicted copy).md"));
        assert!(is_apart("Meeting notes (conflicted copy) 3.md"));
        assert!(!is_apart("Meeting notes.md"));
        assert!(!is_apart("conflicted copy rules.md"));
    }
}
