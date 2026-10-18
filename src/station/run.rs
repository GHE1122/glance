//! The station's own thread: a single-threaded apartment with OLE (what dragging and dropping between programs needs,
//! which the windows' threads, in the multithreaded one, cannot do), its own message loop, the station's shelf and
//! folder held there alone. Other threads only ask it (`ask`) and read what it shows (`shown`), never its state: a
//! message there, a copy of what it holds back.
//!
//! What is copied is heard here (Recently copied), as the settings say: read as it changes, by its sequence number
//! (see `clipboard`), the clipboard read again in a moment where another program holds it.

use super::clipboard::{self, Busy, Content, Why};
use super::drop::Dragged;
use super::folder::Folder;
use super::keep::Keep;
use super::shelf::Item;
use std::sync::atomic::{AtomicIsize, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Arc, Mutex, OnceLock};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
use windows::Win32::System::DataExchange::{AddClipboardFormatListener, RemoveClipboardFormatListener};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, KillTimer, PostMessageW, RegisterClassW, SetTimer, HWND_MESSAGE, MSG, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_CLIPBOARDUPDATE, WM_TIMER,
    WNDCLASSW,
};

/// What the station is asked.
#[derive(Clone, Debug)]
pub enum Ask {
    /// What is on the clipboard put on it (Ctrl+V on it).
    Paste,
    /// What was dropped on it (see `drop`).
    Take(Content),
    Pin(u64, bool),
    Remove(u64),
    /// All that is not pinned taken off.
    Clear,
    /// What `id` holds put on the clipboard.
    Copy(u64),
    /// What `id` holds opened as Explorer opens it (a web address in the browser); shown in Explorer, selected there.
    Open(u64),
    Show(u64),
    /// The settings changed (see `settings::StationSettings`).
    Settings,
    /// Something dragged is over a station now, or no longer.
    Over(bool),
    /// What `id` holds dragged off it (pressed on it and moved: see `drag_from`).
    DragOut(u64),
}

/// What the station shows: what it holds, oldest first, and how it stands.
#[derive(Clone, Debug, Default)]
pub struct Shown {
    pub items: Vec<Item>,
    /// Recently copied on, and paused.
    pub history: bool,
    pub paused: bool,
    /// What was last given could not be taken: no room but what is pinned holds.
    pub full: bool,
    /// Something is dragged over a station, to be dropped.
    pub over: bool,
}

/// Its window's message: asked something (see `ask`).
const ASKED: u32 = WM_APP + 1;
/// Its timers: the clipboard read again (held by another program as it changed), what has stayed its days taken off.
const AGAIN: usize = 1;
const EXPIRE: usize = 2;
const WATCH: usize = 3;
/// How often it looks whether something is dragged (see `catch`, ms).
const WATCH_EVERY_MS: u32 = 100;
/// How soon the clipboard is read again, and how many times at most.
const AGAIN_AFTER_MS: u32 = 60;
const AGAIN_MOST: u32 = 8;
/// How often what has stayed its days is looked for (ms).
const EXPIRE_EVERY_MS: u32 = 60 * 60 * 1000;

static ASKS: OnceLock<Mutex<Sender<Ask>>> = OnceLock::new();
static WINDOW: AtomicIsize = AtomicIsize::new(0);
static SHOWN: Mutex<Option<Arc<Shown>>> = Mutex::new(None);
/// Where each window shows a station, as it was last drawn (screen px): where what is dragged over it may be dropped.
static AREAS: Mutex<Vec<(isize, windows::Win32::Foundation::RECT)>> = Mutex::new(Vec::new());

/// The window `window` shows a station in `areas` (screen px; none: none), as it was just drawn.
pub fn shows_at(window: HWND, areas: Vec<windows::Win32::Foundation::RECT>) {
    let mut held = AREAS.lock().unwrap();
    held.retain(|(of, _)| *of != window.0 as isize);
    held.extend(areas.into_iter().map(|area| (window.0 as isize, area)));
}

/// Where stations are shown (screen px), each window's, of windows still shown.
pub fn areas() -> Vec<windows::Win32::Foundation::RECT> {
    let mut held = AREAS.lock().unwrap();
    held.retain(|(of, _)| unsafe { windows::Win32::UI::WindowsAndMessaging::IsWindowVisible(HWND(*of as *mut _)) }.as_bool());
    held.iter().map(|(_, area)| *area).collect()
}

