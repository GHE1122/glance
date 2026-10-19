//! Recent files: the files lately come to the user's folders, wherever they landed (downloaded, a screenshot, a
//! document saved), newest first, shown as a page of the station (see docs/station-and-notes.md, section 八).
//!
//! Read as the user, never through the elevated sensors: from each fixed drive's change journal, read the way NTFS
//! lets anyone read it (`FSCTL_READ_UNPRIVILEGED_USN_JOURNAL`, through the drive's root folder). Its records name no
//! files there, so each file come is opened by its number (`OpenFileById`) and its path asked of it: a file the user
//! may not open is not opened, and so never listed. The journal keeps what changed while Glance was not running: where
//! it was last read up to is kept, and read on from there, a batch at a time (the list shown and kept between them).
//! A file's number is its drive's own: a file is known by its drive and its number together.
//!
//! Only what a person keeps: the folders of Windows, programs, programs' own data and tools' (`AppData`, `.git`,
//! `node_modules`, …) are left out, and so are a repository's working tree (a checkout writes hundreds of files at
//! once, its tools' to follow) and a program's folder wherever it was put (one holding a program or a library: what is
//! written there, its logs and pictures, are the program's), as are hidden, system and temporary files and those being written (a download in progress);
//! outside Downloads and the desktop, only documents, pictures, videos, music and archives. Glance's own notes are
//! left out too. A file moved or
//! renamed is not one come (its creation is old); one renamed as a download finishes is.
//!
//! A file listed is followed: renamed, it is listed under its new name; moved with its folder (which the journal tells
//! of the folder alone), found again by its number; deleted, taken off. One that cannot be looked for now (its drive
//! away, or locked) stays listed.

use serde::{Deserialize, Serialize};
use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::{Duration, Instant};

/// What a file come is, by its name: what the page's choices pick out (see `Filter`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "snake_case")]
pub enum Kind {
    Screenshot,
    Document,
    Picture,
    Video,
    Audio,
    Archive,
    Program,
    Other,
}

/// Which file: its drive's letter, and its number on that drive.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
pub struct Key {
    pub drive: char,
    pub file: u128,
}

/// A file come lately: which it is, where it is, when it was made (ms since 1970), how large it is, what it is, whether
/// it came into Downloads, and the site it was downloaded from where its download says so.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Came {
    pub key: Key,
    pub path: PathBuf,
    pub came_ms: u64,
    pub bytes: u64,
    pub kind: Kind,
    pub downloaded: bool,
    #[serde(default, skip_serializing_if = "Option::is_none")]
    pub site: Option<String>,
}

/// Which of them a page shows: all; those downloaded; screenshots; documents; pictures and videos.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Filter {
    #[default]
    All,
    Downloads,
    Screenshots,
    Documents,
    Pictures,
}

impl Filter {
    pub const ALL: [Filter; 5] = [Filter::All, Filter::Downloads, Filter::Screenshots, Filter::Documents, Filter::Pictures];

    /// Whether `came` is one it shows.
    pub fn takes(self, came: &Came) -> bool {
        match self {
            Filter::All => true,
            Filter::Downloads => came.downloaded || came.site.is_some(),
            Filter::Screenshots => came.kind == Kind::Screenshot,
            Filter::Documents => came.kind == Kind::Document,
            Filter::Pictures => matches!(came.kind, Kind::Picture | Kind::Screenshot | Kind::Video),
        }
    }
}

/// How long a file stays listed after it was made, and the most listed.
const KEEP_MS: u64 = 7 * 24 * 3_600_000;
const MOST: usize = 300;
/// How often the journals are read (with nothing left to read on).
const EVERY: Duration = Duration::from_secs(2);
/// How often where the journals were read up to is kept, with nothing else changed.
const SAVE_EVERY: Duration = Duration::from_secs(60);
/// How often each file listed is looked for where it was (and at once when a folder was renamed).
const RECHECK_EVERY: Duration = Duration::from_secs(60);
/// How often which folders are tools' is forgotten (one made a repository, or a program's, is seen).
const FORGET_EVERY: Duration = Duration::from_secs(600);
/// The most of a journal read back the first time (its records' bytes): enough for the days kept on a busy drive.
const FIRST_READ_BACK: i64 = 512 << 20;
/// The most of a journal's records read from a drive at a time (the list shown and kept between such batches).
const BATCH: usize = 4 << 20;
/// A journal's page: its records never cross one, so each starts with a record (a read must start at one).
const JOURNAL_PAGE: i64 = 4096;

/// The folders that tell what a file is by where it is: Downloads, the desktop, screenshots'.
#[derive(Clone, Debug, Default)]
pub struct Places {
    pub downloads: Option<PathBuf>,
    pub desktop: Option<PathBuf>,
    pub screenshots: Option<PathBuf>,
}

/// Folders left out wherever they are: tools' and programs' own.
const LEFT_OUT: [&str; 20] = ["appdata", "node_modules", "__pycache__", "venv", "site-packages", "$recycle.bin", "system volume information", "windowsapps", "temp", "tmp", "cache", "caches", "obj", "target", "build", "dist", "thumb", "thumbs", "thumbnail", "thumbnails"];
/// Folders left out at a drive's top: Windows', programs'.
const LEFT_OUT_AT_TOP: [&str; 7] = ["windows", "program files", "program files (x86)", "programdata", "recovery", "$windows.~bt", "msocache"];

/// Whether `path` is in a folder left out (see the module's note), or one of `hidden` (folders the user asked not to
/// be listed). Its name is not looked at: only the folders it is in.
pub fn left_out(path: &Path, hidden: &[PathBuf]) -> bool {
    if hidden.iter().any(|folder| path.starts_with(folder)) {
        return true;
    }
    let folders: Vec<String> = path.parent().into_iter().flat_map(|parent| parent.components()).filter_map(|part| match part {
        std::path::Component::Normal(name) => Some(name.to_string_lossy().to_lowercase()),
        _ => None,
    }).collect();
    folders.first().is_some_and(|top| LEFT_OUT_AT_TOP.contains(&top.as_str())) || folders.iter().any(|folder| folder.starts_with('.') || LEFT_OUT.contains(&folder.as_str()))
}

