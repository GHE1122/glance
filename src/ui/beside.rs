//! The page beside the panel or a widget (see docs/station-and-notes.md, section 八): what opens from them and needs
//! more room than they have, laid out beside them on their inner side, as tall as they are:
//!
//! - quick launch's choosing of programs: every program the Start menu lists, searched by name or pinyin, each added
//!   or taken off at a press; what is not listed dropped on it, as on quick launch;
//! - a note written: its words in a box the text services write (see `editor`: the keys, the input method, selection,
//!   undo and the clipboard theirs), kept as they are typed (a moment after the last key), its colour chosen or it
//!   taken to the Recycle Bin from its menu.
//!
//! A window of its own thread (a single-threaded apartment with OLE, for the shell's pictures and what is dropped on
//! it), drawn as Glance draws, in the skin's colours. It takes the keyboard while it is up (words are typed into its
//! search); it goes as Esc is pressed, as its close is, or as another program's window is clicked (the panel, which
//! takes no focus, leaves it up: the panel and the page are one place). The panel stays open while it is up.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::mpsc::{Receiver, Sender};
use std::sync::{Mutex, OnceLock};

use windows::core::{w, HSTRING};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Dwm::{DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, InvalidateRect, MonitorFromRect, ValidateRect, LOGFONTW, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::Controls::MARGINS;
use windows::Win32::UI::Input::Ime::{ImmGetContext, ImmReleaseContext, ImmSetCandidateWindow, ImmSetCompositionFontW, ImmSetCompositionWindow, CANDIDATEFORM, CFS_EXCLUDE, CFS_POINT, COMPOSITIONFORM};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetKeyState, SetFocus, VIRTUAL_KEY, VK_BACK, VK_CONTROL, VK_DELETE, VK_END, VK_ESCAPE, VK_HOME, VK_LEFT, VK_RETURN, VK_RIGHT};
use windows::Win32::UI::WindowsAndMessaging::*;

use super::canvas::{Align, Canvas, Color, Point};
use super::gfx::{self, Frame, Gfx, Surface};
use super::icons::Icon;
use super::theme::{Skin, Theme};
use super::typing::Typing;
use crate::station::apps::App;
use crate::station::found::{self, Found};

/// What the page shows.
#[derive(Clone, PartialEq, Eq, Debug)]
pub enum Page {
    /// Quick launch's choosing of programs.
    Picker,
    /// A note, by its file's name in the notes' folder (none: a new one, named by its title as it is first kept).
    Note(Option<String>),
}

/// Where the page goes: beside `by` (screen px: the panel or the widget it opens from), on the side `side` (away from
/// the screen's edge it is at), in the skin and its light or dark.
pub struct Opening {
    pub page: Page,
    pub by: RECT,
    pub side: Side,
    pub skin: Skin,
    pub dark: bool,
}

/// Which side of what it opens from the page goes on, where there is room: left of it, right of it, or under it (the
/// top edge's panel).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Side {
    Left,
    Right,
    Below,
}

/// How wide it is (DIPs); its head, its search box, a row, a letter's heading, its foot; the room round its parts.
const WIDTH: f32 = 360.0;
const HEAD: f32 = 52.0;
const SEARCH: f32 = 34.0;
const ROW: f32 = 40.0;
const LETTER: f32 = 26.0;
const FOOT: f32 = 48.0;
const PAD: f32 = 16.0;
/// The gap between it and what it opens from (DIPs).
const GAP: f32 = 8.0;
/// How long a search may be.
const SEARCH_MOST: usize = 64;

/// What is asked of its thread.
enum Ask {
    Open(Opening),
    Close,
}

static ASKS: OnceLock<Mutex<Sender<Ask>>> = OnceLock::new();
/// Its thread's window for asks (message-only), and whether a page is up.
static POST: AtomicIsize = AtomicIsize::new(0);
static UP: AtomicBool = AtomicBool::new(false);
/// Its message: something asked.
const ASKED: u32 = WM_APP + 7;
/// Its timers: the caret's blink, drawn again while pictures are being made, and a note kept a moment after it was
/// last written in (the text services' own timers are numbered past these).
const BLINK: usize = 1;
const AGAIN: usize = 2;
const KEEP: usize = 3;
/// How long after the last key a note is kept.
const KEEP_AFTER_MS: u32 = 600;

/// Whether the page is up.
pub fn is_up() -> bool {
    UP.load(Ordering::Acquire)
}

/// The page `opening` asks for opened (another up goes), on its thread (started when first asked).
pub fn open(opening: Opening) {
    ask(Ask::Open(opening));
}

/// The page up, gone.
pub fn close() {
    if is_up() {
        ask(Ask::Close);
    }
}

fn ask(ask: Ask) {
    let sender = ASKS.get_or_init(|| {
        let (sender, asks) = std::sync::mpsc::channel();
        std::thread::Builder::new().name("glance: beside".into()).spawn(move || run(asks)).expect("a thread started");
        Mutex::new(sender)
    });
    if sender.lock().unwrap().send(ask).is_ok() {
        let post = POST.load(Ordering::Acquire);
        if post != 0 {
            let _ = unsafe { PostMessageW(Some(HWND(post as *mut _)), ASKED, WPARAM(0), LPARAM(0)) };
        }
    }
}

/// The page on its thread: its window, how it is drawn, and what it shows.
struct Beside {
    hwnd: HWND,
    gfx: Rc<Gfx>,
    surface: Option<Surface>,
    _showing: super::life::Showing,
    scale: f32,
    theme: Rc<Theme>,
    lang: super::text::Lang,
    page: Page,
    search: Typing,
    /// How far its list is scrolled, and the most it can be.
    scroll: f32,
    scroll_most: f32,
    pointer: Option<(f32, f32)>,
    hits: Vec<(f32, f32, f32, f32, Hit)>,
    /// Where the caret was drawn (DIPs: its left, its top, its height), for the input method's words to go there.
    caret: Option<(f32, f32, f32)>,
    caret_on: bool,
    /// A picture was not ready as it was last drawn: drawn again soon.
    waiting: bool,
    /// The note written, on a note's page.
    writing: Option<Writing>,
}