/// What a window catching drops saw (see `catch`; on the station's thread, perhaps as it does something already: so
/// told it by message): a drag over a strip along `edge` opens the station there, as a push into the edge does.
pub fn caught(dragged: Dragged, edge: Option<crate::settings::Edge>) {
    match dragged {
        Dragged::Over(at) => {
            if let Some(edge) = edge {
                crate::app().controller.pushed(edge, (at.x, at.y));
            }
            ask(Ask::Over(true));
        }
        Dragged::Gone => ask(Ask::Over(false)),
        Dragged::Dropped(given) => {
            for content in given {
                ask(Ask::Take(content));
            }
            ask(Ask::Over(false));
        }
    }
}

/// Called (on the station's thread) as what it shows changes: to wake what shows it.
static TOLD: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Starts the station's thread; `told` is called as what it shows changes.
pub fn start(told: impl Fn() + Send + Sync + 'static) {
    let _ = TOLD.set(Box::new(told));
    let (sender, asks) = std::sync::mpsc::channel();
    if ASKS.set(Mutex::new(sender)).is_err() {
        return;
    }
    std::thread::Builder::new().name("glance: station".into()).spawn(move || run(asks)).expect("a thread started");
}

/// Asks the station `ask` (done on its thread, soon).
pub fn ask(ask: Ask) {
    let Some(asks) = ASKS.get() else { return };
    if asks.lock().unwrap().send(ask).is_ok() {
        let window = WINDOW.load(Ordering::Acquire);
        if window != 0 {
            let _ = unsafe { PostMessageW(Some(HWND(window as *mut _)), ASKED, WPARAM(0), LPARAM(0)) };
        }
    }
}

/// What the station shows now.
pub fn shown() -> Arc<Shown> {
    SHOWN.lock().unwrap().clone().unwrap_or_default()
}

/// For pictures: as if the station held `items` (its thread not started).
#[cfg(all(test, feature = "studio"))]
pub fn pretend(items: Vec<Item>) {
    *SHOWN.lock().unwrap() = Some(Arc::new(Shown { items, history: true, paused: false, full: false, over: false }));
}

/// `given` opened as Explorer opens it: each file (a folder in Explorer), or a web address in the browser.
fn open(given: &Content) {
    let shell = |what: &std::ffi::OsStr| unsafe {
        windows::Win32::UI::Shell::ShellExecuteW(None, w!("open"), &windows::core::HSTRING::from(what), None, None, windows::Win32::UI::WindowsAndMessaging::SW_SHOWNORMAL);
    };
    match given {
        Content::Files(paths) => paths.iter().for_each(|path| shell(path.as_os_str())),
        Content::Text(text) if text.starts_with("https://") || text.starts_with("http://") => shell(std::ffi::OsStr::new(text)),
        _ => {}
    }
}

/// `paths` shown in Explorer, selected: each folder they are in opened with those of them in it selected.
fn show_in_explorer(paths: &[std::path::PathBuf]) {
    use windows::Win32::UI::Shell::{ILCreateFromPathW, ILFindLastID, ILFree, SHOpenFolderAndSelectItems};
    let mut folders: Vec<(std::path::PathBuf, Vec<&std::path::PathBuf>)> = Vec::new();
    for path in paths {
        let Some(parent) = path.parent() else { continue };
        match folders.iter_mut().find(|(folder, _)| folder == parent) {
            Some((_, inside)) => inside.push(path),
            None => folders.push((parent.to_path_buf(), vec![path])),
        }
    }
    for (folder, inside) in folders {
        unsafe {
            let held = ILCreateFromPathW(&windows::core::HSTRING::from(folder.as_os_str()));
            if held.is_null() {
                continue;
            }
            let items: Vec<_> = inside.iter().map(|path| ILCreateFromPathW(&windows::core::HSTRING::from(path.as_os_str()))).filter(|item| !item.is_null()).collect();
            let last: Vec<_> = items.iter().map(|item| ILFindLastID(*item) as *const _).collect();
            let _ = SHOpenFolderAndSelectItems(held, Some(&last), 0);
            items.into_iter().for_each(|item| ILFree(Some(item)));
            ILFree(Some(held));
        }
    }
}

/// The station on its thread.
struct Station {
    keep: Keep,
    window: HWND,
    asks: Receiver<Ask>,
    /// Listening to the clipboard.
    listening: bool,
    /// The clipboard's sequence number of the last copy taken, and of what the station put there itself.
    taken: u32,
    ours: u32,
    /// Times the clipboard was found held by another since it changed.
    tries: u32,
    /// Something dragged over a station, to be dropped (see `drop`).
    over: bool,
    /// Where drops are caught while something is dragged (see `catch`).
    catchers: super::catch::Catchers,
    /// What is to be dragged off it, asked (dragged once it is done answering: see `drag_off`).
    dragging: Option<u64>,
}