/// What the file at `path` is (see `Kind`), and whether it came into Downloads; none, for a file not listed: one being
/// written or left by a program (a download in progress, Office's lock), or, outside Downloads and the desktop, one that
/// is none of the kinds a person keeps.
pub fn kind_of(path: &Path, places: &Places) -> Option<(Kind, bool)> {
    let name = path.file_name()?.to_string_lossy().to_lowercase();
    if name.starts_with("~$") || name.starts_with('.') {
        return None;
    }
    let extension = path.extension().map(|e| e.to_string_lossy().to_lowercase()).unwrap_or_default();
    if ["tmp", "temp", "crdownload", "part", "partial", "download", "opdownload", "!ut", "lock", "lnk", "ini", "log"].contains(&extension.as_str()) {
        return None;
    }
    let inside = |folder: &Option<PathBuf>| folder.as_ref().is_some_and(|folder| path.starts_with(folder));
    let downloaded = inside(&places.downloads);
    let anything = downloaded || inside(&places.desktop);
    let kind = match extension.as_str() {
        "pdf" | "doc" | "docx" | "xls" | "xlsx" | "ppt" | "pptx" | "odt" | "ods" | "odp" | "rtf" | "txt" | "md" | "csv" | "epub" | "pages" | "key" | "numbers" | "wps" | "et" | "dps" => Kind::Document,
        "png" | "jpg" | "jpeg" | "gif" | "webp" | "heic" | "heif" | "bmp" | "tif" | "tiff" | "svg" | "psd" | "ai" | "avif" | "dng" | "cr2" | "cr3" | "nef" | "arw" => {
            let shot = inside(&places.screenshots) || ["屏幕截图", "screenshot", "截图", "snipaste"].iter().any(|start| name.starts_with(start));
            if shot { Kind::Screenshot } else { Kind::Picture }
        }
        "mp4" | "mkv" | "mov" | "avi" | "webm" | "wmv" | "flv" | "m4v" => Kind::Video,
        "mp3" | "flac" | "wav" | "m4a" | "aac" | "ogg" | "opus" | "wma" => Kind::Audio,
        "zip" | "rar" | "7z" | "tar" | "gz" | "tgz" | "bz2" | "xz" | "zst" | "iso" => Kind::Archive,
        "exe" | "msi" | "msix" | "msixbundle" | "appx" if anything => Kind::Program,
        _ if anything => Kind::Other,
        _ => return None,
    };
    Some((kind, downloaded))
}

/// The site a download came from, as its `Zone.Identifier` says (`text`): the page it was taken from (`ReferrerUrl`),
/// else where the file itself was (`HostUrl`); none where it names no web address (it may say only its zone).
pub fn site_of(text: &str) -> Option<String> {
    let value = |key: &str| text.lines().find_map(|line| line.trim().strip_prefix(key).map(str::trim).map(str::to_string));
    let host = |url: String| -> Option<String> {
        let rest = url.strip_prefix("https://").or_else(|| url.strip_prefix("http://"))?;
        let host = rest.split(['/', '?', '#', ':']).next()?.trim_start_matches("www.");
        (!host.is_empty()).then(|| host.to_string())
    };
    value("ReferrerUrl=").and_then(host).or_else(|| value("HostUrl=").and_then(host))
}

/// One change a journal's record tells: the file's number and its folder's, why (its reasons, as `USN_REASON_*`), its
/// attributes, and when (ms since 1970).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Change {
    pub file: u128,
    pub folder: u128,
    pub reason: u32,
    pub attributes: u32,
    pub at_ms: u64,
}

/// The changes in a read of a journal (`read`: what `FSCTL_READ_UNPRIVILEGED_USN_JOURNAL` gave), and where to read on
/// from. Records of versions 2 (NTFS) and 3 (ReFS, with longer numbers) are read; others are passed over.
pub fn changes_in(read: &[u8]) -> Option<(i64, Vec<Change>)> {
    let next = i64::from_le_bytes(read.get(..8)?.try_into().ok()?);
    let u16_at = |at: usize| read.get(at..at + 2).map(|b| u16::from_le_bytes([b[0], b[1]]));
    let u32_at = |at: usize| read.get(at..at + 4).map(|b| u32::from_le_bytes(b.try_into().unwrap()));
    let u64_at = |at: usize| read.get(at..at + 8).map(|b| u64::from_le_bytes(b.try_into().unwrap()));
    let u128_at = |at: usize| Some(u128::from(u64_at(at)?) | (u128::from(u64_at(at + 8)?) << 64));
    let mut changes = Vec::new();
    let mut at = 8;
    while at + 8 <= read.len() {
        let Some(length) = u32_at(at).filter(|length| *length >= 8) else { break };
        let change = match u16_at(at + 4) {
            Some(2) => (|| Some(Change { file: u128::from(u64_at(at + 8)?), folder: u128::from(u64_at(at + 16)?), at_ms: ms_of(u64_at(at + 32)?), reason: u32_at(at + 40)?, attributes: u32_at(at + 52)? }))(),
            Some(3) => (|| Some(Change { file: u128_at(at + 8)?, folder: u128_at(at + 24)?, at_ms: ms_of(u64_at(at + 48)?), reason: u32_at(at + 56)?, attributes: u32_at(at + 68)? }))(),
            _ => None,
        };
        changes.extend(change);
        at += length as usize;
    }
    Some((next, changes))
}

/// Each file's changes in `changes` as one: its reasons together (a file made, then written again, was made), as it
/// was last (its folder, its attributes, when), in the order each was first changed.
fn together(changes: Vec<Change>) -> Vec<Change> {
    let mut at: HashMap<u128, usize> = HashMap::new();
    let mut together: Vec<Change> = Vec::new();
    for change in changes {
        match at.get(&change.file) {
            Some(&was) => together[was] = Change { reason: together[was].reason | change.reason, ..change },
            None => {
                at.insert(change.file, together.len());
                together.push(change);
            }
        }
    }
    together
}

/// A file time (100 ns since 1601) in ms since 1970.
fn ms_of(file_time: u64) -> u64 {
    (file_time / 10_000).saturating_sub(11_644_473_600_000)
}

/// What the change journals' reasons, attributes and the files' state say of a change.
mod reasons {
    pub const CREATED: u32 = 0x0000_0100;
    pub const DELETED: u32 = 0x0000_0200;
    pub const RENAMED_TO: u32 = 0x0000_2000;
    pub const CLOSED: u32 = 0x8000_0000;
    pub const HIDDEN: u32 = 0x2;
    pub const SYSTEM: u32 = 0x4;
    pub const FOLDER: u32 = 0x10;
    pub const TEMPORARY: u32 = 0x100;
}

