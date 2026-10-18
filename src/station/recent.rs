//! Recent files: the files lately come to the user's folders, wherever they landed (downloaded, a screenshot, a
//! document saved), newest first, shown as a page of the station (see docs/station-and-notes.md, section 八).
//!
//! Read as the user, never through the elevated sensors: from each fixed drive's change journal, read the way NTFS
//! lets anyone read it (`FSCTL_READ_UNPRIVILEGED_USN_JOURNAL`, through the drive's root folder). Its records name no
//! files there, so each file come is opened by its number (`OpenFileById`) and its path asked of it: a file the user
//! may not open is not opened, and so never listed. The journal keeps what changed while Glance was not running: where
//! it was last read up to is kept, and read on from there.
//!
//! Only what a person keeps: the folders of Windows, programs, programs' own data and tools' (`AppData`, `.git`,
//! `node_modules`, …) are left out, and so are a repository's working tree (a checkout writes hundreds of files at
//! once, its tools' to follow) and a program's folder wherever it was put (one holding a program or a library: what is
//! written there, its logs and pictures, are the program's), as are hidden, system and temporary files and those being written (a download in progress);
//! outside Downloads and the desktop, only documents, pictures, videos, music and archives. A file moved or
//! renamed is not one come (its creation is old); one renamed as a download finishes is.

use serde::{Deserialize, Serialize};
use std::path::{Path, PathBuf};
use std::sync::{Arc, Mutex, OnceLock};
use std::time::Duration;

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

/// A file come lately: its number on its drive, where it is, when it was made (ms since 1970), how large it is, what it
/// is, whether it came into Downloads, and the site it was downloaded from where its download says so.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Came {
    pub file: u128,
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
/// How often the journals are read.
const EVERY: Duration = Duration::from_secs(2);
/// How often where the journals were read up to is kept, with nothing else changed.
const SAVE_EVERY: Duration = Duration::from_secs(60);
/// The most of a journal read back the first time (its records' bytes): enough for the days kept on a busy drive.
const FIRST_READ_BACK: i64 = 512 << 20;

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

