//! The settings window: the choices on the left, and on the right the panel
//! as it will look, over the desktop the window was opened on, running live.
//!
//! The window lives on the panel's thread and draws with its device. Its rows
//! are laid out and drawn from the settings on every frame; what moves (a
//! choice's thumb, a switch's knob, rows making way for a dragged one) is
//! kept from frame to frame as transitions.

use std::borrow::Cow;
use std::cell::RefCell;
use std::collections::{HashMap, HashSet};
use std::f32::consts::E;
use std::rc::Rc;
use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::time::{Duration, Instant, SystemTime, UNIX_EPOCH};

use windows::core::{w, BOOL, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::D2D1_GRADIENT_STOP;
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap1, ID2D1RenderTarget, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP,
    D2D1_GAMMA_2_2, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_LAYER_PARAMETERS1, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES,
    D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::Dwm::{
    DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMSBT_MAINWINDOW, DWMWA_SYSTEMBACKDROP_TYPE,
    DWMWA_USE_IMMERSIVE_DARK_MODE,
};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::Controls::{MARGINS, WM_MOUSELEAVE};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Shell::ShellExecuteW;
use windows::Win32::UI::Input::Ime::{ImmAssociateContextEx, ImmGetVirtualKey, HIMC};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, ReleaseCapture, SetCapture, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VIRTUAL_KEY, VK_BACK, VK_DELETE, VK_DOWN, VK_ESCAPE, VK_PROCESSKEY, VK_LEFT, VK_MENU,
    VK_NEXT, VK_PRIOR, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP, VK_CONTROL, VK_LCONTROL, VK_LMENU, VK_LSHIFT, VK_LWIN,
    VK_RCONTROL, VK_RMENU, VK_RSHIFT, VK_RWIN,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, GetClientRect, GetSystemMetrics, LoadCursorW, LoadImageW,
    IsWindowVisible, MessageBoxW, PostMessageW, RegisterClassExW, SetCursor, SetForegroundWindow, MB_ICONINFORMATION, MB_OK, WM_CLOSE,
    WM_KEYDOWN, WM_SYSKEYDOWN, WM_ACTIVATE, WA_INACTIVE, HWND_NOTOPMOST, HWND_TOPMOST, SWP_NOMOVE, SWP_NOSIZE, WM_APP, WM_DISPLAYCHANGE, WM_QUIT, MSG, PM_REMOVE, QS_ALLINPUT,
    DispatchMessageW, MsgWaitForMultipleObjects, PeekMessageW, TranslateMessage,
    SetWindowPos, SetWindowTextW, ShowWindow, HICON, IDC_ARROW, IDC_HAND, IMAGE_ICON, LR_SHARED, MINMAXINFO,
    SM_CXICON, SM_CXSMICON, SW_RESTORE, SW_SHOW, SW_SHOWNORMAL, SWP_NOACTIVATE, SWP_NOZORDER, WM_CAPTURECHANGED, WM_DESTROY,
    WM_DPICHANGED, WM_GETMINMAXINFO, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL,
    WM_ENTERSIZEMOVE, WM_EXITSIZEMOVE, WM_SETCURSOR, WM_SIZE, WNDCLASSEXW, WS_EX_NOREDIRECTIONBITMAP, WS_OVERLAPPEDWINDOW,
};
use windows_numerics::{Matrix3x2, Vector2};

use super::backdrop::Capture;
use super::canvas::{Align, Canvas, Color, Family, Font, Point};
use super::gfx::{self, rect, Frame, Gfx, Surface};
use super::icons::Icon;
use super::motion::{Easing, Transition};
use super::overlay::{Layout, ROWS};
use super::prefs::{self, LanguagePref, Prefs, ProcessSort, ThemePref};
use super::arrange::{self, GAP};
use super::life::Lives;
use super::render::{self, PanelLayers};
use super::skins;
use super::text::Lang;
use super::theme::{self, Shadow, Skin, Theme};
use super::view::{self, Rect, Scene};
use super::wallpaper;
use crate::elevation;
use crate::metrics;
use crate::settings::{Anchor, Edge, OverFullscreen, Part, Sensitivity, Settings, Shortcut, EDGES};

mod layout_page;
use layout_page::{Carry, Zone};
use crate::panel::SIZES;
use crate::update;

/// The window's size, and the least it can be resized to (DIPs).
const SIZE: (f32, f32) = (1300.0, 760.0);
const MIN_SIZE: (f32, f32) = (1080.0, 560.0);
/// The first Windows 11 build that lets a window ask for Mica.
const FIRST_MICA_BUILD: u32 = 22621;

/// The list of choices: its width, its padding (top, sides, bottom), and
/// the thin scroll bar's gutter.
const PANE: f32 = 480.0;
/// The pages' list, left of the page.
const NAV: f32 = 200.0;
const NAV_ITEM: f32 = 40.0;
/// How far in the components' own pages are set, under their heading.
const NAV_INDENT: f32 = 20.0;
const PAD_TOP: f32 = 28.0;
const PAD_SIDE: f32 = 36.0;
const PAD_BOTTOM: f32 = 48.0;
const GUTTER: f32 = 8.0;
/// Rows: their least height, sides, the space between rows of a group, and
/// before a group's heading.
const ROW: f32 = 56.0;
const ROW_SIDE: f32 = 20.0;
const ROW_GAP: f32 = 4.0;
/// The line of its own a row of choices with a word on it takes below.
const CHOICE_LINE: f32 = 40.0;
/// A slider's track, the room for its value, and its thumb's radius.
const SLIDER_WIDTH: f32 = 160.0;
const SLIDER_VALUE: f32 = 44.0;
const SLIDER_THUMB: f32 = 10.0;
/// The narrowest a row's word may be beside its choices.
const MIN_WORDS: f32 = 140.0;
/// The page's name, at the top of the page.
const TITLE: f32 = 52.0;
/// What the overlay's groups' rows are told from the panel's modules by:
/// the start of their ids.
const OVERLAY: &str = "overlay:";
/// What the workbench's modules' rows are told from the readings' by: the start of their ids.
const WORKBENCH: &str = "workbench:";

/// A row inside a module's open card, and how far in from the card's edge
/// its label starts: under the module's name, past the grip.
const ITEM_ROW: f32 = 48.0;
const ITEM_INSET: f32 = 12.0 + 20.0 + 12.0;
/// Room at the right of a module's row for its chevron, after its switch.
const CHEVRON_ROOM: f32 = 28.0;
/// How large a chevron is drawn.
const CHEVRON: f32 = 14.0;
/// The screens in the layout's editor: a screen's picture's corners; and the dashes of an edge the pointer passes on
/// from (the least they are long, and apart).
const MAP_RADIUS: f32 = 3.0;
const MAP_DASH: f32 = 4.0;
const MAP_GAP: f32 = 3.0;
const SKINS_ROW: f32 = 16.0 + 72.0 + 10.0 + 18.0 + 16.0;
/// Desktop shown beside the panel in the preview (DIPs of screen).
const PREVIEW_MARGIN: f32 = 160.0;
/// How often the music page's preview is drawn while something plays (as a chart's speed: a quarter pixel each
/// thirtieth of a second, see `draw`).
const MUSIC_PREVIEW_PACE: f32 = 7.5;
/// How far a wheel notch scrolls the choices, and how quickly they follow.
const WHEEL: f32 = 100.0;
const SCROLL_EASE: f32 = 0.06;
/// How the controls move: a thumb sliding to the chosen option, a switch's
/// knob, rows making way for a dragged one, and the dragged row settling.
const SLIDE: (Duration, Easing) = (Duration::from_millis(240), Easing(0.16, 1.0, 0.3, 1.0));
const FLIP: (Duration, Easing) = (Duration::from_millis(160), Easing(0.16, 1.0, 0.3, 1.0));
const GLIDE: (Duration, Easing) = (Duration::from_millis(200), Easing(0.16, 1.0, 0.3, 1.0));
const SETTLE: (Duration, Easing) = (Duration::from_millis(180), Easing(0.16, 1.0, 0.3, 1.0));
/// How soon to try again when drawing failed for want of a device.
/// A frame that takes longer than this shows as a stutter: noted.
const SLOW_FRAME: Duration = Duration::from_millis(33);
const DEVICE_RETRY: Duration = Duration::from_millis(250);
/// The pen of the preview's charts runs this far behind the newest sample
/// beyond one interval, as the panel's does.
const PEN_LAG_MS: f64 = 100.0;

/// How long the diagnostics' button says they were copied.
const COPIED_FOR: Duration = Duration::from_secs(2);

/// The window, while it is open.
static WINDOW: AtomicIsize = AtomicIsize::new(0);
/// Its thread is making it: asked again meanwhile, no second one is made.
static OPENING: AtomicBool = AtomicBool::new(false);
/// What other threads tell the window: settings changed elsewhere, to be taken up (see `follow_settings`); the
/// overlay's page asked for (see `open_at_overlay`).
const FOLLOW: u32 = WM_APP + 40;
const TO_OVERLAY: u32 = WM_APP + 41;

thread_local! {
    static UI: RefCell<Option<Ui>> = const { RefCell::new(None) };
}

/// Whether it is open and not minimized: what it shows can be seen.
pub fn is_seen() -> bool {
    let hwnd = WINDOW.load(Ordering::Relaxed);
    hwnd != 0 && !unsafe { windows::Win32::UI::WindowsAndMessaging::IsIconic(HWND(hwnd as *mut _)) }.as_bool()
}

/// Brings the settings window up, from any thread: raised if it is open,
/// else made, by the panel's thread.
pub fn show() {
    let hwnd = WINDOW.load(Ordering::Acquire);
    if hwnd == 0 {
        crate::app().controller.open_settings();
        return;
    }
    let hwnd = HWND(hwnd as *mut _);
    unsafe {
        // (Asked of its own thread, not waited for: called from the tray's and the panel's.)
        let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindowAsync(hwnd, SW_RESTORE);
        let _ = SetForegroundWindow(hwnd);
    }
}

/// Makes the window, centred on the monitor the pointer is on, on the
/// The settings window, while it is open.
pub fn hwnd() -> Option<HWND> {
    let hwnd = WINDOW.load(Ordering::Acquire);
    (hwnd != 0).then_some(HWND(hwnd as *mut _))
}

/// thread of its own, which carries it till it is closed (see `run`): moved or sized by hand, the system holds that
/// thread in a loop of its own till it is let go, and the panel, the widgets and the lyrics go on meanwhile. Open,
/// it is raised.
pub fn open() {
    open_at(false);
}

/// Opens the window (as `open`) at the overlay's page: from the overlay's menu.
pub fn open_at_overlay() {
    open_at(true);
}

fn open_at(overlay: bool) {
    if let Some(hwnd) = hwnd() {
        if overlay {
            let _ = unsafe { PostMessageW(Some(hwnd), TO_OVERLAY, WPARAM(0), LPARAM(0)) };
        }
        return show();
    }
    if OPENING.swap(true, Ordering::AcqRel) {
        return;
    }
    if std::thread::Builder::new().name("settings".into()).spawn(move || run(overlay)).is_err() {
        OPENING.store(false, Ordering::Release);
    }
}

/// The window's thread: makes it (on a graphics device of its own, given back as the thread ends), and draws it
/// whenever it is due, between the messages that arrive, till it is gone.
fn run(overlay: bool) {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
    }
    let made = gfx::current().ok().and_then(|gfx| make(gfx, true));
    if let Some(hwnd) = made {
        WINDOW.store(hwnd.0 as isize, Ordering::Release);
    }
    OPENING.store(false, Ordering::Release);
    if made.is_none() {
        return;
    }
    if overlay {
        to_overlay();
    }
    let mut msg = MSG::default();
    loop {
        while unsafe { PeekMessageW(&mut msg, None, 0, 0, PM_REMOVE) }.as_bool() {
            if msg.message == WM_QUIT {
                return;
            }
            unsafe {
                let _ = TranslateMessage(&msg);
                DispatchMessageW(&msg);
            }
        }
        // Gone (closed): the thread with it.
        let Some(due) = tick(Instant::now()) else { return };
        if !due.is_zero() {
            let ms = due.as_micros().div_ceil(1000).min(u32::MAX as u128) as u32;
            unsafe { MsgWaitForMultipleObjects(None, false, ms, QS_ALLINPUT) };
        }
    }
}

/// The overlay's page, turned to.
fn to_overlay() {
    with_ui(|ui| {
        ui.tab = Tab::Overlay;
        ui.turn_to(Page::Monitor);
    });
}

/// Takes up settings changed elsewhere (the panel's bar, the overlay's menu, a drag), from any thread: an open window
/// shows them, and saves on top of them (told on its own thread).
pub fn follow_settings() {
    if let Some(hwnd) = hwnd() {
        let _ = unsafe { PostMessageW(Some(hwnd), FOLLOW, WPARAM(0), LPARAM(0)) };
    }
}

fn take_settings() {
    with_ui(|ui| {
        ui.follow_owed = true;
        ui.next_frame = Instant::now();
        ui.follow_when_free();
    });
}

/// Draws the window if a frame is due, and says when the next one is; `None`
/// while there is no window.
fn tick(now: Instant) -> Option<Duration> {
    with_ui(|ui| {
        // Minimized, nothing is drawn (it is drawn whole as it is restored: see WM_SIZE); looked at again a while
        // later.
        if unsafe { windows::Win32::UI::WindowsAndMessaging::IsIconic(ui.hwnd) }.as_bool() {
            return MINIMIZED_LOOK;
        }
        if ui.relabel_at.is_some_and(|at| now >= at) {
            ui.relabel();
        }
        ui.follow_when_free();
        if now >= ui.next_frame {
            ui.draw(now);
        }
        ui.next_frame.saturating_duration_since(now)
    })
}

/// How often a minimized settings window is looked at (to be drawn again once it is restored).
const MINIMIZED_LOOK: Duration = Duration::from_millis(500);

fn windows_build() -> u32 {
    metrics::reg_string(w!(r"SOFTWARE\Microsoft\Windows NT\CurrentVersion"), w!("CurrentBuild")).parse().unwrap_or(0)
}

/// The switches turned on another thread (see `Ui::switching`).
#[derive(Clone, Copy)]
enum SwitchAsked {
    Startup,
    Remote,
}

/// The desktop inside `rect` as `wallpaper::desktop` reads it, `width` × `height` pixels, on a thread of the
/// caller's (which joins the process's apartment for the decoders it uses).
fn read_desktop(rect: RECT, width: u32, height: u32) -> Capture {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
    }
    wallpaper::desktop(rect, width, height)
}

/// Each screen's desktop as the preview last read it (its name, its work area, the size it was read at): kept for
/// as long as Glance runs, so that the settings open on the desktop as it was (read again as they open, and taken
/// if it has changed), not on an empty stage while it is read. Small: read at the preview's size, in DIPs.
static DESKTOPS: std::sync::Mutex<Vec<(String, RECT, (f32, f32), Capture)>> = std::sync::Mutex::new(Vec::new());

/// Reads screen `screen`'s desktop for the preview (see `read_desktop`), and keeps it (see `DESKTOPS`).
fn read_and_keep(screen: &str, work: RECT, size: (f32, f32)) -> Capture {
    let desktop = read_desktop(work, size.0 as u32, size.1 as u32);
    keep_desktop(screen, work, size, &desktop);
    desktop
}

/// Keeps `desktop`, screen `screen`'s as read at `work` and `size` (see `DESKTOPS`).
fn keep_desktop(screen: &str, work: RECT, size: (f32, f32), desktop: &Capture) {
    if let Some(copy) = desktop.copied() {
        let mut kept = DESKTOPS.lock().unwrap();
        kept.retain(|(name, ..)| name != screen);
        kept.push((screen.to_string(), work, size, copy));
    }
}

/// Screen `screen`'s desktop as last read for the preview, if it was read at its work area `work` and `size`.
fn kept_desktop(screen: &str, work: RECT, size: (f32, f32)) -> Option<Capture> {
    DESKTOPS.lock().unwrap().iter().find(|(name, at, read, _)| name == screen && *at == work && *read == size).and_then(|(.., desktop)| desktop.copied())
}

/// Every screen as the preview shows it: its name, its work area (physical px), and that area's size (DIPs).
fn stages_wanted(screens: &[crate::screens::Screen]) -> Vec<(String, RECT, (f32, f32))> {
    screens
        .iter()
        .filter_map(|known| {
            let m = known.monitor;
            let contact = crate::mouse::monitor_at(POINT { x: (m.left + m.right) / 2, y: (m.top + m.bottom) / 2 })?;
            let work = contact.work;
            let size = (((work.right - work.left) as f32 / contact.scale).round(), ((work.bottom - work.top) as f32 / contact.scale).round());
            Some((known.id.clone(), work, size))
        })
        .collect()
}

/// Reads every screen's desktop for the preview and keeps it, before the settings are first opened: they open on
/// it. On a thread of the caller's.
pub fn read_desktops_ahead() {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
    }
    // (A live wallpaper printed once for them all: see `wallpaper::desktops`.)
    let wanted = stages_wanted(&crate::screens::all());
    let asked: Vec<(RECT, u32, u32)> = wanted.iter().map(|(_, work, size)| (*work, size.0 as u32, size.1 as u32)).collect();
    for ((screen, work, size), desktop) in wanted.iter().zip(wallpaper::desktops(&asked)) {
        keep_desktop(screen, *work, *size, &desktop);
    }
}

/// A desktop read on another thread for the settings (see `Ui::read`): for the preview, or for the map.
enum Read {
    Stage(Stage),
    Map { id: String, monitor: RECT, size: (u32, u32), desktop: Capture },
}

/// How the phone remote is, as its helper says (see `Ui::remote_asked`), and a code asked of it with what for (none
/// asked: none).
struct RemoteAnswer {
    lights: bool,
    screens: bool,
    address: Option<String>,
    code: Option<Option<(crate::phone::Code, CodeFor)>>,
}

/// What the diagnostics are put together for.
#[derive(Clone, Copy)]
enum ReportFor {
    Copy,
    Send,
}

/// The window's own colours: Windows 11's, whatever skin the panel wears.
#[derive(Clone)]
struct Palette {
    text: Color,
    text2: Color,
    text3: Color,
    rule: Color,
    hover: Color,
    control: Color,
    control_on: Color,
    /// The chosen option's ring, and its drop shadow (none in dark).
    control_ring: Color,
    control_drop: Option<Color>,
    switch_off: Color,
    switch_stroke: Color,
    switch_knob: Color,
    window: Color,
    card: Color,
    card_stroke: Color,
    /// A switch that does nothing for now: its outline or track, and its
    /// knob when on.
    disabled: Color,
    disabled_knob: Color,
    /// The accent in the shade for this theme, and the deeper one switches
    /// take in either (the pale dark-mode shade cannot carry a white knob).
    selection: Color,
    switch_on: Color,
    /// The ring of a slider's thumb.
    switch_knob_ring: Color,
}

impl Palette {
    fn new(dark: bool) -> Self {
        let (on, off) = (theme::accent(false), theme::accent(dark));
        let white = |a| Color::hex(0xFFFFFF, a);
        let black = |a| Color::hex(0x000000, a);
        if dark {
            Palette {
                text: white(1.0),
                text2: white(0.76),
                text3: white(0.5),
                rule: white(0.08),
                hover: white(0.06),
                control: white(0.06),
                control_on: white(0.16),
                control_ring: white(0.06),
                control_drop: None,
                switch_off: black(0.1),
                switch_stroke: white(0.6),
                switch_knob: white(0.8),
                window: Color::hex(0x202020, 1.0),
                card: white(0.05),
                card_stroke: black(0.2),
                disabled: white(0.16),
                disabled_knob: white(0.53),
                selection: off,
                switch_on: on,
                switch_knob_ring: Color::hex(0x454545, 1.0),
            }
        } else {
            Palette {
                text: black(0.9),
                text2: black(0.6),
                text3: black(0.44),
                rule: black(0.08),
                hover: black(0.04),
                control: black(0.05),
                control_on: white(0.95),
                control_ring: black(0.06),
                control_drop: Some(black(0.08)),
                switch_off: black(0.03),
                switch_stroke: black(0.6),
                switch_knob: black(0.6),
                window: Color::hex(0xF3F3F3, 1.0),
                card: white(0.7),
                card_stroke: black(0.06),
                disabled: black(0.22),
                disabled_knob: white(1.0),
                selection: on,
                switch_on: on,
                switch_knob_ring: white(1.0),
            }
        }
    }
}

/// The choices of module `id`'s own, shown in its card.
fn module_fields(id: &str) -> &'static [Field] {
    match id {
        "network" => &[Field::RateUnit],
        "processes" => &[Field::ProcessCount, Field::ProcessSort],
        _ => &[],
    }
}

/// The settings, a group to a page: what each screen edge opens and how; each component's own (the monitor's, on
/// tabs of their own, and what plays); and Glance's as a whole.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Page {
    Layout,
    Monitor,
    Workbench,
    Music,
    General,
}

/// The monitor's settings, a group to a tab: how it looks, what it shows, how it reads, and its overlay.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum Tab {
    Look,
    Shown,
    Data,
    Overlay,
}

const TABS: [Tab; 4] = [Tab::Look, Tab::Shown, Tab::Data, Tab::Overlay];

impl Tab {
    fn name(self, lang: Lang) -> &'static str {
        match self {
            Tab::Look => pick(lang, "外观", "Look"),
            Tab::Shown => pick(lang, "显示内容", "Shown"),
            Tab::Data => pick(lang, "数据", "Data"),
            Tab::Overlay => pick(lang, "游戏悬浮窗", "Game overlay"),
        }
    }
}

/// The pages' list as it reads: each page, and the components' heading the monitor's and what plays come under
/// (pressed, it shows the first of them).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
enum NavItem {
    Page(Page),
    Components,
}

const NAV_ITEMS: [NavItem; 6] = [
    NavItem::Page(Page::Layout),
    NavItem::Components,
    NavItem::Page(Page::Monitor),
    NavItem::Page(Page::Workbench),
    NavItem::Page(Page::Music),
    NavItem::Page(Page::General),
];

impl NavItem {
    fn name(self, lang: Lang) -> &'static str {
        match self {
            NavItem::Page(Page::Layout) => pick(lang, "布局", "Layout"),
            NavItem::Components => pick(lang, "组件", "Components"),
            NavItem::Page(Page::Monitor) => pick(lang, "监控", "Monitor"),
            NavItem::Page(Page::Workbench) => pick(lang, "工作台", "Workbench"),
            NavItem::Page(Page::Music) => pick(lang, "音乐", "Music"),
            NavItem::Page(Page::General) => pick(lang, "通用", "General"),
        }
    }

    fn icon(self) -> Icon {
        match self {
            NavItem::Page(Page::Layout) => Icon::LayoutGrid,
            NavItem::Components => Icon::Layers,
            NavItem::Page(Page::Monitor) => Icon::Gauge,
            NavItem::Page(Page::Workbench) => Icon::Briefcase,
            NavItem::Page(Page::Music) => Icon::Music,
            NavItem::Page(Page::General) => Icon::Settings,
        }
    }

    /// Whether it is listed under the components' heading, set in.
    fn under(self) -> bool {
        matches!(self, NavItem::Page(Page::Monitor | Page::Workbench | Page::Music))
    }

    /// The page it shows.
    fn page(self) -> Page {
        match self {
            NavItem::Page(page) => page,
            NavItem::Components => Page::Monitor,
        }
    }
}

impl Page {
    fn name(self, lang: Lang) -> &'static str {
        NavItem::Page(self).name(lang)
    }
}

/// A row of choices, one of which is picked.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Field {
    Theme,
    Language,
    Anchor,
    Columns,
    OverFullscreen,
    Push,
    CloseDelay,
    RateUnit,
    ProcessCount,
    ProcessSort,
    Interval,
    Span,
    LoadAlert,
    TempAlert,
    /// Set on a slider (see `slider`).
    OverlaySize,
    OverlayClear,
    PanelSize,
    OverlayLayout,
    OverlayRows,
    /// The player music is followed from (see `Ui::sources`).
    MusicSource,
}

/// How a button looks: outlined; filled in the accent (the one thing to
/// press); waiting for keys; or busy, greyed and not to be pressed.
#[derive(Clone, Copy, PartialEq)]
enum Button {
    Plain,
    Accent,
    Taking,
    Quiet,
}

/// A row of buttons: its name, its word, and its buttons, right to left
/// (text, what each does, how it looks).
type ButtonRow = (String, String, Vec<(String, Target, Button)>);

/// A row that is on or off.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
enum Switch {
    Live,
    /// Whether pulling the panel off its edge tears it off as a widget.
    TearOff,
    Startup,
    Updates,
    HeatAlert,
    Overlay,
    OverlayInGame,
    /// Whether the overlay's glass blurs what is behind it.
    /// The phone remote.
    Remote,
    /// Whether the remote sets the lights: the program that does, fetched or not.
    RemoteLights,
    /// Whether the remote puts the screens out with their signal kept.
    RemoteScreens,
}

/// What a QR code shown is for.
#[derive(Clone, Copy, PartialEq)]
enum CodeFor {
    /// Where the phone app is to be had.
    App,
    /// A pairing, for the app to scan.
    Pairing,
    /// A pairing, for a phone's camera: it opens the remote's page.
    WebPairing,
    /// The remote's page, for a phone paired by it before.
    WebPage,
}