/// A file looked for by its number: where it is (its path, when it was made, how large it is), gone, or not to be told
/// now (its drive away, it not the user's to open, or locked).
#[derive(Clone, PartialEq, Debug)]
pub enum Looked {
    At(PathBuf, u64, u64),
    Gone,
    Unknown,
}

/// Whether a file is at a path: there, not, or not to be told now (its drive away, or not ready).
#[derive(Clone, Copy, PartialEq, Debug)]
enum There {
    Yes,
    No,
    Unknown,
}

fn there(path: &Path) -> There {
    match std::fs::symlink_metadata(path) {
        Ok(_) => There::Yes,
        Err(error) if error.kind() == std::io::ErrorKind::NotFound => There::No,
        Err(_) => There::Unknown,
    }
}

/// The list, as kept between runs: where each drive's journal was read up to (by its letter, the journal's number,
/// and the place in it), and the files come.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Kept {
    read: Vec<(char, u64, i64)>,
    files: Vec<Came>,
}

impl Kept {
    /// `change` on drive `drive` seen: a file taken off the list where it is gone; one come, or come under another
    /// name, opened by its number (`open`) and listed if it is one listed (made within the days kept before `now`, as
    /// `kind_of` says, and not `out`); whether the list changed. (A change older than the days kept is of a file made
    /// before them: never opened.)
    fn take(&mut self, drive: char, change: Change, now: u64, places: &Places, out: &dyn Fn(&Path) -> bool, open: impl Fn(u128) -> Looked, site: impl Fn(&Path) -> Option<String>) -> bool {
        use reasons::*;
        let key = Key { drive, file: change.file };
        if change.reason & CLOSED == 0 {
            return false;
        }
        if change.reason & DELETED != 0 {
            let before = self.files.len();
            self.files.retain(|came| came.key != key);
            return self.files.len() != before;
        }
        if change.reason & (CREATED | RENAMED_TO) == 0 || change.attributes & (HIDDEN | SYSTEM | FOLDER | TEMPORARY) != 0 || now.saturating_sub(change.at_ms) > KEEP_MS {
            return false;
        }
        let Looked::At(path, made_ms, bytes) = open(change.file) else { return false };
        let had = self.files.iter().position(|came| came.key == key);
        let listed = (now.saturating_sub(made_ms) <= KEEP_MS && !out(&path)).then(|| kind_of(&path, places)).flatten();
        match (listed, had) {
            (Some((kind, downloaded)), Some(at)) => {
                let came = &mut self.files[at];
                let site = if downloaded { site(&path) } else { None };
                let changed = came.path != path || came.kind != kind || came.bytes != bytes;
                *came = Came { path, kind, downloaded, bytes, site: site.or(came.site.take()), ..came.clone() };
                changed
            }
            (Some((kind, downloaded)), None) => {
                let site = site(&path);
                self.files.push(Came { key, path, came_ms: made_ms, bytes, kind, downloaded, site });
                self.files.sort_by_key(|came| std::cmp::Reverse(came.came_ms));
                self.files.truncate(MOST);
                true
            }
            // (Renamed into a folder left out, or to a name not listed: no longer listed.)
            (None, Some(at)) => {
                self.files.remove(at);
                true
            }
            (None, None) => false,
        }
    }

    /// Those made longer ago than the days kept before `now` taken off; whether any were.
    fn age(&mut self, now: u64) -> bool {
        let before = self.files.len();
        self.files.retain(|came| now.saturating_sub(came.came_ms) <= KEEP_MS);
        self.files.len() != before
    }

    /// Each file listed looked for where it was (`there`): one no longer there looked for by its number (`look`) and
    /// followed where it went (moved with its folder), or taken off where it is gone; one now `out` taken off; one that
    /// cannot be looked for now kept as it is. Whether the list changed.
    fn recheck(&mut self, there: impl Fn(&Path) -> There, look: impl Fn(Key) -> Looked, out: &dyn Fn(&Path) -> bool) -> bool {
        let mut changed = false;
        self.files.retain_mut(|came| {
            let keep = match there(&came.path) {
                There::Yes => !out(&came.path),
                There::Unknown => true,
                There::No => match look(came.key) {
                    Looked::At(path, ..) if !out(&path) => {
                        came.path = path;
                        changed = true;
                        true
                    }
                    Looked::At(..) | Looked::Gone => false,
                    Looked::Unknown => true,
                },
            };
            changed |= !keep;
            keep
        });
        changed
    }
}

/// Which folders are tools': in a repository's working tree (it, or a folder above, holds `.git`), or in a program's
/// folder (it, or a folder above, holds a program or a library), as each was looked at (forgotten now and then: one
/// made or removed is seen). The folders above are asked first: one held holds all below it, which are not looked at.
/// Downloads, the desktop, the user's own folder and a drive's top are never a program's, whatever they hold, and are
/// not looked through for one. `looks` says what a folder holds itself: whether a repository, or (asked to look for
/// one) a program.
struct Tools<F: Fn(&Path, bool) -> bool> {
    known: std::cell::RefCell<HashMap<PathBuf, bool>>,
    never: Vec<PathBuf>,
    looks: F,
}

/// Whether the folder at `folder` is a tool's itself, as `Tools` asks: a repository (`.git`), or, where `programs`, a
/// folder holding a program or a library.
fn looked_at(folder: &Path, programs: bool) -> bool {
    folder.join(".git").exists()
        || programs
            && std::fs::read_dir(folder).is_ok_and(|entries| entries.flatten().any(|entry| {
                let name = entry.file_name().to_string_lossy().to_lowercase();
                (name.ends_with(".exe") || name.ends_with(".dll")) && entry.file_type().is_ok_and(|kind| kind.is_file())
            }))
}

impl<F: Fn(&Path, bool) -> bool> Tools<F> {
    fn new(places: &Places, looks: F) -> Self {
        let profile = places.desktop.as_ref().and_then(|desktop| desktop.parent()).map(Path::to_path_buf);
        Tools { known: Default::default(), never: [places.downloads.clone(), places.desktop.clone(), profile].into_iter().flatten().collect(), looks }
    }

    /// Whether the file at `path` is in a tool's folder.
    fn hold(&self, path: &Path) -> bool {
        path.parent().is_some_and(|folder| self.holds(folder))
    }