/// A note as it is written: its file's name (none till it is first kept), its file as last read or written here, its
/// colour, its words' box, whether what is written is not yet kept, and what was last said of keeping it.
struct Writing {
    name: Option<String>,
    stamp: Option<crate::notes::store::Stamp>,
    colour: crate::notes::Colour,
    written_ms: u64,
    editor: super::editor::Editor,
    unkept: bool,
    said: Option<String>,
}

/// What a note's menu does: its colour chosen, it taken to the Recycle Bin.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum NoteChoice {
    Colour(crate::notes::Colour),
    TakeAway,
}

thread_local! {
    /// A note's menu up, with what each row does.
    static MENU: RefCell<Option<(super::menu::Menu, Vec<NoteChoice>)>> = const { RefCell::new(None) };
}

/// What a point on the page is on.
#[derive(Clone, PartialEq, Debug)]
enum Hit {
    Close,
    Search,
    /// A note's menu.
    NoteMenu,
    /// A program found, by what opens it: added, or taken off where it is kept already.
    Program(String, String),
}

thread_local! {
    static BESIDE: RefCell<Option<Beside>> = const { RefCell::new(None) };
}

fn with<R>(act: impl FnOnce(&mut Beside) -> R) -> Option<R> {
    BESIDE.with(|cell| cell.try_borrow_mut().ok().and_then(|mut beside| beside.as_mut().map(act)))
}

fn run(asks: Receiver<Ask>) {
    unsafe {
        let _ = windows::Win32::System::Ole::OleInitialize(None);
        let class = WNDCLASSW { lpfnWndProc: Some(procedure), lpszClassName: w!("GlanceBesideAsks"), ..Default::default() };
        RegisterClassW(&class);
        let Ok(post) = CreateWindowExW(WINDOW_EX_STYLE(0), class.lpszClassName, w!(""), WINDOW_STYLE(0), 0, 0, 0, 0, Some(HWND_MESSAGE), None, None, None) else { return };
        POST.store(post.0 as isize, Ordering::Release);
        let page = WNDCLASSW { style: CS_DBLCLKS, lpfnWndProc: Some(procedure), lpszClassName: w!("GlanceBeside"), hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(), ..Default::default() };
        RegisterClassW(&page);
        // (Asks sent before the window was there.)
        let _ = PostMessageW(Some(post), ASKED, WPARAM(0), LPARAM(0));
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            if message.hwnd == post && message.message == ASKED {
                while let Ok(ask) = asks.try_recv() {
                    match ask {
                        Ask::Open(opening) => show(opening),
                        Ask::Close => gone(),
                    }
                }
                continue;
            }
            // A note's menu up: its own messages its, then what was chosen done.
            let handled = MENU.with(|menu| menu.borrow_mut().as_mut().map(|(open, _)| open.handle(&message)));
            match handled {
                Some(super::menu::Handled::Taken) => continue,
                Some(super::menu::Handled::Done(choice)) => {
                    if let Some((open, choices)) = MENU.with(|menu| menu.borrow_mut().take()) {
                        open.close();
                        if let Some(choice) = choice.and_then(|row| choices.get(row).copied()) {
                            let gone_now = with(|beside| beside.note_chose(choice)).unwrap_or(false);
                            if gone_now {
                                gone();
                            }
                        }
                    }
                    continue;
                }
                _ => {}
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
        }
    }
}