/// One change a journal's record tells: the file's number, why (its reasons, as `USN_REASON_*`), its attributes, and
/// when (ms since 1970).
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Change {
    pub file: u128,
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
    let mut changes = Vec::new();
    let mut at = 8;
    while at + 8 <= read.len() {
        let Some(length) = u32_at(at).filter(|length| *length >= 8) else { break };
        let change = match u16_at(at + 4) {
            Some(2) => (|| Some(Change { file: u128::from(u64_at(at + 8)?), at_ms: ms_of(u64_at(at + 32)?), reason: u32_at(at + 40)?, attributes: u32_at(at + 52)? }))(),
            Some(3) => (|| Some(Change { file: u128::from(u64_at(at + 8)?) | (u128::from(u64_at(at + 16)?) << 64), at_ms: ms_of(u64_at(at + 48)?), reason: u32_at(at + 56)?, attributes: u32_at(at + 68)? }))(),
            _ => None,
        };
        changes.extend(change);
        at += length as usize;
    }
    Some((next, changes))
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

/// The list, as kept between runs: where each drive's journal was read up to (by its letter, the journal's number,
/// and the place in it), and the files come.
#[derive(Clone, Debug, Default, Serialize, Deserialize)]
struct Kept {
    read: Vec<(char, u64, i64)>,
    files: Vec<Came>,
}

impl Kept {
    /// `change` seen: a file taken off the list where it is gone; one come, or come under another name, opened by its
    /// number on `drive` (`open`: its path, when it was made, and how large it is) and listed if it is one listed
    /// (made within the days kept before `now`, as `kind_of` says, and not `out`); whether the list changed.
    fn take(&mut self, change: Change, now: u64, places: &Places, out: &dyn Fn(&Path) -> bool, open: impl Fn(u128) -> Option<(PathBuf, u64, u64)>, site: impl Fn(&Path) -> Option<String>) -> bool {
        use reasons::*;
        if change.reason & CLOSED == 0 {
            return false;
        }
        if change.reason & DELETED != 0 {
            let before = self.files.len();
            self.files.retain(|came| came.file != change.file);
            return self.files.len() != before;
        }
        if change.reason & (CREATED | RENAMED_TO) == 0 || change.attributes & (HIDDEN | SYSTEM | FOLDER | TEMPORARY) != 0 {
            return false;
        }
        let Some((path, made_ms, bytes)) = open(change.file) else { return false };
        let had = self.files.iter().position(|came| came.file == change.file);
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
                self.files.push(Came { file: change.file, path, came_ms: made_ms, bytes, kind, downloaded, site });
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

    /// Those made longer ago than the days kept before `now`, and those `gone` says are gone, taken off; whether any were.
    fn prune(&mut self, now: u64, gone: impl Fn(&Came) -> bool) -> bool {
        let before = self.files.len();
        self.files.retain(|came| now.saturating_sub(came.came_ms) <= KEEP_MS && !gone(came));
        self.files.len() != before
    }
}

/// Which folders are tools': in a repository's working tree (it, or a folder above, holds `.git`), or in a program's
/// folder (it, or a folder above, holds a program or a library), as each was looked at (forgotten now and then: one
/// made or removed is seen). Downloads, the desktop, the user's own folder and a drive's top are never a program's,
/// whatever they hold. `looks` says what a folder holds itself: a repository, a program.
struct Tools<F: Fn(&Path) -> (bool, bool)> {
    known: std::cell::RefCell<std::collections::HashMap<PathBuf, bool>>,
    never: Vec<PathBuf>,
    looks: F,
}

/// What the folder at `folder` holds itself, as `Tools` asks: a repository (`.git`), a program or a library.
fn looked_at(folder: &Path) -> (bool, bool) {
    let program = std::fs::read_dir(folder).is_ok_and(|entries| entries.flatten().any(|entry| {
        let name = entry.file_name().to_string_lossy().to_lowercase();
        (name.ends_with(".exe") || name.ends_with(".dll")) && entry.file_type().is_ok_and(|kind| kind.is_file())
    }));
    (folder.join(".git").exists(), program)
}

impl<F: Fn(&Path) -> (bool, bool)> Tools<F> {
    fn new(places: &Places, looks: F) -> Self {
        let profile = places.desktop.as_ref().and_then(|desktop| desktop.parent()).map(Path::to_path_buf);
        Tools { known: Default::default(), never: [places.downloads.clone(), places.desktop.clone(), profile].into_iter().flatten().collect(), looks }
    }

    /// Whether the file at `path` is in a tool's folder.
    fn hold(&self, path: &Path) -> bool {
        let Some(folder) = path.parent() else { return false };
        if let Some(known) = self.known.borrow().get(folder) {
            return *known;
        }
        let (repository, program) = (self.looks)(folder);
        let program = program && folder.parent().is_some() && !self.never.iter().any(|never| never == folder);
        let held = repository || program || self.hold(folder);
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

/// Where the file numbered `file` among those come lately is.
pub fn path_of(file: u128) -> Option<PathBuf> {
    shown().iter().find(|came| came.file == file).map(|came| came.path.clone())
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
    let file = crate::settings::local_dir().join("recent.json");
    let mut kept: Kept = std::fs::read(&file).ok().and_then(|bytes| serde_json::from_slice(&bytes).ok()).unwrap_or_default();
    let places = windows_places();
    let mut drives: Vec<os::Drive> = Vec::new();
    // (Never yet: an `Instant` counts from the PC's start, and an hour before it is none to be had in its first hour.)
    let mut looked_for_drives: Option<std::time::Instant> = None;
    let mut saved = std::time::Instant::now();
    let tools = Tools::new(&places, looked_at);
    loop {
        let (on, hidden) = {
            let settings = crate::app().settings.lock().unwrap();
            (settings.station.recent, settings.recent_hidden.clone())
        };
        if ON.swap(on, std::sync::atomic::Ordering::Relaxed) != on && !on {
            publish(Vec::new());
        }
        if !on {
            std::thread::sleep(EVERY);
            continue;
        }
        // The drives looked for again now and then (one put in, or formatted).
        if looked_for_drives.is_none_or(|looked| looked.elapsed() > Duration::from_secs(60)) {
            drives = os::drives();
            looked_for_drives = Some(std::time::Instant::now());
            tools.forget();
        }
        let out = |path: &Path| left_out(path, &hidden) || tools.hold(path);
        let now = super::now_ms();
        // The list as it was: those gone meanwhile, or now left out, taken off.
        let mut changed = kept.prune(now, |came| !came.path.exists() || out(&came.path));
        for drive in &mut drives {
            let from = kept.read.iter().find(|(letter, journal, _)| *letter == drive.letter && *journal == drive.journal).map(|(_, _, at)| *at);
            let from = from.unwrap_or_else(|| (drive.next - FIRST_READ_BACK).max(drive.first));
            let Some((to, changes)) = drive.read(from) else { continue };
            for change in changes {
                changed |= kept.take(change, now, &places, &out, |file| drive.open_file(file), |path| std::fs::read_to_string(format!("{}:Zone.Identifier", path.display())).ok().and_then(|text| site_of(&text)));
            }
            kept.read.retain(|(letter, ..)| *letter != drive.letter);
            kept.read.push((drive.letter, drive.journal, to));
        }
        if changed || SHOWN.lock().unwrap().is_none() {
            publish(kept.files.clone());
        }
        // Kept as it changes, and where each journal was read up to now and then (a restart reads on from there).
        if changed || saved.elapsed() > SAVE_EVERY {
            if let Ok(bytes) = serde_json::to_vec(&kept) {
                let _ = std::fs::write(&file, bytes);
            }
            saved = std::time::Instant::now();
        }
        std::thread::sleep(EVERY);
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
    use std::path::PathBuf;
    use windows::core::HSTRING;
    use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, HANDLE};
    use windows::Win32::Storage::FileSystem::{
        CreateFileW, GetDriveTypeW, GetFileInformationByHandle, GetFinalPathNameByHandleW, GetLogicalDrives, GetVolumeInformationW, OpenFileById,
        BY_HANDLE_FILE_INFORMATION, ExtendedFileIdType, FileIdType, FILE_FLAG_BACKUP_SEMANTICS, FILE_ID_128, FILE_ID_DESCRIPTOR, FILE_ID_DESCRIPTOR_0,
        FILE_NAME_NORMALIZED, FILE_SHARE_DELETE, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING,
    };
    use windows::Win32::System::Ioctl::{FSCTL_QUERY_USN_JOURNAL, FSCTL_READ_UNPRIVILEGED_USN_JOURNAL, READ_USN_JOURNAL_DATA_V0, USN_JOURNAL_DATA_V0};
    use windows::Win32::System::IO::DeviceIoControl;

    /// A fixed drive whose journal is followed: its letter, its root folder held open, the journal's number, where
    /// its records start and where the next will be.
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
                let mut data = USN_JOURNAL_DATA_V0::default();
                let mut got = 0u32;
                if DeviceIoControl(handle, FSCTL_QUERY_USN_JOURNAL, None, 0, Some((&mut data as *mut USN_JOURNAL_DATA_V0).cast()), std::mem::size_of::<USN_JOURNAL_DATA_V0>() as u32, Some(&mut got), None).is_err() {
                    let _ = CloseHandle(handle);
                    return None;
                }
                Some(Drive { letter, root: handle, journal: data.UsnJournalID, first: data.FirstUsn, next: data.NextUsn, long: system == "ReFS" })
            }
        }

        /// What changed from `from` on, up to now: where to read on from, and the changes (each file's last, as it
        /// was closed).
        pub fn read(&mut self, from: i64) -> Option<(i64, Vec<super::Change>)> {
            let mut at = from;
            let mut changes = Vec::new();
            let mut buffer = vec![0u8; 1 << 16];
            loop {
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
                // (The journal made anew, or its records from `at` gone: read from where it starts now, next time.)
                if read.is_err() {
                    return None;
                }
                let (next, more) = super::changes_in(&buffer[..got as usize])?;
                changes.extend(more);
                if next == at || got <= 8 {
                    self.next = next;
                    return Some((next, changes));
                }
                at = next;
            }
        }

        /// The file numbered `file` on it, opened as the user (none where it is gone, or theirs not to open): its
        /// path, when it was made (ms since 1970), and how large it is.
        pub fn open_file(&self, file: u128) -> Option<(PathBuf, u64, u64)> {
            let described = FILE_ID_DESCRIPTOR {
                dwSize: std::mem::size_of::<FILE_ID_DESCRIPTOR>() as u32,
                Type: if self.long { ExtendedFileIdType } else { FileIdType },
                Anonymous: if self.long { FILE_ID_DESCRIPTOR_0 { ExtendedFileId: FILE_ID_128 { Identifier: file.to_le_bytes() } } } else { FILE_ID_DESCRIPTOR_0 { FileId: file as i64 } },
            };
            unsafe {
                let handle = OpenFileById(self.root, &described, 0, FILE_SHARE_READ | FILE_SHARE_WRITE | FILE_SHARE_DELETE, None, FILE_FLAG_BACKUP_SEMANTICS).ok()?;
                let mut name = [0u16; 1024];
                let length = GetFinalPathNameByHandleW(handle, &mut name, FILE_NAME_NORMALIZED) as usize;
                let mut information = BY_HANDLE_FILE_INFORMATION::default();
                let told = GetFileInformationByHandle(handle, &mut information).is_ok();
                let _ = CloseHandle(handle);
                if length == 0 || length >= name.len() || !told {
                    return None;
                }
                let path = String::from_utf16_lossy(&name[..length]);
                // (Written as Windows writes a path for a person: "\\?\D:\…" as "D:\…".)
                let path = PathBuf::from(path.strip_prefix(r"\\?\").unwrap_or(&path));
                let made = (u64::from(information.ftCreationTime.dwHighDateTime) << 32) | u64::from(information.ftCreationTime.dwLowDateTime);
                let bytes = (u64::from(information.nFileSizeHigh) << 32) | u64::from(information.nFileSizeLow);
                Some((path, super::ms_of(made), bytes))
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

    /// A journal's read: where to read on from, then records of version 2 and 3 (another passed over).
    #[test]
    fn reads_a_journals_records() {
        let record = |major: u16, file: u128, reason: u32, attributes: u32, stamp: u64| {
            let mut bytes = vec![0u8; if major == 3 { 80 } else { 64 }];
            let length = bytes.len() as u32;
            bytes[..4].copy_from_slice(&length.to_le_bytes());
            bytes[4..6].copy_from_slice(&major.to_le_bytes());
            match major {
                3 => {
                    bytes[8..24].copy_from_slice(&file.to_le_bytes());
                    bytes[48..56].copy_from_slice(&stamp.to_le_bytes());
                    bytes[56..60].copy_from_slice(&reason.to_le_bytes());
                    bytes[68..72].copy_from_slice(&attributes.to_le_bytes());
                }
                _ => {
                    bytes[8..16].copy_from_slice(&(file as u64).to_le_bytes());
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
        read.extend(record(2, 42, 0x8000_0100, 0x20, stamp));
        read.extend(record(4, 1, 0, 0, 0));
        read.extend(record(3, 1u128 << 70, 0x8000_0200, 0, stamp));
        let (next, changes) = changes_in(&read).unwrap();
        assert_eq!(next, 777);
        assert_eq!(changes, vec![
            Change { file: 42, reason: 0x8000_0100, attributes: 0x20, at_ms: 1_760_745_600_000 },
            Change { file: 1u128 << 70, reason: 0x8000_0200, attributes: 0, at_ms: 1_760_745_600_000 },
        ]);
    }

    /// The list as changes come: a file made lately listed (with its site), one made long ago (moved, renamed) not, one
    /// deleted taken off, one renamed followed to its new name, one renamed into a folder left out taken off.
    #[test]
    fn follows_files_as_they_come_and_go() {
        let now = 1_760_800_000_000;
        let places = places();
        let change = |file: u128, reason: u32| Change { file, reason, attributes: 0x20, at_ms: now };
        let mut kept = Kept::default();
        let open = |path: &'static str, made: u64| move |_: u128| Some((PathBuf::from(path), made, 10));
        let site = |_: &Path| Some("github.com".to_string());
        assert!(kept.take(change(1, 0x8000_0100), now, &places, &|path: &Path| left_out(path, &[]), open(r"C:\Users\a\Downloads\a.zip", now - 1000), site));
        assert_eq!((kept.files[0].kind, kept.files[0].downloaded, kept.files[0].site.as_deref()), (Kind::Archive, true, Some("github.com")));
        // Moved here, made long ago: not one come.
        assert!(!kept.take(change(2, 0x8000_2000), now, &places, &|path: &Path| left_out(path, &[]), open(r"D:\Documents\old.pdf", now - KEEP_MS - 1), |_| None));
        // Not yet closed: nothing yet.
        assert!(!kept.take(change(3, 0x100), now, &places, &|path: &Path| left_out(path, &[]), open(r"D:\Documents\new.pdf", now), |_| None));
        assert!(kept.take(change(3, 0x8000_0100), now, &places, &|path: &Path| left_out(path, &[]), open(r"D:\Documents\new.pdf", now - 10), |_| None));
        assert_eq!(kept.files.iter().map(|came| came.file).collect::<Vec<_>>(), [3, 1]);
        // Renamed: followed.
        assert!(kept.take(change(3, 0x8000_2000), now, &places, &|path: &Path| left_out(path, &[]), open(r"D:\Documents\renamed.pdf", now - 10), |_| None));
        assert_eq!(kept.files[0].path, PathBuf::from(r"D:\Documents\renamed.pdf"));
        // Renamed into a folder left out: taken off.
        assert!(kept.take(change(3, 0x8000_2000), now, &places, &|path: &Path| left_out(path, &[]), open(r"D:\Documents\.trash\renamed.pdf", now - 10), |_| None));
        // Deleted: taken off.
        assert!(kept.take(change(1, 0x8000_0200), now, &places, &|path: &Path| left_out(path, &[]), |_| None, |_| None));
        assert!(kept.files.is_empty());
        // A folder, a hidden file: never.
        assert!(!kept.take(Change { attributes: 0x10, ..change(4, 0x8000_0100) }, now, &places, &|path: &Path| left_out(path, &[]), open(r"D:\Documents\x.pdf", now), |_| None));
        assert!(!kept.take(Change { attributes: 0x2, ..change(5, 0x8000_0100) }, now, &places, &|path: &Path| left_out(path, &[]), open(r"D:\Documents\y.pdf", now), |_| None));
    }

    /// A file in a repository's working tree or a program's folder, however deep: a tool's; beside them, or in
    /// Downloads holding a program: not.
    #[test]
    fn knows_tools_files() {
        let looks = |folder: &Path| (folder == Path::new("D:/repo"), folder == Path::new("D:/app") || folder == Path::new("C:/Users/a/Downloads"));
        let tools = Tools::new(&places(), looks);
        assert!(tools.hold(Path::new("D:/repo/docs/deep/a.md")));
        assert!(tools.hold(Path::new("D:/repo/b.md")));
        assert!(tools.hold(Path::new("D:/app/skins/icons/c.png")));
        assert!(!tools.hold(Path::new("D:/loose/d.md")));
        assert!(!tools.hold(Path::new("C:/Users/a/Downloads/e.pdf")));
    }

    /// The list read as Windows keeps the user's files: an ignored test that lists what it finds now (it reads this
    /// PC's drives, as the user).
    #[test]
    #[ignore = "reads this PC's drives"]
    fn lists_what_came_lately_here() {
        let places = windows_places();
        let now = crate::station::now_ms();
        let mut kept = Kept::default();
        let tools = Tools::new(&places, looked_at);
        for mut drive in os::drives() {
            let (_, changes) = drive.read((drive.next - (64 << 20)).max(drive.first)).expect("its journal read");
            for change in changes {
                kept.take(change, now, &places, &|path| left_out(path, &[]) || tools.hold(path), |file| drive.open_file(file), |path| std::fs::read_to_string(format!("{}:Zone.Identifier", path.display())).ok().and_then(|text| site_of(&text)));
            }
        }
        for came in kept.files.iter().take(40) {
            println!("{:?} {:?} {:?} {}", came.kind, came.site, came.path, crate::ui::text::since(came.came_ms, now, crate::ui::text::Lang::Zh));
        }
        assert!(!kept.files.is_empty());
    }
}