    /// Whether `folder` is a tool's, or in one.
    fn holds(&self, folder: &Path) -> bool {
        if let Some(known) = self.known.borrow().get(folder) {
            return *known;
        }
        let held = folder.parent().is_some_and(|above| self.holds(above)) || (self.looks)(folder, folder.parent().is_some() && !self.never.iter().any(|never| never == folder));
        let mut known = self.known.borrow_mut();
        if known.len() > 20_000 {
            known.clear();
        }
        known.insert(folder.to_path_buf(), held);
        held
    }

    fn forget(&self) {
        self.known.borrow_mut().clear();
    }
}

static SHOWN: Mutex<Option<Arc<Vec<Came>>>> = Mutex::new(None);
/// Whether the list is on (see `StationSettings::recent`).
static ON: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the files come lately are listed (switched on in the settings, and followed).
pub fn on() -> bool {
    ON.load(std::sync::atomic::Ordering::Relaxed)
}

/// Where the file `key` among those come lately is.
pub fn path_of(key: Key) -> Option<PathBuf> {
    shown().iter().find(|came| came.key == key).map(|came| came.path.clone())
}
static TOLD: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// The files come lately, newest first (none while the list is off, or not yet read).
pub fn shown() -> Arc<Vec<Came>> {
    SHOWN.lock().unwrap().clone().unwrap_or_default()
}

/// For pictures: as if these had come (its thread not started).
#[cfg(all(test, feature = "studio"))]
pub fn pretend(files: Vec<Came>) {
    ON.store(true, std::sync::atomic::Ordering::Relaxed);
    *SHOWN.lock().unwrap() = Some(Arc::new(files));
}

/// Starts following the drives on a thread of its own; `told` is called whenever the list changed.
pub fn start(told: impl Fn() + Send + Sync + 'static) {
    if TOLD.set(Box::new(told)).is_err() {
        return;
    }
    std::thread::Builder::new().name("glance: recent".into()).spawn(follow).expect("a thread started");
}

#[cfg(windows)]
fn follow() {
    const FILE: &str = "recent.json";
    let folder = crate::settings::local_dir();
    let mut kept: Kept = std::fs::read(folder.join(FILE)).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    let places = windows_places();
    let mut drives: Vec<os::Drive> = Vec::new();
    // (Never yet: an `Instant` counts from the PC's start, and an hour before it is none to be had in its first hour.)
    let mut looked_for_drives: Option<Instant> = None;
    let mut rechecked: Option<Instant> = None;
    let mut forgotten = Instant::now();
    let mut saved = Instant::now();
    // What is not yet kept: the list changed, where the journals were read up to moved.
    let (mut files_owed, mut read_owed) = (false, false);
    let tools = Tools::new(&places, looked_at);
    // The folders (by drive and number) whose files are left out, as one of their files was found to be: their files
    // not opened again (forgotten as a folder is renamed, or with the tools').
    let skipped: std::cell::RefCell<HashSet<(char, u128)>> = Default::default();
    let mut shown_hidden: Vec<PathBuf> = Vec::new();
    loop {
        let (on, hidden) = {
            let settings = crate::app().settings.lock().unwrap();
            (settings.station.recent, settings.recent_hidden.clone())
        };
        let was_on = ON.swap(on, std::sync::atomic::Ordering::Relaxed);
        if !on {
            if was_on {
                publish(Vec::new());
            }
            std::thread::sleep(EVERY);
            continue;
        }
        // The drives looked for again now and then (one put in, or formatted).
        if looked_for_drives.is_none_or(|looked| looked.elapsed() > Duration::from_secs(60)) {
            drives = os::drives();
            looked_for_drives = Some(Instant::now());
        }
        if forgotten.elapsed() > FORGET_EVERY {
            tools.forget();
            skipped.borrow_mut().clear();
            forgotten = Instant::now();
        }
        // (The folders the user asked not to list are left out as it is shown, not dropped from it: listed again,
        // their files come back.)
        // (Glance's own notes too: written in Glance, they are no file come to the user's folders.)
        let own = [crate::notes::folder().path];
        let out = |path: &Path| left_out(path, &own) || tools.hold(path);
        let now = super::now_ms();
        let mut changed = kept.age(now);
        let mut folder_renamed = false;
        let mut more = false;
        for drive in &mut drives {
            // (Where its journal starts and ends now: a place read up to that its records have since run past is read
            // on from where they start; a journal made anew, read as the first time.)
            if !drive.query() {
                continue;
            }
            let read_to = kept.read.iter().find(|(letter, journal, _)| *letter == drive.letter && *journal == drive.journal).map(|(_, _, at)| *at);
            let from = read_to.map_or_else(|| drive.start(FIRST_READ_BACK), |at| at.max(drive.first));
            let Some((to, changes, done)) = drive.read(from, BATCH) else { continue };
            more |= !done;
            let letter = drive.letter;
            for change in together(changes) {
                if change.attributes & reasons::FOLDER != 0 {
                    // (A folder renamed or moved: its files are followed, and which folders are left out asked anew.)
                    if change.reason & reasons::RENAMED_TO != 0 && change.reason & reasons::CLOSED != 0 {
                        folder_renamed = true;
                        skipped.borrow_mut().retain(|(drive, _)| *drive != letter);
                    }
                    continue;
                }
                if change.reason & reasons::DELETED == 0 && skipped.borrow().contains(&(letter, change.folder)) {
                    continue;
                }
                // (A file found left out: its folder's skipped from now on, all left out being so by its folders.)
                let out_here = |path: &Path| {
                    let left = out(path);
                    if left {
                        let mut skipped = skipped.borrow_mut();
                        if skipped.len() > 50_000 {
                            skipped.clear();
                        }
                        skipped.insert((letter, change.folder));
                    }
                    left
                };
                changed |= kept.take(letter, change, now, &places, &out_here, |file| drive.look(file), |path| std::fs::read_to_string(format!("{}:Zone.Identifier", path.display())).ok().and_then(|text| site_of(&text)));
            }
            if read_to != Some(to) {
                kept.read.retain(|(letter, ..)| *letter != drive.letter);
                kept.read.push((drive.letter, drive.journal, to));
                read_owed = true;
            }
        }
        // Each file listed looked for where it was now and then, and as a folder was renamed.
        if folder_renamed || rechecked.is_none_or(|rechecked| rechecked.elapsed() > RECHECK_EVERY) {
            let look = |key: Key| drives.iter().find(|drive| drive.letter == key.drive).map_or(Looked::Unknown, |drive| drive.look(key.file));
            changed |= kept.recheck(there, look, &out);
            rechecked = Some(Instant::now());
        }
        if changed || !was_on || hidden != shown_hidden {
            publish(kept.files.iter().filter(|came| !hidden.iter().any(|folder| came.path.starts_with(folder))).cloned().collect());
            shown_hidden = hidden.clone();
        }
        // Kept as it changes, and where each journal was read up to now and then (a restart reads on from there); one
        // not kept, kept at the next turn.
        files_owed |= changed;
        if files_owed || read_owed && saved.elapsed() > SAVE_EVERY {
            let written = serde_json::to_vec(&kept).map_err(std::io::Error::other).and_then(|bytes| {
                crate::elevation::ensure_folder(&folder)?;
                crate::elevation::write_in_place(&folder, FILE, &bytes)
            });
            if written.is_ok() {
                (files_owed, read_owed) = (false, false);
                saved = Instant::now();
            }
        }
        // (Read on at once while a journal has more: the list shown and kept between its batches.)
        if !more {
            std::thread::sleep(EVERY);
        }
    }
}