thread_local! {
    static STATION: std::cell::RefCell<Option<Station>> = const { std::cell::RefCell::new(None) };
}

fn run(asks: Receiver<Ask>) {
    unsafe {
        if windows::Win32::System::Ole::OleInitialize(None).is_err() {
            crate::journal::note("station: OLE could not be begun; no station");
            return;
        }
        let class = WNDCLASSW { lpfnWndProc: Some(procedure), lpszClassName: w!("GlanceStation"), ..Default::default() };
        RegisterClassW(&class);
        let Ok(window) = CreateWindowExW(WINDOW_EX_STYLE(0), class.lpszClassName, w!(""), WINDOW_STYLE(0), 0, 0, 0, 0, Some(HWND_MESSAGE), None, None, None) else {
            crate::journal::note("station: its window could not be made; no station");
            return;
        };
        let settings = crate::app().settings.lock().unwrap().station;
        let mut keep = Keep::open(Folder::own(), settings.rules(), settings.always_copy);
        keep.expire(super::now_ms());
        STATION.with_borrow_mut(|station| *station = Some(Station { keep, window, asks, listening: false, taken: 0, ours: 0, tries: 0, over: false, catchers: Default::default(), dragging: None }));
        WINDOW.store(window.0 as isize, Ordering::Release);
        let _ = SetTimer(Some(window), EXPIRE, EXPIRE_EVERY_MS, None);
        let _ = SetTimer(Some(window), WATCH, WATCH_EVERY_MS, None);
        with_station(|station| {
            station.follow_settings();
            station.answer();
        });
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            DispatchMessageW(&message);
        }
    }
}

/// What was asked to be dragged off the station, dragged (outside `with_station`: the drag's own loop passes the
/// station's messages on meanwhile, which find it free); its own files held meanwhile, and taken off after, where the
/// settings say so.
fn drag_off() {
    let mut given = None;
    with_station(|station| given = station.dragging.take().and_then(|id| station.drag_given(id)));
    let Some((id, content, stays_here, held)) = given else { return };
    let dragged = super::drag::drag(&content, stays_here);
    with_station(|station| {
        for name in &held {
            station.keep.folder.let_go(name);
        }
        let remove = crate::app().settings.lock().unwrap().station.remove_after_drag;
        if remove && dragged == Some(super::drag::Dragged::Taken) {
            station.keep.remove(id);
        }
        station.show();
    });
}

/// Where a drag off a station begun at `from` is at `now` (screen px): whether it has gone far enough to be one (the
/// system's distance for a drag).
pub fn drag_begins(from: windows::Win32::Foundation::POINT, now: windows::Win32::Foundation::POINT) -> bool {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXDRAG, SM_CYDRAG};
    let (w, h) = unsafe { (GetSystemMetrics(SM_CXDRAG), GetSystemMetrics(SM_CYDRAG)) };
    (now.x - from.x).abs() > w / 2 || (now.y - from.y).abs() > h / 2
}

/// Does `act` with the station; whether it could (not while it is acting already: a message sent to its window while
/// it acts, as the clipboard may send its own, is seen to after).
fn with_station(act: impl FnOnce(&mut Station)) -> bool {
    STATION.with(|station| match station.try_borrow_mut() {
        Ok(mut station) => {
            if let Some(station) = station.as_mut() {
                act(station);
            }
            true
        }
        Err(_) => false,
    })
}

unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        ASKED | WM_CLIPBOARDUPDATE | WM_TIMER => {
            let done = with_station(|station| match message {
                ASKED => station.answer(),
                WM_CLIPBOARDUPDATE => {
                    station.tries = 0;
                    station.copied();
                }
                _ if wparam.0 == AGAIN => {
                    let _ = unsafe { KillTimer(Some(window), AGAIN) };
                    station.copied();
                }
                _ if wparam.0 == WATCH => station.catchers.follow(),
                _ => {
                    station.keep.expire(super::now_ms());
                    station.show();
                }
            });
            if !done {
                let _ = unsafe { PostMessageW(Some(window), message, wparam, lparam) };
            }
            drag_off();
        }
        _ => return unsafe { DefWindowProcW(window, message, wparam, lparam) },
    }
    LRESULT(0)
}