/// The page `opening` asks for, up beside what it opens from.
fn show(opening: Opening) {
    gone();
    let Ok(gfx) = gfx::current() else { return };
    let showing = super::life::Showing::of(&gfx);
    unsafe {
        let mut screen = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(MonitorFromRect(&opening.by, MONITOR_DEFAULTTONEAREST), &mut screen);
        let room = screen.rcWork;
        let monitor = MonitorFromRect(&opening.by, MONITOR_DEFAULTTONEAREST);
        let (mut dpi, mut unused) = (96u32, 96u32);
        let _ = windows::Win32::UI::HiDpi::GetDpiForMonitor(monitor, windows::Win32::UI::HiDpi::MDT_EFFECTIVE_DPI, &mut dpi, &mut unused);
        let scale = dpi.max(96) as f32 / 96.0;
        let (wide, gap) = ((WIDTH * scale).round() as i32, (GAP * scale).round() as i32);
        let least = (480.0 * scale) as i32;
        // Beside it on its side away from the edge, else on the other, else over it; under the top edge's panel, as
        // tall as the room under it is.
        let left_of = opening.by.left - gap - wide;
        let right_of = opening.by.right + gap;
        let (x, y, tall) = match opening.side {
            Side::Below => {
                let tall = (room.bottom - opening.by.bottom - gap - gap).max(least).min(room.bottom - room.top);
                ((opening.by.left + opening.by.right - wide) / 2, opening.by.bottom + gap, tall)
            }
            side => {
                let tall = (opening.by.bottom - opening.by.top).max(least).min(room.bottom - room.top);
                let x = match (side == Side::Left, left_of >= room.left, right_of + wide <= room.right) {
                    (true, true, _) | (false, true, false) => left_of,
                    (false, _, true) | (true, false, true) => right_of,
                    _ => (opening.by.left + opening.by.right - wide) / 2,
                };
                (x, opening.by.top, tall)
            }
        };
        let x = x.clamp(room.left, room.right - wide);
        let y = y.clamp(room.top, room.bottom - tall);
        let Ok(hwnd) = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_TOPMOST | WS_EX_NOREDIRECTIONBITMAP, w!("GlanceBeside"), &HSTRING::from("Glance"), WS_POPUP, x, y, wide, tall, None, None, None, None) else { return };
        let _ = DwmExtendFrameIntoClientArea(hwnd, &MARGINS { cxLeftWidth: 1, cxRightWidth: 1, cyTopHeight: 1, cyBottomHeight: 1 });
        let round = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(hwnd, DWMWA_WINDOW_CORNER_PREFERENCE, &round as *const _ as *const _, size_of_val(&round) as u32);
        let beside = Beside {
            hwnd,
            gfx,
            surface: None,
            _showing: showing,
            scale,
            theme: Rc::new(Theme::new(opening.skin, opening.dark)),
            lang: crate::update::language(),
            page: opening.page,
            search: Typing::most(SEARCH_MOST),
            scroll: 0.0,
            scroll_most: 0.0,
            pointer: None,
            hits: Vec::new(),
            caret: None,
            caret_on: true,
            waiting: false,
            writing: None,
        };
        let page = beside.page.clone();
        let theme = beside.theme.clone();
        BESIDE.with(|cell| *cell.borrow_mut() = Some(beside));
        // A note: its words read, and a box for them where the page writes them.
        if let Page::Note(name) = page {
            let folder = crate::notes::folder();
            let read = name.as_ref().and_then(|name| folder.read(name));
            let shown = crate::notes::shown();
            let colour = match &name {
                Some(name) => shown.iter().find(|note| &note.name == name).map(|note| note.colour).unwrap_or_default(),
                // (A new note takes the next colour round.)
                None => crate::notes::Colour::ALL[shown.len() % crate::notes::Colour::ALL.len()],
            };
            let rect = RECT { left: (PAD * scale) as i32, top: ((HEAD + 4.0) * scale) as i32, right: wide - (PAD * scale) as i32, bottom: tall - (FOOT * scale) as i32 };
            let ink = theme.text;
            let color = windows::Win32::Foundation::COLORREF(((ink.b * 255.0) as u32) << 16 | ((ink.g * 255.0) as u32) << 8 | (ink.r * 255.0) as u32);
            if let Some(editor) = super::editor::Editor::new(hwnd, rect, "Microsoft YaHei UI", 15.0 * scale, color) {
                if let Some((text, _)) = &read {
                    editor.set_text(text);
                }
                let _ = editor.send(WM_SETFOCUS, WPARAM(0), LPARAM(0));
                let written_ms = read.as_ref().map_or_else(crate::station::now_ms, |(_, stamp)| stamp.written_ms);
                let writing = Writing { name, stamp: read.map(|(_, stamp)| stamp), colour, written_ms, editor, unkept: false, said: None };
                with(|beside| beside.writing = Some(writing));
            }
        }
        UP.store(true, Ordering::Release);
        // The Start menu read (again): drawn once it is.
        let told = HWND(hwnd.0);
        let told = told.0 as isize;
        found::look(move || {
            let _ = PostMessageW(Some(HWND(told as *mut _)), WM_TIMER, WPARAM(AGAIN), LPARAM(0));
        });
        // What is dropped on it kept at hand, as on quick launch.
        let target = crate::station::drop::Target::new(|dragged| crate::station::run::caught(dragged, None, crate::station::run::Lands::Apps));
        let _ = windows::Win32::System::Ole::RegisterDragDrop(hwnd, &target);
        DROP_TARGET.with(|held| *held.borrow_mut() = Some(target));
        let _ = SetTimer(Some(hwnd), BLINK, windows::Win32::UI::WindowsAndMessaging::GetCaretBlinkTime(), None);
        let _ = ShowWindow(hwnd, SW_SHOW);
        let _ = SetForegroundWindow(hwnd);
        let _ = SetFocus(Some(hwnd));
        with(Beside::draw);
    }
}

thread_local! {
    /// What takes drops on the page, while it is up.
    static DROP_TARGET: RefCell<Option<windows::Win32::System::Ole::IDropTarget>> = const { RefCell::new(None) };
}

/// The page up, gone (a note on it kept first).
fn gone() {
    with(Beside::keep);
    if let Some((open, _)) = MENU.with(|menu| menu.borrow_mut().take()) {
        open.close();
    }
    if let Some(beside) = BESIDE.with(|cell| cell.borrow_mut().take()) {
        unsafe {
            let _ = windows::Win32::System::Ole::RevokeDragDrop(beside.hwnd);
            let _ = DestroyWindow(beside.hwnd);
        }
        DROP_TARGET.with(|held| held.borrow_mut().take());
    }
    UP.store(false, Ordering::Release);
}

unsafe extern "system" fn procedure(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let point = || ((lparam.0 & 0xFFFF) as i16 as f32, ((lparam.0 >> 16) & 0xFFFF) as i16 as f32);
    // A note's words' box: the keys, the input method's, what the pointer does over it, its own timers.
    if let Some(answered) = with(|beside| beside.write(message, wparam, lparam)).flatten() {
        return answered;
    }
    match message {
        WM_PAINT => {
            with(Beside::draw);
            let _ = unsafe { ValidateRect(Some(hwnd), None) };
        }
        WM_MOUSEMOVE => {
            let (x, y) = point();
            with(|beside| {
                beside.pointer = Some((x / beside.scale, y / beside.scale));
                beside.redraw();
            });
        }
        WM_LBUTTONUP => {
            let (x, y) = point();
            let close = with(|beside| beside.press(x / beside.scale, y / beside.scale)).unwrap_or(false);
            if close {
                gone();
            }
        }
        WM_MOUSEWHEEL => {
            let turns = ((wparam.0 >> 16) & 0xFFFF) as u16 as i16 as f32 / 120.0;
            with(|beside| {
                beside.scroll = (beside.scroll - turns * 3.0 * ROW).clamp(0.0, beside.scroll_most);
                beside.redraw();
            });
        }
        WM_KEYDOWN => {
            let key = VIRTUAL_KEY(wparam.0 as u16);
            if key == VK_ESCAPE {
                gone();
            } else {
                with(|beside| beside.key(key));
            }
        }
        WM_CHAR => {
            with(|beside| beside.char(wparam.0 as u16));
        }
        WM_TIMER => {
            with(|beside| {
                match wparam.0 {
                    BLINK => beside.caret_on = !beside.caret_on,
                    KEEP => {
                        let _ = unsafe { KillTimer(Some(hwnd), KEEP) };
                        beside.keep();
                    }
                    _ => {}
                }
                beside.redraw();
            });
        }
        WM_IME_STARTCOMPOSITION => {
            with(|beside| beside.place_ime());
            return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) };
        }
        // Another program's window come in front: the page goes (the panel takes no focus, and leaves it up; its own
        // menu leaves it up too).
        WM_ACTIVATE if (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE => {
            let coming = HWND(lparam.0 as *mut _);
            let ours = !coming.is_invalid() && unsafe { GetWindowThreadProcessId(coming, None) } == unsafe { windows::Win32::System::Threading::GetCurrentThreadId() };
            if !ours {
                gone();
            }
        }
        _ => return unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
    LRESULT(0)
}