fn publish(files: Vec<Came>) {
    *SHOWN.lock().unwrap() = Some(Arc::new(files));
    if let Some(told) = TOLD.get() {
        told();
    }
}

/// This user's Downloads, desktop and screenshots' folders.
#[cfg(windows)]
fn windows_places() -> Places {
    use windows::Win32::UI::Shell::{FOLDERID_Desktop, FOLDERID_Downloads, FOLDERID_Screenshots, SHGetKnownFolderPath, KF_FLAG_DEFAULT};
    let known = |id: &windows::core::GUID| unsafe {
        let found = SHGetKnownFolderPath(id, KF_FLAG_DEFAULT, None).ok()?;
        let path = found.to_string().ok().map(PathBuf::from);
        windows::Win32::System::Com::CoTaskMemFree(Some(found.0 as *const _));
        path
    };
    Places { downloads: known(&FOLDERID_Downloads), desktop: known(&FOLDERID_Desktop), screenshots: known(&FOLDERID_Screenshots) }
}

#[cfg(windows)]
mod os {
    use super::{Looked, JOURNAL_PAGE};
    use std::path::PathBuf;
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{CloseHandle, ERROR_FILE_NOT_FOUND, ERROR_INVALID_PARAMETER, GENERIC_READ, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, GetDriveTypeW, GetFileInformationByHandle, GetFinalPathNameByHandleW, GetLogicalDrives, GetVolumeInformationW, OpenFileById,
        BY_HANDLE_FILE_INFORMATION, ExtendedFileIdType, FileIdType, FILE_FLAG_BACKUP_SEMANTICS, FILE_ID_128, FILE_ID_DESCRIPTOR, FILE_ID_DESCRIPTOR_0,
        FILE_NAME_NORMALIZED, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::System::Ioctl::{FSCTL_QUERY_USN_JOURNAL, FSCTL_READ_UNPRIVILEGED_USN_JOURNAL, READ_USN_JOURNAL_DATA_V0, USN_JOURNAL_DATA_V0};
    use windows::Win32::System::IO::DeviceIoControl;

    /// A fixed drive whose journal is followed: its letter, its root folder held open, the journal's number, where
    /// its records start and where the next will be (as last asked).
    pub struct Drive {
        pub letter: char,
        root: HANDLE,
        pub journal: u64,
        pub first: i64,
        pub next: i64,
        /// Its numbers twice as long (ReFS's).
        long: bool,
    }

    impl Drop for Drive {
        fn drop(&mut self) {
            let _ = unsafe { CloseHandle(self.root) };
        }
    }

    /// The fixed drives with a change journal the user may read (NTFS's and ReFS's).
    pub fn drives() -> Vec<Drive> {
        let mask = unsafe { GetLogicalDrives() };
        (0..26u8).filter(|bit| mask & (1 << bit) != 0).filter_map(|bit| Drive::at((b'A' + bit) as char)).collect()
    }

    impl Drive {
        fn at(letter: char) -> Option<Drive> {
            let root = HSTRING::from(format!("{letter}:\\"));
            unsafe {
                // (DRIVE_FIXED.)
                if GetDriveTypeW(&root) != 3 {
                    return None;
                }
                let mut system = [0u16; 32];
                GetVolumeInformationW(&root, None, None, None, None, Some(&mut system)).ok()?;
                let system = String::from_utf16_lossy(&system).trim_end_matches('\0').to_string();
                if system != "NTFS" && system != "ReFS" {
                    return None;
                }
                let handle = CreateFileW(&root, GENERIC_READ.0, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, None, OPEN_EXISTING, FILE_FLAG_BACKUP_SEMANTICS, None).ok()?;
                let mut drive = Drive { letter, root: handle, journal: 0, first: 0, next: 0, long: system == "ReFS" };
                drive.query().then_some(drive)
            }
        }

        /// Its journal asked again: its number, where its records start and where the next will be; whether it has
        /// one now.
        pub fn query(&mut self) -> bool {
            let mut data = USN_JOURNAL_DATA_V0::default();
            let mut got = 0u32;
            let asked = unsafe { DeviceIoControl(self.root, FSCTL_QUERY_USN_JOURNAL, None, 0, Some((&mut data as *mut USN_JOURNAL_DATA_V0).cast()), std::mem::size_of::<USN_JOURNAL_DATA_V0>() as u32, Some(&mut got), None) };
            if asked.is_err() {
                return false;
            }
            (self.journal, self.first, self.next) = (data.UsnJournalID, data.FirstUsn, data.NextUsn);
            true
        }

        /// Where its journal is first read from: `back` bytes of records before its end (the page that place is in,
        /// where a record starts), or where its records start.
        pub fn start(&self, back: i64) -> i64 {
            ((self.next - back).div_euclid(JOURNAL_PAGE) * JOURNAL_PAGE).max(self.first)
        }

        /// What changed from `from` on (a record's place), up to where its journal ended as last asked, or `most`
        /// bytes of records: where to read on from, the changes (each file's, as it was closed), and whether that is
        /// all there was; none where it could not be read.
        pub fn read(&self, from: i64, most: usize) -> Option<(i64, Vec<super::Change>, bool)> {
            let mut at = from;
            let mut changes = Vec::new();
            let mut buffer = vec![0u8; 1 << 16];
            let mut read_bytes = 0usize;
            loop {
                if at >= self.next {
                    return Some((at, changes, true));
                }
                let ask = READ_USN_JOURNAL_DATA_V0 {
                    StartUsn: at,
                    ReasonMask: super::reasons::CREATED | super::reasons::DELETED | super::reasons::RENAMED_TO | super::reasons::CLOSED,
                    ReturnOnlyOnClose: 1,
                    Timeout: 0,
                    BytesToWaitFor: 0,
                    UsnJournalID: self.journal,
                };
                let mut got = 0u32;
                let read = unsafe {
                    DeviceIoControl(self.root, FSCTL_READ_UNPRIVILEGED_USN_JOURNAL, Some((&ask as *const READ_USN_JOURNAL_DATA_V0).cast()), std::mem::size_of::<READ_USN_JOURNAL_DATA_V0>() as u32, Some(buffer.as_mut_ptr().cast()), buffer.len() as u32, Some(&mut got), None)
                };
                // (Its records from `at` gone meanwhile, or the journal made anew: asked again at the next turn.)
                if read.is_err() {
                    return None;
                }
                let (next, more) = super::changes_in(&buffer[..got as usize])?;
                changes.extend(more);
                read_bytes += got as usize;
                if next == at || got <= 8 {
                    return Some((next, changes, true));
                }
                at = next;
                if read_bytes >= most {
                    return Some((at, changes, at >= self.next));
                }
            }
        }

        /// The file numbered `file` on it, opened as the user: where it is (its path, when it was made, in ms since
        /// 1970, and how large it is), gone, or not to be told (theirs not to open, or locked).
        pub fn look(&self, file: u128) -> Looked {
            let described = FILE_ID_DESCRIPTOR {
                dwSize: std::mem::size_of::<FILE_ID_DESCRIPTOR>() as u32,
                Type: if self.long { ExtendedFileIdType } else { FileIdType },
                Anonymous: if self.long { FILE_ID_DESCRIPTOR_0 { ExtendedFileId: FILE_ID_128 { Identifier: file.to_le_bytes() } } } else { FILE_ID_DESCRIPTOR_0 { FileId: file as i64 } },
            };
            unsafe {
                let handle = match OpenFileById(self.root, &described, 0, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, None, FILE_FLAG_BACKUP_SEMANTICS) {
                    Ok(handle) => handle,
                    // (A number no file has: the file deleted, its number's sequence moved on.)
                    Err(error) if error.code() == ERROR_INVALID_PARAMETER.to_hresult() || error.code() == ERROR_FILE_NOT_FOUND.to_hresult() => return Looked::Gone,
                    Err(_) => return Looked::Unknown,
                };
                let mut name = [0u16; 1024];
                let length = GetFinalPathNameByHandleW(handle, &mut name, FILE_NAME_NORMALIZED) as usize;
                let mut information = BY_HANDLE_FILE_INFORMATION::default();
                let told = GetFileInformationByHandle(handle, &mut information).is_ok();
                let _ = CloseHandle(handle);
                if length == 0 || length >= name.len() || !told {
                    return Looked::Unknown;
                }
                let path = String::from_utf16_lossy(&name[..length]);
                // (Written as Windows writes a path for a person: "\\?\D:\…" as "D:\…".)
                let path = PathBuf::from(path.strip_prefix(r"\\?\").unwrap_or(&path));
                let made = (u64::from(information.ftCreationTime.dwHighDateTime) << 32) | u64::from(information.ftCreationTime.dwLowDateTime);
                let bytes = (u64::from(information.nFileSizeHigh) << 32) | u64::from(information.nFileSizeLow);
                Looked::At(path, super::ms_of(made), bytes)
            }
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn places() -> Places {
        Places { downloads: Some(PathBuf::from(r"C:\Users\a\Downloads")), desktop: Some(PathBuf::from(r"C:\Users\a\Desktop")), screenshots: Some(PathBuf::from(r"C:\Users\a\Pictures\Screenshots")) }
    }

    /// Windows', programs' and tools' folders left out, and those asked; a person's own kept.
    #[test]
    fn leaves_out_what_is_not_a_persons() {
        let hidden = [PathBuf::from(r"D:\Private")];
        for path in [r"C:\Windows\Temp\a.pdf", r"C:\Program Files\x\b.png", r"C:\Users\a\AppData\Local\c.png", r"D:\code\.git\objects\d.zip", r"D:\code\node_modules\e.md", r"D:\Private\f.pdf", r"D:\proj\target\release\g.zip"] {
            assert!(left_out(Path::new(path), &hidden), "{path}");
        }
        for path in [r"C:\Users\a\Downloads\setup.exe", r"D:\Documents\report.pdf", r"D:\Windows Stuff\notes.md", r"C:\Users\a\Pictures\Screenshots\s.png"] {
            assert!(!left_out(Path::new(path), &hidden), "{path}");
        }
    }

    /// What a file is by its name and where it came: a screenshot by its folder or name, anything in Downloads (and
    /// downloaded), a program only there or on the desktop, a download in progress or a lock not at all.
    #[test]
    fn tells_what_a_file_is() {
        let places = places();
        let of = |path: &str| kind_of(Path::new(path), &places);
        assert_eq!(of(r"C:\Users\a\Pictures\Screenshots\x.png"), Some((Kind::Screenshot, false)));
        assert_eq!(of(r"D:\shots\屏幕截图 2026-10-18 154210.png"), Some((Kind::Screenshot, false)));
        assert_eq!(of(r"D:\photos\sea.JPG"), Some((Kind::Picture, false)));
        assert_eq!(of(r"C:\Users\a\Downloads\glance-setup.exe"), Some((Kind::Program, true)));
        assert_eq!(of(r"C:\Users\a\Downloads\data.bin"), Some((Kind::Other, true)));
        assert_eq!(of(r"C:\Users\a\Desktop\tool.exe"), Some((Kind::Program, false)));
        assert_eq!(of(r"D:\build\tool.exe"), None);
        assert_eq!(of(r"D:\code\main.rs"), None);
        assert_eq!(of(r"C:\Users\a\Downloads\big.zip.crdownload"), None);
        assert_eq!(of(r"D:\Documents\~$report.docx"), None);
        assert_eq!(of(r"D:\Documents\report.docx"), Some((Kind::Document, false)));
    }

    /// A download's site: the page it was taken from, else where it was; none where only its zone is said.
    #[test]
    fn reads_where_a_download_came_from() {
        assert_eq!(site_of("[ZoneTransfer]\r\nZoneId=3\r\nReferrerUrl=https://github.com/lulu-loopp/glance/releases\r\nHostUrl=https://objects.githubusercontent.com/x\r\n").as_deref(), Some("github.com"));
        assert_eq!(site_of("[ZoneTransfer]\nZoneId=3\nHostUrl=https://www.example.com:8443/a.zip\n").as_deref(), Some("example.com"));
        assert_eq!(site_of("[ZoneTransfer]\nZoneId=3\n"), None);
        assert_eq!(site_of("[ZoneTransfer]\nZoneId=3\nHostUrl=about:internet\n"), None);
    }

    /// A journal's read: where to read on from, then records of version 2 and 3 (another passed over), with their
    /// folders.
    #[test]
    fn reads_a_journals_records() {
        let record = |major: u16, file: u128, folder: u128, reason: u32, attributes: u32, stamp: u64| {
            let mut bytes = vec![0u8; if major == 3 { 80 } else { 64 }];
            let length = bytes.len() as u32;
            bytes[..4].copy_from_slice(&length.to_le_bytes());
            bytes[4..6].copy_from_slice(&major.to_le_bytes());
            match major {
                3 => {
                    bytes[8..24].copy_from_slice(&file.to_le_bytes());
                    bytes[24..40].copy_from_slice(&folder.to_le_bytes());
                    bytes[48..56].copy_from_slice(&stamp.to_le_bytes());
                    bytes[56..60].copy_from_slice(&reason.to_le_bytes());
                    bytes[68..72].copy_from_slice(&attributes.to_le_bytes());
                }
                _ => {
                    bytes[8..16].copy_from_slice(&(file as u64).to_le_bytes());
                    bytes[16..24].copy_from_slice(&(folder as u64).to_le_bytes());
                    bytes[32..40].copy_from_slice(&stamp.to_le_bytes());
                    bytes[40..44].copy_from_slice(&reason.to_le_bytes());
                    bytes[52..56].copy_from_slice(&attributes.to_le_bytes());
                }
            }
            bytes
        };
        // 2026-10-18 00:00 UTC as a file time.
        let stamp = (1_760_745_600_000u64 + 11_644_473_600_000) * 10_000;
        let mut read = 777i64.to_le_bytes().to_vec();
        read.extend(record(2, 42, 5, 0x8000_0100, 0x20, stamp));
        read.extend(record(4, 1, 1, 0, 0, 0));
        read.extend(record(3, 1u128 << 70, 1u128 << 66, 0x8000_0200, 0, stamp));
        let (next, changes) = changes_in(&read).unwrap();
        assert_eq!(next, 777);
        assert_eq!(changes, vec![
            Change { file: 42, folder: 5, reason: 0x8000_0100, attributes: 0x20, at_ms: 1_760_745_600_000 },
            Change { file: 1u128 << 70, folder: 1u128 << 66, reason: 0x8000_0200, attributes: 0, at_ms: 1_760_745_600_000 },
        ]);
    }

    /// The list as changes come: a file made lately listed (with its site), one made long ago (moved, renamed) not, one
    /// deleted taken off, one renamed followed to its new name, one renamed into a folder left out taken off.
    #[test]
    fn follows_files_as_they_come_and_go() {
        let now = 1_760_800_000_000;
        let places = places();
        let change = |file: u128, reason: u32| Change { file, folder: 9, reason, attributes: 0x20, at_ms: now };
        let mut kept = Kept::default();
        let open = |path: &'static str, made: u64| move |_: u128| Looked::At(PathBuf::from(path), made, 10);
        let site = |_: &Path| Some("github.com".to_string());
        let out = |path: &Path| left_out(path, &[]);
        assert!(kept.take('C', change(1, 0x8000_0100), now, &places, &out, open(r"C:\Users\a\Downloads\a.zip", now - 1000), site));
        assert_eq!((kept.files[0].kind, kept.files[0].downloaded, kept.files[0].site.as_deref()), (Kind::Archive, true, Some("github.com")));
        // Moved here, made long ago: not one come.
        assert!(!kept.take('D', change(2, 0x8000_2000), now, &places, &out, open(r"D:\Documents\old.pdf", now - KEEP_MS - 1), |_| None));
        // Not yet closed: nothing yet.
        assert!(!kept.take('D', change(3, 0x100), now, &places, &out, open(r"D:\Documents\new.pdf", now), |_| None));
        assert!(kept.take('D', change(3, 0x8000_0100), now, &places, &out, open(r"D:\Documents\new.pdf", now - 10), |_| None));
        assert_eq!(kept.files.iter().map(|came| came.key.file).collect::<Vec<_>>(), [3, 1]);
        // Renamed: followed.
        assert!(kept.take('D', change(3, 0x8000_2000), now, &places, &out, open(r"D:\Documents\renamed.pdf", now - 10), |_| None));
        assert_eq!(kept.files[0].path, PathBuf::from(r"D:\Documents\renamed.pdf"));
        // Renamed into a folder left out: taken off.
        assert!(kept.take('D', change(3, 0x8000_2000), now, &places, &out, open(r"D:\Documents\.trash\renamed.pdf", now - 10), |_| None));
        // Deleted: taken off.
        assert!(kept.take('C', change(1, 0x8000_0200), now, &places, &out, |_| Looked::Gone, |_| None));
        assert!(kept.files.is_empty());
        // A folder, a hidden file: never.
        assert!(!kept.take('D', Change { attributes: 0x10, ..change(4, 0x8000_0100) }, now, &places, &out, open(r"D:\Documents\x.pdf", now), |_| None));
        assert!(!kept.take('D', Change { attributes: 0x2, ..change(5, 0x8000_0100) }, now, &places, &out, open(r"D:\Documents\y.pdf", now), |_| None));
        // A change older than the days kept: its file never opened.
        assert!(!kept.take('D', Change { at_ms: now - KEEP_MS - 1, ..change(6, 0x8000_0100) }, now, &places, &out, |_| panic!("opened"), |_| None));
    }

    /// The same number on two drives: two files, one deleted leaving the other listed.
    #[test]
    fn knows_a_file_by_its_drive_and_number() {
        let now = 1_760_800_000_000;
        let places = places();
        let change = |reason: u32| Change { file: 7, folder: 9, reason, attributes: 0x20, at_ms: now };
        let out = |path: &Path| left_out(path, &[]);
        let mut kept = Kept::default();
        assert!(kept.take('C', change(0x8000_0100), now, &places, &out, |_| Looked::At(PathBuf::from(r"C:\Users\a\Downloads\c.zip"), now - 20, 1), |_| None));
        assert!(kept.take('D', change(0x8000_0100), now, &places, &out, |_| Looked::At(PathBuf::from(r"D:\Documents\d.pdf"), now - 10, 1), |_| None));
        assert_eq!(kept.files.len(), 2);
        assert!(kept.take('D', change(0x8000_0200), now, &places, &out, |_| Looked::Gone, |_| None));
        assert_eq!(kept.files.iter().map(|came| came.path.clone()).collect::<Vec<_>>(), [PathBuf::from(r"C:\Users\a\Downloads\c.zip")]);
    }

    /// A file made and written again within one read: made, as one change (its reasons together).
    #[test]
    fn takes_a_files_changes_together() {
        let change = |file: u128, reason: u32, at_ms: u64| Change { file, folder: 9, reason, attributes: 0x20, at_ms };
        let together = together(vec![change(1, 0x8000_0102, 1), change(2, 0x8000_0100, 2), change(1, 0x8000_0002, 3)]);
        assert_eq!(together, vec![change(1, 0x8000_0102, 3), change(2, 0x8000_0100, 2)]);
    }

    /// Each file listed looked for: one there kept; one moved with its folder followed by its number; one gone taken
    /// off; one whose drive is away, or not ready, kept; one now in a tool's folder taken off.
    #[test]
    fn looks_for_each_file_again() {
        let came = |drive: char, file: u128, path: &str| Came { key: Key { drive, file }, path: PathBuf::from(path), came_ms: 1, bytes: 1, kind: Kind::Document, downloaded: false, site: None };
        let mut kept = Kept { read: Vec::new(), files: vec![came('D', 1, r"D:\a\one.pdf"), came('D', 2, r"D:\b\two.pdf"), came('D', 3, r"D:\c\three.pdf"), came('E', 4, r"E:\four.pdf"), came('D', 5, r"D:\d\five.pdf"), came('D', 6, r"D:\repo\six.pdf")] };
        let there = |path: &Path| match path.to_str().unwrap() {
            r"D:\a\one.pdf" | r"D:\repo\six.pdf" => There::Yes,
            r"D:\d\five.pdf" => There::Unknown,
            _ => There::No,
        };
        let look = |key: Key| match (key.drive, key.file) {
            ('D', 2) => Looked::At(PathBuf::from(r"D:\b renamed\two.pdf"), 1, 1),
            ('D', 3) => Looked::Gone,
            _ => Looked::Unknown,
        };
        assert!(kept.recheck(there, look, &|path: &Path| path.starts_with(r"D:\repo")));
        assert_eq!(kept.files.iter().map(|came| came.path.to_str().unwrap()).collect::<Vec<_>>(), [r"D:\a\one.pdf", r"D:\b renamed\two.pdf", r"E:\four.pdf", r"D:\d\five.pdf"]);
        assert!(!kept.recheck(|_| There::Yes, |_| Looked::Unknown, &|_| false));
    }

    /// A file in a repository's working tree or a program's folder, however deep: a tool's; beside them, or in
    /// Downloads holding a program: not. The folders above asked first (one held: none below it looked at), and
    /// Downloads never looked through for a program.
    #[test]
    fn knows_tools_files() {
        let looked: std::cell::RefCell<Vec<(PathBuf, bool)>> = Default::default();
        let looks = |folder: &Path, programs: bool| {
            looked.borrow_mut().push((folder.to_path_buf(), programs));
            folder == Path::new("D:/repo") || programs && (folder == Path::new("D:/app") || folder == Path::new("C:/Users/a/Downloads"))
        };
        let tools = Tools::new(&Places { downloads: Some(PathBuf::from("C:/Users/a/Downloads")), desktop: Some(PathBuf::from("C:/Users/a/Desktop")), screenshots: None }, looks);
        assert!(tools.hold(Path::new("D:/repo/docs/deep/a.md")));
        assert!(!looked.borrow().iter().any(|(folder, _)| folder.starts_with("D:/repo/docs")));
        assert!(tools.hold(Path::new("D:/repo/b.md")));
        assert!(tools.hold(Path::new("D:/app/skins/icons/c.png")));
        assert!(!tools.hold(Path::new("D:/loose/d.md")));
        assert!(!tools.hold(Path::new("C:/Users/a/Downloads/e.pdf")));
        assert!(looked.borrow().iter().any(|(folder, programs)| folder == Path::new("C:/Users/a/Downloads") && !programs));
    }

    /// The list read as Windows keeps the user's files: an ignored test that lists what it finds now (it reads this
    /// PC's drives, as the user), read back from a place within a busy journal (not a record's: the page it is in).
    #[test]
    #[ignore = "reads this PC's drives"]
    fn lists_what_came_lately_here() {
        let places = windows_places();
        let now = crate::station::now_ms();
        let mut kept = Kept::default();
        let tools = Tools::new(&places, looked_at);
        for drive in os::drives() {
            let mut from = drive.start(64 << 20);
            let mut batches = 0;
            loop {
                let (to, changes, done) = drive.read(from, BATCH).expect("its journal read");
                for change in together(changes) {
                    kept.take(drive.letter, change, now, &places, &|path| left_out(path, &[]) || tools.hold(path), |file| drive.look(file), |path| std::fs::read_to_string(format!("{}:Zone.Identifier", path.display())).ok().and_then(|text| site_of(&text)));
                }
                batches += 1;
                from = to;
                if done {
                    break;
                }
            }
            println!("{}: {batches} batches", drive.letter);
            // Read back from within: the page that place is in, its first record.
            assert!(drive.read(drive.start(4096 + 8), BATCH).is_some(), "{} read from within", drive.letter);
        }
        for came in kept.files.iter().take(40) {
            println!("{:?} {:?} {:?} {}", came.kind, came.site, came.path, crate::ui::text::since(came.came_ms, now, crate::ui::text::Lang::Zh));
        }
        assert!(!kept.files.is_empty());
    }
}
