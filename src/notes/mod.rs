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

/// How often the folder is looked at for what changed in it.
const EVERY: Duration = Duration::from_secs(1);

static SHOWN: Mutex<Option<Arc<Vec<Note>>>> = Mutex::new(None);
static TOLD: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();
static AGAIN: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

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
    Folder { path, apart: crate::update::language().pick("（冲突副本）", " (conflicted copy)") }
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

/// How the notes show, taken from the settings as they change.
pub fn follow_settings(settings: &crate::settings::Settings) {
    *VIEW.lock().unwrap() = settings.notes_view;
}

static VIEW: Mutex<crate::settings::NotesView> = Mutex::new(crate::settings::NotesView::Deck);

/// The folder looked at again soon (written here: what shows of it follows at once).
pub fn read_again() {
    AGAIN.store(true, std::sync::atomic::Ordering::Release);
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
    let mut looked = std::time::Instant::now();
    loop {
        let notes = folder().notes();
        let now: Vec<(String, store::Stamp, Colour)> = notes.iter().map(|note| (note.name.clone(), note.stamp, note.colour)).collect();
        if now != held || SHOWN.lock().unwrap().is_none() {
            held = now;
            *SHOWN.lock().unwrap() = Some(Arc::new(notes));
            if let Some(told) = TOLD.get() {
                told();
            }
        }
        // (Asked to read again: at once; else after a while.)
        while looked.elapsed() < EVERY && !AGAIN.swap(false, std::sync::atomic::Ordering::AcqRel) {
            std::thread::sleep(Duration::from_millis(50));
        }
        looked = std::time::Instant::now();
    }
}