impl Station {
    /// Does what it was asked, in turn.
    fn answer(&mut self) {
        while let Ok(ask) = self.asks.try_recv() {
            match ask {
                Ask::Paste => match clipboard::read(self.window, Why::Pasted) {
                    Ok(Some(copied)) => {
                        let _ = self.keep.take(copied.content, false, copied.stays_here, super::now_ms());
                    }
                    Ok(None) => {}
                    Err(Busy) => crate::journal::note("station: the clipboard was held by another program as it was pasted"),
                },
                Ask::Take(content) => {
                    let _ = self.keep.take(content, false, false, super::now_ms());
                }
                Ask::Pin(id, pinned) => self.keep.pin(id, pinned),
                Ask::Remove(id) => self.keep.remove(id),
                Ask::Clear => self.keep.clear(),
                Ask::Copy(id) => {
                    let given = self.keep.shelf.get(id).and_then(|item| Some((self.keep.content_of(item)?, item.stays_here)));
                    if let Some((content, stays_here)) = given {
                        if let Some(sequence) = clipboard::put(self.window, &content, stays_here) {
                            self.ours = sequence;
                        }
                    }
                }
                Ask::Open(id) => {
                    if let Some(given) = self.keep.shelf.get(id).and_then(|item| self.keep.content_of(item)) {
                        open(&given);
                    }
                }
                Ask::Show(id) => {
                    if let Some(Content::Files(paths)) = self.keep.shelf.get(id).and_then(|item| self.keep.content_of(item)) {
                        show_in_explorer(&paths);
                    }
                }
                Ask::Settings => self.follow_settings(),
                Ask::Over(over) => self.over = over,
                Ask::DragOut(id) => self.dragging = Some(id),
            }
        }
        self.show();
    }

    /// What a drag off it of `id` gives (see `drag_off`): its number, what it gives, whether it stays on this PC, and
    /// the files of its own it holds meanwhile.
    fn drag_given(&mut self, id: u64) -> Option<(u64, Content, bool, Vec<String>)> {
        let item = self.keep.shelf.get(id)?.clone();
        // A picture of its own dragged as its file (what Explorer and the chat programs take where it is dropped);
        // copied, it is the picture.
        let content = match &item.what {
            super::shelf::What::Picture { file } => Content::Files(vec![self.keep.folder.path(file)]),
            _ => self.keep.content_of(&item)?,
        };
        let held: Vec<String> = item.what.own_files().into_iter().map(str::to_string).collect();
        for name in &held {
            self.keep.folder.hold(name);
        }
        Some((id, content, item.stays_here, held))
    }

    /// The settings as they are now: the shelf's rules, and the clipboard listened to while Recently copied is on.
    fn follow_settings(&mut self) {
        let settings = crate::app().settings.lock().unwrap().station;
        self.keep.rules = settings.rules();
        self.keep.always_copy = settings.always_copy;
        let listen = settings.history;
        if listen != self.listening {
            let done = unsafe { if listen { AddClipboardFormatListener(self.window) } else { RemoveClipboardFormatListener(self.window) } };
            if done.is_ok() {
                self.listening = listen;
                // (What was copied before is no copy heard now.)
                self.taken = clipboard::sequence();
            }
        }
        // Rules made tighter: what is past them goes now.
        self.keep.expire(super::now_ms());
    }

    /// The clipboard changed (or was held by another as it did, and is read again): what is on it taken into Recently
    /// copied, unless it is paused, was taken already, or the station put it there itself.
    fn copied(&mut self) {
        let settings = crate::app().settings.lock().unwrap().station;
        if !settings.history || settings.paused {
            return;
        }
        let sequence = clipboard::sequence();
        if sequence == self.taken || sequence == self.ours {
            return;
        }
        match clipboard::read(self.window, Why::Changed) {
            Ok(copied) => {
                self.taken = sequence;
                // (What its copier asked kept out, or nothing the station keeps: nothing taken.)
                if let Some(copied) = copied.filter(|copied| copied.sequence != self.ours) {
                    self.taken = copied.sequence;
                    let _ = self.keep.take(copied.content, true, copied.stays_here, super::now_ms());
                    self.show();
                }
            }
            Err(Busy) => {
                self.tries += 1;
                if self.tries <= AGAIN_MOST {
                    let _ = unsafe { SetTimer(Some(self.window), AGAIN, AGAIN_AFTER_MS, None) };
                }
            }
        }
    }

    /// What it shows now, for the other threads, who are told.
    fn show(&self) {
        let settings = crate::app().settings.lock().unwrap().station;
        let shown = Shown { items: self.keep.shelf.items.clone(), history: settings.history, paused: settings.paused, full: self.keep.full, over: self.over };
        *SHOWN.lock().unwrap() = Some(Arc::new(shown));
        if let Some(told) = TOLD.get() {
            told();
        }
    }
}