/// What a press lands on.
#[derive(Clone, PartialEq, Debug)]
enum Target {
    Skin(Skin),
    Choice(Field, usize),
    Switch(Switch),
    /// A module's switch, and the rest of its row, which opens and closes
    /// its card.
    Module(String),
    Expand(String),
    /// One of a module's items.
    Item(String, String),
    Grip(String),
    /// The shortcut's keys: pressed, the next combination becomes them.
    Shortcut,
    /// Clears the shortcut.
    ClearShortcut,
    /// A slider's track.
    Slider(Field),
    /// A component in the layout's library, to be dragged onto an edge.
    Library(Part),
    /// A component on an edge of a screen (by its name) in the layout's editor: dragged along it, to another, or off;
    /// pressed, the panel that edge opens shows.
    Chip(String, Edge, Part),
    /// The button on it that takes it off the edge.
    Unplace(String, Edge),
    /// An edge of a screen shown large in the editor: pressed, the panel it opens shows.
    Zone(String, Edge),
    /// Every screen small in the editor (none), or the one by its name, large.
    View(Option<String>),
    /// Every screen alike, or each set apart.
    Same,
    /// A page (or the components' heading), in the pages' list.
    Nav(NavItem),
    /// One of the monitor's tabs.
    Tab(Tab),
    /// One of what plays shows (see `Row::Element`).
    Element(&'static str),
    /// Opens or closes the fold of the modules few want.
    More,
    /// Asks for a newer release now.
    CheckNow,
    Diagnostics,
    Update,
    Uninstall,
    Quit,
    /// Shows the code that leads to the phone app.
    GetApp,
    /// Offers a pairing and shows its code: for the app, or (true) for a phone's browser.
    Pair(bool),
    /// Shows the code that opens the remote's web page again.
    OpenPage,
    /// Unpairs the phone of that id (not by its place among those shown: the list is read again every second, and a
    /// phone paired or removed meanwhile would move another into it).
    Unpair(String),
    /// Sends the diagnostics to Glance's site, shown first in a window of
    /// their own to be read and agreed to.
    SendReport,
}

/// Modules for developers: listed under "More" while off, out of the way.
const MORE: [&str; 2] = ["wsl", "docker"];

enum Row {
    /// The page's name.
    Title,
    /// The monitor's tabs, under its page's name.
    Tabs,
    /// One of what plays shows, on or off (an item of the lyrics module: see `music::Shows`).
    Element(&'static str),
    /// Words of their own, under the rows they are about.
    Note(&'static str),
    /// The fold of the modules few want, and how many it holds.
    More(usize),
    Skins,
    /// The components, to be dragged onto the screens' edges in the preview's room (see `layout_page`).
    Library,
    Choice(Field),
    Switch(Switch),
    Module(String),
    /// In a module's open card: one of its items, or a choice of its own.
    Item(String, String),
    ModuleChoice(String, Field),
    /// A value set by sliding.
    Slider(Field),
    Shortcut,
    /// This Glance's version, how the last asking went, and asking now.
    Version,
    Diagnostics,
    Update,
    Uninstall,
    Quit,
    /// The phone remote's: where the app is to be had, pairing a phone,
    /// the code last asked for, and each phone paired (its place in the
    /// remote's list, and its name).
    GetApp,
    Pair,
    Code,
    /// Where the remote's page for a phone's browser is.
    WebPage,
    /// A phone paired with: its id and its name.
    Phone(String, String),
    /// What the rows under it are, said small above them: a group of their own.
    Group(&'static str),
}

/// How tall a group's name is, above its rows.
const GROUP: f32 = 24.0;
/// How tall the monitor's tabs are, and how far below their names their marker is.
const TABS_ROW: f32 = 48.0;
/// How tall the code's own square is, in its row.
const CODE: f32 = 176.0;
/// The room for the words beside it: what the row has left.
const CODE_WORDS: f32 = PANE - 2.0 * PAD_SIDE - GUTTER - 2.0 * ROW_SIDE - CODE - 20.0;

/// A module row being dragged: by how far below the row's top it was
/// taken, and where the pointer is (document DIPs).
struct Drag {
    id: String,
    grab: f32,
    pointer: f32,
}

/// A screen's desktop, small, for the map: the screen by its name, the
/// picture's size (physical px), and the picture as a bitmap once drawn.
struct MapArt {
    id: String,
    /// Where the screen was as the picture was taken (physical px).
    monitor: RECT,
    /// The size it was last asked for at (physical px): what it holds, or will once it is read.
    asked: (u32, u32),
    /// The picture held: none till the first is read (on another thread: see `Ui::read`); the one before while
    /// another is read.
    desktop: Option<Capture>,
    bitmap: Option<ID2D1Bitmap1>,
}

/// The desktop the preview shows, at the size of the work area (DIPs).
struct Stage {
    size: (f32, f32),
    /// The screen it is, by its name, and its work area as the desktop
    /// was taken (physical px).
    screen: Option<String>,
    work: RECT,
    /// None till it has been read (on another thread: see `Ui::read`).
    desktop: Option<Capture>,
    /// The desktop as a bitmap, for the zoom it was made at.
    bitmap: Option<(u32, ID2D1Bitmap1)>,
}

/// The phone's page as it is used from here: how the remote last said the
/// PC is, what of the page can be pressed and where, how far the page is
/// moved up, what is in hand, and the thread the remote is asked on (what
/// to ask goes to it; how the PC then is comes back).
struct PhonePage {
    state: Option<serde_json::Value>,
    parts: Vec<(Rect, super::phone_art::Part)>,
    /// The phone's screen in the window, and how many of the window's DIPs one of its own is.
    at: Option<(Rect, f32)>,
    scroll: f32,
    /// How far the page can be moved up at most.
    most: f32,
    /// A slider in hand: its track in the window and how far along it is; or what else was pressed.
    slid: Option<(super::phone_art::Part, Rect, f32)>,
    pressed: Option<super::phone_art::Part>,
    /// A colour being made for the lights, while the ring it is made on is open.
    making: Option<super::phone_art::Making>,
    /// Since when what is pressed has been held (a colour held a while: the colours kept are taken away); and
    /// whether they are being.
    held_since: Instant,
    removing: bool,
    link: Option<(std::sync::mpsc::Sender<(String, String, String)>, std::sync::mpsc::Receiver<Option<serde_json::Value>>)>,
    /// When the remote was last asked how the PC is, and how many askings are unanswered.
    asked: Instant,
    waiting: usize,
}

/// How often the remote is asked how the PC is, while its page shows.
const PHONE_ASKED_EVERY: Duration = Duration::from_millis(1500);

struct Ui {
    hwnd: HWND,
    gfx: Rc<Gfx>,
    /// Counted as shown on its device while it is (see `life`).
    showing: Option<crate::ui::life::Showing>,
    /// Out of the struct while a frame is drawn on it.
    surface: Option<Surface>,
    mica: bool,
    /// Physical pixels per DIP.
    scale: f32,
    settings: Settings,
    /// The settings as this window last took them: what it changed since
    /// is where `settings` differs.
    base: Settings,
    prefs: Prefs,
    /// The page shown, and the monitor's tab (shown on its page).
    page: Page,
    tab: Tab,
    /// The players music can be followed from, as the music page offers them (as Windows identifies each, and its
    /// name): those playing music now, and the one chosen, playing or not; taken anew each frame.
    sources: Vec<(String, String)>,
    /// What plays, as the music page's preview draws it; since when the example has played (see
    /// `music::paint_example`).
    music: super::music::Music,
    example_from: Instant,
    lang: Lang,
    dark: bool,
    palette: Palette,
    autostart: bool,
    /// The phone remote: whether this Glance came with it, whether it is
    /// on, the code shown (and what for), and the phones paired, as last
    /// read (and when).
    remote_there: bool,
    remote: bool,
    /// Whether the remote has the program it sets lights by; that being
    /// fetched or taken away just now (on another thread: it takes a
    /// while), and whether the last fetching failed.
    remote_lights: bool,
    /// Whether the remote puts the screens out with their signal kept.
    remote_screens: bool,
    fetching: Option<std::thread::JoinHandle<bool>>,
    fetch_failed: bool,
    /// A switch that needs the sensors' rights, being turned (on another thread: their process is asked, and
    /// answers when the system has done it); what it is then.
    switching: Option<(SwitchAsked, std::thread::JoinHandle<bool>)>,
    /// The diagnostics being put together (on another thread: the sensors' part is asked of their process), and
    /// what for.
    reporting: Option<(ReportFor, std::thread::JoinHandle<String>)>,
    code: Option<(crate::phone::Code, CodeFor)>,
    phones: Vec<crate::phone::Phone>,
    /// The phone remote's helper asked something (and how it is then) on another thread: each answer of its is a
    /// program started and waited for, not to be waited for by the thread that draws every window.
    remote_asked: Option<std::thread::JoinHandle<RemoteAnswer>>,
    phones_at: Instant,
    /// Whether this copy may start with Windows (see elevation.rs).
    may_autostart: bool,
    /// The installer's uninstaller beside this executable, if it was
    /// installed somewhere ordinary programs cannot change.
    uninstaller: Option<std::path::PathBuf>,
    /// When the diagnostics were last copied.
    copied_at: Option<Instant>,
    /// Where the remote's page for a phone's browser is.
    web_address: Option<String>,
    /// The diagnostics being sent (on a thread of its own), and how the
    /// last sending went: the name the site gave them, or none.
    sending: Option<std::thread::JoinHandle<Option<String>>>,
    sent: Option<Option<String>>,
    /// Since when the fans in the phone remote's picture have been turning.
    phone_from: Instant,
    phone: PhonePage,
    scroll: f32,
    scroll_target: f32,
    motion: HashMap<String, Transition>,
    /// Thumbs land where they belong without sliding (after relabelling).
    settle_thumbs: bool,
    /// Sized (or moved) by hand, in Windows' own loop: what is drawn is sized as it is (see `gfx::room`).
    by_hand: bool,
    /// Sized by hand in a frame since it was last drawn at rest (see `Gfx::settle`).
    was_sized: bool,
    /// Taking a new shortcut: the next key combination pressed, and whether
    /// the last one lacked Ctrl, Alt or Win.
    recording: bool,
    needs_modifier: bool,
    pointer: Option<(f32, f32)>,
    pressed: Option<Target>,
    /// The screens, as the map shows them.
    screens: Vec<crate::screens::Screen>,
    /// The edge last pressed in the layout's editor: the panel it opens slides out from it there.
    shown_edge: Option<Edge>,
    /// The screen the layout's editor shows large, by its name (none: every screen, small).
    view: Option<String>,
    /// A component carried in the layout's editor, and its edges as last drawn (where they take it).
    carry: Option<Carry>,
    /// Settings changed elsewhere, to be taken up once nothing is in hand (see `follow_when_free`).
    follow_owed: bool,
    zones: Vec<Zone>,
    /// What plays, as the layout's library shows it; the readings and the workbench, as it shows them (each drawn
    /// in layers of its own: see `render::PanelLayers`).
    library_music: super::music::Music,
    library_layers: [PanelLayers; 2],
    /// When the screens were last asked for.
    screens_at: Instant,
    map_art: Vec<MapArt>,
    /// The desktops of the other screens the preview has shown, kept: turning to a page that shows another screen
    /// (the overlay's) and back takes them as they are, not read from the wallpaper and scaled anew each time.
    kept_stages: Vec<Stage>,
    /// The desktops read on other threads (every screen's for the preview as the window opens, and any one asked for
    /// since: see `read`), come back: for the preview, or for the map.
    read: (std::sync::mpsc::Sender<Read>, std::sync::mpsc::Receiver<Read>),
    /// The control the keyboard acts on, and whether the keyboard has been
    /// used since the last click (the focus is shown only then).
    focus: Option<Target>,
    keyboard: bool,
    drag: Option<Drag>,
    /// A slider being slid, and where its track is.
    sliding: Option<(Field, Rect)>,
    /// The modules whose cards are open (all closed as the window opens).
    expanded: HashSet<String>,
    /// The modules of `MORE` listed with the rest (those on as the page was
    /// turned to: one switched on or off there stays where it is till then),
    /// and whether the fold of the others is open.
    raised: HashSet<String>,
    more_open: bool,
    targets: Vec<(Rect, Target)>,
    relabel_at: Option<Instant>,
    stage: Stage,
    layers: PanelLayers,
    last_frame: Instant,
    next_frame: Instant,
    /// The client size it was last drawn at (physical pixels), if it has been drawn.
    drawn_size: Option<(u32, u32)>,
    /// Drawing the preview failed this frame (the device was lost).
    failed: bool,
    /// The largest panel size at which the panel still grows on the screen the preview shows (see
    /// `arrange::largest`): past it a larger size changed the figure and nothing else.
    largest: f32,
}

impl Lives for Ui {
    fn device(&self) -> &Rc<Gfx> {
        &self.gfx
    }

    fn showing(&mut self) -> &mut Option<crate::ui::life::Showing> {
        &mut self.showing
    }

    /// Its surface, its preview's layers, and the desktops it shows made bitmaps.
    fn let_go_of_shown(&mut self) {
        self.surface = None;
        self.layers.release();
        self.stage.bitmap = None;
        for stage in &mut self.kept_stages {
            stage.bitmap = None;
        }
        for art in &mut self.map_art {
            art.bitmap = None;
        }
        self.music.release();
        self.library_music.release();
        for layers in &mut self.library_layers {
            layers.release();
        }
    }
}

/// Makes the window, draws its first frame, and shows it (unless not `shown`: for pictures of it, drawn off screen).
fn make(gfx: Rc<Gfx>, shown: bool) -> Option<HWND> {
    let (work, scale) = crate::panel::work_area_at_cursor()?;
    let scale = scale as f32;
    unsafe {
        let instance = GetModuleHandleW(None).ok()?;
        let icon = |size| LoadImageW(Some(instance.into()), PCWSTR(1 as _), IMAGE_ICON, size, size, LR_SHARED).ok().map(|h| HICON(h.0));
        let class = WNDCLASSEXW {
            cbSize: size_of::<WNDCLASSEXW>() as u32,
            lpfnWndProc: Some(procedure),
            hInstance: instance.into(),
            lpszClassName: w!("GlanceSettings"),
            hCursor: LoadCursorW(None, IDC_ARROW).ok()?,
            hIcon: icon(GetSystemMetrics(SM_CXICON)).unwrap_or_default(),
            hIconSm: icon(GetSystemMetrics(SM_CXSMICON)).unwrap_or_default(),
            ..Default::default()
        };
        RegisterClassExW(&class);
        // Sized and centred in physical pixels of the monitor it opens on.
        let (work_w, work_h) = (work.right - work.left, work.bottom - work.top);
        let width = ((SIZE.0 * scale) as i32).min(work_w * 9 / 10);
        let height = ((SIZE.1 * scale) as i32).min(work_h * 9 / 10);
        let hwnd = CreateWindowExW(
            WS_EX_NOREDIRECTIONBITMAP,
            w!("GlanceSettings"),
            w!("Glance"),
            WS_OVERLAPPEDWINDOW,
            work.left + (work_w - width) / 2,
            work.top + (work_h - height) / 2,
            width,
            height,
            None,
            None,
            Some(instance.into()),
            None,
        )
        .ok()?;
        // Nothing in the window is typed: no input method, which would take
        // the keys of a shortcut being chosen for itself (they come as
        // VK_PROCESSKEY while it is on).
        let _ = ImmAssociateContextEx(hwnd, HIMC::default(), 0);
        // Mica, the Windows 11 window material, where the system has it: the
        // window's content leaves it showing through.
        let mica = windows_build() >= FIRST_MICA_BUILD;
        if mica {
            let _ = DwmSetWindowAttribute(hwnd, DWMWA_SYSTEMBACKDROP_TYPE, &DWMSBT_MAINWINDOW as *const _ as *const _, 4);
            let _ = DwmExtendFrameIntoClientArea(hwnd, &MARGINS { cxLeftWidth: -1, cxRightWidth: -1, cyTopHeight: -1, cyBottomHeight: -1 });
        }
        let surface = Some(Surface::new(&gfx, hwnd).ok()?);
        let app = crate::app();
        let settings = app.settings.lock().unwrap().clone();
        let prefs = Prefs::resolve(&settings.view, &app.controller.known_modules());
        let size = ((work_w as f32 / scale).round(), (work_h as f32 / scale).round());
        let centre = POINT { x: (work.left + work.right) / 2, y: (work.top + work.bottom) / 2 };
        let screen = crate::screens::all().into_iter().find(|screen| crate::mouse::monitor_at(centre).is_some_and(|contact| contact.monitor == screen.monitor)).map(|screen| screen.id);
        // On the desktop as it was last read, if it was (see `DESKTOPS`): light or dark as it is from the first frame
        // (read again on another thread, the window showing meanwhile).
        let desktop = screen.as_deref().and_then(|screen| kept_desktop(screen, work, size));
        let stage = Stage { size, screen, work, desktop, bitmap: None };
        let window_scale = GetDpiForWindow(hwnd) as f32 / 96.0;
        let mut ui = Ui {
            hwnd,
            gfx,
            surface,
            mica,
            scale: window_scale,
            lang: Lang::resolve(prefs.language),
            dark: false,
            palette: Palette::new(false),
            base: settings.clone(),
            settings,
            prefs,
            page: Page::Layout,
            tab: Tab::Look,
            sources: Vec::new(),
            music: super::music::Music::default(),
            example_from: Instant::now(),
            autostart: crate::source::autostart(),
            remote_there: crate::phone::helper().is_some(),
            remote: crate::phone::on(),
            // (As its helper says, asked as the window opens: see `remote_asked`.)
            remote_lights: false,
            remote_screens: false,
            fetching: None,
            fetch_failed: false,
            switching: None,
            reporting: None,
            code: None,
            phones: crate::phone::phones(),
            remote_asked: None,
            phones_at: Instant::now(),
            may_autostart: elevation::may_start_unasked(),
            // Run elevated, so only where no ordinary program can change it.
            copied_at: None,
            web_address: None,
            sending: None,
            sent: None,
            phone_from: Instant::now(),
            phone: PhonePage { state: None, parts: Vec::new(), at: None, scroll: 0.0, most: 0.0, slid: None, pressed: None, making: None, held_since: Instant::now(), removing: false, link: None, asked: Instant::now(), waiting: 0 },
            uninstaller: std::env::current_exe().ok().and_then(|exe| elevation::trusted(&exe.with_file_name("uninstall.exe"))),
            scroll: 0.0,
            scroll_target: 0.0,
            motion: HashMap::new(),
            settle_thumbs: true,
            by_hand: false,
            was_sized: false,
            recording: false,
            needs_modifier: false,
            pointer: None,
            pressed: None,
            screens: crate::screens::all(),
            shown_edge: None,
            view: None,
            carry: None,
            follow_owed: false,
            zones: Vec::new(),
            library_music: super::music::Music::default(),
            library_layers: [PanelLayers::default(), PanelLayers::default()],
            screens_at: Instant::now(),
            map_art: Vec::new(),
            kept_stages: Vec::new(),
            read: std::sync::mpsc::channel(),
            focus: None,
            keyboard: false,
            drag: None,
            sliding: None,
            expanded: HashSet::new(),
            raised: HashSet::new(),
            more_open: false,
            targets: Vec::new(),
            relabel_at: None,
            stage,
            layers: PanelLayers::default(),
            showing: None,
            last_frame: Instant::now(),
            next_frame: Instant::now(),
            drawn_size: None,
            failed: false,
            largest: SIZES.1,
        };
        ui.raise_more();
        if ui.remote_there {
            ui.ask_remote(|| None);
        }
        ui.read_stages();
        ui.restyle();
        ui.draw(Instant::now());
        UI.with(|cell| *cell.borrow_mut() = Some(ui));
        if !shown {
            return Some(hwnd);
        }
        // Glance started hidden (by the scheduler, an installer, a script)
        // has the system apply that to the first window of its own it
        // shows, in place of what is asked: shown again, it shows.
        let _ = ShowWindow(hwnd, SW_SHOW);
        if !IsWindowVisible(hwnd).as_bool() {
            let _ = ShowWindow(hwnd, SW_SHOW);
        }
        let _ = SetForegroundWindow(hwnd);
        Some(hwnd)
    }
}

/// Runs `act` on the window's state, unless it is already in use further up
/// this thread's stack (a message sent while handling another).
fn with_ui<R>(act: impl FnOnce(&mut Ui) -> R) -> Option<R> {
    UI.with(|cell| cell.try_borrow_mut().ok().and_then(|mut ui| ui.as_mut().map(act)))
}

fn point(lparam: LPARAM) -> (i32, i32) {
    ((lparam.0 & 0xFFFF) as u16 as i16 as i32, ((lparam.0 >> 16) & 0xFFFF) as u16 as i16 as i32)
}

unsafe extern "system" fn procedure(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        FOLLOW => {
            take_settings();
            LRESULT(0)
        }
        TO_OVERLAY => {
            to_overlay();
            LRESULT(0)
        }
        // The screens changed: this thread asks for them anew too (see `screens::changed`).
        WM_DISPLAYCHANGE => {
            crate::screens::changed();
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
        WM_GETMINMAXINFO => {
            let scale = unsafe { GetDpiForWindow(hwnd) } as f32 / 96.0;
            let info = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
            info.ptMinTrackSize = POINT { x: (MIN_SIZE.0 * scale) as i32, y: (MIN_SIZE.1 * scale) as i32 };
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let suggested = unsafe { &*(lparam.0 as *const RECT) };
            with_ui(|ui| ui.scale = ((wparam.0 & 0xFFFF) as f32) / 96.0);
            unsafe {
                let _ = SetWindowPos(
                    hwnd,
                    None,
                    suggested.left,
                    suggested.top,
                    suggested.right - suggested.left,
                    suggested.bottom - suggested.top,
                    SWP_NOZORDER | SWP_NOACTIVATE,
                );
            }
            LRESULT(0)
        }
        // Sized or moved by hand from here till it is let go: drawn as it is sized (see `gfx::sizing`), and once
        // more at rest as it is let go.
        WM_ENTERSIZEMOVE => {
            with_ui(|ui| ui.by_hand = true);
            LRESULT(0)
        }
        WM_EXITSIZEMOVE => {
            with_ui(|ui| {
                ui.by_hand = false;
                ui.draw(Instant::now());
            });
            LRESULT(0)
        }
        // Drawn at once, so the content keeps up while the window is resized.
        WM_SIZE => {
            // Drawn at its new size; minimized, or told of the size it was last drawn at (as it is first shown,
            // drawn already), nothing new to draw. (Its surface keeps what was drawn.)
            let size = ((lparam.0 & 0xFFFF) as u32, ((lparam.0 >> 16) & 0xFFFF) as u32);
            if wparam.0 != windows::Win32::UI::WindowsAndMessaging::SIZE_MINIMIZED as usize {
                with_ui(|ui| {
                    if ui.drawn_size != Some(size) {
                        ui.draw(Instant::now());
                    }
                });
            }
            LRESULT(0)
        }
        WM_MOUSEMOVE => {
            let (x, y) = point(lparam);
            with_ui(|ui| {
                if ui.pointer.is_none() {
                    let mut track = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                    let _ = unsafe { TrackMouseEvent(&mut track) };
                }
                ui.moved(x as f32 / ui.scale, y as f32 / ui.scale);
            });
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with_ui(|ui| {
                ui.pointer = None;
                ui.next_frame = Instant::now();
            });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = point(lparam);
            with_ui(|ui| ui.press(x as f32 / ui.scale, y as f32 / ui.scale));
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = point(lparam);
            with_ui(|ui| ui.release(x as f32 / ui.scale, y as f32 / ui.scale));
            LRESULT(0)
        }
        // The pointer taken away mid-drag (Alt+Tab, another program): let go
        // where it is. (Let go by the window itself, it is already.)
        WM_CAPTURECHANGED => {
            with_ui(|ui| ui.let_go());
            LRESULT(0)
        }
        WM_KEYDOWN | WM_SYSKEYDOWN => {
            let shift = unsafe { GetKeyState(VK_SHIFT.0 as i32) } < 0;
            let alt = unsafe { GetKeyState(VK_MENU.0 as i32) } < 0;
            // Bit 30: the key was already down (an auto-repeat).
            let repeat = lparam.0 & (1 << 30) != 0;
            // A key an input method has taken comes as VK_PROCESSKEY: the key
            // pressed is asked of it.
            let key = match VIRTUAL_KEY(wparam.0 as u16) {
                VK_PROCESSKEY => VIRTUAL_KEY(unsafe { ImmGetVirtualKey(hwnd) } as u16),
                key => key,
            };
            match with_ui(|ui| ui.key(key, shift, alt, repeat)) {
                Some(true) => LRESULT(0),
                _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
            }
        }
        WM_MOUSEWHEEL => {
            let delta = (wparam.0 >> 16) as u16 as i16;
            with_ui(|ui| ui.wheel(delta));
            LRESULT(0)
        }
        WM_SETCURSOR if (lparam.0 & 0xFFFF) as u32 == 1 => {
            // Over the client area: a hand on what can be dragged.
            let grip = with_ui(|ui| matches!(ui.hovered(), Some(Target::Grip(_) | Target::Library(_) | Target::Chip(..))) || ui.drag.is_some() || ui.carry.as_ref().is_some_and(|carry| carry.moved)).unwrap_or(false);
            if grip {
                unsafe { SetCursor(LoadCursorW(None, IDC_HAND).ok()) };
                return LRESULT(1);
            }
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
        // Closed: everything it drew with is let go; the device stays with
        // the panel.
        // Gone to another window: a new shortcut is no longer being taken.
        WM_ACTIVATE if wparam.0 & 0xFFFF == WA_INACTIVE as usize => {
            with_ui(|ui| ui.record(false));
            // No longer above all, gone from the front (see `WM_ACTIVATE`, below).
            unsafe {
                let _ = SetWindowPos(hwnd, Some(HWND_NOTOPMOST), 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE);
            }
            crate::app().controller.settings_front(None);
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
        // In front: the panel, staying up, gives way to it.
        // (Above all while in front, the panel and the widgets below it: they never leave the windows above all to
        // give way to it, as one in front of a window held over a whole screen could not come back among them.)
        WM_ACTIVATE => {
            unsafe {
                let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE);
            }
            crate::app().controller.settings_front(Some(hwnd));
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
        WM_DESTROY => {
            crate::app().controller.settings_front(None);
            // The shortcut, if let go while a new one was taken, is taken up again.
            crate::tray::hold_hotkey(false);
            // Its surfaces and bitmaps go first; then the device gives back
            // what they held (unless desktop widgets still draw with it).
            // Closed mid-drag: what was dragged is kept.
            with_ui(|ui| ui.let_go());
            // (All it drew with goes: see `life`.)
            if let Some(mut ui) = UI.with(|cell| cell.borrow_mut().take()) {
                ui.gone();
            }
            WINDOW.store(0, Ordering::Release);
            crate::playing::show_in_settings(false);
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

/// Sets `field`, a slider's, to `value` in `settings`.
fn put_slider(settings: &mut Settings, field: Field, value: f32) {
    match field {
        Field::PanelSize => settings.panel_size = value,
        Field::OverlayClear => settings.overlay.clear = value,
        _ => settings.overlay.size = value,
    }
}

/// The text of the window in both languages.
fn pick(lang: Lang, zh: &'static str, en: &'static str) -> &'static str {
    lang.pick(zh, en)
}

impl Ui {
    /// Follows the theme and the language now chosen.
    fn restyle(&mut self) {
        // Following the backdrop is the panel's; the window follows the system then.
        self.dark = match self.prefs.theme {
            ThemePref::Light => false,
            ThemePref::Dark => true,
            ThemePref::System | ThemePref::Backdrop => crate::os::apps_dark(),
        };
        self.palette = Palette::new(self.dark);
        unsafe {
            let dark = BOOL::from(self.dark);
            let _ = DwmSetWindowAttribute(self.hwnd, DWMWA_USE_IMMERSIVE_DARK_MODE, &dark as *const _ as *const _, 4);
            let _ = SetWindowTextW(self.hwnd, &HSTRING::from(pick(self.lang, "Glance 设置", "Glance Settings")));
        }
        self.next_frame = Instant::now();
    }

    /// Takes the language now chosen, once its thumb has slid there.
    fn relabel(&mut self) {
        self.relabel_at = None;
        self.lang = Lang::resolve(self.prefs.language);
        self.settle_thumbs = true;
        self.restyle();
    }

    /// Keeps what was changed here and hands it to the panel: only that,
    /// over the settings as they are now (changed elsewhere meanwhile,
    /// perhaps, before this window followed).
    fn save(&mut self) {
        self.settings.view = serde_json::to_value(&self.prefs).unwrap();
        let app = crate::app();
        let mut now = None;
        app.change(|held| {
            *held = crate::settings::merged(held, &self.base, &self.settings);
            now = Some(held.clone());
        });
        self.take(now.unwrap());
        self.next_frame = Instant::now();
    }

    /// Takes `settings`, as they are now, as its own.
    fn take(&mut self, settings: Settings) {
        self.prefs = Prefs::resolve(&settings.view, &crate::app().controller.known_modules());
        self.base = settings.clone();
        self.settings = settings;
    }

    fn rows(&self) -> Vec<Row> {
        let mut rows = vec![Row::Title];
        match (self.page, self.tab) {
            (Page::Layout, _) => rows.extend([
                Row::Library,
                Row::Group(pick(self.lang, "呼出方式", "How it opens")),
                Row::Choice(Field::Anchor),
                Row::Choice(Field::Push),
                Row::Choice(Field::CloseDelay),
                Row::Shortcut,
                Row::Choice(Field::OverFullscreen),
                Row::Switch(Switch::TearOff),
            ]),
            (Page::Monitor, tab) => {
                rows.push(Row::Tabs);
                self.monitor_rows(&mut rows, tab);
            }
            (Page::Workbench, _) => {
                for entry in &self.prefs.workbench {
                    let id = format!("{WORKBENCH}{}", entry.id);
                    rows.push(Row::Module(id.clone()));
                    if self.opened(&id) > 0.0 {
                        rows.extend(self.items_here(&id).into_iter().map(|name| Row::Item(id.clone(), name)));
                    }
                }
                rows.push(Row::Note(pick(
                    self.lang,
                    "工作台放手头常用的东西。时间与媒体在监控和工作台里各有一份，分别开关。中转站、便签、快捷 App 做好后会加到这里",
                    "The workbench holds what is at hand. The time and media are in the monitor and in the workbench alike, each switched on its own. The station, notes and quick launch will come here",
                )));
            }
            (Page::Music, _) => {
                rows.push(Row::Choice(Field::MusicSource));
                rows.extend(super::prefs::items("lyrics").iter().map(|item| Row::Element(item.name)));
                rows.push(Row::Note(pick(
                    self.lang,
                    "查找歌词时会把歌名发给网易云音乐或 LRCLIB，本地歌曲先使用文件自带的歌词。歌词需要播放器提供播放进度（网易云音乐需要 InfLink-rs 插件）",
                    "To find lyrics the track's title is sent to NetEase Cloud Music or LRCLIB; a track played from a file shows its own first. Lyrics need the player to say how far into the track it is (NetEase Cloud Music, with the InfLink-rs plugin)",
                )));
            }
            (Page::General, _) => {
                rows.push(Row::Choice(Field::Language));
                rows.push(Row::Group(pick(self.lang, "手机互联", "Phone")));
                self.remote_rows(&mut rows);
                rows.push(Row::Group(pick(self.lang, "系统", "System")));
                // The switches together, then the buttons.
                rows.extend([Row::Switch(Switch::Startup), Row::Switch(Switch::Updates), Row::Version]);
                if update::available().is_some() {
                    rows.push(Row::Update);
                }
                rows.push(Row::Diagnostics);
                if self.uninstaller.is_some() {
                    rows.push(Row::Uninstall);
                }
                rows.push(Row::Quit);
            }
        }
        rows
    }

    /// The rows of the monitor's tab `tab`, after the tabs.
    fn monitor_rows(&self, rows: &mut Vec<Row>, tab: Tab) {
        match tab {
            Tab::Look => rows.extend([Row::Skins, Row::Slider(Field::PanelSize), Row::Choice(Field::Theme), Row::Switch(Switch::Live), Row::Choice(Field::Columns)]),
            Tab::Shown => {
                let module = |rows: &mut Vec<Row>, id: &str| {
                    rows.push(Row::Module(id.to_string()));
                    // A card's rows, while it is open or still closing.
                    if self.opened(id) > 0.0 {
                        rows.extend(self.items_here(id).into_iter().map(|name| Row::Item(id.to_string(), name)));
                        rows.extend(module_fields(id).iter().map(|field| Row::ModuleChoice(id.to_string(), *field)));
                    }
                };
                let (folded, listed): (Vec<&prefs::ModuleEntry>, Vec<&prefs::ModuleEntry>) =
                    self.prefs.modules.iter().filter(|entry| entry.id != "lyrics").partition(|entry| self.folded(&entry.id));
                for entry in listed {
                    module(rows, &entry.id);
                }
                if !folded.is_empty() {
                    rows.push(Row::More(folded.len()));
                    if self.more_open {
                        for entry in folded {
                            module(rows, &entry.id);
                        }
                    }
                }
            }
            Tab::Overlay => {
                rows.extend([
                    Row::Switch(Switch::Overlay),
                    Row::Switch(Switch::OverlayInGame),
                    Row::Slider(Field::OverlaySize),
                    Row::Slider(Field::OverlayClear),
                    Row::Choice(Field::OverlayLayout),
                ]);
                // Rows only for a strip.
                if self.settings.overlay.layout == Layout::Strip {
                    rows.push(Row::Choice(Field::OverlayRows));
                }
                // What it shows, group by group, as the panel's modules are.
                for entry in &self.settings.overlay.groups {
                    let id = format!("{OVERLAY}{}", entry.id);
                    rows.push(Row::Module(id.clone()));
                    if self.opened(&id) > 0.0 {
                        rows.extend(self.items_here(&id).into_iter().map(|name| Row::Item(id.clone(), name)));
                    }
                }
            }
            Tab::Data => rows.extend([
                Row::Choice(Field::Interval),
                Row::Choice(Field::Span),
                Row::Choice(Field::LoadAlert),
                Row::Choice(Field::TempAlert),
                Row::Switch(Switch::HeatAlert),
            ]),
        }
    }

    /// The phone's rows: the remote on or off, and while it is, what it does and the phones paired with it.
    fn remote_rows(&self, rows: &mut Vec<Row>) {
        rows.push(Row::Switch(Switch::Remote));
        if self.remote {
            rows.push(Row::Switch(Switch::RemoteLights));
            rows.push(Row::Switch(Switch::RemoteScreens));
            // A code shows under the row that asked for it.
            let shown = self.code.as_ref().map(|(_, what)| *what);
            rows.push(Row::GetApp);
            if shown == Some(CodeFor::App) {
                rows.push(Row::Code);
            }
            rows.push(Row::Pair);
            if matches!(shown, Some(CodeFor::Pairing | CodeFor::WebPairing)) {
                rows.push(Row::Code);
            }
            if self.web_address.is_some() {
                rows.push(Row::WebPage);
                if shown == Some(CodeFor::WebPage) {
                    rows.push(Row::Code);
                }
            }
            // The phones paired, each a row.
            rows.extend(self.phones.iter().map(|phone| Row::Phone(phone.id.clone(), phone.name.clone())));
        }
    }

    /// Shows the monitor's tab `tab`, from its top.
    fn turn_tab(&mut self, tab: Tab) {
        if tab != self.tab {
            self.tab = tab;
            self.scroll = 0.0;
            self.scroll_target = 0.0;
            self.drag = None;
            self.raise_more();
        }
        self.next_frame = Instant::now();
    }

    /// Shows page `page`, from its top.
    fn turn_to(&mut self, page: Page) {
        if page != self.page {
            self.page = page;
            self.scroll = 0.0;
            self.scroll_target = 0.0;
            self.drag = None;
            self.raise_more();
        }
        self.next_frame = Instant::now();
    }

    /// The modules of `MORE` listed with the rest from here on: those on; the
    /// fold closed.
    fn raise_more(&mut self) {
        self.raised = self.prefs.modules.iter().filter(|entry| MORE.contains(&entry.id.as_str()) && entry.on).map(|entry| entry.id.clone()).collect();
        self.more_open = false;
    }

    /// Whether module `id` is listed in the fold rather than with the rest.
    fn folded(&self, id: &str) -> bool {
        MORE.contains(&id) && !self.raised.contains(id)
    }

    /// Each row with its top and height, from the top of the list (DIPs).
    fn layout(&self) -> Vec<(Row, f32, f32)> {
        let mut y = PAD_TOP;
        let mut after_heading = false;
        let mut placed = Vec::new();
        // The card whose rows are being placed: its module, where its rows
        // start, and how open it is. Its rows keep their places inside it;
        // what follows it moves up by the part of them still hidden.
        let mut card: Option<(String, f32, f32)> = None;
        for row in self.rows() {
            let inside = match &row {
                Row::Item(id, _) | Row::ModuleChoice(id, _) => Some(id.clone()),
                _ => None,
            };
            if card.as_ref().is_some_and(|(id, ..)| Some(id) != inside.as_ref()) {
                let (_, start, open) = card.take().unwrap();
                y -= (1.0 - open) * (y - start);
            }
            if let Some(id) = inside.filter(|_| card.is_none()) {
                card = Some((id.clone(), y, self.opened(&id)));
            }
            // A heading's space above it takes in the title's below; the
            // first row of a group sits 8 under its heading, the rest 4 apart.
            let (before, height) = match &row {
                Row::Title => (0.0, TITLE),
                Row::Tabs => (0.0, TABS_ROW),
                Row::Note(words) => (12.0, self.gfx.wrapped_height(words, Font::new(Family::Segoe, 12.0, 400.0), PANE - 2.0 * PAD_SIDE - GUTTER - 8.0) + 8.0),
                Row::Slider(_) => (if after_heading { 8.0 } else { ROW_GAP }, ROW),
                Row::Skins => (if after_heading { 8.0 } else { ROW_GAP }, SKINS_ROW),
                Row::Library => (if after_heading { 8.0 } else { ROW_GAP }, self.library_height()),
                // A choice with a word on it: the word above, the choices on
                // a line of their own.
                Row::Choice(field) if self.choice_hint(*field).is_some() => {
                    let options = self.choices(*field).1;
                    let words = self.words_height(&row).max(ROW);
                    (if after_heading { 8.0 } else { ROW_GAP }, if self.stacked(&options) { words + CHOICE_LINE } else { words })
                }
                // Inside its module's card, right under the row above.
                Row::Item(..) => (0.0, (self.words_height(&row) - ROW + ITEM_ROW).max(ITEM_ROW)),
                Row::ModuleChoice(..) => (0.0, ITEM_ROW),
                // As tall as the code, or as the words beside it where those are taller.
                Row::Code => {
                    let words = self.code.as_ref().map_or(0.0, |(_, what)| 22.0 + self.gfx.wrapped_height(&code_words(self.lang, *what, self.web_address.as_deref()).1, Font::new(Family::Segoe, 12.0, 400.0), CODE_WORDS));
                    (ROW_GAP, CODE.max(words) + 2.0 * ROW_SIDE)
                }
                Row::Group(_) => (20.0, GROUP),
                _ => (if after_heading { 8.0 } else { ROW_GAP }, self.words_height(&row).max(ROW)),
            };
            y += before;
            after_heading = matches!(row, Row::Title);
            placed.push((row, y, height));
            y += height;
        }
        if let Some((_, start, open)) = card {
            y -= (1.0 - open) * (y - start);
        }
        let _ = y;
        placed
    }

    /// The preview shows the screen named `screen`: its desktop, at its
    /// size.
    fn show_screen(&mut self, screen: &str) {
        let Some(m) = self.screens.iter().find(|known| known.id == screen).map(|known| known.monitor) else { return };
        let Some(contact) = crate::mouse::monitor_at(POINT { x: (m.left + m.right) / 2, y: (m.top + m.bottom) / 2 }) else { return };
        let work = contact.work;
        let size = (((work.right - work.left) as f32 / contact.scale).round(), ((work.bottom - work.top) as f32 / contact.scale).round());
        // Shown already, and as it is now (its size, scale and place the same).
        let is = |stage: &Stage| stage.screen.as_deref() == Some(screen) && stage.work == work && stage.size == size;
        if is(&self.stage) {
            return;
        }
        // Shown before, as it is now: taken as it was kept; else read from the wallpaper on another thread (shown
        // without it meanwhile).
        let next = match self.kept_stages.iter().position(is) {
            Some(at) => self.kept_stages.swap_remove(at),
            None => {
                self.read_stage(screen.to_string(), work, size);
                Stage { size, screen: Some(screen.to_string()), work, desktop: None, bitmap: None }
            }
        };
        let before = std::mem::replace(&mut self.stage, next);
        // One kept for each screen, as it last was.
        if let Some(of) = before.screen.clone() {
            self.kept_stages.retain(|kept| kept.screen.as_deref() != Some(of.as_str()));
            self.kept_stages.push(before);
        }
        let known: Vec<&str> = self.screens.iter().map(|known| known.id.as_str()).collect();
        self.kept_stages.retain(|kept| kept.screen.as_deref().is_some_and(|id| known.contains(&id)));
    }

    /// Asks the phone remote's helper `ask` (a code, or what to do), then how the remote is, on another thread (see
    /// `remote_asked`); one at a time, a press meanwhile not taken.
    fn ask_remote(&mut self, ask: impl FnOnce() -> Option<Option<(crate::phone::Code, CodeFor)>> + Send + 'static) {
        if self.remote_asked.as_ref().is_some_and(|asked| !asked.is_finished()) {
            return;
        }
        self.remote_asked = Some(std::thread::spawn(move || {
            let code = ask();
            RemoteAnswer { lights: crate::phone::lights(), screens: crate::phone::screens_kept(), address: crate::phone::web_address(), code }
        }));
        self.next_frame = Instant::now();
    }

    /// Reads every screen's desktop as the preview shows it (its wallpaper, at its work area's size), on another
    /// thread: each kept as it comes (see `draw`).
    fn read_stages(&mut self) {
        let wanted = stages_wanted(&self.screens);
        // As they were last read, at once (read again below, and taken if changed).
        for (screen, work, size) in &wanted {
            if let Some(desktop) = kept_desktop(screen, *work, *size) {
                self.kept_stages.retain(|kept| kept.screen.as_deref() != Some(screen.as_str()));
                self.kept_stages.push(Stage { size: *size, screen: Some(screen.clone()), work: *work, desktop: Some(desktop), bitmap: None });
            }
        }
        let send = self.read.0.clone();
        std::thread::spawn(move || {
            for (screen, work, size) in wanted {
                let desktop = read_and_keep(&screen, work, size);
                let stage = Stage { size, screen: Some(screen), work, desktop: Some(desktop), bitmap: None };
                if send.send(Read::Stage(stage)).is_err() {
                    break;
                }
            }
        });
    }

    /// Reads screen `screen`'s desktop for the preview (its work area `work`, `size` DIPs) on another thread.
    fn read_stage(&self, screen: String, work: RECT, size: (f32, f32)) {
        let send = self.read.0.clone();
        std::thread::spawn(move || {
            let desktop = read_and_keep(&screen, work, size);
            let _ = send.send(Read::Stage(Stage { size, screen: Some(screen), work, desktop: Some(desktop), bitmap: None }));
        });
    }

    /// The screen the preview shows, by its name: on the layout's page, the
    /// one its editor shows large; on the overlay's, the one
    /// the overlay was put on; and the one this window is on, for the
    /// other pages and until either was.
    fn stage_screen(&self) -> Option<String> {
        let at = |point: POINT| {
            let here = crate::mouse::monitor_at(point)?.monitor;
            self.screens.iter().find(|screen| screen.monitor == here).map(|screen| screen.id.clone())
        };
        let chosen = match self.page {
            Page::Layout => self.editor_screen(),
            Page::Monitor if self.tab == Tab::Overlay => self.settings.overlay.screen.and_then(|(x, y)| at(POINT { x, y })),
            _ => None,
        };
        chosen.or_else(|| {
            let mut window = RECT::default();
            let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowRect(self.hwnd, &mut window) };
            at(POINT { x: (window.left + window.right) / 2, y: (window.top + window.bottom) / 2 })
        })
    }

    /// The component the preview shows: the workbench on its page, what plays on the music's, else the readings.
    fn previewed(&self) -> Part {
        match self.page {
            Page::Workbench => Part::Workbench,
            Page::Music => Part::Music,
            _ => Part::Monitor,
        }
    }

    /// The edge the preview's panel opens from, on the screen it shows: the first of that screen's that opens what it
    /// shows, else the first that opens anything; where none does, the one a panel opened by hand opens from.
    fn preview_edge(&self) -> Edge {
        let places = self.settings.places();
        let screen = self.stage.screen.as_deref().unwrap_or_default();
        let lit = places.lit(screen);
        let part = self.previewed();
        lit.each().find(|edge| places.part(screen, *edge) == Some(part)).or(lit.each().next()).unwrap_or(places.by_hand().0)
    }

    /// The desktop of the screen named `id` at `monitor`, `size` physical
    /// pixels large, for the layout's editor: taken once, and again when its
    /// size changes.
    fn map_picture(&mut self, frame: &Frame, id: &str, monitor: RECT, size: (u32, u32)) -> Option<ID2D1Bitmap1> {
        // The picture held is drawn smaller or larger to the size wanted. Read again (on another thread, the one held
        // drawn meanwhile) only where it is smaller than that, or more than twice as large; and not while the window
        // is sized by hand (each size of it would read the desktop again): once it is let go.
        let holds = |asked: (u32, u32)| asked.0 >= size.0 && asked.1 >= size.1 && asked.0 <= size.0 * 2 && asked.1 <= size.1 * 2;
        let found = self.map_art.iter().position(|art| art.id == id && art.monitor == monitor);
        let at = match found {
            Some(at) if holds(self.map_art[at].asked) || self.by_hand => at,
            _ => {
                let send = self.read.0.clone();
                let of = id.to_string();
                std::thread::spawn(move || {
                    let desktop = read_desktop(monitor, size.0, size.1);
                    let _ = send.send(Read::Map { id: of, monitor, size, desktop });
                });
                match found {
                    Some(at) => {
                        self.map_art[at].asked = size;
                        at
                    }
                    None => {
                        self.map_art.retain(|art| art.id != id);
                        self.map_art.push(MapArt { id: id.to_string(), monitor, asked: size, desktop: None, bitmap: None });
                        self.map_art.len() - 1
                    }
                }
            }
        };
        let art = &mut self.map_art[at];
        if art.bitmap.is_none() {
            art.bitmap = art.desktop.as_ref().and_then(|desktop| desktop.bitmap(&frame.dc, 1.0).ok());
        }
        art.bitmap.clone()
    }

    /// How tall a row has to be for its name and its word, the word broken
    /// into lines in the room it has beside the row's controls; none taller
    /// than a row for a row without a word.
    fn words_height(&self, row: &Row) -> f32 {
        let width = PANE - 2.0 * PAD_SIDE - GUTTER;
        let font = Font::new(Family::Segoe, 14.0, 400.0);
        let button = |text: &str| self.gfx.measure(text, font) + 32.0;
        let (word, room): (Cow<'static, str>, f32) = match row {
            Row::Switch(switch) => match self.switch(*switch).1 {
                Some(word) => (word, width - 2.0 * ROW_SIDE - 40.0 - 16.0),
                None => return 0.0,
            },
            Row::Choice(field) => {
                let Some(word) = self.choice_hint(*field) else { return 0.0 };
                let options = self.choices(*field).1;
                let room = if self.stacked(&options) { width - 2.0 * ROW_SIDE } else { width - 2.0 * ROW_SIDE - self.segmented_width(&options) - 16.0 };
                (Cow::Borrowed(word), room)
            }
            Row::Element(name) => match self.item_label("lyrics", name).1 {
                Some(word) => (Cow::Borrowed(word), width - 2.0 * ROW_SIDE - 40.0 - 16.0),
                None => return 0.0,
            },
            Row::Item(id, name) => match self.item_label(id, name).1 {
                Some(word) => (Cow::Borrowed(word), width - ROW_SIDE - CHEVRON_ROOM - 40.0 - 12.0 - ITEM_INSET),
                None => return 0.0,
            },
            _ => match self.button_row(row, Instant::now()) {
                Some((_, word, buttons)) => {
                    let taken: f32 = buttons.iter().map(|(text, ..)| button(text) + 8.0).sum();
                    (Cow::Owned(word), width - 2.0 * ROW_SIDE - taken - 8.0)
                }
                None => return 0.0,
            },
        };
        // A 20 DIP line for the name, 2 apart, the word's lines, and room
        // above and below.
        22.0 + self.gfx.wrapped_height(&word, Font::new(Family::Segoe, 12.0, 400.0), room.max(40.0)) + 18.0
    }

    /// How wide a row of choices is.
    fn segmented_width(&self, options: &[String]) -> f32 {
        let font = Font::new(Family::Segoe, 13.0, 400.0);
        options.iter().map(|o| (self.gfx.measure(o, font) + 24.0).max(44.0)).sum::<f32>() + 4.0
    }

    /// Whether a row of choices with a word goes on a line of its own under
    /// its name: when beside it, it would leave the word too narrow a column
    /// to read (a few characters to a line).
    fn stacked(&self, options: &[String]) -> bool {
        let width = PANE - 2.0 * PAD_SIDE - GUTTER - 2.0 * ROW_SIDE;
        width - self.segmented_width(options) - 16.0 < MIN_WORDS
    }

    /// What a row of buttons says, its word, and its buttons, right to left:
    /// each one's text, what it does, and how it looks. None for a row of
    /// another kind, or an update row with no update.
    fn button_row(&self, row: &Row, now: Instant) -> Option<ButtonRow> {
        let lang = self.lang;
        let p = |zh, en| pick(lang, zh, en).to_string();
        Some(match row {
            Row::Update => {
                let (version, state) = update::available()?;
                let name = match lang {
                    Lang::Zh => format!("Glance {version} 可用"),
                    Lang::En => format!("Glance {version} is available"),
                };
                let (detail, action) = match state {
                    update::State::Ready => (p("下载并运行已签名的安装程序", "Downloads and runs the signed installer"), p("更新", "Update")),
                    update::State::Downloading => (p("正在下载安装程序", "Downloading the installer"), p("下载中", "Downloading")),
                    update::State::Failed => (p("下载失败，请稍后重试", "Download failed; try again later"), p("重试", "Retry")),
                    update::State::Rejected => (p("签名或版本不符，未运行；请手动下载", "Signature or version did not match; not run. Download it yourself"), p("重试", "Retry")),
                };
                let kind = if state == update::State::Downloading { Button::Quiet } else { Button::Accent };
                (name, detail, vec![(action, Target::Update, kind)])
            }
            Row::Version => {
                let version = env!("CARGO_PKG_VERSION");
                let name = match lang {
                    Lang::Zh => format!("当前版本 {version}"),
                    Lang::En => format!("Version {version}"),
                };
                let detail = match update::check() {
                    update::Check::Checking => p("正在检查…", "Checking…"),
                    update::Check::Answered if update::available().is_some() => p("有新版本，见下方", "A newer one is out; see below"),
                    update::Check::Answered => p("已是最新", "The latest"),
                    update::Check::Unanswered => p("没能连上 GitHub 和 Gitee，稍后再试", "GitHub and Gitee did not answer; try later"),
                    update::Check::Idle => p("还没有检查过", "Not checked yet"),
                };
                let button = if update::check() == update::Check::Checking {
                    (p("检查中", "Checking"), Target::CheckNow, Button::Quiet)
                } else {
                    (p("检查更新", "Check for updates"), Target::CheckNow, Button::Plain)
                };
                (name, detail, vec![button])
            }
            Row::Shortcut => {
                let detail = match (self.recording, self.needs_modifier, self.settings.shortcut) {
                    (true, true, _) => p("要配合 Ctrl、Alt 或 Win 一起按", "Hold Ctrl, Alt or Win with it"),
                    (true, false, _) => p("按下新的组合键，Esc 取消", "Press the new keys; Esc to cancel"),
                    (false, _, None) => p("未设置，点右边的按钮设置", "None; click the button to set one"),
                    (false, _, Some(_)) if crate::tray::hotkey_taken() => p("已被其他程序占用，请换一个", "Another program is using it; choose another"),
                    (false, _, Some(_)) => p("打开或收起面板", "Opens and closes the panel"),
                };
                let keys = match (self.recording, self.settings.shortcut) {
                    (true, _) => (p("请按键…", "Press keys…"), Target::Shortcut, Button::Taking),
                    (false, Some(shortcut)) => (crate::tray::shortcut_name(shortcut), Target::Shortcut, Button::Plain),
                    (false, None) => (p("未设置", "None"), Target::Shortcut, Button::Plain),
                };
                // With a shortcut set, a button to clear it, right of its keys.
                let mut buttons = Vec::new();
                if self.settings.shortcut.is_some() {
                    buttons.push((p("清除", "Clear"), Target::ClearShortcut, Button::Plain));
                }
                buttons.push(keys);
                (p("快捷键", "Shortcut"), detail, buttons)
            }
            Row::Diagnostics => {
                // "Copied" for a moment after the press.
                let copied = self.copied_at.is_some_and(|at| now.duration_since(at) < COPIED_FOR);
                let action = if copied { p("已复制", "Copied") } else { p("复制", "Copy") };
                // What became of the last sending, once there was one.
                let (detail, send) = match (&self.sending, &self.sent) {
                    (Some(_), _) => (p("正在发送…", "Sending…"), (p("发送中", "Sending"), Target::SendReport, Button::Quiet)),
                    (None, Some(Some(id))) => (
                        match lang {
                            Lang::Zh => format!("已发送，编号 {id}。向我们提问时请附上这个编号"),
                            Lang::En => format!("Sent, as {id}. Please quote it when asking us about it"),
                        },
                        (p("再发一次", "Send again"), Target::SendReport, Button::Plain),
                    ),
                    (None, Some(None)) => (p("没有发出去，可以先复制", "It did not get there; copy it instead"), (p("重试", "Retry"), Target::SendReport, Button::Plain)),
                    (None, None) => (p("硬件识别不对、灯光或显示器控制不了时，发给我们或复制附上", "When hardware is read wrongly or lights or monitors cannot be set: send it to us, or copy it into a report"), (p("发送", "Send"), Target::SendReport, Button::Plain)),
                };
                (p("诊断信息", "Diagnostics"), detail, vec![(action, Target::Diagnostics, Button::Plain), send])
            }
            Row::Uninstall => (
                p("卸载 Glance", "Uninstall Glance"),
                p("连同设置一起删除，会先确认", "Removes it and its settings; asks first"),
                vec![(p("卸载", "Uninstall"), Target::Uninstall, Button::Plain)],
            ),
            Row::GetApp => (
                p("下载手机 APP", "Get the phone app"),
                p("安卓手机扫码下载；iPhone 暂请使用下方的网页遥控", "Scan with an Android phone to download it; on an iPhone, use the web page below for now"),
                vec![(if self.code.as_ref().is_some_and(|(_, what)| *what == CodeFor::App) { p("收起", "Hide") } else { p("显示二维码", "Show the code") }, Target::GetApp, Button::Plain)],
            ),
            Row::Pair => (
                p("配对手机", "Pair a phone"),
                p("每个配对码仅供一部手机使用，10 分钟内有效", "Each code is for one phone, and lasts ten minutes"),
                vec![(p("用 APP", "For the app"), Target::Pair(false), Button::Accent), (p("用网页", "For the web page"), Target::Pair(true), Button::Plain)],
            ),
            Row::WebPage => (
                p("重新打开网页遥控", "Open the web page again"),
                p("已通过网页配对的手机，扫码即可再次打开", "A phone paired by the web page scans this to open it again"),
                vec![(if self.code.as_ref().is_some_and(|(_, what)| *what == CodeFor::WebPage) { p("收起", "Hide") } else { p("显示二维码", "Show the code") }, Target::OpenPage, Button::Plain)],
            ),
            Row::Phone(at, name) => (
                if name.is_empty() { p("一部手机", "A phone") } else { name.clone() },
                p("已配对", "Paired"),
                vec![(p("移除", "Remove"), Target::Unpair(at.clone()), Button::Plain)],
            ),
            Row::Quit => (
                p("退出 Glance", "Quit Glance"),
                p("面板和托盘图标都会关闭", "Closes the panel and the tray icon"),
                vec![(p("退出", "Quit"), Target::Quit, Button::Plain)],
            ),
            _ => return None,
        })
    }

    /// How open module `id`'s card is: 0 closed, 1 open, between while it
    /// slides.
    fn opened(&self, id: &str) -> f32 {
        let open = if self.expanded.contains(id) { 1.0 } else { 0.0 };
        self.motion.get(&format!("open:{id}")).map_or(open, |t| t.value(Instant::now()).clamp(0.0, 1.0))
    }

    /// Opens or closes module `id`'s card, sliding unless `at_once`.
    fn set_open(&mut self, id: &str, open: bool, at_once: bool) {
        if open {
            self.expanded.insert(id.to_string());
        } else {
            self.expanded.remove(id);
        }
        self.animate(format!("open:{id}"), if open { 1.0 } else { 0.0 }, SLIDE, Instant::now(), at_once);
        self.next_frame = Instant::now();
    }

    fn content_height(&self) -> f32 {
        self.layout().last().map_or(0.0, |(_, y, h)| y + h) + PAD_BOTTOM
    }

    /// The labels of a row of choices, and which is chosen.
    fn choices(&self, field: Field) -> (&'static str, Vec<String>, Option<usize>) {
        let lang = self.lang;
        let s = |zh: &'static str, en: &'static str| lang.pick(zh, en).to_string();
        let seconds = |n: &str| if lang == Lang::Zh { format!("{n} 秒") } else { format!("{n} s") };
        let minutes = |n: &str| if lang == Lang::Zh { format!("{n} 分") } else { format!("{n} min") };
        let (settings, prefs) = (&self.settings, &self.prefs);
        let at = |values: &[u64], value: u64| values.iter().position(|&v| v == value);
        match field {
            Field::Theme => (
                pick(lang, "明暗", "Theme"),
                vec![s("跟随系统", "System"), s("浅色", "Light"), s("深色", "Dark"), s("跟随背景", "Backdrop")],
                [ThemePref::System, ThemePref::Light, ThemePref::Dark, ThemePref::Backdrop].iter().position(|&t| t == prefs.theme),
            ),
            Field::Language => (
                pick(lang, "语言", "Language"),
                vec![s("跟随系统", "System"), "中文".into(), "English".into()],
                [LanguagePref::System, LanguagePref::Zh, LanguagePref::En].iter().position(|&l| l == prefs.language),
            ),
            Field::OverlayLayout => (
                pick(lang, "布局", "Layout"),
                vec![s("卡片", "Card"), s("横条", "Strip")],
                [Layout::Card, Layout::Strip].iter().position(|&l| l == settings.overlay.layout),
            ),
            Field::MusicSource => {
                let mut options = vec![s("自动", "Automatic")];
                options.extend(self.sources.iter().map(|(_, name)| name.clone()));
                let chosen = match &settings.music_source {
                    Some(source) => self.sources.iter().position(|(id, _)| *id == source.id).map(|at| at + 1),
                    None => Some(0),
                };
                (pick(lang, "音乐来源", "Music from"), options, chosen)
            }
            Field::OverlayRows => (
                pick(lang, "行数", "Rows"),
                ROWS.iter().map(|rows| rows.to_string()).collect(),
                ROWS.iter().position(|&rows| rows == settings.overlay.rows),
            ),
            Field::Anchor => (
                pick(lang, "面板位置", "Position"),
                vec![s("鼠标处", "By the mouse"), s("边缘正中", "Centred")],
                [Anchor::Pointer, Anchor::Center].iter().position(|&a| a == settings.anchor),
            ),
            Field::OverFullscreen => (
                pick(lang, "全屏游戏中呼出", "Over fullscreen games"),
                vec![s("不呼出", "Never"), s("仅快捷键", "Shortcut"), s("快捷键和推边缘", "Shortcut and edge")],
                [OverFullscreen::Never, OverFullscreen::Shortcut, OverFullscreen::Both].iter().position(|&o| o == settings.over_fullscreen),
            ),
            Field::Columns => (
                pick(lang, "面板栏数", "Columns"),
                vec![s("自动", "Auto"), "1".into(), "2".into(), "3".into(), "4".into()],
                [None, Some(1), Some(2), Some(3), Some(4)].iter().position(|&c| c == settings.columns),
            ),
            Field::Push => (
                pick(lang, "推边缘呼出", "Push into the edge"),
                vec![s("轻", "Light"), s("中", "Medium"), s("重", "Firm")],
                [Sensitivity::Light, Sensitivity::Medium, Sensitivity::Firm].iter().position(|&p| p == settings.sensitivity),
            ),
            Field::CloseDelay => (
                pick(lang, "离开后收起", "Close after"),
                vec![s("立即", "At once"), seconds("0.5"), seconds("1")],
                at(&[200, 500, 1000], settings.close_delay_ms),
            ),
            Field::RateUnit => (pick(lang, "网速单位", "Network unit"), vec!["MB/s".into(), "Mbps".into()], Some(prefs.network.bits as usize)),
            Field::ProcessCount => (
                pick(lang, "进程显示行数", "Rows of processes"),
                vec!["5".into(), "8".into(), "12".into()],
                at(&[5, 8, 12], prefs.processes.count as u64),
            ),
            Field::ProcessSort => (
                pick(lang, "进程排序", "Sort processes by"),
                vec!["CPU".into(), s("内存", "Memory"), s("读写", "I/O"), "GPU".into()],
                [ProcessSort::Cpu, ProcessSort::Memory, ProcessSort::Io, ProcessSort::Gpu].iter().position(|&p| p == prefs.processes.sort),
            ),
            Field::Interval => (
                pick(lang, "刷新间隔", "Refresh every"),
                vec![seconds("0.5"), seconds("1"), seconds("2")],
                at(&[500, 1000, 2000], settings.interval_ms),
            ),
            Field::Span => (
                pick(lang, "曲线时长", "Graph spans"),
                vec![seconds("30"), minutes("1"), minutes("2"), minutes("5")],
                at(&[30, 60, 120, 300], prefs.chart_seconds as u64),
            ),
            Field::OverlaySize | Field::OverlayClear | Field::PanelSize => (self.slider(field).0, Vec::new(), None),
            Field::LoadAlert => (pick(lang, "负载警示", "Load alert"), vec!["70%".into(), "85%".into(), "95%".into()], at(&[70, 85, 95], prefs.hot_load as u64)),
            Field::TempAlert => (
                pick(lang, "温度警示", "Temperature alert"),
                vec!["75 °C".into(), "85 °C".into(), "95 °C".into()],
                at(&[75, 85, 95], prefs.hot_temp as u64),
            ),
        }
    }

    fn choose(&mut self, field: Field, index: usize) {
        let (settings, prefs) = (&mut self.settings, &mut self.prefs);
        match field {
            Field::Theme => prefs.theme = [ThemePref::System, ThemePref::Light, ThemePref::Dark, ThemePref::Backdrop][index],
            Field::Language => prefs.language = [LanguagePref::System, LanguagePref::Zh, LanguagePref::En][index],
            Field::Anchor => settings.anchor = [Anchor::Pointer, Anchor::Center][index],
            Field::OverlayLayout => settings.overlay.layout = [Layout::Card, Layout::Strip][index],
            Field::OverlayRows => settings.overlay.rows = ROWS[index],
            Field::MusicSource => {
                settings.music_source = index.checked_sub(1).and_then(|at| self.sources.get(at)).map(|(id, name)| crate::settings::MusicSource { id: id.clone(), name: name.clone() });
            }
            Field::Columns => settings.columns = [None, Some(1), Some(2), Some(3), Some(4)][index],
            Field::OverFullscreen => settings.over_fullscreen = [OverFullscreen::Never, OverFullscreen::Shortcut, OverFullscreen::Both][index],
            Field::Push => settings.sensitivity = [Sensitivity::Light, Sensitivity::Medium, Sensitivity::Firm][index],
            Field::CloseDelay => settings.close_delay_ms = [200, 500, 1000][index],
            Field::RateUnit => prefs.network.bits = index == 1,
            Field::ProcessCount => prefs.processes.count = [5, 8, 12][index],
            Field::ProcessSort => prefs.processes.sort = [ProcessSort::Cpu, ProcessSort::Memory, ProcessSort::Io, ProcessSort::Gpu][index],
            Field::Interval => settings.interval_ms = [500, 1000, 2000][index],
            Field::Span => prefs.chart_seconds = [30.0, 60.0, 120.0, 300.0][index],
            Field::LoadAlert => prefs.hot_load = [70.0, 85.0, 95.0][index],
            Field::OverlaySize | Field::OverlayClear | Field::PanelSize => {}
            Field::TempAlert => prefs.hot_temp = [75.0, 85.0, 95.0][index],
        }
        self.save();
        match field {
            Field::Theme => self.restyle(),
            // Everything is relabelled in the new language once the thumb
            // has slid; mid-way it would jump.
            Field::Language => self.relabel_at = Some(Instant::now() + SLIDE.0),
            _ => {}
        }
    }

    /// Starts or stops taking a new shortcut. Meanwhile the old one is let
    /// go, or its keys would never come here.
    fn record(&mut self, on: bool) {
        if on == self.recording {
            return;
        }
        self.recording = on;
        self.needs_modifier = false;
        crate::tray::hold_hotkey(on);
        self.next_frame = Instant::now();
    }

    /// No shortcut: nothing opens the panel by keys.
    fn clear_shortcut(&mut self) {
        self.settings.shortcut = None;
        self.save();
        self.record(false);
    }

    /// A key pressed while taking a new shortcut: Escape gives up, Backspace
    /// or Delete clears the shortcut, a modifier alone waits for the key it
    /// goes with, and a key with Ctrl, Alt or Win held becomes the shortcut.
    fn take_shortcut(&mut self, key: VIRTUAL_KEY) {
        self.next_frame = Instant::now();
        let down = |key: VIRTUAL_KEY| unsafe { GetKeyState(key.0 as i32) } < 0;
        let modifiers = [VK_SHIFT, VK_CONTROL, VK_MENU, VK_LSHIFT, VK_RSHIFT, VK_LCONTROL, VK_RCONTROL, VK_LMENU, VK_RMENU, VK_LWIN, VK_RWIN];
        if key == VK_ESCAPE {
            self.record(false);
            return;
        }
        if key == VK_BACK || key == VK_DELETE {
            self.clear_shortcut();
            return;
        }
        if modifiers.contains(&key) {
            return;
        }
        let shortcut = Shortcut { ctrl: down(VK_CONTROL), alt: down(VK_MENU), shift: down(VK_SHIFT), win: down(VK_LWIN) || down(VK_RWIN), key: key.0 };
        if !shortcut.usable() {
            // Noted, for a report of a shortcut that would not take.
            crate::journal::note(format!("shortcut: key 0x{:02X} without Ctrl, Alt or Win", key.0));
            self.needs_modifier = true;
            return;
        }
        self.settings.shortcut = Some(shortcut);
        self.save();
        self.record(false);
    }

    /// A slider's name, its least and most values, its step, and its value.
    fn slider(&self, field: Field) -> (&'static str, f32, f32, f32, f32) {
        let lang = self.lang;
        let (sizes, overlay) = (SIZES, &self.settings.overlay);
        match field {
            // No larger than the screen holds it: at that, said so.
            Field::PanelSize => {
                // (On a step, as the slider moves by them: a most between two could never be reached.)
                let most = (sizes.0 + ((self.largest - sizes.0) / 0.05 + 1e-4).floor() * 0.05).clamp(sizes.0, sizes.1);
                let value = self.settings.panel_size.clamp(sizes.0, most);
                let name = if most < sizes.1 && value >= most - 0.001 { pick(lang, "面板大小（已达屏幕上限）", "Panel size (as large as this screen holds)") } else { pick(lang, "面板大小", "Panel size") };
                (name, sizes.0, most, 0.05, value)
            }
            Field::OverlayClear => (pick(lang, "玻璃透明度", "Glass clarity"), 0.0, 1.0, 0.05, overlay.clear.clamp(0.0, 1.0)),
            _ => (pick(lang, "大小", "Size"), sizes.0, sizes.1, 0.05, overlay.size.clamp(sizes.0, sizes.1)),
        }
    }

    /// Sets a slider's value, to its nearest step; the panel and the overlay
    /// show it at once, with the settings as they are held (it is kept by
    /// `keep_slider`).
    fn set_slider(&mut self, field: Field, value: f32) {
        let (_, min, max, step, now) = self.slider(field);
        let value = (min + ((value - min) / step).round() * step).clamp(min, max);
        // (Slid within a step, nothing changes: the panel and the overlay are not made to draw themselves anew
        // at every move of the pointer.)
        if value == now {
            return;
        }
        put_slider(&mut self.settings, field, value);
        crate::app().preview(|live| put_slider(live, field, value));
        self.next_frame = Instant::now();
    }

    /// Keeps a slider's value, and only it: the settings changed elsewhere
    /// while it slid (the overlay placed, a game's offer made) stand.
    fn keep_slider(&mut self, field: Field) {
        let value = self.slider(field).4;
        let mut now = None;
        crate::app().change(|held| {
            put_slider(held, field, value);
            now = Some(held.clone());
        });
        self.take(now.unwrap());
        self.next_frame = Instant::now();
    }

    /// A slider slid to `x` along its `track`.
    fn slide(&mut self, field: Field, track: Rect, x: f32) {
        let (_, min, max, ..) = self.slider(field);
        let share = ((x - track.x - SLIDER_THUMB) / (track.w - 2.0 * SLIDER_THUMB)).clamp(0.0, 1.0);
        self.set_slider(field, min + share * (max - min));
    }

    /// A word under a row of choices' name, for the few that need one.
    fn choice_hint(&self, field: Field) -> Option<&'static str> {
        match field {
            Field::MusicSource => Some(pick(
                self.lang,
                "自动时，音乐组件只跟随音乐播放器，视频不算。浏览器无法区分音乐和视频，在网页上听歌时请在这里选择浏览器。时间与媒体仍然跟随任何正在播放的内容",
                "Automatic, the music component follows music players, not videos. A browser does not tell music from video: to follow music in a web page, choose the browser here. The time and media still follow whatever plays",
            )),
            Field::Anchor => Some(pick(self.lang, "面板沿屏幕边缘弹出，出现在鼠标所在的位置，或边缘正中", "Along the edge: where the mouse is, or in the middle")),
            Field::Push => Some(pick(
                self.lang,
                "鼠标移到实线边后再往外推一下就会打开；力度越重越不容易误触",
                "Move the mouse to a solid edge and push on: the panel opens. Firmer is harder to set off by accident",
            )),
            Field::OverFullscreen => Some(pick(
                self.lang,
                "全屏和无边框全屏的游戏都算。全屏模式的游戏会暂时切出，收起面板后自动回来",
                "Fullscreen and borderless games alike. A game in exclusive fullscreen steps out until the panel closes",
            )),
            _ => None,
        }
    }

    /// A switch's label, its hint, and whether it is on.
    fn switch(&self, switch: Switch) -> (&'static str, Option<Cow<'static, str>>, bool) {
        let lang = self.lang;
        let p = |zh, en| pick(lang, zh, en);
        let (name, hint, on) = match switch {
            Switch::Live => (
                p("背景实时刷新", "Live backdrop"),
                Some(p("玻璃与亚克力材质背后的画面将实时更新；背景变化时，会占用一定的处理器与显卡资源", "What shows behind glass and acrylic is updated in real time; while the background changes, some processor and graphics resources are used")),
                self.settings.live_backdrop,
            ),
            Switch::TearOff => (
                p("拖出面板成为小组件", "Drag the panel off as a widget"),
                Some(p("按住面板拖离屏幕边缘，即可将其转为桌面小组件；关闭后，拖动面板将不再生效", "Drag the panel away from the screen edge to turn it into a desktop widget; when off, dragging the panel has no effect")),
                self.settings.tear_off,
            ),
            // Only a copy no ordinary program can replace (one the installer
            // put in Program Files) may start elevated unasked.
            Switch::Startup => (
                p("开机时启动", "Start with Windows"),
                (!self.may_autostart).then(|| {
                    p("当前安装位置不受保护，无法启用；点击查看原因", "Unavailable in this location; click for details")
                }),
                self.autostart,
            ),
            Switch::Overlay => (
                p("悬浮窗", "Overlay"),
                Some(p("始终显示在屏幕上。可拖动调整位置，滚轮调透明度；鼠标移到它上面会出现按钮：锁定位置、鼠标穿透、关闭和设置", "Always on screen. Drag to move; the wheel sets how clear it is; the pointer over it brings its buttons: lock, click-through, close and settings")),
                self.settings.overlay.on,
            ),
            Switch::OverlayInGame => (
                p("游戏时自动显示", "Show while playing"),
                Some(p(
                    "全屏游戏运行期间显示，游戏结束后隐藏。帧率、1% low 与帧时间仅在游戏期间显示",
                    "Shown while a fullscreen game runs, hidden when it ends. Frame rate, 1% low and frame time show only during a game",
                )),
                self.settings.overlay.in_game,
            ),
            Switch::HeatAlert => (
                p("过热提醒", "Heat alert"),
                Some(p("达到温度警示值 30 秒后从托盘提醒", "A tray warning after 30 s at the alert")),
                self.settings.heat_alert,
            ),
            Switch::RemoteLights => {
                let hint = if self.fetching.is_some() {
                    p("正在处理，下载需要一些时间…", "Working; the download takes a moment…")
                } else if self.fetch_failed {
                    p("下载未成功，请稍后重试", "The download did not succeed; try again later")
                } else {
                    p("主板、显卡等的灯光由开源软件 OpenRGB 控制。首次开启时将从其官方地址下载（约 22 MB）；关闭后不再控制灯光，并删除已下载的 OpenRGB", "A board's and a card's lights are set through OpenRGB, an open-source program. Turned on first, it is downloaded from its own project (about 22 MB); turned off, those lights are left alone and the download is removed")
                };
                return (p("控制灯光", "Control the lights"), Some(Cow::Borrowed(hint)), self.remote_lights);
            }
            Switch::RemoteScreens => (
                p("关屏时保持显示器连接", "Keep monitors connected while off"),
                Some(p(
                    "适用于显示器同时连接其他电脑、鼠标键盘接在显示器上，或长时间关屏后窗口移至其他屏幕的情况。可单独关闭的显示器照常关闭，其余显示器以黑色画面覆盖并调至最暗，背光保持开启",
                    "For a monitor that has another computer plugged in too, a mouse or keyboard plugged into a monitor, or windows that move to other screens after a long while dark. A monitor that can be turned off by itself still is; the others are covered in black and made as dim as they go, their backlight still on",
                )),
                self.remote_screens,
            ),
            Switch::Remote => {
                let hint = if self.remote_there {
                    p("通过手机关闭屏幕与灯光、调节音量和亮度。开启后将运行一个不提权的辅助程序，仅对本地网络开放", "Screens, lights, volume and brightness from a phone. On, a small program runs (never elevated), open to the local network only")
                } else {
                    p("此版本的 Glance 未附带遥控程序", "This copy of Glance came without the remote")
                };
                return (p("开启手机互联", "Use the phone"), Some(Cow::Borrowed(hint)), self.remote);
            }
            // The version this is, where the user looks for a newer one.
            Switch::Updates => {
                return (p("自动检查更新", "Check for updates by itself"), Some(Cow::Borrowed(p("每天检查一次，同时向 Glance 的网站告知当前版本，用于统计使用情况；不含任何可识别这台电脑的信息", "Once a day; Glance's site is told this version, for its count of how many run. Nothing that tells this PC from another"))), self.settings.check_updates);
            }
        };
        (name, hint.map(Cow::Borrowed), on)
    }

    /// Why Start with Windows cannot be turned on for this copy, and how it can.
    fn explain_no_autostart(&self) {
        let place = std::env::current_exe().ok().and_then(|exe| exe.parent().map(|dir| dir.display().to_string())).unwrap_or_default();
        let (title, text) = match self.lang {
            Lang::Zh => (
                "无法启用开机时启动".to_string(),
                format!(
                    "Glance 当前位于：\n{place}\n\n这个位置不受保护：普通程序可以修改它或它的上级文件夹，因此可能把 Glance 替换掉。开机时启动会让 Glance 不经确认就以管理员身份运行，只有在受保护的位置才能安全地这样做。\n\n如需启用，请使用安装程序将 Glance 安装到 Program Files，或磁盘根目录下的新文件夹（例如 D:\\Glance）。"
                ),
            ),
            Lang::En => (
                "Start with Windows is unavailable".to_string(),
                format!(
                    "Glance is currently located in:\n{place}\n\nThis location is not protected: ordinary programs can change it or a folder above it, and so could replace Glance. Starting with Windows runs Glance with administrator rights without asking, which is safe only from a protected location.\n\nTo turn it on, use the installer to install Glance to Program Files, or to a new folder at the root of a drive (for example D:\\Glance)."
                ),
            ),
        };
        unsafe { MessageBoxW(Some(self.hwnd), &HSTRING::from(text), &HSTRING::from(title), MB_OK | MB_ICONINFORMATION) };
    }

    fn flip(&mut self, switch: Switch) {
        let settings = &mut self.settings;
        match switch {
            Switch::Live => settings.live_backdrop ^= true,
            Switch::TearOff => settings.tear_off ^= true,
            Switch::Updates => settings.check_updates ^= true,
            Switch::HeatAlert => settings.heat_alert ^= true,
            Switch::Overlay => {
                settings.overlay.on ^= true;
                settings.overlay.offered = true;
            }
            Switch::OverlayInGame => {
                settings.overlay.in_game ^= true;
                settings.overlay.offered = true;
            }
            Switch::Startup if !self.may_autostart && !self.autostart => {
                self.explain_no_autostart();
                return;
            }
            Switch::RemoteLights => {
                // Fetched (or taken away) on a thread of its own: the window goes on meanwhile.
                if self.fetching.is_none() {
                    let wanted = !self.remote_lights;
                    self.fetch_failed = false;
                    self.fetching = Some(std::thread::spawn(move || crate::phone::set_lights(wanted) == wanted));
                }
                self.next_frame = Instant::now();
                return;
            }
            Switch::RemoteScreens => {
                // What the remote then says, not what was asked (see `tick`).
                let kept = !self.remote_screens;
                self.ask_remote(move || {
                    crate::phone::keep_screens(kept);
                    None
                });
                self.next_frame = Instant::now();
                return;
            }
            Switch::Remote => {
                // What the system then reports, not what was asked (see `tick`).
                if self.remote_there && self.switching.is_none() {
                    let on = !self.remote;
                    self.switching = Some((SwitchAsked::Remote, std::thread::spawn(move || crate::source::switch(crate::link::Switch::Remote, on))));
                }
                self.next_frame = Instant::now();
                return;
            }
            Switch::Startup => {
                // What the system then reports, not what was asked (see `tick`).
                if self.switching.is_none() {
                    let on = !self.autostart;
                    self.switching = Some((SwitchAsked::Startup, std::thread::spawn(move || crate::source::switch(crate::link::Switch::Autostart, on))));
                }
                self.next_frame = Instant::now();
                return;
            }
        }
        self.save();
    }

    /// The diagnostics put together: shown whole, with room for the user's own words and pictures, and sent if
    /// they agree.
    fn send_report(&mut self, report: String) {
        let lang = self.lang;
        let asked = super::ask::Asked {
            title: pick(lang, "发送诊断信息", "Send diagnostics"),
            intro: pick(
                lang,
                "下面是将要发送给 Glance（glancepc.com）的全部内容：你写的话、你添加的截图，和最下面的诊断信息（这台电脑的硬件、Glance 读到的数值和它最近的日志）。不包含你的文件、账号或网络地址。",
                "Below is all that would be sent to Glance (glancepc.com): what you write, the pictures you add, and the diagnostics at the bottom (this PC's hardware, what Glance reads of it, and Glance's recent log). Nothing of your files, accounts or network address.",
            ),
            own: Some(super::ask::Own {
                words: pick(lang, "你想说的话（可不填）", "Your own words (may be left empty)"),
                hint: pick(lang, "比如：主板温度没有显示；风扇转速和别的软件里看到的不一样", "For instance: the board's temperature is missing; a fan's speed is not what another program shows"),
                pictures: pick(lang, "截图（可不添，最多 3 张）", "Pictures (if you like, up to 3)"),
                add: pick(lang, "添加截图", "Add a picture"),
                ways: pick(lang, "点击选择、拖进窗口，或按 Ctrl+V 粘贴", "Click to choose one, drop it here, or paste it with Ctrl+V"),
                paste: pick(lang, "粘贴剪贴板里的截图", "Paste the clipboard's"),
                waiting: pick(lang, "剪贴板里有截图，按 Ctrl+V 即可粘贴", "There is a picture on the clipboard: Ctrl+V pastes it"),
                kinds: pick(lang, "图片", "Pictures"),
            }),
            sent: Some((pick(lang, "诊断信息", "Diagnostics"), &report)),
            yes: pick(lang, "发送", "Send"),
            no: pick(lang, "取消", "Cancel"),
            dark: self.dark,
        };
        if let Some(said) = super::ask::show(self.hwnd, &asked) {
            // (Their words first, set off from what Glance says of the PC.)
            let report = if said.words.is_empty() { report } else { format!("The user says: {}

{report}", said.words) };
            self.sent = None;
            self.sending = Some(std::thread::spawn(move || crate::diagnostics::send(&report, &said.pictures)));
        }
    }

    /// A module's title and what it describes on this machine.
    fn module(&self, id: &str) -> (String, Option<String>) {
        if let Some(bare) = id.strip_prefix(WORKBENCH) {
            return self.module(bare);
        }
        let (lang, info) = (self.lang, &crate::app().info);
        let p = |zh, en| pick(lang, zh, en).to_string();
        if let Some(group) = id.strip_prefix(OVERLAY) {
            return match group {
                "frames" => (p("帧率", "Frame rate"), Some(p("游戏时显示，其余时候显示“—”", "While a game plays; “—” otherwise"))),
                "cpu" => ("CPU".into(), None),
                "gpu" => ("GPU".into(), None),
                "memory" => (p("内存", "Memory"), None),
                "vram" => (p("显存", "Video memory"), None),
                "network" => (p("网速", "Network"), None),
                "clock" => (p("时间", "Time"), Some(p("现在几点", "The time of day"))),
                _ => (p("麦克风", "Microphone"), Some(p("默认麦克风是否静音，静音时标红", "Whether the default microphone is muted: red while it is"))),
            };
        }
        match id {
            "cpu" => ("CPU".into(), Some(info.cpu_name.clone())),
            _ if id.starts_with("gpu:") => ("GPU".into(), info.gpu_of(id).map(|i| info.gpus[i].name.clone())),
            "memory" => (p("内存", "Memory"), info.memory_modules.clone()),
            "network" => (p("网络", "Network"), Some(info.network_adapter.clone().unwrap_or_else(|| p("所有物理网卡的上下行速度", "Traffic over every physical adapter")))),
            "disk" => (p("磁盘", "Disk"), (!info.drives.is_empty()).then(|| info.drives.join(", "))),
            "processes" => (p("进程", "Processes"), Some(p("占用最多的程序，点表头排序", "The busiest programs; click a heading to sort"))),
            "storage" => (p("存储", "Storage"), Some(p("各分区的空间", "Space on each drive"))),
            // A board whose sensors are not read says so, rather than
            // looking like a switch that does nothing.
            "board" if crate::app().controller.seen.lock().unwrap().board.is_none() => (
                p("主板", "Motherboard"),
                Some(p("没有读到这块主板的传感器，原因见“通用”页的诊断信息", "This board's sensors were not read; the diagnostics on the General page say why")),
            ),
            "board" => (p("主板", "Motherboard"), (!info.board.is_empty()).then(|| info.board.clone())),
            "battery" => (p("电池", "Battery"), Some(p("笔记本电脑", "Laptops"))),
            "game" => (p("游戏", "Game"), Some(p("全屏游戏的帧率、帧时间和它占用的资源", "A fullscreen game's frame rate, frame times and what it uses"))),
            "docker" => ("Docker".into(), Some(p("运行中的容器各占多少处理器、内存、网络和磁盘，问的是 Docker 当前连接的引擎", "What each container running uses, as the engine Docker's current context points at says"))),
            "wsl" => ("WSL".into(), Some(p("WSL 2 的处理器和内存占用，只从 Windows 读取，不会启动或唤醒发行版", "WSL 2's processors and memory, read from Windows alone: no distribution is started or kept up"))),
            "time" => (p("时间与媒体", "Time and media"), Some(p("时间和日期，以及音量和正在播放的内容", "The time and the date, the sound's level and what is playing"))),
            "lyrics" => (
                p("音乐", "Music"),
                Some(p(
                    "正在播放的歌曲：封面、播放进度、播放控制、音量，以及随播放滚动的歌词。查找歌词时会把歌名发给网易云音乐或 LRCLIB，本地歌曲先使用文件自带的歌词；歌词需要播放器提供播放进度（网易云音乐需要 InfLink-rs 插件）",
                    "What plays: its cover, its track, its controls, how loud, and its lyrics as it plays. To find lyrics the track's title is sent to NetEase Cloud Music or LRCLIB; a track played from a file shows its own first. Lyrics need the player to say how far into the track it is (NetEase Cloud Music, with the InfLink-rs plugin)",
                )),
            ),
            _ => (p("系统", "System"), Some(p("开机时长、进程和句柄数", "Uptime, processes, handles"))),
        }
    }

    /// Whether module `id` is on.
    fn module_on(&self, id: &str) -> bool {
        if let Some(bare) = id.strip_prefix(WORKBENCH) {
            return self.prefs.workbench.iter().any(|entry| entry.id == bare && entry.on);
        }
        match id.strip_prefix(OVERLAY) {
            Some(group) => self.settings.overlay.groups.iter().any(|entry| entry.id == group && entry.on),
            None => self.prefs.modules.iter().any(|entry| entry.id == id && entry.on),
        }
    }

    /// The list a row's module (`id`) is in, the readings', the workbench's or the overlay's, and its id there.
    fn list<'a>(&mut self, id: &'a str) -> (&mut Vec<super::prefs::ModuleEntry>, &'a str) {
        if let Some(bare) = id.strip_prefix(WORKBENCH) {
            return (&mut self.prefs.workbench, bare);
        }
        match id.strip_prefix(OVERLAY) {
            Some(group) => (&mut self.settings.overlay.groups, group),
            None => (&mut self.prefs.modules, id),
        }
    }

    /// Whether item `name` of a row's module `id` is chosen.
    fn item_on(&self, id: &str, name: &str) -> bool {
        if let Some(bare) = id.strip_prefix(WORKBENCH) {
            return prefs::shows_in(&self.prefs.workbench, bare, name);
        }
        match id.strip_prefix(OVERLAY) {
            Some(group) => super::overlay::shows(&self.settings.overlay.groups, group, name),
            None => self.prefs.shows(id, name),
        }
    }

    /// The items of module `id` this machine has shown it can read: only
    /// those are listed.
    fn items_here(&self, id: &str) -> Vec<String> {
        if let Some(bare) = id.strip_prefix(WORKBENCH) {
            return self.items_here(bare);
        }
        // An overlay's group of one item has no card: its switch is the item's.
        if let Some(group) = id.strip_prefix(OVERLAY) {
            let items = super::overlay::GROUPS.iter().find(|(g, _)| *g == group).map_or(&[][..], |(_, items)| *items);
            return if items.len() > 1 { items.iter().map(|(name, _)| name.to_string()).collect() } else { Vec::new() };
        }
        let app = crate::app();
        let seen = app.controller.seen.lock().unwrap();
        let info = &app.info;
        let gpu = info.gpu_of(id).map(|index| (&info.gpus[index], seen.gpus.get(index).cloned().unwrap_or_default()));
        // A module this machine has shown nothing of (no battery, no board
        // sensors, a GPU not read) has no lane, and nothing to set.
        let present = match id.split(':').next().unwrap_or(id) {
            "battery" => seen.battery,
            "board" => seen.board.is_some(),
            "gpu" => gpu.as_ref().is_some_and(|(_, seen)| seen.present),
            _ => true,
        };
        if !present {
            return Vec::new();
        }
        prefs::items(id)
            .iter()
            .map(|item| item.name)
            .filter(|name| match (id.split(':').next().unwrap_or(id), *name) {
                (_, "chart") => true,
                ("cpu", "temp") => seen.cpu_temp,
                ("cpu", "clock") => seen.cpu_clock,
                ("cpu", "power") => seen.cpu_power,
                ("cpu", "ccds") => seen.ccds.len() > 1,
                ("cpu", "threads") => seen.threads > 0,
                ("gpu", name) => gpu.as_ref().is_some_and(|(info, seen)| match name {
                    "temp" => seen.temp,
                    "vram" => info.mem_total > 0,
                    "clock" => seen.clock,
                    "power" => seen.power,
                    "fan" => seen.fan,
                    "hotspot" => seen.hot_spot,
                    "shared" => info.shared_total > 0,
                    _ => !seen.engines.is_empty(),
                }),
                ("memory", "dimms") => seen.dimms > 0,
                ("memory", "speed") => app.info.memory_speed.is_some(),
                ("battery", "power") => seen.battery_power,
                ("battery", "health") => seen.battery_health,
                ("network", "address") => seen.address,
                ("network", "wifi") => seen.wifi,
                ("network", "link") => seen.link,
                ("disk", "drives") => !seen.drives.is_empty() || info.drives.len() > 1,
                ("disk", "active") => seen.disk_active,
                ("storage", "volumes") => !seen.volumes.is_empty(),
                ("time", "lyrics") => prefs::LYRICS_OFFERED,
                _ => true,
            })
            .flat_map(|name| match (id, name) {
                // A switch for each sensor the board has shown.
                ("board", "temps" | "fans") => {
                    let board = seen.board.clone().unwrap_or_default();
                    let sensors = if name == "temps" { board.temps } else { board.fans };
                    sensors.into_iter().map(|sensor| format!("{name}:{sensor}")).collect()
                }
                // A switch for each drive.
                ("storage", "volumes") => seen.volumes.iter().map(|volume| format!("volumes:{volume}")).collect(),
                _ => vec![name.to_string()],
            })
            .collect()
    }

    /// Whether module `id` has a card to open: items, or choices of its own.
    fn opens(&self, id: &str) -> bool {
        !module_fields(id.trim_start_matches(WORKBENCH)).is_empty() || !self.items_here(id).is_empty()
    }

    /// What an item of module `id` is called, and a word on it.
    fn item_label(&self, id: &str, name: &str) -> (Cow<'static, str>, Option<&'static str>) {
        let id = id.trim_start_matches(WORKBENCH);
        match name.split_once(':') {
            // A drive, by its letter.
            Some(("volumes", volume)) => (Cow::Owned(volume.to_string()), None),
            // One of the board's sensors, by its name on the panel.
            Some((group, sensor)) => (Cow::Owned(super::view::board_sensor(sensor, group, self.lang)), None),
            None => {
                let (label, word) = self.module_item_label(id, name);
                (Cow::Borrowed(label), word)
            }
        }
    }

    /// What one of the items every module of kind `id` has is called, and a word on it.
    fn module_item_label(&self, id: &str, name: &str) -> (&'static str, Option<&'static str>) {
        let p = |zh, en| pick(self.lang, zh, en);
        match (id.split(':').next().unwrap_or(id), name) {
            ("overlay", "fps") => (p("帧率", "Frame rate"), None),
            ("overlay", "time") => (p("时间", "Time"), None),
            ("overlay" | "time", "twelve") => (p("12 小时制", "12-hour clock"), Some(p("关闭时为 24 小时制", "Off, a 24-hour clock"))),
            ("time", "sound") => (p("音量", "Volume"), Some(p("拖动调节音量，可静音", "Dragged to set how loud the sound is; muted too"))),
            ("time", "media") => (p("正在播放", "Now playing"), Some(p("可暂停、播放和切换曲目", "Paused, played and skipped"))),
            ("time", "lyrics") => (p("歌词", "Lyrics"), Some(p("正在唱的这一句和下一句；会把歌名发给网易云音乐或 LRCLIB 来查找", "The line sung now and the next; the track's title is sent to NetEase Cloud Music or LRCLIB to find them"))),
            ("lyrics", "translation") => (p("翻译", "Translation"), Some(p("有翻译时显示在正在唱的这一句下面", "Under the line sung now, where there is one"))),
            ("lyrics", "cover") => (p("封面", "Cover"), Some(p("音乐组件显示专辑封面", "The music widget shows the album cover"))),
            ("lyrics", "title") => (p("歌名与歌手", "Title and artist"), None),
            ("lyrics", "spectrum") => (p("律动条", "Spectrum"), Some(p("随音乐跳动的频谱", "Bars moving with the music"))),
            ("lyrics", "controls") => (p("常驻播放控件", "Controls always shown"), Some(p("关闭时，播放控件在鼠标移入后出现", "Off, the controls show as the pointer comes over the widget"))),
            ("lyrics", "clear") => (p("无背景", "No background"), Some(p("去掉音乐组件的底色，文字加描边，在任何桌面上都能看清", "The music widget without its ground, its words edged to be read on any desktop"))),
            ("overlay", "low") => (p("1% low", "1% low"), Some(p("最慢 1% 的帧换算成的帧率", "The slowest 1% of frames, as frames a second"))),
            ("overlay", "frametime") => (p("帧时间", "Frame time"), Some(p("最近一秒里最慢的一帧", "The slowest frame of the last second"))),
            ("overlay", "cpu" | "gpu") => (p("占用", "Use"), None),
            ("overlay", "down") => (p("下载", "Download"), None),
            ("overlay", "up") => (p("上传", "Upload"), None),
            ("overlay", _) if name.ends_with("_temp") => (p("温度", "Temperature"), None),
            ("overlay", _) if name.ends_with("_power") => (p("功耗", "Power"), None),
            ("network" | "disk", "chart") => (p("速率图表", "Rate chart"), None),
            ("battery", "chart") => (p("电量图表", "Charge chart"), None),
            ("battery", "power") => (p("功率", "Power"), Some(p("充电或放电的功率；拔掉电源时就是整台电脑的耗电", "Watts going in or out: on battery, what the whole machine draws"))),
            ("battery", "health") => (p("健康度", "Health"), Some(p("充满时的容量占出厂设计容量的比例", "What it holds when full, against what it was made to hold"))),
            ("game", "chart") => (p("帧率图表", "Frame rate chart"), None),
            ("game", "low") => (p("1% low", "1% low"), Some(p("最慢 1% 的帧换算成的帧率", "The slowest 1% of frames, as frames a second"))),
            ("game", "longest") => (p("最长一帧", "Longest frame"), Some(p("最近一秒里最慢的一帧，卡顿时会变大", "The slowest frame of the last second: a stutter shows here"))),
            ("game", "frametimes") => (
                p("帧时间图表", "Frame time chart"),
                Some(p("每秒最长一帧的走势，卡顿是一根尖刺；开着时最长一帧显示在图旁", "The longest frame of each second: a stutter is a spike. With it on, the longest frame shows beside it")),
            ),
            ("game", "usage") => (p("游戏占用", "Game's use"), Some(p("这个游戏自己用了多少 CPU 和 GPU", "How much of the CPU and the GPU the game itself uses"))),
            ("game", "memory") => (p("游戏内存", "Game's memory"), Some(p("这个游戏自己占的内存和显存", "The memory and video memory the game itself holds"))),
            ("game", "limit") => (p("显卡限制", "GPU limit"), Some(p("显卡是否被功耗墙或温度墙压住了频率（N 卡）", "Whether the GPU's clock is held back by its power or temperature limit (NVIDIA)"))),
            ("game", "mic") => (p("麦克风", "Microphone"), Some(p("默认麦克风是否静音，静音时标红", "Whether the default microphone is muted: red while it is"))),
            ("game", "time") => (p("游玩时长", "Time played"), Some(p("这次玩了多久，离开不到 5 分钟不重新计时", "How long this session has run; away for under 5 minutes, it carries on"))),
            ("wsl", "memory") => (p("内存", "Memory"), Some(p("WSL 占用的内存和它最多能用的内存；开着占用图表时画出走势", "What WSL holds of the memory, and the most it may; charted with the usage chart on"))),
            ("wsl", "distros") => (p("运行中的发行版", "Distributions running"), None),
            ("wsl", "gpu") => ("GPU", Some(p("WSL 里的程序（CUDA、图形界面等）占用的 GPU", "The GPU that programs in WSL use (CUDA, their windows)"))),
            (_, "chart") => (p("占用图表", "Usage chart"), Some(p("关闭后只显示数字，面板更紧凑", "Off, the figures alone: a more compact panel"))),
            ("system", "uptime") => (p("开机时长", "Uptime"), None),
            ("system", "processes") => (p("进程数", "Processes"), None),
            ("system", "threads") => (p("线程数", "Threads"), None),
            ("system", "handles") => (p("句柄数", "Handles"), None),
            (_, "temp") => (p("温度", "Temperature"), None),
            (_, "clock") => (p("频率", "Clock"), None),
            (_, "power") => (p("功耗", "Power"), None),
            (_, "ccds") => (p("各 CCD 温度", "Each CCD's temperature"), None),
            (_, "threads") => (p("线程", "Threads"), Some(p("每个线程的占用", "Each thread's load"))),
            (_, "vram") => (p("显存", "Video memory"), None),
            (_, "fan") => (p("风扇", "Fan"), None),
            (_, "hotspot") => (p("热点温度", "Hot spot"), Some(p("芯片上最热一点的温度；只有驱动单独提供它的显卡才有", "The warmest point of the chip, on the cards whose driver says it apart"))),
            (_, "shared") => (p("共享显存", "Shared memory"), Some(p("从内存借用的部分", "Borrowed from system memory"))),
            (_, "engines") => (p("各引擎", "Engines"), Some(p("3D、复制、视频编解码", "3D, copy, video"))),
            (_, "dimms") => (p("内存条温度", "Module temperatures"), None),
            ("memory", "speed") => (p("内存频率", "Memory speed"), Some(p("内存当前的运行频率；低于内存条的额定频率时一并显示，多半是 XMP 或 EXPO 没打开", "What the memory runs at; below what the modules are rated for, that too (XMP or EXPO off, often)"))),
            (_, "committed") => (p("已提交", "Committed"), None),
            (_, "cached") => (p("缓存", "Cached"), None),
            (_, "adapter") => (p("网卡型号", "Adapter"), None),
            (_, "wifi") => (p("Wi-Fi 名称", "Wi-Fi name"), Some(p("连接的无线网络；Windows 要求打开“定位服务”才提供", "The wireless network it is on; Windows tells it only with its location service on"))),
            (_, "address") => (p("地址", "Address"), None),
            (_, "link") => (p("链路速度", "Link speed"), None),
            (_, "totals") => (p("开机以来的流量", "Traffic since boot"), None),
            (_, "drives") => (p("各硬盘温度", "Drive temperatures"), None),
            (_, "active") => (p("活动时间", "Active time"), None),
            (_, "scale") => (p("满刻度", "Chart scale"), Some(p("曲线图顶端代表的速度", "The rate the top of the chart stands for"))),
            _ => (p("其他", "Other"), None),
        }
    }

    // ---- Input ----

    fn hovered(&self) -> Option<Target> {
        let (x, y) = self.pointer?;
        self.targets.iter().find(|(r, _)| x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h).map(|(_, t)| t.clone())
    }

    fn moved(&mut self, x: f32, y: f32) {
        self.pointer = Some((x, y));
        if let Some((part, track, at)) = &mut self.phone.slid {
            *at = part.along(*track, x, y);
        }
        if let Some((field, track)) = self.sliding {
            self.slide(field, track, x);
        }
        if let Some(drag) = &mut self.drag {
            drag.pointer = y + self.scroll;
            self.reorder();
        }
        self.carry_to(x, y);
        self.next_frame = Instant::now();
    }

    fn press(&mut self, x: f32, y: f32) {
        self.pointer = Some((x, y));
        if self.page == Page::General && self.press_phone(x, y) {
            return;
        }
        self.pressed = self.hovered();
        // A press anywhere else gives up taking a new shortcut.
        if self.recording && !matches!(self.pressed, Some(Target::Shortcut | Target::ClearShortcut)) {
            self.record(false);
        }
        // A press moves the focus there too, without showing it.
        if let Some(target) = &self.pressed {
            self.keyboard = false;
            self.focus = Some(self.stop_for(target));
        }
        // A slider takes the value where it is pressed, and follows the
        // pointer until it is let go.
        if let Some(Target::Slider(field)) = self.pressed {
            if let Some((track, _)) = self.targets.iter().find(|(_, target)| *target == Target::Slider(field)) {
                let track = *track;
                self.sliding = Some((field, track));
                self.slide(field, track, x);
                unsafe { SetCapture(self.hwnd) };
            }
        }
        if let Some(target) = self.pressed.clone() {
            if self.take_up(&target, x, y) {
                self.next_frame = Instant::now();
                return;
            }
        }
        if let Some(Target::Grip(id)) = &self.pressed {
            // A card travels closed.
            let id = id.clone();
            self.set_open(&id, false, true);
            let id = &id;
            let top = self.row_top(id);
            self.drag = Some(Drag { id: id.clone(), grab: y + self.scroll - top, pointer: y + self.scroll });
            unsafe { SetCapture(self.hwnd) };
        }
        self.next_frame = Instant::now();
    }

    /// A slider or a module let go (or the pointer taken away mid-drag, by
    /// another window or the window closing): kept where it was left.
    /// Settings changed elsewhere, taken up once nothing is in hand: a slider slid shows its value live through
    /// the panel's settings, a row or a component dragged is kept as it is let go; taken meanwhile, what is in hand
    /// went back where it was.
    fn follow_when_free(&mut self) {
        if !self.follow_owed || self.sliding.is_some() || self.drag.is_some() || self.carry.is_some() {
            return;
        }
        self.follow_owed = false;
        let settings = crate::app().settings.lock().unwrap().clone();
        self.take(settings);
        self.next_frame = Instant::now();
    }

    fn let_go(&mut self) {
        // A component in hand is put down nowhere: where it came from, it stays. (Taken up and not yet moved too:
        // left, it went on following the pointer with no button held, and the next press put it down.)
        if self.carry.take().is_some() {
            self.pressed = None;
        }
        // The phone's slider held is let go where it was, nothing sent (left, it followed the pointer, and the
        // next press anywhere sent its value).
        self.phone.slid = None;
        // Shown live as it slid.
        if let Some((field, _)) = self.sliding.take() {
            self.pressed = None;
            self.keep_slider(field);
        }
        if let Some(drag) = self.drag.take() {
            // The held row settles into its place.
            let slot = self.row_top(&drag.id);
            let now = Instant::now();
            let mut settle = Transition::settled(drag.pointer - drag.grab - slot);
            settle.retarget(0.0, SETTLE.0, SETTLE.1, now);
            self.motion.insert(format!("row:{}", drag.id), settle);
            self.pressed = None;
            self.save();
        }
    }

    fn release(&mut self, x: f32, y: f32) {
        self.pointer = Some((x, y));
        if self.release_phone(x, y) {
            return;
        }
        if self.sliding.is_some() || self.drag.is_some() {
            unsafe {
                let _ = ReleaseCapture();
            }
            self.let_go();
            return;
        }
        if self.carry.is_some() {
            self.put_down();
            self.next_frame = Instant::now();
            return;
        }
        let pressed = self.pressed.take();
        if pressed.is_none() || pressed != self.hovered() {
            self.next_frame = Instant::now();
            return;
        }
        // The focus follows the choice a click made.
        let target = pressed.unwrap();
        self.activate(target.clone());
        self.focus = Some(self.stop_for(&target));
    }

    /// Does what pressing `target` does.
    fn activate(&mut self, target: Target) {
        match target {
            Target::Skin(skin) => {
                self.settings.skin = match skin {
                    Skin::Paper => "paper",
                    Skin::Glass => "glass",
                    Skin::Fluent => "fluent",
                }
                .into();
                self.save();
            }
            Target::Choice(field, index) => self.choose(field, index),
            // (Carried, or pressed and let go: see `put_down`.)
            Target::Library(_) | Target::Chip(..) => {}
            Target::View(_) | Target::Same | Target::Zone(..) | Target::Unplace(..) => {
                self.press_editor(&target);
            }
            Target::Switch(switch) => self.flip(switch),
            Target::Shortcut => self.record(!self.recording),
            Target::ClearShortcut => self.clear_shortcut(),
            Target::Nav(item) => self.turn_to(item.page()),
            Target::Tab(tab) => self.turn_tab(tab),
            Target::Element(name) => {
                let on = self.item_on("lyrics", name);
                self.prefs.set_item("lyrics", name, !on);
                self.save();
            }
            Target::CheckNow => update::check_now(update::Asker::Settings),
            Target::Module(id) => {
                let (list, bare) = self.list(&id);
                if let Some(entry) = list.iter_mut().find(|entry| entry.id == bare) {
                    entry.on ^= true;
                }
                self.save();
            }
            Target::Expand(id) => {
                let open = !self.expanded.contains(&id);
                self.set_open(&id, open, false);
            }
            Target::More => {
                self.more_open ^= true;
                self.animate("chevron:more".into(), if self.more_open { 180.0 } else { 0.0 }, SLIDE, Instant::now(), false);
            }
            // An item is changed only while its module is on.
            Target::Item(id, name) if self.module_on(&id) => {
                let on = self.item_on(&id, &name);
                match (id.strip_prefix(OVERLAY), id.strip_prefix(WORKBENCH)) {
                    (Some(group), _) => super::overlay::set_item(&mut self.settings.overlay.groups, group, &name, !on),
                    (None, Some(bare)) => prefs::set_item_in(&mut self.prefs.workbench, bare, &name, !on),
                    (None, None) => self.prefs.set_item(&id, &name, !on),
                }
                self.save();
            }
            Target::Item(..) => {}
            // The uninstaller asks first and closes Glance itself.
            Target::Uninstall => {
                // Checked again as it is run: the window may have been open a while.
                if let Some(uninstaller) = self.uninstaller.as_deref().and_then(elevation::trusted) {
                    let path = HSTRING::from(uninstaller.as_os_str());
                    unsafe { ShellExecuteW(Some(self.hwnd), w!("open"), &path, PCWSTR::null(), PCWSTR::null(), SW_SHOWNORMAL) };
                }
            }
            Target::Update => update::install(),
            Target::Diagnostics if self.reporting.is_none() => self.reporting = Some((ReportFor::Copy, std::thread::spawn(crate::diagnostics::report))),
            Target::Diagnostics => {}
            Target::Quit => crate::quit(),
            // Put together first (see `tick`), then shown whole: nothing leaves this PC that was not read and agreed to.
            Target::SendReport if self.sending.is_none() && self.reporting.is_none() => self.reporting = Some((ReportFor::Send, std::thread::spawn(crate::diagnostics::report))),
            Target::SendReport => {}
            // (Pressed again, its code is put away.)
            Target::GetApp if self.code.as_ref().is_some_and(|(_, what)| *what == CodeFor::App) => self.code = None,
            // (Each asked of the remote's helper on another thread: see `tick`.)
            Target::GetApp => self.ask_remote(|| Some(crate::phone::app_code().map(|code| (code, CodeFor::App)))),
            Target::Pair(web) => self.ask_remote(move || Some(crate::phone::offer(web).map(|code| (code, if web { CodeFor::WebPairing } else { CodeFor::Pairing })))),
            Target::OpenPage if self.code.as_ref().is_some_and(|(_, what)| *what == CodeFor::WebPage) => self.code = None,
            // (Where it is now: this PC's address may have changed.)
            Target::OpenPage => self.ask_remote(|| Some(crate::phone::web_address().as_deref().and_then(crate::phone::code_of).map(|code| (code, CodeFor::WebPage)))),
            Target::Unpair(id) => {
                self.ask_remote(move || {
                    crate::phone::unpair(&id);
                    None
                });
            }
            // Slid by the pointer, stepped by the arrows; pressing alone does nothing more.
            Target::Grip(_) | Target::Slider(_) => {}
        }
        self.next_frame = Instant::now();
    }

    // ---- Keyboard ----

    /// The focus stop a target belongs to: a row of choices, or the skin
    /// cards, is one stop, at the option chosen.
    fn stop_for(&self, target: &Target) -> Target {
        match target {
            Target::Choice(field, _) => Target::Choice(*field, self.choices(*field).2.unwrap_or(0)),
            Target::Skin(_) => Target::Skin(Skin::named(&self.settings.skin)),
            Target::Grip(id) => Target::Module(id.clone()),
            other => other.clone(),
        }
    }

    /// Every focus stop, in the order the list shows them (scrolled out of
    /// view or not).
    fn stops(&self) -> Vec<Target> {
        let pages = NAV_ITEMS.iter().filter(|item| **item != NavItem::Components).map(|item| Target::Nav(*item));
        let editor = if self.page == Page::Layout { self.editor_stops() } else { Vec::new() };
        pages.chain(self.layout().into_iter()
            .flat_map(|(row, ..)| match row {
                Row::Skins => vec![Target::Skin(Skin::named(&self.settings.skin))],
                // The screens; then each edge where the pointer stops at
                // it, then where it passes on.
                Row::Library => vec![],
                Row::Choice(field) => vec![Target::Choice(field, self.choices(field).2.unwrap_or(0))],
                // A switch's own button comes first.
                Row::Shortcut if self.settings.shortcut.is_some() => vec![Target::Shortcut, Target::ClearShortcut],
                Row::Shortcut => vec![Target::Shortcut],
                Row::Version if update::check() == update::Check::Checking => vec![],
                Row::Version => vec![Target::CheckNow],
                Row::Switch(switch) => vec![Target::Switch(switch)],
                Row::Slider(field) => vec![Target::Slider(field)],
                // A module's row, which opens its card if it has one, then its switch.
                Row::Module(id) if self.opens(&id) => vec![Target::Expand(id.clone()), Target::Module(id)],
                Row::Module(id) => vec![Target::Module(id)],
                Row::Item(id, name) if self.module_on(&id) && self.expanded.contains(&id) => vec![Target::Item(id, name)],
                Row::ModuleChoice(id, field) if self.module_on(&id) && self.expanded.contains(&id) => vec![Target::Choice(field, self.choices(field).2.unwrap_or(0))],
                Row::Item(..) | Row::ModuleChoice(..) => vec![],
                Row::Update if update::available().is_some_and(|(_, state)| state == update::State::Downloading) => vec![],
                Row::Update => vec![Target::Update],
                Row::Diagnostics => vec![Target::SendReport, Target::Diagnostics],
                Row::Uninstall => vec![Target::Uninstall],
                Row::Quit => vec![Target::Quit],
                Row::GetApp => vec![Target::GetApp],
                Row::Pair => vec![Target::Pair(false), Target::Pair(true)],
                Row::WebPage => vec![Target::OpenPage],
                Row::Phone(id, _) => vec![Target::Unpair(id)],
                Row::Code | Row::Group(_) => vec![],
                Row::More(_) => vec![Target::More],
                Row::Tabs => TABS.iter().map(|tab| Target::Tab(*tab)).collect(),
                Row::Element(name) => vec![Target::Element(name)],
                Row::Title | Row::Note(_) => vec![],
            }))
            .chain(editor)
            .collect()
    }

    /// A key pressed (`repeat` when it is held down and auto-repeating): Tab
    /// and Shift+Tab move between controls, the arrows change a row's
    /// choice, Space and Enter press, Alt with the up and down arrows moves a
    /// module, Escape closes the window. Returns whether the key was used;
    /// any other combination with Alt is left to the system.
    fn key(&mut self, key: VIRTUAL_KEY, shift: bool, alt: bool, repeat: bool) -> bool {
        if self.recording {
            self.take_shortcut(key);
            return true;
        }
        let moves_module = alt && (key == VK_UP || key == VK_DOWN) && matches!(self.focus, Some(Target::Module(_) | Target::Expand(_)));
        if alt && !moves_module {
            return false;
        }
        // The focus shows once a key that moves it or acts on it is used
        // (not for a key that does nothing here: one of a shortcut, say).
        if matches!(key, VK_TAB | VK_SPACE | VK_RETURN | VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN) {
            self.keyboard = true;
        }
        self.next_frame = Instant::now();
        let stops = self.stops();
        let at = self.focus.as_ref().and_then(|focus| stops.iter().position(|stop| stop == focus));
        match key {
            VK_TAB if !stops.is_empty() => {
                let next = match (at, shift) {
                    (None, false) => 0,
                    (None, true) => stops.len() - 1,
                    (Some(i), false) => (i + 1) % stops.len(),
                    (Some(i), true) => (i + stops.len() - 1) % stops.len(),
                };
                self.focus = Some(stops[next].clone());
                self.reveal_focus();
            }
            VK_ESCAPE => unsafe {
                let _ = PostMessageW(Some(self.hwnd), WM_CLOSE, WPARAM(0), LPARAM(0));
            },
            // Holding the key presses once.
            VK_SPACE | VK_RETURN if repeat => {}
            VK_SPACE | VK_RETURN => {
                if let Some(focus) = self.focus.clone() {
                    self.activate(focus);
                }
            }
            VK_LEFT | VK_RIGHT | VK_UP | VK_DOWN => {
                let forward = key == VK_RIGHT || key == VK_DOWN;
                match self.focus.clone() {
                    Some(Target::Module(id) | Target::Expand(id)) if moves_module => self.shift_module(&id, forward),
                    Some(Target::Slider(field)) => {
                        let (_, min, max, step, value) = self.slider(field);
                        let next = (value + if forward { step } else { -step }).clamp(min, max);
                        if next != value {
                            self.set_slider(field, next);
                            self.keep_slider(field);
                        }
                    }
                    Some(Target::Choice(field, index)) => {
                        let count = self.choices(field).1.len();
                        let next = if forward { (index + 1).min(count - 1) } else { index.saturating_sub(1) };
                        if next != index {
                            self.activate(Target::Choice(field, next));
                            self.focus = Some(Target::Choice(field, next));
                        }
                    }
                    Some(Target::Skin(skin)) => {
                        let order = [Skin::Paper, Skin::Glass, Skin::Fluent];
                        let index = order.iter().position(|s| *s == skin).unwrap_or(0);
                        let next = if forward { (index + 1).min(order.len() - 1) } else { index.saturating_sub(1) };
                        if next != index {
                            self.activate(Target::Skin(order[next]));
                            self.focus = Some(Target::Skin(order[next]));
                        }
                    }
                    // Elsewhere the up and down arrows scroll the list.
                    _ if key == VK_UP || key == VK_DOWN => self.scroll_by(if forward { WHEEL / 2.0 } else { -WHEEL / 2.0 }),
                    _ => return false,
                }
            }
            VK_PRIOR | VK_NEXT => {
                let page = self.client().1 * 0.8;
                self.scroll_by(if key == VK_NEXT { page } else { -page });
            }
            _ => return false,
        }
        true
    }

    /// Moves module `id` one place up or down the list.
    fn shift_module(&mut self, id: &str, down: bool) {
        let before = self.row_tops();
        // Past the next module listed with it: one in the fold, or listed
        // there, is passed over.
        let folded = |other: &str| !id.starts_with(OVERLAY) && !id.starts_with(WORKBENCH) && self.folded(other);
        if folded(id) {
            return;
        }
        let skip: Vec<String> = self.prefs.modules.iter().filter(|entry| folded(&entry.id) || entry.id == "lyrics").map(|entry| entry.id.clone()).collect();
        let (list, bare) = self.list(id);
        let Some(at) = list.iter().position(|entry| entry.id == bare) else { return };
        let mut to = at;
        loop {
            to = if down { to + 1 } else { to.wrapping_sub(1) };
            if to >= list.len() {
                return;
            }
            if !skip.contains(&list[to].id) {
                break;
            }
        }
        let entry = list.remove(at);
        list.insert(to, entry);
        self.glide_rows(before, "");
        self.save();
        self.reveal_focus();
    }

    fn scroll_by(&mut self, distance: f32) {
        let most = (self.content_height() - self.client().1).max(0.0);
        self.scroll_target = (self.scroll_target + distance).clamp(0.0, most);
    }

    /// Scrolls the list so the focused control is in view.
    fn reveal_focus(&mut self) {
        let Some(focus) = self.focus.clone() else { return };
        let row = self.layout().into_iter().find(|(row, ..)| match (row, &focus) {
            (Row::Skins, Target::Skin(_)) | (Row::Update, Target::Update) | (Row::Diagnostics, Target::Diagnostics | Target::SendReport) | (Row::Uninstall, Target::Uninstall) | (Row::Quit, Target::Quit) => true,
            (Row::Version, Target::CheckNow) | (Row::Shortcut, Target::Shortcut | Target::ClearShortcut) => true,
            (Row::GetApp, Target::GetApp) | (Row::Pair, Target::Pair(_)) | (Row::WebPage, Target::OpenPage) => true,
            (Row::Phone(a, _), Target::Unpair(b)) => a == b,
            (Row::Choice(f), Target::Choice(g, _)) => f == g,
            (Row::Switch(s), Target::Switch(t)) => s == t,
            (Row::Element(a), Target::Element(b)) => a == b,
            (Row::Tabs, Target::Tab(_)) => true,
            (Row::Slider(f), Target::Slider(g)) => f == g,
            (Row::Module(m), Target::Module(n) | Target::Expand(n)) => m == n,
            (Row::Item(m, a), Target::Item(n, b)) => m == n && a == b,
            (Row::ModuleChoice(_, f), Target::Choice(g, _)) => f == g,
            _ => false,
        });
        let Some((_, top, height)) = row else { return };
        let view = self.client().1;
        let margin = 24.0;
        let most = (self.content_height() - view).max(0.0);
        if top - margin < self.scroll_target {
            self.scroll_target = (top - margin).clamp(0.0, most);
        } else if top + height + margin > self.scroll_target + view {
            self.scroll_target = (top + height + margin - view).clamp(0.0, most);
        }
    }

    fn wheel(&mut self, delta: i16) {
        let Some((x, y)) = self.pointer else { return };
        // Over the phone's screen, its page is what moves.
        if let Some((_, k)) = self.phone.at.filter(|(at, _)| self.page == Page::General && x >= at.x && x < at.x + at.w && y >= at.y && y < at.y + at.h) {
            self.phone.scroll = (self.phone.scroll - delta as f32 / 120.0 * WHEEL / k).clamp(0.0, self.phone.most);
            self.next_frame = Instant::now();
            return;
        }
        if !(NAV..NAV + PANE).contains(&x) {
            return;
        }
        let height = self.client().1;
        let most = (self.content_height() - height).max(0.0);
        self.scroll_target = (self.scroll_target - delta as f32 / 120.0 * WHEEL).clamp(0.0, most);
        self.next_frame = Instant::now();
    }

    /// Where a module's row is laid out (document DIPs).
    fn row_top(&self, id: &str) -> f32 {
        self.layout().iter().find(|(row, ..)| matches!(row, Row::Module(m) if m == id)).map_or(0.0, |(_, y, _)| *y)
    }

    /// Moves the dragged row to where its middle now is; the rows it passes
    /// glide from where they were to where they now belong.
    fn reorder(&mut self) {
        let Some(drag) = &self.drag else { return };
        let middle = drag.pointer - drag.grab + ROW / 2.0;
        let before = self.row_tops();
        let id = drag.id.clone();
        let prefix = [OVERLAY, WORKBENCH].into_iter().find(|prefix| id.starts_with(prefix)).unwrap_or("");
        let (list, bare) = self.list(&id);
        let held = list.iter().position(|entry| entry.id == bare).unwrap();
        let entry = list.remove(held);
        let next = list.iter().position(|other| before.get(&format!("{prefix}{}", other.id)).is_some_and(|top| middle < top + ROW / 2.0)).unwrap_or(list.len());
        list.insert(next, entry);
        if next == held {
            return;
        }
        self.glide_rows(before, &id);
    }

    /// Where each module's row is laid out (document DIPs).
    fn row_tops(&self) -> HashMap<String, f32> {
        self.layout().into_iter().filter_map(|(row, y, _)| if let Row::Module(id) = row { Some((id, y)) } else { None }).collect()
    }

    /// After the modules' order changed: every row but `moved` glides from
    /// where it was (`before`) to where it now belongs.
    fn glide_rows(&mut self, before: HashMap<String, f32>, moved: &str) {
        let now = Instant::now();
        let after = self.row_tops();
        let id = moved.to_string();
        for (other, was) in before {
            let Some(&now_at) = after.get(&other) else { continue };
            if other == id || now_at == was {
                continue;
            }
            let key = format!("row:{other}");
            let current = self.motion.get(&key).map_or(0.0, |t| t.value(now));
            let mut glide = Transition::settled(was - now_at + current);
            glide.retarget(0.0, GLIDE.0, GLIDE.1, now);
            self.motion.insert(key, glide);
        }
    }

    fn client(&self) -> (f32, f32) {
        let mut rect = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut rect);
        }
        ((rect.right - rect.left) as f32 / self.scale, (rect.bottom - rect.top) as f32 / self.scale)
    }

    /// A transition's value, made at `start` if it is new, and sent toward
    /// `to` if that changed.
    fn animate(&mut self, key: String, to: f32, how: (Duration, Easing), now: Instant, jump: bool) -> f32 {
        let transition = self.motion.entry(key).or_insert_with(|| Transition::settled(to));
        if jump {
            transition.jump(to);
        } else if transition.target() != to {
            transition.retarget(to, how.0, how.1, now);
        }
        transition.value(now)
    }

    // ---- Drawing ----

    /// Takes up the thread's current graphics device if it is a new one,
    /// making the window's surface and the preview's bitmaps again on it.
    fn follow_device(&mut self) -> bool {
        let Ok(current) = gfx::current() else { return false };
        if Rc::ptr_eq(&current, &self.gfx) && self.surface.is_some() {
            return true;
        }
        // All it drew with was the other device's (see `life`). (A window has one composition target: the old one
        // goes first.)
        self.gone();
        let Ok(surface) = Surface::new(&current, self.hwnd) else {
            gfx::lost();
            return false;
        };
        self.surface = Some(surface);
        self.gfx = current;
        true
    }

    fn draw(&mut self, now: Instant) {
        // The screens as they are about now, and the one the preview
        // shows.
        if now.duration_since(self.screens_at) >= Duration::from_secs(1) {
            self.screens = crate::screens::all();
            self.screens_at = now;
        }
        // Desktops read meanwhile: the one shown, if it waited for it; else kept (unless kept already); the map's in
        // place, if it is still of that screen as it is.
        while let Ok(read) = self.read.1.try_recv() {
            match read {
                Read::Stage(stage) => {
                    let same = |other: &Stage| other.screen == stage.screen && other.work == stage.work && other.size == stage.size;
                    // (What was shown at once, as it was kept: taken again only if the desktop has changed since.)
                    let changed = |other: &Stage| other.desktop.as_ref().is_none_or(|was| stage.desktop.as_ref().is_some_and(|now| now.digest != was.digest));
                    if same(&self.stage) {
                        if changed(&self.stage) {
                            self.stage = stage;
                        }
                    } else {
                        match self.kept_stages.iter().position(same) {
                            Some(at) if changed(&self.kept_stages[at]) => self.kept_stages[at] = stage,
                            Some(_) => {}
                            None => self.kept_stages.push(stage),
                        }
                    }
                }
                Read::Map { id, monitor, size, desktop } => {
                    if let Some(art) = self.map_art.iter_mut().find(|art| art.id == id && art.monitor == monitor && art.asked == size) {
                        art.desktop = Some(desktop);
                        art.bitmap = None;
                    }
                }
            }
        }
        if let Some(screen) = self.stage_screen() {
            self.show_screen(&screen);
        }
        crate::playing::show_in_settings(matches!(self.page, Page::Music | Page::Layout));
        self.sources = crate::playing::music_players();
        if let Some(chosen) = self.settings.music_source.as_ref().filter(|chosen| !self.sources.iter().any(|(id, _)| *id == chosen.id)) {
            self.sources.push((chosen.id.clone(), chosen.name.clone()));
        }
        let dt = now.duration_since(self.last_frame).as_secs_f32();
        self.last_frame = now;
        self.scroll += (self.scroll_target - self.scroll) * (1.0 - E.powf(-dt / SCROLL_EASE));
        if (self.scroll_target - self.scroll).abs() < 0.05 {
            self.scroll = self.scroll_target;
        }
        let mut rect = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut rect);
        }
        let size = ((rect.right - rect.left).max(1) as u32, (rect.bottom - rect.top).max(1) as u32);
        if !self.follow_device() {
            self.next_frame = now + DEVICE_RETRY;
            return;
        }
        self.shown();
        let gfx = self.gfx.clone();
        let mut surface = self.surface.take().unwrap();
        let mut creep = None;
        self.failed = false;
        let began = Instant::now();
        let sizing = gfx::sizing(self.by_hand);
        let drawn = surface.draw(&gfx, size, self.scale, |frame| {
            frame.origin(0.0, 0.0);
            creep = self.paint(frame, now);
        });
        drop(sizing);
        // A frame slow enough to be seen, noted (as the widgets' are).
        let took = began.elapsed();
        if took > SLOW_FRAME {
            let ms = |d: Duration| d.as_secs_f32() * 1000.0;
            let (given, handed) = (surface.took.given, surface.took.handed);
            crate::widget::note_sparingly("settings frame", || {
                format!("settings: a frame took {:.1} ms (its surface given in {:.1}, painted {:.1}, handed to the compositor {:.1})", ms(took), ms(given), ms(took.saturating_sub(given + handed)), ms(handed))
            });
        }
        self.surface = Some(surface);
        self.drawn_size = drawn.is_ok().then_some(size);
        // The device is gone; a new one is made for the next frame.
        if drawn.is_err() || self.failed {
            gfx::lost();
            self.next_frame = now + DEVICE_RETRY;
            return;
        }
        self.gfx.sweep();
        // Sized by hand, and now drawn at rest: what was made for it as it was sized is let go (see `Gfx::settle`).
        if self.by_hand {
            self.was_sized = true;
        } else if std::mem::take(&mut self.was_sized) {
            self.gfx.settle();
        }
        // (A row held still moves only as the pointer does, which asks for a frame itself.)
        let moving = self.motion.values().any(|t| !t.done(now)) || self.scroll != self.scroll_target;
        self.settle_thumbs = false;
        self.next_frame = if moving {
            now
        } else {
            // Otherwise as often as the preview's charts creep a quarter pixel.
            now + creep.map_or(Duration::from_millis(250), |speed: f32| Duration::from_secs_f32((0.25 / speed).min(0.25)))
        };
    }

    /// Draws everything, and returns how fast the preview's charts move
    /// (physical pixels a second).
    fn paint(&mut self, frame: &Frame, now: Instant) -> Option<f32> {
        let palette = self.palette.clone();
        let (width, height) = self.client();
        if !self.mica {
            fill(frame, palette.window, 0.0, 0.0, width, height, 0.0);
        }
        self.targets.clear();
        self.paint_nav(frame, &palette);
        unsafe { frame.dc.PushAxisAlignedClip(&rect(NAV, 0.0, PANE, height), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
        self.paint_list(frame, &palette, now, height);
        unsafe { frame.dc.PopAxisAlignedClip() };
        self.paint_preview(frame, &palette, width, height)
    }

    /// The pages' list: the window's name, then each page, the one shown lit
    /// and marked in the accent.
    fn paint_nav(&mut self, frame: &Frame, palette: &Palette) {
        let lang = self.lang;
        let hovered = self.hovered();
        let left = 16.0;
        let width = NAV - 2.0 * left + 8.0;
        let title = Font::new(Family::SegoeDisplay, 20.0, 600.0);
        text_centred(frame, pick(lang, "设置", "Settings"), title, palette.text, left + 8.0, PAD_TOP + 18.0, width, Align::Start);
        let label = Font::new(Family::Segoe, 14.0, 400.0);
        for (i, item) in NAV_ITEMS.into_iter().enumerate() {
            // The components' own pages set in under their heading.
            let indent = if item.under() { NAV_INDENT } else { 0.0 };
            let r = Rect { x: left + indent, y: PAD_TOP + 52.0 + i as f32 * (NAV_ITEM + 4.0), w: width - indent, h: NAV_ITEM };
            let target = Target::Nav(item);
            let (shown, hover) = (item == NavItem::Page(self.page), hovered.as_ref() == Some(&target));
            if shown || hover {
                fill(frame, if shown { palette.control } else { palette.hover }, r.x, r.y, r.w, r.h, 6.0);
            }
            if shown {
                fill(frame, palette.selection, r.x, r.y + 12.0, 3.0, r.h - 24.0, 1.5);
            }
            let cy = r.y + r.h / 2.0;
            frame.icon(item.icon(), Point { x: r.x + 22.0, y: cy }, 16.0, palette.text);
            text_centred(frame, item.name(lang), label, palette.text, r.x + 46.0, cy, r.w - 50.0, Align::Start);
            // A newer Glance is out: a dot on the page it is taken from.
            if item == NavItem::Page(Page::General) && update::available().is_some() {
                fill(frame, palette.selection, r.x + r.w - 20.0, cy - 3.0, 6.0, 6.0, 3.0);
            }
            self.targets.push((r, target));
        }
    }

    fn paint_list(&mut self, frame: &Frame, palette: &Palette, now: Instant, height: f32) {
        let lang = self.lang;
        let left = NAV + PAD_SIDE;
        let width = PANE - 2.0 * PAD_SIDE - GUTTER;
        let scroll = self.scroll;
        let hovered = self.hovered();
        let label = Font::new(Family::Segoe, 14.0, 400.0);
        let hint = Font::new(Family::Segoe, 12.0, 400.0);
        let mut dragged = None;
        // The diagnostics sent: how that went, once it is over.
        if self.sending.as_ref().is_some_and(|sending| sending.is_finished()) {
            self.sent = Some(self.sending.take().unwrap().join().ok().flatten());
        }
        if self.sending.is_some() {
            self.next_frame = now + Duration::from_millis(300);
        }
        // A switch turned: as it is now.
        if self.switching.as_ref().is_some_and(|(_, turning)| turning.is_finished()) {
            let (what, turning) = self.switching.take().unwrap();
            let on = turning.join().unwrap_or(false);
            match what {
                SwitchAsked::Startup => self.autostart = on,
                SwitchAsked::Remote => {
                    self.remote = on;
                    self.code = None;
                    self.phones = crate::phone::phones();
                    self.ask_remote(|| None);
                }
            }
            self.next_frame = now;
        }
        // The diagnostics put together: copied, or shown to be sent.
        if self.reporting.as_ref().is_some_and(|(_, putting)| putting.is_finished()) {
            let (what, putting) = self.reporting.take().unwrap();
            if let Ok(report) = putting.join() {
                match what {
                    ReportFor::Copy => {
                        if crate::diagnostics::copy(self.hwnd, &report) {
                            self.copied_at = Some(Instant::now());
                        }
                    }
                    ReportFor::Send => self.send_report(report),
                }
            }
            self.next_frame = now;
        }
        // The remote's helper answered: as it is now, and what was asked of it.
        if self.remote_asked.as_ref().is_some_and(|asked| asked.is_finished()) {
            if let Ok(answer) = self.remote_asked.take().unwrap().join() {
                self.remote_lights = answer.lights;
                self.remote_screens = answer.screens;
                self.web_address = answer.address;
                if let Some(code) = answer.code {
                    self.code = code;
                }
                self.phones = crate::phone::phones();
            }
            self.next_frame = now;
        }
        if self.switching.is_some() || self.reporting.is_some() || self.remote_asked.is_some() {
            self.next_frame = self.next_frame.min(now + Duration::from_millis(100));
        }
        // The lights' program fetched (or taken away): how that went, once it is over.
        if self.fetching.as_ref().is_some_and(|fetching| fetching.is_finished()) {
            let done = self.fetching.take().unwrap().join().unwrap_or(false);
            self.fetch_failed = !done;
            self.ask_remote(|| None);
        }
        if self.fetching.is_some() {
            self.next_frame = now + Duration::from_millis(300);
        }
        // The phones paired, read again now and then while they are shown: one that has just taken the pairing offered appears, and its code goes.
        if self.page == Page::General && self.remote && now.duration_since(self.phones_at) > Duration::from_secs(1) {
            let phones = crate::phone::phones();
            if phones.len() > self.phones.len() && self.code.as_ref().is_some_and(|(_, what)| matches!(what, CodeFor::Pairing | CodeFor::WebPairing)) {
                self.code = None;
            }
            self.phones = phones;
            self.phones_at = now;
            self.next_frame = now + Duration::from_secs(1);
        }
        let placed = self.layout();
        // How far each module's card reaches down: its row, and as it opens,
        // that share of the rows inside it; and where the card's top is.
        // A card's rows follow its module's.
        let mut reach: HashMap<String, f32> = HashMap::new();
        let mut card_top: HashMap<String, f32> = HashMap::new();
        for (row, top, row_height) in &placed {
            match row {
                Row::Module(id) => {
                    card_top.insert(id.clone(), *top);
                    reach.insert(id.clone(), *row_height);
                }
                Row::Item(id, _) | Row::ModuleChoice(id, _) => {
                    reach.insert(id.clone(), top + row_height - card_top[id]);
                }
                _ => {}
            }
        }
        let open: HashMap<String, f32> = card_top.keys().map(|id| (id.clone(), self.opened(id))).collect();
        for (id, extent) in reach.iter_mut() {
            *extent = ROW + (*extent - ROW) * open[id];
        }
        for (row, top, row_height) in placed {
            let y = top - scroll;
            let extent = if let Row::Module(id) = &row { reach[id] } else { row_height };
            if y > height || y + extent + 60.0 < 0.0 {
                continue;
            }
            // A row inside a card shows only within the card as far as it
            // has opened, and takes clicks once it is open.
            let inside = match &row {
                Row::Item(id, _) | Row::ModuleChoice(id, _) => {
                    let glide = self.motion.get(&format!("row:{id}")).map_or(0.0, |t| t.value(now));
                    let top = card_top[id] - scroll + glide;
                    unsafe { frame.dc.PushAxisAlignedClip(&rect(left, top, width, reach[id]), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
                    Some(open[id] >= 1.0)
                }
                _ => None,
            };
            let usable = inside.unwrap_or(true);
            match row {
                Row::More(count) => self.more_row(frame, palette, count, left, y, width, row_height, now),
                Row::Title => {
                    let title = Font::new(Family::SegoeDisplay, 28.0, 600.0);
                    text_centred(frame, self.page.name(lang), title, palette.text, left, y + 22.0, width, Align::Start);
                }
                // Each tab's name, the one shown marked under it in the accent, over a hairline the width of the list.
                Row::Tabs => {
                    let font = |shown: bool| Font::new(Family::Segoe, 14.0, if shown { 600.0 } else { 400.0 });
                    let base = y + row_height - 8.0;
                    fill(frame, palette.rule, left, base, width, 1.0, 0.0);
                    let mut x = left;
                    for tab in TABS {
                        let name = tab.name(lang);
                        let shown = tab == self.tab;
                        let w = frame.gfx.measure(name, font(true)) + 24.0;
                        let target = Target::Tab(tab);
                        let hover = hovered.as_ref() == Some(&target);
                        let r = Rect { x, y, w, h: row_height - 8.0 };
                        if hover && !shown {
                            fill(frame, palette.hover, r.x, r.y + 4.0, r.w, r.h - 8.0, 6.0);
                        }
                        text_centred(frame, name, font(shown), if shown { palette.text } else { palette.text2 }, x + 12.0, y + (row_height - 8.0) / 2.0, w - 12.0, Align::Start);
                        if shown {
                            fill(frame, palette.selection, x + 12.0, base - 2.0, w - 24.0, 3.0, 1.5);
                        }
                        self.targets.push((r, target));
                        x += w;
                    }
                }
                Row::Element(name) => {
                    card(frame, palette, left, y, width, row_height, palette.card);
                    let (title, word) = self.item_label("lyrics", name);
                    let on = self.item_on("lyrics", name);
                    let switch_left = left + width - ROW_SIDE - 40.0;
                    field_label(frame, palette, &title, word, label, hint, left + ROW_SIDE, y + row_height / 2.0, switch_left - 16.0 - left - ROW_SIDE);
                    let target = Target::Element(name);
                    let pressed = self.pressed == Some(target.clone());
                    self.toggle(frame, palette, format!("element:{name}"), on, true, switch_left, y + row_height / 2.0, now, pressed);
                    self.targets.push((Rect { x: left, y, w: width, h: row_height }, target));
                }
                Row::Note(words) => frame.text_wrapped(words, hint, palette.text2, left + 4.0, y + 4.0, width - 8.0),
                Row::Slider(field) => {
                    card(frame, palette, left, y, width, row_height, palette.card);
                    let (name, min, max, _, value) = self.slider(field);
                    let cy = y + row_height / 2.0;
                    // The value on the right; the track before it; the name
                    // in the room left.
                    let right = left + width - ROW_SIDE;
                    let track = Rect { x: right - SLIDER_VALUE - 12.0 - SLIDER_WIDTH, y, w: SLIDER_WIDTH, h: row_height };
                    text_centred(frame, name, label, palette.text, left + ROW_SIDE, cy, track.x - 12.0 - (left + ROW_SIDE), Align::Start);
                    let shown = format!("{:.0}%", value * 100.0);
                    text_centred(frame, &shown, label, palette.text2, right - SLIDER_VALUE, cy, SLIDER_VALUE, Align::End);
                    let (start, end) = (track.x + SLIDER_THUMB, track.x + track.w - SLIDER_THUMB);
                    // (A slider whose most is its least, the screen holding no more than that: at its start.)
                    let at = start + if max > min { (value - min) / (max - min) * (end - start) } else { 0.0 };
                    fill(frame, palette.switch_stroke.alpha(0.45), start, cy - 2.0, end - start, 4.0, 2.0);
                    fill(frame, palette.switch_on, start, cy - 2.0, at - start, 4.0, 2.0);
                    // As Windows' own: a ring around a dot in the accent, the
                    // dot larger under the pointer.
                    let lit = hovered == Some(Target::Slider(field)) || self.sliding.is_some_and(|(f, _)| f == field);
                    let ring = SLIDER_THUMB;
                    fill(frame, palette.card_stroke, at - ring - 1.0, cy - ring - 1.0, 2.0 * ring + 2.0, 2.0 * ring + 2.0, ring + 1.0);
                    fill(frame, palette.switch_knob_ring, at - ring, cy - ring, 2.0 * ring, 2.0 * ring, ring);
                    let dot = if lit { 7.0 } else { 5.0 };
                    fill(frame, palette.switch_on, at - dot, cy - dot, 2.0 * dot, 2.0 * dot, dot);
                    self.targets.push((track, Target::Slider(field)));
                }
                Row::Skins => {
                    card(frame, palette, left, y, width, row_height, palette.card);
                    let gap = 12.0;
                    let cell = (width - 2.0 * ROW_SIDE - 2.0 * gap) / 3.0;
                    let names = [
                        (Skin::Paper, pick(lang, "记录纸", "Chart paper")),
                        (Skin::Glass, pick(lang, "磨砂玻璃", "Frosted glass")),
                        (Skin::Fluent, "Windows 11"),
                    ];
                    let current = theme::Skin::named(&self.settings.skin);
                    for (i, (skin, name)) in names.into_iter().enumerate() {
                        let x = left + ROW_SIDE + i as f32 * (cell + gap);
                        let box_ = Rect { x, y: y + 16.0, w: cell, h: 72.0 };
                        let chosen = skin == current;
                        let hover = hovered == Some(Target::Skin(skin));
                        skin_miniature(frame, skin, box_);
                        let ring = if chosen { (palette.selection, 2.0) } else { (palette.card_stroke, 1.0) };
                        if hover && !chosen {
                            let _ = skins::shadows(frame, &[(box_, 6.0)], &[Shadow { y: 2.0, blur: 8.0, spread: 0.0, color: Color::hex(0, 0.08) }]);
                        }
                        stroke_outside(frame, box_, 6.0, ring.0, ring.1);
                        let font = if chosen { Font::new(Family::Segoe, 13.0, 600.0) } else { Font::new(Family::Segoe, 13.0, 400.0) };
                        text_centred(frame, name, font, if chosen { palette.text } else { palette.text2 }, x, y + 16.0 + 72.0 + 10.0 + 9.0, cell, Align::Start);
                        self.targets.push((Rect { x, y: y + 16.0, w: cell, h: 72.0 + 10.0 + 18.0 }, Target::Skin(skin)));
                    }
                }
                Row::Library => self.paint_library(frame, palette, left, y, width, row_height, &hovered),
                Row::Choice(field) => {
                    card(frame, palette, left, y, width, row_height, palette.card);
                    let (name, options, chosen) = self.choices(field);
                    let right = left + width - ROW_SIDE;
                    match self.choice_hint(field) {
                        Some(word) if self.stacked(&options) => {
                            let words = row_height - CHOICE_LINE;
                            field_label(frame, palette, name, Some(word), label, hint, left + ROW_SIDE, y + words / 2.0, width - 2.0 * ROW_SIDE);
                            self.segmented(frame, palette, field, &options, chosen, true, right, y + row_height - CHOICE_LINE / 2.0 - 8.0, now, &hovered);
                        }
                        Some(word) => {
                            let room = width - 2.0 * ROW_SIDE - self.segmented_width(&options) - 16.0;
                            field_label(frame, palette, name, Some(word), label, hint, left + ROW_SIDE, y + row_height / 2.0, room);
                            self.segmented(frame, palette, field, &options, chosen, true, right, y + row_height / 2.0, now, &hovered);
                        }
                        None => {
                            text_centred(frame, name, label, palette.text, left + ROW_SIDE, y + row_height / 2.0, width / 2.0, Align::Start);
                            self.segmented(frame, palette, field, &options, chosen, true, right, y + row_height / 2.0, now, &hovered);
                        }
                    }
                }
                Row::Switch(switch) => {
                    card(frame, palette, left, y, width, row_height, palette.card);
                    let (name, detail, on) = self.switch(switch);
                    let switch_left = left + width - ROW_SIDE - 40.0;
                    field_label(frame, palette, name, detail.as_deref(), label, hint, left + ROW_SIDE, y + row_height / 2.0, switch_left - 16.0 - left - ROW_SIDE);
                    let key = format!("switch:{switch:?}");
                    let pressed = self.pressed == Some(Target::Switch(switch));
                    self.toggle(frame, palette, key, on, true, left + width - ROW_SIDE - 40.0, y + row_height / 2.0, now, pressed);
                    self.targets.push((Rect { x: left, y, w: width, h: row_height }, Target::Switch(switch)));
                }
                Row::Module(id) => {
                    let offset = self.motion.get(&format!("row:{id}")).map_or(0.0, |t| t.value(now));
                    if self.drag.as_ref().is_some_and(|drag| drag.id == id) {
                        dragged = Some((id, row_height));
                        continue;
                    }
                    self.module_row(frame, palette, &id, left, y + offset, width, row_height, reach[&id], now, &hovered, false);
                }
                // The rows inside a module's card (drawn with its row) glide
                // with it, each under a hairline; while the module is off they
                // are dimmed and do nothing, and keep their state.
                Row::Item(id, name) => {
                    let y = y + self.motion.get(&format!("row:{id}")).map_or(0.0, |t| t.value(now));
                    fill(frame, palette.rule, left + 1.0, y, width - 2.0, 1.0, 0.0);
                    let active = self.module_on(&id);
                    let (name_text, detail) = self.item_label(&id, &name);
                    let target = Target::Item(id.clone(), name.clone());
                    let colors = if active { (palette.text, palette.text2) } else { (palette.text3, palette.text3) };
                    let switch_left = left + width - ROW_SIDE - CHEVRON_ROOM - 40.0;
                    field_label_in(frame, colors, &name_text, detail, label, hint, left + ITEM_INSET, y + row_height / 2.0, switch_left - 12.0 - left - ITEM_INSET);
                    let pressed = self.pressed == Some(target.clone());
                    let on = self.item_on(&id, &name);
                    self.toggle(frame, palette, format!("item:{id}:{name}"), on, active, switch_left, y + row_height / 2.0, now, pressed);
                    if active && usable {
                        self.targets.push((Rect { x: left, y, w: width, h: row_height }, target));
                    }
                }
                Row::ModuleChoice(id, field) => {
                    let y = y + self.motion.get(&format!("row:{id}")).map_or(0.0, |t| t.value(now));
                    fill(frame, palette.rule, left + 1.0, y, width - 2.0, 1.0, 0.0);
                    let active = self.module_on(&id);
                    let (name, options, chosen) = self.choices(field);
                    let color = if active { palette.text } else { palette.text3 };
                    text_centred(frame, name, label, color, left + ITEM_INSET, y + row_height / 2.0, width / 2.0 - ITEM_INSET, Align::Start);
                    let right = left + width - ROW_SIDE - CHEVRON_ROOM;
                    let targets = self.targets.len();
                    self.segmented(frame, palette, field, &options, chosen, active, right, y + row_height / 2.0, now, &hovered);
                    if !usable {
                        self.targets.truncate(targets);
                    }
                }
                Row::Group(name) => {
                    let small = Font::new(Family::Segoe, 13.0, 600.0);
                    text_centred(frame, name, small, palette.text2, left + 4.0, y + row_height / 2.0, width, Align::Start);
                }
                Row::Code => {
                    card(frame, palette, left, y, width, row_height, palette.card);
                    if let Some((code, what)) = &self.code {
                        // Dark on white whatever the theme (what a phone's camera reads), a margin of four cells round it.
                        let (x0, y0) = (left + ROW_SIDE, y + (row_height - CODE) / 2.0);
                        fill(frame, Color::hex(0xFFFFFF, 1.0), x0, y0, CODE, CODE, 8.0);
                        let cell = CODE / (code.0.len() as f32 + 8.0);
                        for (down, row) in code.0.iter().enumerate() {
                            // (Each run of dark cells as one piece, and a hair over the row below: no seams between them.)
                            let mut across = 0;
                            while across < row.len() {
                                let run = row[across..].iter().take_while(|dark| **dark).count();
                                if run > 0 {
                                    fill(frame, Color::hex(0x000000, 1.0), x0 + (across as f32 + 4.0) * cell, y0 + (down as f32 + 4.0) * cell, run as f32 * cell, cell + 0.3, 0.0);
                                }
                                across += run.max(1);
                            }
                        }
                        let (title, word) = code_words(lang, *what, self.web_address.as_deref());
                        let word = word.as_ref();
                        let text_left = x0 + CODE + 20.0;
                        field_label(frame, palette, title, Some(word), label, hint, text_left, y + row_height / 2.0, CODE_WORDS);

                    }
                }
                Row::Update | Row::Version | Row::Shortcut | Row::Diagnostics | Row::Uninstall | Row::Quit | Row::GetApp | Row::Pair | Row::WebPage | Row::Phone(..) => {
                    let Some((name, detail, buttons)) = self.button_row(&row, now) else { continue };
                    card(frame, palette, left, y, width, row_height, palette.card);
                    let mut right = left + width - ROW_SIDE;
                    for (text, target, kind) in buttons {
                        right = self.button(frame, palette, &text, right, y + row_height / 2.0, target, kind, &hovered) - 8.0;
                    }
                    field_label(frame, palette, &name, Some(&detail), label, hint, left + ROW_SIDE, y + row_height / 2.0, right - 8.0 - left - ROW_SIDE);
                }
            }
            if inside.is_some() {
                unsafe { frame.dc.PopAxisAlignedClip() };
            }
        }
        // The dragged row passes over the others.
        if let Some((id, row_height)) = dragged {
            let drag = self.drag.as_ref().unwrap();
            let y = drag.pointer - drag.grab - scroll;
            self.module_row(frame, palette, &id, left, y, width, row_height, row_height, now, &hovered, true);
        }
        // The focus, shown once the keyboard is in use.
        if self.keyboard {
            if let Some(focus) = &self.focus {
                if let Some((r, target)) = self.targets.iter().find(|(_, target)| target == focus) {
                    // A skin's target holds its label flush with the card's
                    // edge, so its ring keeps some room; the rest hug theirs.
                    let room = if matches!(target, Target::Skin(_)) { 8.0 } else { -1.0 };
                    let ring = Rect { x: r.x - room, y: r.y - room, w: r.w + 2.0 * room, h: r.h + 2.0 * room };
                    let radius = 5.0 + room.max(0.0);
                    stroke_outside(frame, ring, radius, palette.text, 2.0);
                }
            }
        }
        // A thin thumb shows where in the list the view is.
        let content = self.content_height();
        if content > height {
            let thumb = (height * height / content).max(24.0);
            let at = (height - thumb) * self.scroll / (content - height);
            fill(frame, palette.text3.alpha(0.6), NAV + PANE - GUTTER + 2.0, at + 2.0, 3.0, thumb - 4.0, 1.5);
        }
    }

    /// A module's row, and when its card is open (`reach` below the row's
    /// top), the card around the rows inside it: one rounded box.
    /// The fold of the modules few want: its name, what is in it, and a
    /// chevron that turns over as it opens.
    #[allow(clippy::too_many_arguments)]
    fn more_row(&mut self, frame: &Frame, palette: &Palette, count: usize, left: f32, y: f32, width: f32, height: f32, now: Instant) {
        let label = Font::new(Family::Segoe, 14.0, 400.0);
        let hint = Font::new(Family::Segoe, 12.0, 400.0);
        card(frame, palette, left, y, width, height, palette.card);
        let cy = y + height / 2.0;
        let turn = self.animate("chevron:more".into(), if self.more_open { 180.0 } else { 0.0 }, SLIDE, now, false);
        let cx = left + width - ROW_SIDE - 6.0;
        frame.place(Matrix3x2::rotation_around(turn, Vector2 { X: cx, Y: cy }));
        frame.icon(Icon::ChevronDown, Point { x: cx, y: cy }, CHEVRON, palette.text2);
        frame.origin(0.0, 0.0);
        let names: Vec<String> = self.prefs.modules.iter().filter(|entry| self.folded(&entry.id) && prefs::offered(&entry.id)).map(|entry| self.module(&entry.id).0).collect();
        let title = if self.lang == Lang::Zh { format!("更多模块（{count}）") } else { format!("More modules ({count})") };
        let text_left = left + ITEM_INSET;
        field_label_line(frame, palette, &title, Some(&names.join(pick(self.lang, "、", ", "))), label, hint, text_left, cy, cx - 24.0 - text_left);
        self.targets.push((Rect { x: left, y, w: width, h: height }, Target::More));
    }

    #[allow(clippy::too_many_arguments)]
    fn module_row(&mut self, frame: &Frame, palette: &Palette, id: &str, left: f32, y: f32, width: f32, height: f32, reach: f32, now: Instant, hovered: &Option<Target>, held: bool) {
        let label = Font::new(Family::Segoe, 14.0, 400.0);
        let hint = Font::new(Family::Segoe, 12.0, 400.0);
        if held {
            let shape = Rect { x: left, y, w: width, h: height };
            let _ = skins::shadows(frame, &[(shape, 6.0)], &[Shadow { y: 4.0, blur: 16.0, spread: 0.0, color: Color::hex(0, 0.12) }]);
            // Opaque while it passes over the other rows.
            fill(frame, palette.window, left, y, width, height, 6.0);
            card(frame, palette, left, y, width, height, palette.card);
            stroke_inside(frame, shape, 6.0, palette.selection);
        } else {
            // The whole card, its rows inside it in the same shade, as
            // Windows' own settings draw an open one.
            card(frame, palette, left, y, width, reach, palette.card);
        }
        let on = self.module_on(id);
        let (title, detail) = self.module(id);
        // The grip: six dots, to drag the row by (none in the fold, where a
        // module has no place among the others).
        let grip = Rect { x: left + 12.0, y: y + height / 2.0 - 16.0, w: 20.0, h: 32.0 };
        let draggable = !self.folded(id);
        if draggable && (*hovered == Some(Target::Grip(id.to_string())) || held) {
            fill(frame, palette.hover, grip.x, grip.y, grip.w, grip.h, 4.0);
        }
        for (dx, dy) in [(-2.0, -5.0), (2.0, -5.0), (-2.0, 0.0), (2.0, 0.0), (-2.0, 5.0), (2.0, 5.0)].into_iter().filter(|_| draggable) {
            let dot = D2D1_ELLIPSE { point: Vector2 { X: grip.x + 10.0 + dx, Y: grip.y + 16.0 + dy }, radiusX: 1.2, radiusY: 1.2 };
            unsafe { frame.dc.FillEllipse(&dot, frame.brush(palette.text3)) };
        }
        let text_left = left + ITEM_INSET;
        // The switch, and right of it, if the module has a card to open, a
        // chevron that turns over as it opens.
        let opens = self.opens(id);
        let open = self.expanded.contains(id);
        let cy = y + height / 2.0;
        let switch_left = left + width - ROW_SIDE - CHEVRON_ROOM - 40.0;
        if opens {
            let turn = self.animate(format!("chevron:{id}"), if open { 180.0 } else { 0.0 }, SLIDE, now, false);
            let cx = left + width - ROW_SIDE - 6.0;
            frame.place(Matrix3x2::rotation_around(turn, Vector2 { X: cx, Y: cy }));
            frame.icon(Icon::ChevronDown, Point { x: cx, y: cy }, CHEVRON, palette.text2);
            frame.origin(0.0, 0.0);
        }
        field_label_line(frame, palette, &title, detail.as_deref(), label, hint, text_left, cy, switch_left - 12.0 - text_left);
        let pressed = self.pressed == Some(Target::Module(id.to_string()));
        self.toggle(frame, palette, format!("module:{id}"), on, true, switch_left, cy, now, pressed);
        if !held {
            // The grip drags, the switch switches, and the rest of the row
            // opens and closes the card (or, with none, switches too).
            if draggable {
                self.targets.push((grip, Target::Grip(id.to_string())));
            }
            let switch_area = Rect { x: switch_left - 12.0, y, w: 40.0 + 24.0, h: height };
            self.targets.push((switch_area, Target::Module(id.to_string())));
            let row = Rect { x: left, y, w: width, h: height };
            self.targets.push((row, if opens { Target::Expand(id.to_string()) } else { Target::Module(id.to_string()) }));
        }
    }

    /// A button right-aligned at `right` and centred on `cy`, pressed as
    /// `target`: outlined in the card's hairline, or, the one thing a row
    /// asks for, filled in the accent; returns where its left edge is.
    #[allow(clippy::too_many_arguments)]
    fn button(&mut self, frame: &Frame, palette: &Palette, text: &str, right: f32, cy: f32, target: Target, kind: Button, hovered: &Option<Target>) -> f32 {
        let font = Font::new(Family::Segoe, 14.0, 400.0);
        let w = frame.gfx.measure(text, font) + 32.0;
        let rect = Rect { x: right - w, y: cy - 16.0, w, h: 32.0 };
        let hover = kind != Button::Quiet && hovered.as_ref() == Some(&target);
        let ink = match kind {
            Button::Accent => {
                fill(frame, if hover { palette.switch_on.alpha(0.9) } else { palette.switch_on }, rect.x, rect.y, rect.w, rect.h, 6.0);
                Color::hex(0xFFFFFF, 1.0)
            }
            _ => {
                if hover || kind == Button::Taking {
                    fill(frame, palette.hover, rect.x, rect.y, rect.w, rect.h, 6.0);
                }
                stroke_inside(frame, rect, 6.0, if kind == Button::Taking { palette.selection } else { palette.rule });
                if kind == Button::Quiet { palette.text2 } else { palette.text }
            }
        };
        text_centred(frame, text, font, ink, rect.x + 16.0, cy, w, Align::Start);
        if kind != Button::Quiet {
            self.targets.push((rect, target));
        }
        rect.x
    }

    /// A row of options with one thumb that slides under the chosen one,
    /// right-aligned at `right` and centred on `cy`; unless `enabled`, dimmed
    /// and not to be pressed.
    #[allow(clippy::too_many_arguments)]
    fn segmented(&mut self, frame: &Frame, palette: &Palette, field: Field, options: &[String], chosen: Option<usize>, enabled: bool, right: f32, cy: f32, now: Instant, hovered: &Option<Target>) {
        let font = Font::new(Family::Segoe, 13.0, 400.0);
        let widths: Vec<f32> = options.iter().map(|o| (frame.gfx.measure(o, font) + 24.0).max(44.0)).collect();
        let total = widths.iter().sum::<f32>() + 4.0;
        let (x0, y0) = (right - total, cy - 16.0);
        fill(frame, palette.control, x0, y0, total, 32.0, 6.0);
        let mut x = x0 + 2.0;
        let mut spans = Vec::new();
        for w in &widths {
            spans.push((x, *w));
            x += w;
        }
        if let Some(index) = chosen {
            let (to_x, to_w) = (spans[index].0 - x0, spans[index].1);
            let jump = self.settle_thumbs;
            let tx = self.animate(format!("thumb-x:{field:?}"), to_x, SLIDE, now, jump);
            let tw = self.animate(format!("thumb-w:{field:?}"), to_w, SLIDE, now, jump);
            let thumb = Rect { x: x0 + tx, y: y0 + 2.0, w: tw, h: 28.0 };
            if let Some(drop) = palette.control_drop {
                fill(frame, drop, thumb.x, thumb.y + 1.0, thumb.w, thumb.h, 4.0);
            }
            stroke_outside(frame, thumb, 4.0, palette.control_ring, 1.0);
            fill(frame, palette.control_on, thumb.x, thumb.y, thumb.w, thumb.h, 4.0);
        }
        for (i, ((sx, sw), option)) in spans.iter().zip(options).enumerate() {
            let target = Target::Choice(field, i);
            let lit = chosen == Some(i) || *hovered == Some(target.clone());
            let color = match (enabled, lit) {
                (false, _) => palette.text3,
                (true, true) => palette.text,
                (true, false) => palette.text2,
            };
            let text_w = frame.gfx.measure(option, font);
            text_centred(frame, option, font, color, sx + (sw - text_w) / 2.0, cy, *sw, Align::Start);
            if enabled {
                self.targets.push((Rect { x: *sx, y: y0, w: *sw, h: 32.0 }, target));
            }
        }
    }

    /// A switch, 40 × 22, with its left edge at `x` and centred on `cy`;
    /// unless `enabled`, in the greys of one that does nothing for now.
    #[allow(clippy::too_many_arguments)]
    fn toggle(&mut self, frame: &Frame, palette: &Palette, key: String, on: bool, enabled: bool, x: f32, cy: f32, now: Instant, pressed: bool) {
        let dim = 1.0 - self.animate(format!("{key}:enabled"), if enabled { 1.0 } else { 0.0 }, FLIP, now, false).clamp(0.0, 1.0);
        let t = self.animate(key, if on { 1.0 } else { 0.0 }, FLIP, now, false).clamp(0.0, 1.0);
        let track = Rect { x, y: cy - 11.0, w: 40.0, h: 22.0 };
        // Off: an outlined track; on: a filled one. Between, the two
        // cross-fade. Disabled, the off track is outline alone, and both
        // take the disabled grey.
        if t < 1.0 {
            fill(frame, palette.switch_off.alpha((1.0 - t) * (1.0 - dim)), track.x, track.y, track.w, track.h, 11.0);
            stroke_inside(frame, track, 11.0, mix(palette.switch_stroke, palette.disabled, dim).alpha(1.0 - t));
        }
        if t > 0.0 {
            fill(frame, mix(palette.switch_on, palette.disabled, dim).alpha(t), track.x, track.y, track.w, track.h, 11.0);
        }
        // The knob grows as it crosses, and stretches while pressed.
        let size = 12.0 + 2.0 * t;
        let stretch = if pressed && enabled { 16.0 - size } else { 0.0 };
        let left = x + 4.0 + 17.0 * t - if on { stretch } else { 0.0 };
        let knob_off = mix(palette.switch_knob, palette.disabled, dim);
        let knob_on = mix(Color::hex(0xFFFFFF, 1.0), palette.disabled_knob, dim);
        fill(frame, mix(knob_off, knob_on, t), left, cy - size / 2.0, size + stretch, size, size / 2.0);
    }

    /// The overlay's preview: the whole screen, and the overlay where it was
    /// put, showing the readings chosen (an example game's frames, with none
    /// running).
    /// The phone app as a phone would show it for this PC now, in the
    /// middle of `area`: as tall as the room, in a phone's dark outline.
    /// Whether anything in it moves.
    fn paint_phone(&mut self, frame: &Frame, area: Rect) -> bool {
        let app = crate::app();
        let reads = app.controller.history.read().unwrap().back().map_or(serde_json::Value::Null, |sample| crate::remote::figures(sample, &app.info));
        // The outline round it; the whole no taller or wider than the room, nor larger than a phone is.
        let (bezel, margin) = (10.0, 24.0);
        let (wide, tall) = super::phone_art::SCREEN;
        let k = ((area.h - 2.0 * margin - 2.0 * bezel) / tall).min((area.w - 2.0 * margin - 2.0 * bezel) / wide).min(1.0);
        if k <= 0.0 {
            return false;
        }
        let (w, h) = (wide * k, tall * k);
        let (x, y) = (area.x + (area.w - w) / 2.0, area.y + (area.h - h) / 2.0);
        fill(frame, Color::hex(0x15181D, 1.0), x - bezel, y - bezel, w + 2.0 * bezel, h + 2.0 * bezel, 26.0 * k + bezel);
        let name = std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into());
        self.hear_phone();
        let held = self.phone.slid.as_ref().map(|(part, _, at)| (part, *at));
        let state = self.phone.state.as_ref().filter(|_| self.remote);
        // (The ring is shut with the lights' card: where they are off, or the remote does not answer.)
        if !state.is_some_and(|state| state["lights"].as_bool() == Some(true) && state["night"].as_bool() != Some(true)) {
            self.phone.making = None;
            self.phone.removing = false;
        }
        // A colour held a while: the colours kept can be taken away (and it is not chosen as it is let go).
        if matches!(self.phone.pressed, Some(super::phone_art::Part::Colour(_))) && self.phone.held_since.elapsed() >= super::buttons::LINGER {
            self.phone.pressed = None;
            self.phone.making = None;
            self.phone.removing = true;
        }
        let painted = super::phone_art::paint(frame, Rect { x, y, w, h }, self.lang, &name, &reads, state, !self.remote, self.phone_from.elapsed().as_secs_f32(), self.phone.scroll, held, self.phone.making.as_ref(), self.phone.removing);
        self.phone.at = Some((Rect { x, y, w, h }, k));
        self.phone.most = (painted.tall - tall).max(0.0);
        self.phone.scroll = self.phone.scroll.min(self.phone.most);
        self.phone.parts = painted.parts;
        // (Drawn again soon whether or not its fans turn: how the PC is, is asked meanwhile.)
        true
    }

    /// Asks the remote, on a thread of its own (it answers when it has done it: the window goes on meanwhile).
    fn ask_phone(&mut self, method: &str, path: &str, body: String) {
        let link = self.phone.link.get_or_insert_with(|| {
            let (ask, asked) = std::sync::mpsc::channel::<(String, String, String)>();
            let (answer, answered) = std::sync::mpsc::channel();
            std::thread::spawn(move || {
                for (method, path, body) in asked {
                    if answer.send(crate::phone::said(&method, &path, &body)).is_err() {
                        break;
                    }
                }
            });
            (ask, answered)
        });
        if link.0.send((method.to_string(), path.to_string(), body)).is_ok() {
            self.phone.waiting += 1;
            self.phone.asked = Instant::now();
        }
    }

    /// Takes what the remote has answered, and asks it again how the PC is once that is due.
    fn hear_phone(&mut self) {
        if !self.remote {
            self.phone.state = None;
            return;
        }
        while let Some(answer) = self.phone.link.as_ref().and_then(|(_, answered)| answered.try_recv().ok()) {
            self.phone.waiting = self.phone.waiting.saturating_sub(1);
            // (What a look answers while something else was asked since is not how it is now: the last answer stands.)
            if self.phone.waiting == 0 && self.phone.slid.is_none() {
                if let Some(state) = answer {
                    self.phone.state = Some(state);
                }
            }
        }
        if self.phone.waiting == 0 && (self.phone.state.is_none() || self.phone.asked.elapsed() >= PHONE_ASKED_EVERY) {
            self.ask_phone("GET", "/v1/state", String::new());
        }
    }

    /// What of the phone's page is at a point of the window.
    fn phone_part(&self, x: f32, y: f32) -> Option<(Rect, super::phone_art::Part)> {
        self.phone.parts.iter().find(|(r, _)| x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h).cloned()
    }

    /// A press on the phone's page: a slider follows the pointer from there; anything else is done when it is let go on it.
    fn press_phone(&mut self, x: f32, y: f32) -> bool {
        let Some((track, part)) = self.phone_part(x, y) else { return false };
        if part.slides() {
            let at = part.along(track, x, y);
            self.phone.slid = Some((part, track, at));
            unsafe { SetCapture(self.hwnd) };
        } else {
            self.phone.pressed = Some(part);
            self.phone.held_since = Instant::now();
        }
        self.next_frame = Instant::now();
        true
    }

    /// The phone's page let go of at a point: what was slid is asked for as it was left, what was pressed is done.
    fn release_phone(&mut self, x: f32, y: f32) -> bool {
        use super::phone_art::Part;
        use serde_json::json;
        let state = self.phone.state.clone().unwrap_or(serde_json::Value::Null);
        let flag = |name: &str| state.get(name).and_then(serde_json::Value::as_bool);
        if let Some((part, _, at)) = self.phone.slid.take() {
            unsafe {
                let _ = ReleaseCapture();
            }
            // A colour being made: the lights take it as it is left.
            if let (Part::Hue | Part::Depth, Some(making)) = (&part, self.phone.making.as_mut()) {
                match part {
                    Part::Hue => making.hue = at * 360.0,
                    _ => making.depth = at,
                }
                let made = making.hex();
                if let Some(state) = self.phone.state.as_mut() {
                    state["light"]["color"] = json!(made);
                }
                self.ask_phone("POST", "/v1/lights", json!({ "on": true, "color": made }).to_string());
                self.next_frame = Instant::now();
                return true;
            }
            let (path, body) = match part {
                Part::Brightness => ("/v1/lights", json!({ "on": true, "brightness": (5.0 + at * 95.0).round() as u32 })),
                Part::MonitorBrightness(index) => ("/v1/monitor", json!({ "index": index, "brightness": (at * 100.0).round() as u32 })),
                _ => ("/v1/volume", json!({ "level": (at * 100.0).round() as u32 })),
            };
            // (Shown as asked for till the remote says how it is.)
            if let Some(state) = self.phone.state.as_mut() {
                match &part {
                    Part::Brightness => state["light"]["brightness"] = json!((5.0 + at * 95.0).round()),
                    Part::MonitorBrightness(index) => state["monitors"][*index]["brightness"] = json!((at * 100.0).round()),
                    _ => state["volume"] = json!((at * 100.0).round()),
                }
            }
            self.ask_phone("POST", path, body.to_string());
            self.next_frame = Instant::now();
            return true;
        }
        let Some(part) = self.phone.pressed.take() else { return false };
        if self.phone_part(x, y).map(|(_, part)| part) != Some(part.clone()) {
            return true;
        }
        let (path, body) = match &part {
            Part::Night => ("/v1/night", json!({ "on": flag("night") != Some(true) })),
            Part::Screen => ("/v1/screen", json!({ "on": flag("screen") == Some(false) })),
            Part::Lights => ("/v1/lights", json!({ "on": flag("lights") == Some(false) })),
            Part::Power(id) => ("/v1/power", json!({ "plan": id })),
            Part::Colour(colour) => {
                self.phone.making = None;
                ("/v1/lights", json!({ "on": true, "color": colour }))
            }
            // The ring opened from the colour in use, or shut as it is.
            Part::Make => {
                self.phone.making = match self.phone.making {
                    Some(_) => None,
                    None => Some(super::phone_art::Making::from(state["light"]["color"].as_str().unwrap_or("FFFFFF"))),
                };
                self.next_frame = Instant::now();
                return true;
            }
            // Kept with the others, for this PC's settings and every phone, where it is not one of them already.
            Part::Use => {
                let Some(making) = self.phone.making.take() else { return true };
                self.next_frame = Instant::now();
                let (made, mut kept) = (making.hex(), super::phone_art::kept_colours(&state));
                if kept.contains(&made) {
                    return true;
                }
                kept.push(made);
                ("/v1/colours", json!({ "colours": kept }))
            }
            Part::Remove(colour) => {
                let kept: Vec<String> = super::phone_art::kept_colours(&state).into_iter().filter(|one| one != colour).collect();
                ("/v1/colours", json!({ "colours": kept }))
            }
            Part::Done => {
                self.phone.removing = false;
                self.next_frame = Instant::now();
                return true;
            }
            // Given up: the lights go back to the colour they had.
            Part::GiveUp => {
                let Some(making) = self.phone.making.take() else { return true };
                self.next_frame = Instant::now();
                if state["light"]["color"].as_str().is_some_and(|now| now.eq_ignore_ascii_case(&making.was)) {
                    return true;
                }
                ("/v1/lights", json!({ "on": true, "color": making.was }))
            }
            Part::Effect(effect) => ("/v1/lights", json!({ "on": true, "effect": effect })),
            Part::Monitor(index) => ("/v1/monitor", json!({ "index": index, "on": state["monitors"][*index]["on"].as_bool() == Some(false) })),
            Part::Keys(level) => ("/v1/keys", json!({ "level": level })),
            Part::Mute => ("/v1/volume", json!({ "muted": flag("muted") != Some(true) })),
            Part::Media(what) => ("/v1/media", json!({ "do": what })),
            // Sleep is asked about first: everything on the PC stops.
            Part::Sleep => {
                let lang = self.lang;
                let asked = super::ask::Asked {
                    title: pick(lang, "让电脑睡眠？", "Put the PC to sleep?"),
                    intro: pick(
                        lang,
                        "电脑上的一切都会停下：下载、远程连接、正在运行的程序。只想关屏关灯、让电脑继续运行，请用夜间模式。",
                        "Everything on the PC stops: downloads, remote connections, programs at work. To turn off only the screens and the lights and leave the PC running, use night mode.",
                    ),
                    own: None,
                    sent: None,
                    yes: pick(lang, "睡眠", "Sleep"),
                    no: pick(lang, "取消", "Cancel"),
                    dark: self.dark,
                };
                let agreed = super::ask::show(self.hwnd, &asked).is_some();
                if !agreed {
                    return true;
                }
                ("/v1/sleep", json!({}))
            }
            Part::Brightness | Part::MonitorBrightness(_) | Part::Volume | Part::Hue | Part::Depth => return true,
        };
        // Shown as asked for at once, till the remote says how it is: it takes a moment to do it and answer.
        if let Some(state) = self.phone.state.as_mut() {
            match &part {
                Part::Night => state["night"] = body["on"].clone(),
                Part::Screen => state["screen"] = body["on"].clone(),
                Part::Lights => state["lights"] = body["on"].clone(),
                Part::Power(id) => {
                    for mode in state["power"].as_array_mut().into_iter().flatten() {
                        mode["on"] = json!(mode["id"].as_str() == Some(id));
                    }
                }
                Part::Colour(_) | Part::GiveUp => state["light"]["color"] = body["color"].clone(),
                Part::Use | Part::Remove(_) => state["colours"] = body["colours"].clone(),
                Part::Effect(effect) => state["light"]["effect"] = json!(effect),
                Part::Monitor(index) => state["monitors"][*index]["on"] = body["on"].clone(),
                Part::Keys(level) => state["keys"]["level"] = json!(level),
                Part::Mute => state["muted"] = body["muted"].clone(),
                Part::Media("toggle") => state["media"]["playing"] = json!(state["media"]["playing"].as_bool() == Some(false)),
                _ => {}
            }
        }
        self.ask_phone("POST", path, body.to_string());
        self.next_frame = Instant::now();
        true
    }

    fn paint_overlay_preview(&mut self, frame: &Frame, area: Rect) {
        let overlay = &self.settings.overlay;
        let readings = {
            let history = crate::app().controller.history.read().unwrap();
            let Some(sample) = history.back() else { return };
            super::overlay::preview(sample, &overlay.chosen(), self.lang)
        };
        let frames = super::overlay::preview_frames();
        let (sw, sh) = self.stage.size;
        let size = overlay.size.clamp(SIZES.0, SIZES.1);
        let width = |text: &str, font| frame.gfx.measure(text, font);
        let line = |font| frame.gfx.baseline(font);
        let metrics = super::overlay::Metrics { width: &width, line: &line };
        let shape = overlay.shape((sw - 2.0 * super::overlay::INSET) / size);
        let glass = |k: f32| {
            let px = self.scale * k * size;
            (px, super::overlay::size(&readings, shape, px, &metrics), super::overlay::radius(&readings, shape, px, &metrics))
        };
        // At its own size, as on screen: the part of the screen around it
        // that the preview holds (all of a screen smaller than that); smaller
        // only where the overlay, shadow and all, is wider or taller than
        // the preview.
        let k = (area.w / sw).min(area.h / sh).max(1.0);
        let (_, (gw, gh), _) = glass(k);
        let room = 2.0 * super::overlay::MARGIN;
        let k = k.min(area.w / (gw * size + room)).min(area.h / (gh * size + room));
        let (px, (gw, gh), _) = glass(k);
        let (w, h) = (gw * size, gh * size);
        let inset = super::overlay::INSET;
        let x = inset + overlay.at.0.clamp(0.0, 1.0) * (sw - 2.0 * inset - w).max(0.0);
        let y = inset + overlay.at.1.clamp(0.0, 1.0) * (sh - 2.0 * inset - h).max(0.0);
        let view = |at: f32, length: f32, room: f32, extent: f32| (at + length / 2.0 - room / 2.0 / k).clamp(0.0, (extent - room / k).max(0.0));
        let (vx, vy) = (view(x, w, area.w, sw), view(y, h, area.h, sh));
        let (ox, oy) = (area.x - vx * k + ((area.w - sw * k) / 2.0).max(0.0), area.y - vy * k + ((area.h - sh * k) / 2.0).max(0.0));
        // (Inside the preview's rounded corners.)
        let Ok(mask) = (unsafe { frame.gfx.factory.CreateRoundedRectangleGeometry(&rounded(area, 8.0)) }) else { return };
        let layer = D2D1_LAYER_PARAMETERS1 {
            contentBounds: rect(area.x, area.y, area.w, area.h),
            geometricMask: std::mem::ManuallyDrop::new(windows::core::Interface::cast(&mask).ok()),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: 1.0,
            ..Default::default()
        };
        unsafe { frame.dc.PushLayer(&layer, None) };
        if self.stage.bitmap.as_ref().is_none_or(|(key, _)| *key != 1.0f32.to_bits()) {
            self.stage.bitmap = self.stage.desktop.as_ref().and_then(|desktop| desktop.bitmap(&frame.dc, 1.0).ok()).map(|b| (1.0f32.to_bits(), b));
        }
        if let Some((_, bitmap)) = &self.stage.bitmap {
            unsafe {
                frame.dc.DrawBitmap(bitmap, Some(&rect(ox, oy, sw * k, sh * k)), 1.0, D2D1_INTERPOLATION_MODE_LINEAR, None, None);
            }
        }
        // The glass, tinted for the screen around it, over the screen behind
        // it blurred as the compositor blurs it.
        let screen = Matrix3x2::scale(k, k) * Matrix3x2::translation(ox, oy);
        let placed = Matrix3x2::scale(size, size) * Matrix3x2::translation(x, y) * screen;
        let around = super::overlay::MARGIN + 16.0;
        let whole = |v: f32| v.round() as i32;
        let hole = RECT { left: whole(x), top: whole(y), right: whole(x + w), bottom: whole(y + h) };
        let ring = RECT { left: whole(x - around), top: whole(y - around), right: whole(x + w + around), bottom: whole(y + h + around) };
        let frost = 1.0 - overlay.clear.clamp(0.0, 1.0);
        let glass = super::overlay::Glass { presence: frost, ..super::overlay::glass(&mut self.stage.desktop.as_ref().map_or(Vec::new(), |desktop| desktop.luminances(ring, hole, 1)), None) };
        // (Its tint alone over the screen as it is: the overlay does not blur what is behind it.)
        frame.place(placed);
        frame.crisp_text();
        super::overlay::shadow(frame, &readings, shape, glass, false, px);
        super::overlay::paint(frame, &readings, &frames, shape, glass, super::overlay::Hand::Free, px);
        frame.origin(0.0, 0.0);
        unsafe { frame.dc.PopLayer() };
        drop(std::mem::ManuallyDrop::into_inner(layer.geometricMask));
    }

    /// What plays, with the elements chosen, slid out from the edge that opens it on the screen the window opened on
    /// (as large as it was made there: see `MusicRooms`), as the readings' and the workbench's previews show theirs;
    /// the example while no music plays. Returns how soon it is to be drawn again (as the preview's charts' speed: see
    /// `paint`), while something plays.
    fn paint_music_preview(&mut self, frame: &Frame, area: Rect) -> Option<f32> {
        let edge = self.preview_edge();
        let (sw, sh) = self.stage.size;
        let shows = super::music::Shows::from(&self.settings.view);
        let size = super::music::rests_at(self.settings.music_rooms.at(edge == Edge::Top), &shows);
        let zoom = self.settings.panel_size.clamp(SIZES.0, SIZES.1);
        let (pw, ph) = (size.0 * zoom, size.1 * zoom);
        let inset = Theme::new(Skin::named(&self.settings.skin), false).inset * zoom;
        let along = |at: f32, length: f32, extent: f32| (at - length / 2.0).min(extent - GAP - length).max(GAP);
        let pos = match edge {
            Edge::Left => (inset, along(sh / 2.0, ph, sh)),
            Edge::Right => (sw - inset - pw, along(sh / 2.0, ph, sh)),
            Edge::Top => (along(sw / 2.0, pw, sw), inset),
        };
        let (k, stage_x, stage_y) = self.staged(area, edge, (pw, ph), false);
        let (ox, oy) = (area.x + stage_x, area.y + stage_y);
        self.stage_picture(frame, area, (ox, oy), k, zoom);
        unsafe { frame.dc.PushAxisAlignedClip(&rect(area.x, area.y, area.w, area.h), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
        let (at, scale) = ((ox + pos.0 * k, oy + pos.1 * k), zoom * k);
        let pointer = self.pointer.map(|(x, y)| ((x - at.0) / scale, (y - at.1) / scale)).filter(|(x, y)| *x >= 0.0 && *y >= 0.0 && *x < size.0 && *y < size.1);
        frame.place(Matrix3x2::scale(scale, scale) * Matrix3x2::translation(at.0, at.1));
        let ground = super::music::Ground::of(&shows);
        let now = Instant::now();
        let painted = if super::music::heard() {
            self.music.paint(frame, size, &shows, ground, self.scale * scale, pointer, now)
        } else {
            self.music.paint_example(frame, size, &shows, ground, self.scale * scale, pointer, now, self.example_from.elapsed().as_secs_f64())
        };
        frame.origin(0.0, 0.0);
        unsafe { frame.dc.PopAxisAlignedClip() };
        (painted.busy || !super::music::heard()).then_some(MUSIC_PREVIEW_PACE)
    }

    /// The preview: the panel as it will look over the desktop, running live.
    fn paint_preview(&mut self, frame: &Frame, palette: &Palette, width: f32, height: f32) -> Option<f32> {
        let lang = self.lang;
        let x0 = NAV + PANE + PAD_SIDE;
        let head = Font::new(Family::SegoeDisplay, 20.0, 600.0);
        let title = if self.page == Page::Layout { pick(lang, "屏幕", "Screens") } else { pick(lang, "预览", "Preview") };
        text_centred(frame, title, head, palette.text, x0, PAD_TOP + 18.0, 200.0, Align::Start);
        let detail = match self.page {
            Page::Layout => pick(lang, "屏幕的排列与 Windows 设置一致，桌面为当前壁纸", "The screens as Windows arranges them, over your wallpaper"),
            Page::General => pick(lang, "手机上的 Glance", "Glance on the phone"),
            Page::Music => pick(lang, "正在播放的内容，桌面为当前壁纸", "What plays now, over your wallpaper"),
            _ => pick(lang, "实时数据，桌面为当前壁纸", "Live readings over your wallpaper"),
        };
        let title_w = frame.gfx.measure(title, head);
        let area = Rect { x: x0, y: PAD_TOP + 40.0, w: width - NAV - PANE - 2.0 * PAD_SIDE, h: height - PAD_TOP - 40.0 - PAD_TOP };
        // (No wider than the preview under it: where it is narrow, cut short with an ellipsis.)
        let detail_x = x0 + title_w + 12.0;
        text_centred(frame, detail, Font::new(Family::Segoe, 12.0, 400.0), palette.text2, detail_x, PAD_TOP + 20.0, (area.x + area.w - detail_x).max(0.0), Align::Start);
        if area.w <= 0.0 || area.h <= 0.0 {
            return None;
        }
        fill(frame, palette.card, area.x, area.y, area.w, area.h, 8.0);
        stroke_inside(frame, area, 8.0, palette.card_stroke);

        if self.page == Page::Monitor && self.tab == Tab::Overlay {
            self.paint_overlay_preview(frame, area);
            return None;
        }
        if self.page == Page::Music {
            return self.paint_music_preview(frame, area);
        }
        if self.page == Page::Layout {
            return self.paint_layout(frame, palette, area, Instant::now());
        }
        // The general page: the phone app as a phone shows it, in a phone's outline.
        if self.page == Page::General {
            // (Its fans turn: drawn again some thirty times a second while they do.)
            return self.paint_phone(frame, area).then_some(7.5);
        }

        // The readings (or the workbench), laid out for the screen the window opened on, centred along the edge they
        // open from.
        let prefs = self.prefs.clone().holding(self.previewed());
        let edge = self.preview_edge();
        self.panel_on_stage(frame, area, Some(edge), Some(&prefs), false).0
    }

    /// How the stage is shown in `area` with a panel `(pw, ph)` large (stage DIPs) slid out from `edge`: how large a
    /// stage DIP is drawn, and where the stage's corner is in the area. The whole screen (`whole`), else the whole
    /// height of it (along the top, the panel and the screen below) with the whole panel and a strip of desktop
    /// beside it.
    fn staged(&self, area: Rect, edge: Edge, (pw, ph): (f32, f32), whole: bool) -> (f32, f32, f32) {
        let (sw, sh) = self.stage.size;
        let fit = (area.w / sw).min(area.h / sh).min(1.0);
        match (whole, edge) {
            (true, _) => (fit, (area.w - sw * fit) / 2.0, (area.h - sh * fit) / 2.0),
            (false, Edge::Left | Edge::Right) => {
                let k = (area.h / sh).min(area.w / (pw + PREVIEW_MARGIN).min(sw)).min(1.0);
                // A screen that fits whole, in the middle; one that does
                // not, its side the panel is on.
                let x = if sw * k <= area.w {
                    (area.w - sw * k) / 2.0
                } else if edge == Edge::Left {
                    0.0
                } else {
                    area.w - sw * k
                };
                (k, x, (area.h - sh * k) / 2.0)
            }
            (false, Edge::Top) => {
                let shown = ph + PREVIEW_MARGIN;
                let k = (area.w / sw).min(area.h / shown).min(1.0);
                (k, (area.w - sw * k) / 2.0, ((area.h - shown * k) / 2.0).max(0.0))
            }
        }
    }

    /// The stage's desktop in `area` and, where `edge` is one, the panel showing `prefs` slid out from it, running
    /// live: the whole screen (`whole`), else the whole height of it (along the top, the panel and the screen below)
    /// with the whole panel and a strip of desktop beside it. Returns how fast the panel's charts move (physical pixels
    /// a second), and where the screen was drawn.
    fn panel_on_stage(&mut self, frame: &Frame, area: Rect, edge: Option<Edge>, prefs: Option<&Prefs>, whole: bool) -> (Option<f32>, Rect) {
        let lang = self.lang;
        let (sw, sh) = self.stage.size;
        let fit = (area.w / sw).min(area.h / sh).min(1.0);
        let (Some(edge), Some(prefs)) = (edge, prefs) else {
            let (ox, oy) = (area.x + (area.w - sw * fit) / 2.0, area.y + (area.h - sh * fit) / 2.0);
            self.stage_picture(frame, area, (ox, oy), fit, 1.0);
            return (None, Rect { x: ox, y: oy, w: sw * fit, h: sh * fit });
        };
        let app = crate::app();
        let skin = Skin::named(&self.settings.skin);
        let interval = self.settings.interval_ms as f64;
        // Held while the preview is drawn; the sampler waits that long.
        let history = app.controller.history.read().unwrap();
        let samples = crate::panel::contiguous(&history);
        if samples.is_empty() {
            return (None, Rect { x: area.x, y: area.y, w: area.w, h: area.h });
        }
        let wall = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64() * 1000.0;
        let pen = wall - interval - PEN_LAG_MS;
        let measure = Theme::new(skin, false);
        // The preview shows what the panel would hold if it opened now.
        let seen = app.controller.seen.lock().unwrap().clone();
        let probe = Scene { music_apart: false, info: &app.info, prefs, theme: &measure, lang, history: samples, seen: &seen, pen_ms: pen, process_scroll: 0.0, hover: None, pinned: false, overlay: crate::app().controller.overlay_wanted(), buttons: true, newer: update::available().is_some() };
        let heights: Vec<f32> = view::lanes(&probe).iter().map(|lane| lane.height(&measure)).collect();
        self.largest = arrange::largest(&measure, edge, &heights, (sw, sh), self.settings.columns, SIZES.1);
        let size = self.settings.panel_size.clamp(SIZES.0, SIZES.1);
        let (layout, zoom) = arrange::arrange(&measure, edge, heights, (sw, sh), self.settings.columns, size);
        let (pw, ph) = (layout.width() * zoom, layout.height() * zoom);
        let inset = measure.inset * zoom;
        let along = |at: f32, length: f32, extent: f32| (at - length / 2.0).min(extent - GAP - length).max(GAP);
        let pos = match edge {
            Edge::Left => (inset, along(sh / 2.0, ph, sh)),
            Edge::Right => (sw - inset - pw, along(sh / 2.0, ph, sh)),
            Edge::Top => (along(sw / 2.0, pw, sw), inset),
        };
        // The desktop behind the panel decides its theme when it follows the
        // backdrop, and how frosted glass is.
        let behind = RECT { left: pos.0 as i32, top: pos.1 as i32, right: (pos.0 + pw) as i32, bottom: (pos.1 + ph) as i32 };
        // (As grey, till the desktop has been read.)
        let tone = self.stage.desktop.as_ref().map_or((0.5, 0.0), |desktop| desktop.luminance(behind, 1.0));
        let dark = theme::is_dark(prefs.theme, Some(tone.0).filter(|_| skin.sees_backdrop()));
        let theme = Theme::new(skin, dark);
        let frost = if skin == Skin::Glass { skins::frost(tone.0, tone.1, dark) } else { 0.0 };
        let scene = Scene { music_apart: false, info: &app.info, prefs, theme: &theme, lang, history: samples, seen: &seen, pen_ms: pen, process_scroll: 0.0, hover: None, pinned: false, overlay: crate::app().controller.overlay_wanted(), buttons: true, newer: update::available().is_some() };
        let lanes = view::lanes(&scene);

        let (k, stage_x, stage_y) = self.staged(area, edge, (pw, ph), whole);
        let (ox, oy) = (area.x + stage_x, area.y + stage_y);
        let mask = unsafe {
            frame.gfx.factory.CreateRoundedRectangleGeometry(&D2D1_ROUNDED_RECT { rect: rect(area.x, area.y, area.w, area.h), radiusX: 8.0, radiusY: 8.0 })
        };
        let layer = D2D1_LAYER_PARAMETERS1 {
            contentBounds: rect(area.x, area.y, area.w, area.h),
            geometricMask: std::mem::ManuallyDrop::new(mask.ok().map(|m| windows::core::Interface::cast(&m).unwrap())),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: Matrix3x2::identity(),
            opacity: 1.0,
            ..Default::default()
        };
        unsafe { frame.dc.PushLayer(&layer, None) };
        // The desktop in the panel's units, which a zoomed panel shrinks.
        self.stage_picture(frame, area, (ox, oy), k, zoom);
        let local = Matrix3x2::scale(zoom, zoom) * Matrix3x2::translation(pos.0, pos.1) * Matrix3x2::scale(k, k) * Matrix3x2::translation(ox, oy);
        // The stage's desktop is a pixel a DIP: a panel DIP covers `zoom` of them.
        let backdrop = self.stage.bitmap.as_ref().map(|(_, bitmap)| (bitmap, Vector2 { X: pos.0 / zoom, Y: pos.1 / zoom }, self.stage.desktop.as_ref().map_or(0, |desktop| desktop.digest), zoom));
        let picture = render::Picture { scene: &scene, lanes: &lanes, layout: &layout, small: None, size: (layout.width(), layout.height()), at: (0.0, 0.0), edge: Some(edge), backdrop, frost, rough: false };
        let drawn = self.layers.draw(frame, &picture, local, self.scale * zoom * k);
        unsafe { frame.dc.PopLayer() };
        drop(std::mem::ManuallyDrop::into_inner(layer.geometricMask));
        match drawn {
            Ok(drawn) if drawn.grounded => self.gfx.clear_caches(),
            Ok(_) => {}
            Err(_) => self.failed = true,
        }
        (Some(layout.plot_width(&theme) * self.scale * zoom * k / prefs.chart_seconds as f32), Rect { x: ox, y: oy, w: sw * k, h: sh * k })
    }

    /// The stage's desktop at `at`, `k` DIPs to one of its own, clipped to `area`: made a bitmap for the panel's
    /// `zoom` (a zoomed panel's units shrink it), as it is drawn under it.
    fn stage_picture(&mut self, frame: &Frame, area: Rect, at: (f32, f32), k: f32, zoom: f32) {
        let (sw, sh) = self.stage.size;
        let zoom_key = zoom.to_bits();
        if self.stage.bitmap.as_ref().is_none_or(|(key, _)| *key != zoom_key) {
            self.stage.bitmap = self.stage.desktop.as_ref().and_then(|desktop| desktop.bitmap(&frame.dc, zoom).ok()).map(|b| (zoom_key, b));
        }
        if let Some((_, bitmap)) = &self.stage.bitmap {
            // (Inside the area's rounded corners.)
            let Ok(mask) = (unsafe { frame.gfx.factory.CreateRoundedRectangleGeometry(&rounded(area, 8.0)) }) else { return };
            let layer = D2D1_LAYER_PARAMETERS1 {
                contentBounds: rect(area.x, area.y, area.w, area.h),
                geometricMask: std::mem::ManuallyDrop::new(windows::core::Interface::cast(&mask).ok()),
                maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
                maskTransform: Matrix3x2::identity(),
                opacity: 1.0,
                ..Default::default()
            };
            unsafe {
                frame.dc.PushLayer(&layer, None);
                frame.dc.DrawBitmap(bitmap, Some(&rect(at.0, at.1, sw * k, sh * k)), 1.0, D2D1_INTERPOLATION_MODE_LINEAR, None, None);
                frame.dc.PopLayer();
            }
        }
    }
}

fn mix(a: Color, b: Color, t: f32) -> Color {
    Color { r: a.r + (b.r - a.r) * t, g: a.g + (b.g - a.g) * t, b: a.b + (b.b - a.b) * t, a: a.a + (b.a - a.a) * t }
}

fn rounded(r: Rect, radius: f32) -> D2D1_ROUNDED_RECT {
    D2D1_ROUNDED_RECT { rect: rect(r.x, r.y, r.w, r.h), radiusX: radius, radiusY: radius }
}

fn fill(frame: &Frame, color: Color, x: f32, y: f32, w: f32, h: f32, radius: f32) {
    let radius = radius.min(w / 2.0).min(h / 2.0);
    unsafe { frame.dc.FillRoundedRectangle(&rounded(Rect { x, y, w, h }, radius), frame.brush(color)) };
}

/// The picture `bitmap` filling the box `r`, its corners rounded, as
/// opaque as `opacity`.
fn picture_rounded(frame: &Frame, bitmap: &ID2D1Bitmap1, r: Rect, radius: f32, opacity: f32) {
    let Ok(mask) = (unsafe { frame.gfx.factory.CreateRoundedRectangleGeometry(&rounded(r, radius)) }) else { return };
    let layer = D2D1_LAYER_PARAMETERS1 {
        contentBounds: rect(r.x, r.y, r.w, r.h),
        geometricMask: std::mem::ManuallyDrop::new(windows::core::Interface::cast(&mask).ok()),
        maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
        maskTransform: Matrix3x2::identity(),
        opacity,
        ..Default::default()
    };
    unsafe {
        frame.dc.PushLayer(&layer, None);
        frame.dc.DrawBitmap(bitmap, Some(&rect(r.x, r.y, r.w, r.h)), 1.0, D2D1_INTERPOLATION_MODE_LINEAR, None, None);
        frame.dc.PopLayer();
    }
    drop(std::mem::ManuallyDrop::into_inner(layer.geometricMask));
}

/// A one-DIP border drawn inside the box, as CSS's `border`.
fn stroke_inside(frame: &Frame, r: Rect, radius: f32, color: Color) {
    let inner = Rect { x: r.x + 0.5, y: r.y + 0.5, w: r.w - 1.0, h: r.h - 1.0 };
    unsafe { frame.dc.DrawRoundedRectangle(&rounded(inner, (radius - 0.5).max(0.0)), frame.brush(color), 1.0, None) };
}

/// A ring `width` DIPs wide just outside the box, as CSS's `box-shadow: 0 0 0 width`.
fn stroke_outside(frame: &Frame, r: Rect, radius: f32, color: Color, width: f32) {
    let ring = Rect { x: r.x - width / 2.0, y: r.y - width / 2.0, w: r.w + width, h: r.h + width };
    unsafe { frame.dc.DrawRoundedRectangle(&rounded(ring, radius + width / 2.0), frame.brush(color), width, None) };
}

/// A row's card: filled, with a hairline border.
fn card(frame: &Frame, palette: &Palette, x: f32, y: f32, w: f32, h: f32, color: Color) {
    fill(frame, color, x, y, w, h, 6.0);
    stroke_inside(frame, Rect { x, y, w, h }, 6.0, palette.card_stroke);
}

/// One line of text centred on `cy`.
#[allow(clippy::too_many_arguments)]
fn text_centred(frame: &Frame, text: &str, font: Font, color: Color, x: f32, cy: f32, width: f32, align: Align) {
    let (ascent, descent) = frame.gfx.baseline(font);
    frame.text(text, font, color, x, cy - (ascent + descent) / 2.0, width, align);
}

/// A row's label, and below it, if there is one, its hint, centred together on `cy`.
#[allow(clippy::too_many_arguments)]
fn field_label(frame: &Frame, palette: &Palette, label: &str, hint: Option<&str>, label_font: Font, hint_font: Font, x: f32, cy: f32, width: f32) {
    field_label_in(frame, (palette.text, palette.text2), label, hint, label_font, hint_font, x, cy, width);
}

/// A row's label and hint in the colours given (label, hint), the hint
/// broken into as many lines as `width` makes it.
#[allow(clippy::too_many_arguments)]
fn field_label_in(frame: &Frame, colors: (Color, Color), label: &str, hint: Option<&str>, label_font: Font, hint_font: Font, x: f32, cy: f32, width: f32) {
    match hint {
        None => text_centred(frame, label, label_font, colors.0, x, cy, width, Align::Start),
        Some(hint) => {
            // A 20 DIP line, 2 apart, then the hint's lines.
            let width = width.max(40.0);
            let top = cy - (22.0 + frame.gfx.wrapped_height(hint, hint_font, width)) / 2.0;
            text_centred(frame, label, label_font, colors.0, x, top + 10.0, width, Align::Start);
            frame.text_wrapped(hint, hint_font, colors.1, x, top + 22.0, width);
        }
    }
}

/// What is said beside a code: what to do with it, and what comes of that.
fn code_words(lang: Lang, what: CodeFor, page: Option<&str>) -> (&'static str, Cow<'static, str>) {
    let (title, words) = match what {
        // (The first of each on one line: short enough for the room beside the code.)
        CodeFor::App => (pick(lang, "扫码下载 Glance", "Scan to get Glance"), crate::phone::APP_PAGE.trim_start_matches("https://")),
        CodeFor::Pairing => (pick(lang, "用 Glance APP 扫描", "Scan with the Glance app"), pick(lang, "打开手机上的 Glance，扫描此配对码。10 分钟内有效，仅供一部手机使用", "Open Glance on the phone and scan this code. Good for ten minutes, for one phone")),
        // (The address itself too, to be typed where the code cannot be scanned.)
        CodeFor::WebPage => {
            let words = pick(lang, "在此前配对时使用的浏览器中打开。电脑的网络地址变更后，原配对随之失效，请通过“用网页”重新配对", "Open it in the browser the phone was paired with. Once this PC's address on the network has changed, that pairing no longer holds: pair again with “For the web page”");
            return (pick(lang, "用手机相机扫描", "Scan with the camera"), Cow::Owned(format!("{}
{words}", page.unwrap_or_default())));
        }
        CodeFor::WebPairing => (pick(lang, "用手机相机扫描", "Scan with the camera"), pick(lang, "扫码后将在浏览器中打开，之后可添加到主屏幕。网页未加密，请仅在可信任的网络（如家庭网络）中使用。10 分钟内有效，仅供一部手机使用", "It opens in the browser, and can then be added to the home screen. The page is not encrypted: use it only on a network you trust, such as your home's. Good for ten minutes, for one phone")),
    };
    (title, Cow::Borrowed(words))
}

/// A row's label and hint, each on one line, cut short if they do not fit.
#[allow(clippy::too_many_arguments)]
fn field_label_line(frame: &Frame, palette: &Palette, label: &str, hint: Option<&str>, label_font: Font, hint_font: Font, x: f32, cy: f32, width: f32) {
    match hint {
        None => text_centred(frame, label, label_font, palette.text, x, cy, width, Align::Start),
        Some(hint) => {
            // A 20 DIP line, 2 apart, then a 16 DIP one.
            let top = cy - 19.0;
            text_centred(frame, label, label_font, palette.text, x, top + 10.0, width, Align::Start);
            text_centred(frame, hint, hint_font, palette.text2, x, top + 22.0 + 8.0, width, Align::Start);
        }
    }
}

/// A miniature of each skin, drawn the same whichever is active.
fn skin_miniature(frame: &Frame, skin: Skin, r: Rect) {
    let dc = &frame.dc;
    let inner = (r.x + 10.0, r.w - 20.0);
    let cy = r.y + r.h / 2.0;
    match skin {
        Skin::Paper => {
            fill(frame, Color::hex(0xEEF2EC, 1.0), r.x, r.y, r.w, r.h, 6.0);
            // Three ruled lines, the middle one in the signal colour.
            for (i, color) in [0x16221E, 0xD23F1C, 0x16221E].into_iter().enumerate() {
                let y = cy + (i as f32 - 1.0) * 13.5;
                fill(frame, Color::hex(color, 1.0), inner.0, y - 0.75, inner.1, 1.5, 0.0);
            }
        }
        Skin::Glass => {
            let stops = [
                D2D1_GRADIENT_STOP { position: 0.0, color: Color::hex(0x7AA7FF, 1.0).d2d() },
                D2D1_GRADIENT_STOP { position: 0.55, color: Color::hex(0xC79BFF, 1.0).d2d() },
                D2D1_GRADIENT_STOP { position: 1.0, color: Color::hex(0xFFB38A, 1.0).d2d() },
            ];
            let span = (r.w + r.h) * std::f32::consts::FRAC_1_SQRT_2 / 2.0 * std::f32::consts::FRAC_1_SQRT_2;
            let (cx, my) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
            unsafe {
                if let Ok(stops) = ID2D1RenderTarget::CreateGradientStopCollection(dc, &stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP) {
                    let line = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES {
                        startPoint: Vector2 { X: cx - span, Y: my - span },
                        endPoint: Vector2 { X: cx + span, Y: my + span },
                    };
                    if let Ok(brush) = dc.CreateLinearGradientBrush(&line, None, &stops) {
                        dc.FillRoundedRectangle(&rounded(r, 6.0), &brush);
                    }
                }
            }
            for i in 0..3 {
                let y = cy - 19.0 + i as f32 * 14.0;
                let pill = Rect { x: inner.0, y, w: inner.1, h: 10.0 };
                fill(frame, Color::hex(0xFFFFFF, 0.35), pill.x, pill.y, pill.w, pill.h, 5.0);
                stroke_inside(frame, pill, 5.0, Color::hex(0xFFFFFF, 0.4));
                fill(frame, Color::hex(0xFFFFFF, 0.9), pill.x + 4.0, pill.y, pill.w - 8.0, 1.0, 0.0);
            }
        }
        Skin::Fluent => {
            fill(frame, Color::hex(0xECEEF2, 1.0), r.x, r.y, r.w, r.h, 6.0);
            for i in 0..3 {
                let y = cy - 8.5 + i as f32 * 7.0;
                let (w, color) = if i == 0 { (inner.1 * 0.6, theme::accent(false)) } else { (inner.1, Color::hex(0xC8CCD2, 1.0)) };
                fill(frame, color, inner.0, y, w, 3.0, 1.5);
            }
        }
    }
}

/// Pictures of every page (and each of the monitor's tabs) at the least, the usual and a wide size of the window,
/// drawn off screen (the window made, not shown), to see that nothing reaches past what holds it. Into
/// `target/settings`: `cargo test --release --lib --features studio draws_every_page -- --ignored`.
#[cfg(all(test, feature = "studio"))]
mod pictures {
    use super::*;

    #[test]
    #[ignore = "draws on this PC's graphics card, to pictures to be looked at"]
    fn draws_every_page() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        // This machine's names, and two minutes of made-up readings up to now.
        let info = crate::metrics::Sampler::new().info;
        let now_ms = SystemTime::now().duration_since(UNIX_EPOCH).unwrap().as_secs_f64() * 1000.0;
        let history = (0..120).map(|i| crate::studio::made_up(&info, now_ms - (120 - i) as f64 * 1000.0, i as u64, 0.4, None)).collect();
        crate::pictured(info, history);
        let gfx = gfx::current().unwrap();
        let hwnd = make(gfx.clone(), false).unwrap();
        let out = std::path::PathBuf::from(format!("{}/target/settings", env!("CARGO_MANIFEST_DIR")));
        std::fs::create_dir_all(&out).unwrap();
        let shown = [
            (Page::Layout, Tab::Look, "layout"),
            (Page::Monitor, Tab::Look, "monitor-look"),
            (Page::Monitor, Tab::Shown, "monitor-shown"),
            (Page::Monitor, Tab::Data, "monitor-data"),
            (Page::Monitor, Tab::Overlay, "monitor-overlay"),
            (Page::Workbench, Tab::Look, "workbench"),
            (Page::Music, Tab::Look, "music"),
            (Page::General, Tab::Look, "general"),
        ];
        let scale = with_ui(|ui| ui.scale).unwrap();
        for (w, h) in [MIN_SIZE, SIZE, (1700.0, 900.0)] {
            unsafe {
                let _ = SetWindowPos(hwnd, None, 0, 0, (w * scale) as i32, (h * scale) as i32, SWP_NOZORDER | SWP_NOACTIVATE);
            }
            for (page, tab, name) in shown {
                with_ui(|ui| {
                    ui.tab = tab;
                    ui.turn_to(page);
                });
                // Drawn a while (the desktops read meanwhile, what moves come to rest), then pictured.
                for _ in 0..12 {
                    with_ui(|ui| ui.draw(Instant::now()));
                    std::thread::sleep(Duration::from_millis(80));
                }
                let (cw, ch) = with_ui(|ui| ui.client()).unwrap();
                let size = ((cw * scale).round() as u32, (ch * scale).round() as u32);
                let pixels = gfx.draw_offscreen(size, scale, |frame| {
                    with_ui(|ui| {
                        // (On the window's own ground: Mica is the system's, not drawn.)
                        fill(frame, ui.palette.window, 0.0, 0.0, cw, ch, 0.0);
                        ui.paint(frame, Instant::now())
                    });
                })
                .unwrap();
                crate::studio::save_png(&out.join(format!("{name}-{}.png", w as u32)), size.0, size.1, &pixels).unwrap();
            }
        }
        unsafe {
            let _ = windows::Win32::UI::WindowsAndMessaging::DestroyWindow(hwnd);
        }
    }
}