impl Beside {
    fn redraw(&self) {
        let _ = unsafe { InvalidateRect(Some(self.hwnd), None, false) };
    }

    /// A message for a note's words' box, given it where it is one of its: what it answers (none: the page's own).
    fn write(&mut self, message: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        let writing = self.writing.as_mut()?;
        let point = POINT { x: (lparam.0 & 0xFFFF) as i16 as i32, y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32 };
        let its = match message {
            // (Esc leaves the page; the rest of the keys and the input method's are the box's.)
            WM_KEYDOWN => wparam.0 != usize::from(VK_ESCAPE.0),
            WM_KEYUP | WM_CHAR | WM_SYSCHAR | WM_UNICHAR | WM_SETFOCUS | WM_KILLFOCUS => true,
            WM_IME_STARTCOMPOSITION | WM_IME_COMPOSITION | WM_IME_ENDCOMPOSITION | WM_IME_NOTIFY | WM_IME_SETCONTEXT | WM_IME_CHAR | WM_IME_REQUEST | WM_IME_COMPOSITIONFULL | WM_IME_SELECT => true,
            WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_RBUTTONDOWN => writing.editor.holds(point),
            // (A press begun in it is followed till let go, wherever the pointer goes.)
            WM_LBUTTONUP | WM_MOUSEMOVE => writing.editor.holds(point) || unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetCapture() } == self.hwnd,
            WM_SETCURSOR => {
                let mut at = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut at);
                    let _ = windows::Win32::Graphics::Gdi::ScreenToClient(self.hwnd, &mut at);
                }
                writing.editor.holds(at)
            }
            WM_MOUSEWHEEL => true,
            WM_TIMER => !matches!(wparam.0, BLINK | AGAIN | KEEP),
            _ => false,
        };
        if !its {
            return None;
        }
        let answered = writing.editor.send(message, wparam, lparam);
        if writing.editor.take_changed() {
            writing.unkept = true;
            writing.said = None;
            unsafe {
                let _ = SetTimer(Some(self.hwnd), KEEP, KEEP_AFTER_MS, None);
            }
        }
        self.caret_on = true;
        self.redraw();
        // (The input method's, unanswered by the box, are the system's as well.)
        match (answered, message) {
            (None, WM_IME_STARTCOMPOSITION | WM_IME_COMPOSITION | WM_IME_ENDCOMPOSITION | WM_IME_NOTIFY | WM_IME_SETCONTEXT | WM_IME_CHAR | WM_IME_REQUEST | WM_SETCURSOR) => None,
            (answered, _) => Some(answered.unwrap_or(LRESULT(0))),
        }
    }

    /// The note written kept in its file, where what is written is not kept yet (see `notes::Folder::write`): a new
    /// one named by its title as it is first kept (an empty one never), one changed outside meanwhile kept apart.
    fn keep(&mut self) {
        let lang = self.lang;
        let Some(writing) = self.writing.as_mut().filter(|writing| writing.unkept) else { return };
        let text = writing.editor.text();
        let folder = crate::notes::folder();
        let name = match &writing.name {
            Some(name) => name.clone(),
            None if text.trim().is_empty() => {
                writing.unkept = false;
                return;
            }
            None => {
                let title = crate::notes::Note { name: String::new(), text: text.clone(), colour: writing.colour, stamp: Default::default() }.title().to_string();
                let today = super::text::Local::at(crate::station::now_ms());
                let named = folder.name_for(&title, &match lang {
                    super::text::Lang::Zh => format!("便签 {}月{}日", today.month, today.day),
                    super::text::Lang::En => format!("Note {}-{}", today.month, today.day),
                });
                let _ = folder.colour(&named, writing.colour);
                named
            }
        };
        match folder.write(&name, &text, writing.stamp) {
            Ok(crate::notes::Written::Done(stamp)) => {
                writing.name = Some(name);
                writing.stamp = Some(stamp);
                writing.written_ms = stamp.written_ms;
            }
            Ok(crate::notes::Written::Apart(apart)) => {
                writing.said = Some(match lang {
                    super::text::Lang::Zh => format!("这条便签在别处被改过，你写的已另存为“{apart}”"),
                    super::text::Lang::En => format!("Changed elsewhere meanwhile: what you wrote is kept as \"{apart}\""),
                });
                writing.stamp = folder.stamp(&apart);
                writing.name = Some(apart);
            }
            Err(_) => writing.said = Some(lang.pick("没能保存，请检查便签文件夹", "Not kept: see the notes' folder").to_string()),
        }
        writing.unkept = false;
        crate::notes::read_again();
    }

    /// The note's menu, at its button.
    fn note_menu(&self, at: POINT) {
        use crate::notes::Colour;
        use super::menu::{Item, Menu};
        let Some(writing) = &self.writing else { return };
        let lang = self.lang;
        let names = [(Colour::Blue, lang.pick("蓝色", "Blue")), (Colour::Yellow, lang.pick("黄色", "Yellow")), (Colour::Green, lang.pick("绿色", "Green")), (Colour::Pink, lang.pick("粉色", "Pink")), (Colour::Purple, lang.pick("紫色", "Purple")), (Colour::Grey, lang.pick("灰色", "Grey"))];
        let mut rows: Vec<(NoteChoice, &'static str, Icon, bool, bool)> = names.iter().map(|(colour, name)| (NoteChoice::Colour(*colour), *name, Icon::Image, writing.colour == *colour, false)).collect();
        if writing.name.is_some() {
            rows.push((NoteChoice::TakeAway, lang.pick("删除（移到回收站）", "Delete (to the Recycle Bin)"), Icon::Close, false, true));
        }
        let items = rows.iter().map(|(_, label, icon, checked, rule_before)| Item { label, icon: Some(*icon), checked: *checked, rule_before: *rule_before }).collect();
        if let Some(menu) = Menu::open_at(items, self.theme.dark, at, false, false) {
            MENU.with(|held| *held.borrow_mut() = Some((menu, rows.into_iter().map(|(choice, ..)| choice).collect())));
        }
    }

    /// `choice` made in the note's menu: done; whether the page is to go.
    fn note_chose(&mut self, choice: NoteChoice) -> bool {
        self.keep();
        let Some(writing) = self.writing.as_mut() else { return false };
        let folder = crate::notes::folder();
        match choice {
            NoteChoice::Colour(colour) => {
                writing.colour = colour;
                if let Some(name) = &writing.name {
                    let _ = folder.colour(name, colour);
                }
                crate::notes::read_again();
                self.redraw();
                false
            }
            NoteChoice::TakeAway => {
                let taken = writing.name.as_ref().is_some_and(|name| folder.take_away(name));
                if taken {
                    writing.unkept = false;
                    crate::notes::read_again();
                }
                taken
            }
        }
    }

    /// The programs it lists now: those the search asks for, in the Start menu's order; none till it has been read.
    fn listed(&self) -> Vec<Found> {
        let query = self.search.string();
        found::all().map(|all| all.iter().filter(|found| found::matches(found, &query)).cloned().collect()).unwrap_or_default()
    }

    /// A press let go at (`x`, `y`) DIPs: done; whether the page is to go.
    fn press(&mut self, x: f32, y: f32) -> bool {
        let Some(hit) = self.hits.iter().find(|(hx, hy, w, h, _)| x >= *hx && x < hx + w && y >= *hy && y < hy + h).map(|hit| hit.4.clone()) else { return false };
        match hit {
            Hit::Close => return true,
            Hit::Search => {}
            Hit::NoteMenu => {
                let mut at = POINT { x: ((x - 120.0) * self.scale) as i32, y: ((y + 14.0) * self.scale) as i32 };
                unsafe {
                    let _ = windows::Win32::Graphics::Gdi::ClientToScreen(self.hwnd, &mut at);
                }
                self.note_menu(at);
            }
            Hit::Program(target, name) => {
                let app = App { target, name };
                crate::station::apps::change(move |apps| {
                    match apps.iter().position(|kept| kept.target == app.target) {
                        Some(at) => {
                            apps.remove(at);
                        }
                        None => apps.push(app),
                    }
                });
                // (Drawn again once the settings have it.)
                unsafe {
                    let _ = SetTimer(Some(self.hwnd), AGAIN, 120, None);
                }
            }
        }
        self.redraw();
        false
    }

    fn key(&mut self, key: VIRTUAL_KEY) {
        let ctrl = unsafe { GetKeyState(i32::from(VK_CONTROL.0)) } < 0;
        match key {
            VK_LEFT => self.search.go(self.search.step(self.search.caret, false), false),
            VK_RIGHT => self.search.go(self.search.step(self.search.caret, true), false),
            VK_HOME => self.search.go(0, false),
            VK_END => self.search.go(self.search.text.len(), false),
            VK_BACK => self.search.erase(false),
            VK_DELETE => self.search.erase(true),
            // Enter: the first one found added.
            VK_RETURN => {
                if let Some(first) = self.listed().into_iter().next() {
                    let kept = crate::station::apps::shown().iter().any(|app| app.target == first.target);
                    if !kept {
                        let app = App { target: first.target, name: first.name };
                        crate::station::apps::change(move |apps| apps.push(app));
                    }
                }
            }
            _ if ctrl && key.0 == u16::from(b'A') => {
                self.search.go(0, false);
                self.search.go(self.search.text.len(), true);
            }
            _ => return,
        }
        self.scroll = 0.0;
        self.caret_on = true;
        self.redraw();
    }

    fn char(&mut self, unit: u16) {
        // (Control characters are keys, seen to in `key`.)
        if unit < 0x20 || unit == 0x7F {
            return;
        }
        self.search.put(&[unit]);
        self.scroll = 0.0;
        self.caret_on = true;
        self.redraw();
    }

    /// The input method's words and choices put at the caret.
    fn place_ime(&self) {
        let Some((x, y, h)) = self.caret else { return };
        let px = |dips: f32| (dips * self.scale).round() as i32;
        unsafe {
            let context = ImmGetContext(self.hwnd);
            if context.is_invalid() {
                return;
            }
            let _ = ImmSetCompositionWindow(context, &COMPOSITIONFORM { dwStyle: CFS_POINT, ptCurrentPos: POINT { x: px(x), y: px(y) }, ..Default::default() });
            let around = RECT { left: px(x), top: px(y), right: px(x) + 1, bottom: px(y + h) };
            let _ = ImmSetCandidateWindow(context, &CANDIDATEFORM { dwIndex: 0, dwStyle: CFS_EXCLUDE, ptCurrentPos: POINT { x: around.left, y: around.bottom }, rcArea: around });
            let mut face = LOGFONTW { lfHeight: -px(self.theme.body.size), lfWeight: 400, lfCharSet: windows::Win32::Graphics::Gdi::DEFAULT_CHARSET, ..Default::default() };
            for (unit, to) in "Microsoft YaHei UI".encode_utf16().zip(face.lfFaceName.iter_mut()) {
                *to = unit;
            }
            let _ = ImmSetCompositionFontW(context, &face);
            let _ = ImmReleaseContext(self.hwnd, context);
        }
    }

    fn draw(&mut self) {
        let mut inside = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut inside);
        }
        let size = (inside.right.max(0) as u32, inside.bottom.max(0) as u32);
        if size.0 == 0 || size.1 == 0 {
            return;
        }
        if self.surface.is_none() {
            if let Ok(gfx) = gfx::current() {
                self.gfx = gfx;
            }
            self.surface = Surface::new(&self.gfx, self.hwnd).ok();
        }
        let Some(mut surface) = self.surface.take() else { return };
        let gfx = self.gfx.clone();
        let area = (size.0 as f32 / self.scale, size.1 as f32 / self.scale);
        if surface.draw(&gfx, size, self.scale, |frame| self.paint(frame, area)).is_ok() {
            self.surface = Some(surface);
        } else {
            gfx::lost();
        }
        self.place_ime();
        // Pictures still being made: drawn again soon; else not.
        unsafe {
            if self.waiting {
                let _ = SetTimer(Some(self.hwnd), AGAIN, 250, None);
            } else {
                let _ = KillTimer(Some(self.hwnd), AGAIN);
            }
        }
    }

    /// Its ground, in the skin's colours: chart paper's paper; the others a flyout's, light or dark.
    fn ground(&self) -> Color {
        match (self.theme.skin, self.theme.dark) {
            (Skin::Paper, _) => self.theme.paper,
            (_, true) => Color::hex(0x2C2C2C, 1.0),
            (_, false) => Color::hex(0xF9F9F9, 1.0),
        }
    }

    /// The colour of what is chosen and of the one thing to press: Windows 11's accent, chart paper's ink.
    fn accent(&self) -> Color {
        match self.theme.skin {
            Skin::Paper => self.theme.text,
            _ => super::theme::accent(self.theme.dark),
        }
    }

    fn paint(&mut self, frame: &Frame, (width, height): (f32, f32)) {
        match self.page {
            Page::Picker => self.paint_picker(frame, (width, height)),
            Page::Note(_) => self.paint_note(frame, (width, height)),
        }
    }

    /// A note's page: its colour, when it was written, its menu and its close over its words; under them how many
    /// there are and where it is kept (or what was said of keeping it).
    fn paint_note(&mut self, frame: &Frame, (width, height): (f32, f32)) {
        let theme = self.theme.clone();
        let lang = self.lang;
        let mut hits = Vec::new();
        let lit = |at: (f32, f32, f32, f32), pointer: Option<(f32, f32)>| pointer.is_some_and(|(x, y)| x >= at.0 && x < at.0 + at.2 && y >= at.1 && y < at.1 + at.3);
        frame.fill(self.ground(), 0.0, 0.0, width, height);
        let Some(writing) = &self.writing else {
            frame.text(lang.pick("没能打开编辑器", "The editor could not be opened"), theme.body, theme.text2, PAD, PAD, width - 2.0 * PAD, Align::Start);
            self.hits = hits;
            return;
        };
        let (mark, _) = super::view::note_colours(&theme, writing.colour);
        // Its colour over all of it, faintly; its mark by when it was written.
        frame.fill(mark.alpha(if theme.dark { 0.10 } else { 0.08 }), 0.0, 0.0, width, height);
        frame.fill_circle(mark, Point { x: PAD + 5.0, y: 26.0 }, 5.0);
        let when = super::text::since(writing.written_ms, crate::station::now_ms(), lang);
        let said = match (&writing.name, lang) {
            (None, super::text::Lang::Zh) => "新便签".to_string(),
            (None, super::text::Lang::En) => "New note".to_string(),
            (Some(_), super::text::Lang::Zh) => format!("{when} 修改"),
            (Some(_), super::text::Lang::En) => format!("Written {when}"),
        };
        frame.text(&said, theme.small, theme.text2, PAD + 18.0, 18.0, width - 2.0 * PAD - 80.0, Align::Start);
        for (at, hit, icon) in [(width - PAD - 28.0, Hit::Close, Icon::Close), (width - PAD - 60.0, Hit::NoteMenu, Icon::Ellipsis)] {
            let place = (at, 12.0, 28.0, 28.0);
            if lit(place, self.pointer) {
                frame.fill_rounded(theme.hover, place.0, place.1, place.2, place.3, theme.control_radius.min(14.0));
            }
            frame.icon(icon, Point { x: place.0 + 14.0, y: place.1 + 14.0 }, 16.0, theme.text2);
            hits.push((place.0, place.1, place.2, place.3, hit));
        }
        // Its words, as the text services draw them, and the caret where they say it is.
        let editor = &writing.editor;
        frame.in_pixels(|target| editor.draw(target));
        if let (Some((x, y, w, h)), true) = (editor.caret(), self.caret_on) {
            let scale = self.scale;
            frame.fill(theme.text, x as f32 / scale, y as f32 / scale, (w as f32 / scale).max(1.2), h as f32 / scale);
        }
        // Its foot: how many characters; kept as it is written, and where (or what was said of it).
        let bottom = height - FOOT;
        frame.fill(theme.rule, PAD, bottom, width - 2.0 * PAD, 1.0);
        let text = editor.text();
        let count = text.chars().filter(|c| !c.is_whitespace()).count();
        let counted = match lang {
            super::text::Lang::Zh => format!("{count} 字 · 自动保存"),
            super::text::Lang::En => format!("{count} characters · kept as you write"),
        };
        frame.text(&counted, theme.small, theme.text3, PAD, bottom + (FOOT - 16.0) / 2.0, width - 2.0 * PAD, Align::Start);
        let place = match (&writing.said, &writing.name) {
            (Some(said), _) => said.clone(),
            (None, Some(name)) => name.clone(),
            (None, None) => String::new(),
        };
        frame.text(&place, theme.small, if writing.said.is_some() { theme.signal } else { theme.text3 }, PAD, bottom + (FOOT - 16.0) / 2.0, width - 2.0 * PAD, Align::End);
        self.hits = hits;
    }

    fn paint_picker(&mut self, frame: &Frame, (width, height): (f32, f32)) {
        let theme = self.theme.clone();
        let lang = self.lang;
        let mut hits = Vec::new();
        let lit = |hits_at: (f32, f32, f32, f32), pointer: Option<(f32, f32)>| pointer.is_some_and(|(x, y)| x >= hits_at.0 && x < hits_at.0 + hits_at.2 && y >= hits_at.1 && y < hits_at.1 + hits_at.3);
        frame.fill(self.ground(), 0.0, 0.0, width, height);
        // Its head: what it is, how many are kept, and its close.
        let kept = crate::station::apps::shown();
        let inner = width - 2.0 * PAD;
        frame.text(lang.pick("添加程序", "Add programs"), theme.title, theme.text, PAD, 16.0, inner, Align::Start);
        let said = match lang {
            super::text::Lang::Zh => format!("已添加 {} 个", kept.len()),
            super::text::Lang::En => format!("{} kept at hand", kept.len()),
        };
        let title_w = frame.measure(lang.pick("添加程序", "Add programs"), theme.title);
        frame.text(&said, theme.small, theme.text3, PAD + title_w + 10.0, 19.0, inner - title_w - 40.0, Align::Start);
        let close = (width - PAD - 28.0, 12.0, 28.0, 28.0);
        if lit(close, self.pointer) {
            frame.fill_rounded(theme.hover, close.0, close.1, close.2, close.3, theme.control_radius.min(14.0));
        }
        frame.icon(Icon::Close, Point { x: close.0 + 14.0, y: close.1 + 14.0 }, 16.0, theme.text2);
        hits.push((close.0, close.1, close.2, close.3, Hit::Close));
        // The search, its caret blinking.
        let field = (PAD, HEAD, inner, SEARCH);
        frame.fill_rounded(theme.track, field.0, field.1, field.2, field.3, theme.control_radius.max(4.0).min(SEARCH / 2.0));
        frame.icon(Icon::Search, Point { x: field.0 + 16.0, y: field.1 + SEARCH / 2.0 }, 14.0, theme.text3);
        let text_x = field.0 + 32.0;
        let text_y = field.1 + (SEARCH - theme.body.size * 1.35) / 2.0;
        let words = self.search.string();
        if words.is_empty() {
            frame.text(lang.pick("搜索程序，可以用拼音首字母", "Search by name"), theme.body, theme.text3, text_x, text_y, field.2 - 40.0, Align::Start);
        } else {
            frame.text(&words, theme.body, theme.text, text_x, text_y, field.2 - 40.0, Align::Start);
        }
        let before = String::from_utf16_lossy(&self.search.text[..self.search.caret]);
        let caret_x = text_x + frame.measure(&before, theme.body);
        let caret_h = theme.body.size * 1.35;
        self.caret = Some((caret_x, text_y, caret_h));
        if self.caret_on {
            frame.fill(theme.text, caret_x, text_y + 1.0, 1.2, caret_h - 2.0);
        }
        hits.push((field.0, field.1, field.2, field.3, Hit::Search));
        // The list: under each first letter (none while searching), each with its picture, its name, and what a press
        // does (added; kept already: taken off).
        let top = HEAD + SEARCH + 12.0;
        let bottom = height - FOOT;
        let listed = self.listed();
        let searching = !words.trim().is_empty();
        let mut rows: Vec<(f32, Option<char>, Option<&Found>)> = Vec::new();
        let mut y = 0.0;
        let mut letter = None;
        for found in &listed {
            let first = found.initials.first().map(|bits| if bits.trailing_zeros() < 26 { (b'A' + bits.trailing_zeros() as u8) as char } else { '#' });
            if !searching && first != letter {
                letter = first;
                rows.push((y, first, None));
                y += LETTER;
            }
            rows.push((y, None, Some(found)));
            y += ROW;
        }
        self.scroll_most = (y - (bottom - top)).max(0.0);
        self.scroll = self.scroll.clamp(0.0, self.scroll_most);
        frame.clip(0.0, top, width, bottom - top);
        let mut waiting = false;
        if found::all().is_none() {
            frame.text(lang.pick("正在读取开始菜单里的程序…", "Reading the Start menu's programs…"), theme.small, theme.text3, PAD, top + 8.0, inner, Align::Start);
        } else if listed.is_empty() {
            frame.text(lang.pick("没有找到这个程序", "No program found by that"), theme.small, theme.text3, PAD, top + 8.0, inner, Align::Start);
        }
        for (at, letter, found) in rows {
            let ry = top + at - self.scroll;
            if ry + ROW < top || ry > bottom {
                continue;
            }
            if let Some(letter) = letter {
                frame.text(&letter.to_string(), theme.small, theme.text3, PAD, ry + (LETTER - 16.0) / 2.0 + 2.0, inner, Align::Start);
                continue;
            }
            let Some(found) = found else { continue };
            let row = (PAD - 6.0, ry, inner + 12.0, ROW);
            let hovered = lit(row, self.pointer) && self.pointer.is_some_and(|(_, py)| py >= top && py < bottom);
            if hovered {
                frame.fill_rounded(theme.hover, row.0, row.1 + 1.0, row.2, row.3 - 2.0, theme.control_radius.max(6.0));
            }
            match crate::station::thumbs::of(std::path::Path::new(&found.target), 64) {
                Some(picture) => frame.picture(&picture, PAD, ry + (ROW - 26.0) / 2.0, 26.0, 5.0, 1.0),
                None => {
                    waiting = true;
                    frame.fill_rounded(theme.track, PAD, ry + (ROW - 26.0) / 2.0, 26.0, 26.0, 5.0);
                }
            }
            // What a press does: added, outlined in the accent; kept already, ticked (a press takes it off).
            let is_kept = kept.iter().any(|app| app.target == found.target);
            let (label, ink) = if is_kept { (lang.pick("已添加", "Kept"), theme.text3) } else { (lang.pick("添加", "Add"), self.accent()) };
            let label_w = frame.measure(label, theme.small);
            let pill_w = label_w + if is_kept { 18.0 } else { 20.0 };
            let pill = (PAD + inner - pill_w, ry + (ROW - 24.0) / 2.0, pill_w, 24.0);
            frame.text(&found.name, theme.body, theme.text, PAD + 38.0, ry + (ROW - theme.body.size * 1.35) / 2.0, inner - 38.0 - pill_w - 8.0, Align::Start);
            let label_y = pill.1 + (pill.3 - 16.0) / 2.0;
            if is_kept {
                frame.icon(Icon::Check, Point { x: pill.0 + 6.0, y: pill.1 + pill.3 / 2.0 }, 12.0, ink);
                frame.text(label, theme.small, ink, pill.0 + 16.0, label_y, label_w + 2.0, Align::Start);
            } else {
                let radius = if theme.skin == Skin::Glass { pill.3 / 2.0 } else { theme.control_radius.max(4.0) };
                frame.stroke_rounded(super::canvas::Fill::Solid(ink.alpha(0.7)), pill.0, pill.1, pill.2, pill.3, radius, 1.0);
                frame.text(label, theme.small, ink, pill.0 + 10.0, label_y, label_w + 2.0, Align::Start);
            }
            hits.push((row.0, row.1.max(top), row.2, (row.1 + row.3).min(bottom) - row.1.max(top), Hit::Program(found.target.clone(), found.name.clone())));
        }
        frame.unclip();
        // A thin thumb where in the list the view is.
        if self.scroll_most > 0.0 {
            let tall = bottom - top;
            let thumb = (tall * tall / (tall + self.scroll_most)).max(16.0);
            frame.fill_rounded(theme.text3.alpha(0.6), width - 6.0, top + (tall - thumb) * self.scroll / self.scroll_most, 2.0, thumb, 1.0);
        }
        // Its foot: what to do with what is not listed.
        frame.fill(theme.rule, PAD, bottom, inner, 1.0);
        frame.icon(Icon::Add, Point { x: PAD + 8.0, y: bottom + FOOT / 2.0 }, 14.0, theme.text3);
        frame.text(lang.pick("这里没有的程序：把它的 exe 或快捷方式拖到这里", "Not listed: drag its program or shortcut here"), theme.small, theme.text2, PAD + 24.0, bottom + (FOOT - 16.0) / 2.0, inner - 24.0, Align::Start);
        self.waiting = waiting;
        self.hits = hits;
    }
}

#[cfg(all(test, feature = "studio"))]
mod pictures {
    use super::*;

    /// Quick launch's choosing of programs in each skin, light and dark, as it opens and searched by pinyin
    /// (`cargo test --features studio --lib -- --ignored beside_picker` writes `target/beside/<skin>-<dark>-<search>.png`).
    #[test]
    #[ignore = "writes pictures to look at"]
    fn beside_picker() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        crate::station::thumbs::start(|| {});
        let found = |name: &str, target: &str| Found { name: name.into(), target: target.into(), initials: found::initials(name) };
        let mut listed = vec![
            found("微信", r"C:\Windows\System32\cmd.exe"),
            found("网易云音乐", r"C:\Windows\System32
otepad.exe"),
            found("Visual Studio Code", r"C:\Windows\explorer.exe"),
            found("计算器", r"shell:AppsFolder\Microsoft.WindowsCalculator_8wekyb3d8bbwe!App"),
            found("Steam", r"C:\Windows\System32\mspaint.exe"),
            found("文件资源管理器", r"C:\Windows\explorer.exe"),
            found("7-Zip", r"C:\Windows\System32\charmap.exe"),
        ];
        listed.sort_by(|a, b| (a.initials.first().map(|bits| bits.trailing_zeros()), &a.name).cmp(&(b.initials.first().map(|bits| bits.trailing_zeros()), &b.name)));
        found::pretend(listed);
        crate::station::apps::pretend(vec![App { target: r"C:\Windows\System32\cmd.exe".into(), name: "微信".into() }]);
        let gfx = gfx::current().unwrap();
        let out = std::path::PathBuf::from(format!("{}/target/beside", env!("CARGO_MANIFEST_DIR")));
        std::fs::create_dir_all(&out).unwrap();
        let (w, h) = (WIDTH, 560.0);
        for pass in 0..2 {
            for (skin, name) in [(Skin::Glass, "glass"), (Skin::Fluent, "fluent"), (Skin::Paper, "paper")] {
                for dark in [false, true] {
                    for search in ["", "wy"] {
                        let mut beside = Beside {
                            hwnd: HWND::default(),
                            gfx: gfx.clone(),
                            surface: None,
                            _showing: super::super::life::Showing::of(&gfx),
                            scale: 1.5,
                            theme: Rc::new(Theme::new(skin, dark)),
                            lang: super::super::text::Lang::Zh,
                            page: Page::Picker,
                            search: Typing::most(SEARCH_MOST),
                            scroll: 0.0,
                            scroll_most: 0.0,
                            pointer: Some((100.0, HEAD + SEARCH + 12.0 + 30.0 + 26.0)),
                            hits: Vec::new(),
                            caret: None,
                            caret_on: true,
                            waiting: false,
                            writing: None,
                        };
                        beside.search.put(&search.encode_utf16().collect::<Vec<_>>());
                        let pixels = gfx.draw_offscreen(((w * 1.5) as u32, (h * 1.5) as u32), 1.5, |frame| beside.paint(frame, (w, h))).unwrap();
                        if pass == 1 {
                            let file = format!("{name}-{}-{}.png", if dark { "dark" } else { "light" }, if search.is_empty() { "all" } else { search });
                            crate::studio::save_png(&out.join(file), (w * 1.5) as u32, (h * 1.5) as u32, &pixels).unwrap();
                        }
                    }
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(1500));
        }
    }
}
