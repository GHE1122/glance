//! Desktop widgets: the panel's readings torn off onto the desktop, each in
//! a window of its own above the others. Sized by its edges, a widget shows
//! as much as fits (see `arrange::Ladder`), each reading moving from its
//! place in one layout to its place in the next (see `morph`); let go near the edge of its screen, it sticks to it as a rail or a
//! strip; flung, it slides away and is put away. Pinned, it is neither moved
//! nor sized; letting clicks through, Ctrl held lets it take them again.
//! Moved near an edge, where it would stick shows before it is let go.

use std::rc::Rc;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::D2D_RECT_F;
use windows::Win32::Graphics::Direct2D::{ID2D1Bitmap1, D2D1_LAYER_PARAMETERS1};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};
use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, PostMessageW, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN, WM_APP};
use windows_numerics::{Matrix3x2, Vector2};

use crate::journal::note;

/// How long a kind of note that can come many times a second (a widget
/// being dragged takes the desktop again and again, and may draw slowly
/// all the while) is kept to one: the log is for what went wrong, and a
/// user's report carries only its last lines.
const SPARINGLY: Duration = Duration::from_secs(30);

/// Notes `what` of `kind`, unless one of that kind was noted within
/// `SPARINGLY`: then it is only counted, and the next one noted says how
/// many came between.
pub(crate) fn note_sparingly(kind: &'static str, what: impl FnOnce() -> String) {
    static LAST: std::sync::Mutex<Vec<(&'static str, Instant, u32)>> = std::sync::Mutex::new(Vec::new());
    let mut last = LAST.lock().unwrap();
    let now = Instant::now();
    match last.iter_mut().find(|(of, _, _)| *of == kind) {
        Some((_, at, between)) if now.duration_since(*at) < SPARINGLY => *between += 1,
        Some((_, at, between)) => {
            let more = std::mem::take(between);
            *at = now;
            drop(last);
            note(if more > 0 { format!("{} ({more} more like it since the last)", what()) } else { what() });
        }
        None => {
            last.push((kind, now, 0));
            drop(last);
            note(what());
        }
    }
}
use crate::panel::{cursor_position, monitor_at, Contact};
use crate::settings::{WidgetAt, WidgetSide};
use crate::ui::buttons::{self, Button};
use crate::ui::arrange::{self, Choice, Form, Want};
use crate::ui::backdrop::Capture;
use crate::ui::canvas::{Canvas, Color, Fill, Point};
use crate::ui::forms::{self, Small};
use crate::ui::gfx::{self, Gfx, Layer, Surface};
use crate::ui::icons::Icon;
use crate::ui::morph::{self, Part};
use crate::ui::prefs::Prefs;
use crate::ui::render::{self, PanelLayers};
use crate::ui::seen::Seen;
use crate::ui::skins;
use crate::ui::text::Lang;
use crate::ui::theme::{self, Skin, Theme};
use crate::ui::view::{self, Scene};
use crate::ui::window::Window;
use crate::reading::Sample;
use crate::settings::Edge;

/// How near its edge the pointer sizes a widget rather than moving it (DIPs).
const EDGE: f32 = 8.0;
/// How near a corner, along either of its edges, the pointer sizes both
/// (DIPs): a corner easy to take without turning into one edge.
const CORNER: f32 = 20.0;

/// The edges a press at (`x`, `y`) on a widget `w` × `h` sizes: left,
/// right, top, bottom.
fn edges_at(x: f32, y: f32, w: f32, h: f32) -> (bool, bool, bool, bool) {
    let (left, right, top, bottom) = (x < EDGE, x > w - EDGE, y < EDGE, y > h - EDGE);
    let (side, end) = (left || right, top || bottom);
    (left || end && x < CORNER, right || end && x > w - CORNER, top || side && y < CORNER, bottom || side && y > h - CORNER)
}
/// How far the pointer moves a pressed widget before it moves (DIPs).
const MOVE_FROM: f32 = 4.0;
/// How far past what its form fills a widget's edge follows the hand:
/// half the way, as a band would stretch, loose enough that the way to a
/// layout showing much more is not a long pull.
pub(crate) const GIVE: f32 = 0.5;
/// How long a widget let go after it was sized, or stuck to an edge, takes
/// to settle onto what it shows.
pub(crate) const SETTLE: Duration = Duration::from_millis(260);
/// How fast a widget let go (DIPs a millisecond, over the last moments of
/// the hand) is flung rather than put down.
const FLING: f32 = 1.1;
/// How long the hand's last moments are, for its speed.
const FLING_SPAN: Duration = Duration::from_millis(80);
/// What a flung widget keeps of its speed each millisecond.
const FRICTION: f32 = 0.997;
/// How near the edge of its screen's work area a widget let go sticks to
/// it (DIPs), and how far from it, pulled, it comes away.
const STICK: f32 = 36.0;
const UNSTICK: f32 = 52.0;
/// A frame that takes longer than this is noted in the journal, its parts'
/// times with it.
const SLOW_FRAME: Duration = Duration::from_millis(12);
/// How often a widget at rest is drawn again (its charts step with the
/// samples).
const REST_FRAME: Duration = Duration::from_millis(250);
/// With live refraction, how often the desktop behind is taken again as it
/// changes, as the panel's is: fifteen times a second (what shows through
/// the glass is blurred; faster cost twice the processor for little that
/// shows).
const LIVE_INTERVAL: Duration = Duration::from_millis(66);
/// Without live refraction, how old the desktop taken behind a widget is
/// before letting go of it takes it again.
const RETAKE_AFTER: Duration = Duration::from_secs(3);
/// Without live refraction, how long the windows on the desktop are left
/// to settle after one changed (moved, came to the front, opened, closed)
/// before the desktop is taken again; and how often at most it is.
const SETTLED: Duration = Duration::from_millis(300);
const RETAKE_EVERY: Duration = Duration::from_secs(1);

/// When another program's window last changed on the desktop (see
/// `WindowWatch`), as milliseconds since `EPOCH`; 0 for never.
static WINDOWS_CHANGED: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// Where the widgets are on the screens (physical px): a window changing
/// elsewhere changes nothing behind them.
static WIDGET_AREAS: std::sync::Mutex<Vec<RECT>> = std::sync::Mutex::new(Vec::new());
/// The order of the windows on the screens changed: the desktop's own may
/// have moved among the others (Win+D brings it in front of them, after the
/// front has changed), and the widgets kept on it are put back just above it.
static ORDER_CHANGED: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// How many widgets are on the desktop, shown: while there are, the device
/// they draw with keeps what it holds (see `any_shown`).
static SHOWN: std::sync::atomic::AtomicUsize = std::sync::atomic::AtomicUsize::new(0);

/// Whether a widget is on the desktop, shown.
pub fn any_shown() -> bool {
    SHOWN.load(std::sync::atomic::Ordering::Relaxed) > 0
}

/// Whether `r` meets a widget (physical px).
fn meets_a_widget(r: RECT) -> bool {
    WIDGET_AREAS.lock().unwrap().iter().any(|a| r.left < a.right && a.left < r.right && r.top < a.bottom && a.top < r.bottom)
}

/// What is behind the widgets changed, now.
fn behind_changed() {
    if let Some(epoch) = EPOCH.get() {
        WINDOWS_CHANGED.store(epoch.elapsed().as_millis() as u64 + 1, std::sync::atomic::Ordering::Relaxed);
    }
}

/// One of Glance's own windows (the panel) came or went at `rect`: the
/// system does not say so (see `WindowWatch`), and a widget that took the
/// desktop with it there shows it still.
pub fn own_window_changed(rect: RECT) {
    if meets_a_widget(rect) {
        behind_changed();
    }
}
static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();

/// How often the other programs' windows are looked at, while there are widgets.
const WINDOWS_LOOKED_AT: Duration = Duration::from_millis(250);

/// The other programs' windows, looked at on a thread of their own every `WINDOWS_LOOKED_AT` for as long as this is
/// kept: one shown that meets a widget moved, sized, come or gone, or put in front of another there (each widget's
/// backdrop may have changed), and the window just above the desktop's own changed (the desktop put in front of the
/// others, Win+D, say: the widgets kept on it follow). Looked at, not heard of: the system tells of such changes only
/// with the rest of its window events, each one from every program a call into this process that takes the system's
/// lock for windows; a program that raises them by the thousand (Task Manager's list, filled), or moves a window
/// hundreds of times a second, makes every program that hears them hold that lock up, and the pointer with it.
struct WindowWatch {
    /// Dropped, it ends the looking.
    _ends: std::sync::mpsc::Sender<()>,
}

/// What the widgets care about of the other programs' windows, at one look.
#[derive(PartialEq, Default)]
struct WindowsSeen {
    /// Those shown (not minimized, nor kept off the screen) that meet a widget, from the front, with where they are.
    meeting: Vec<(isize, RECT)>,
    /// The window just above the desktop's own.
    above_desktop: isize,
}

impl WindowsSeen {
    fn look() -> WindowsSeen {
        use windows::Win32::Graphics::Dwm::{DwmGetWindowAttribute, DWMWA_CLOAKED};
        use windows::Win32::UI::WindowsAndMessaging::{EnumWindows, GetWindow, GetWindowRect, GetWindowThreadProcessId, IsIconic, IsWindowVisible, GW_HWNDPREV};
        unsafe extern "system" fn each(window: HWND, found: LPARAM) -> windows::core::BOOL {
            unsafe { (*(found.0 as *mut Vec<HWND>)).push(window) };
            true.into()
        }
        let mut windows: Vec<HWND> = Vec::new();
        let _ = unsafe { EnumWindows(Some(each), LPARAM(&mut windows as *mut _ as isize)) };
        let own = std::process::id();
        let meeting = windows
            .into_iter()
            .filter(|&window| {
                let mut pid = 0u32;
                unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
                let mut cloaked = 0u32;
                let cloaked = unsafe { DwmGetWindowAttribute(window, DWMWA_CLOAKED, &mut cloaked as *mut _ as *mut _, size_of::<u32>() as u32) }.is_ok() && cloaked != 0;
                pid != own && unsafe { IsWindowVisible(window) }.as_bool() && !unsafe { IsIconic(window) }.as_bool() && !cloaked
            })
            .filter_map(|window| {
                let mut r = RECT::default();
                unsafe { GetWindowRect(window, &mut r) }.ok()?;
                meets_a_widget(r).then_some((window.0 as isize, r))
            })
            .collect();
        let above_desktop = crate::ui::window::desktop_window().and_then(|desktop| unsafe { GetWindow(desktop, GW_HWNDPREV) }.ok()).map_or(0, |above| above.0 as isize);
        WindowsSeen { meeting, above_desktop }
    }
}

impl WindowWatch {
    fn start() -> WindowWatch {
        EPOCH.get_or_init(Instant::now);
        let (ends, ended) = std::sync::mpsc::channel::<()>();
        std::thread::Builder::new().name("glance: windows watched".into()).spawn(move || {
            let mut seen = WindowsSeen::look();
            while ended.recv_timeout(WINDOWS_LOOKED_AT) == Err(std::sync::mpsc::RecvTimeoutError::Timeout) {
                let now = WindowsSeen::look();
                if now.meeting != seen.meeting {
                    behind_changed();
                }
                // (The widgets' thread woken for it: they follow at once.)
                if now.above_desktop != seen.above_desktop && !ORDER_CHANGED.swap(true, std::sync::atomic::Ordering::Relaxed) {
                    crate::app().controller.wake();
                }
                seen = now;
            }
        }).expect("a thread started");
        WindowWatch { _ends: ends }
    }
}

/// When another program's window last changed, if one has.
fn windows_changed() -> Option<Instant> {
    let ms = WINDOWS_CHANGED.load(std::sync::atomic::Ordering::Relaxed);
    let epoch = EPOCH.get()?;
    (ms > 0).then(|| *epoch + Duration::from_millis(ms - 1))
}
/// How long one layout takes to turn into the next: as long as the frame
/// takes to settle, on the same ease.
pub(crate) const MORPH: Duration = SETTLE;
/// The buttons shown above a widget the pointer is on (under it, where its
/// screen has no room above), clear of what it shows: their size, and how
/// far from it (DIPs).
const BUTTON: f32 = buttons::SIZE;
const BUTTON_INSET: f32 = 6.0;
/// How often a widget that lets clicks through looks for the pointer while
/// it is over it (it hears nothing of it): for its one button to show, and
/// to take the click on it.
const WATCHED: Duration = Duration::from_millis(40);
/// How near the edge of its screen a widget moved shows where it would
/// stick (DIPs).
const HINT_SEEN: f32 = 64.0;

/// The side of its screen a widget is stuck to.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Side {
    Left,
    Right,
    Top,
    Bottom,
}

impl Side {
    fn kept(self) -> WidgetSide {
        match self {
            Side::Left => WidgetSide::Left,
            Side::Right => WidgetSide::Right,
            Side::Top => WidgetSide::Top,
            Side::Bottom => WidgetSide::Bottom,
        }
    }

    fn from_kept(side: WidgetSide) -> Self {
        match side {
            WidgetSide::Left => Side::Left,
            WidgetSide::Right => Side::Right,
            WidgetSide::Top => Side::Top,
            WidgetSide::Bottom => Side::Bottom,
        }
    }
}

/// A widget in hand: moved, from where it was taken (the pointer's offset
/// from its corner, physical px), the pointer's last places (for a fling)
/// and whether it was just torn off and has not left the edge yet (put
/// down there, it is neither flung nor stuck back to it); or
/// sized by `edges` (left, right, top, bottom), from the pointer and its
/// room as the drag began.
enum Drag {
    Move { grab: POINT, trail: Vec<(Instant, POINT)>, torn: bool, moved: bool },
    Size { edges: (bool, bool, bool, bool), from: POINT, room: (f32, f32), corner: POINT },
    /// The sound's track slid along: where it begins and how wide it is (from the widget's corner, its own DIPs).
    Slide { from: f32, wide: f32 },
}

/// What the widgets are drawn with, as the panel has them now.
pub struct Look<'a> {
    pub prefs: &'a Prefs,
    pub skin: Skin,
    pub lang: Lang,
    /// Live refraction: the desktop behind taken again every so often (see
    /// `LIVE_INTERVAL`).
    pub live: bool,
}

/// What a widget showed: its form at its zoom, its size (its own DIPs), its
/// lanes laid out (or the small layout it showed instead) and how much of
/// each.
#[derive(Clone)]
struct Shown {
    form: Form,
    zoom: f32,
    size: (f32, f32),
    layout: view::Layout,
    small: Option<Small>,
    detail: view::Detail,
}

/// What a layout turns from: what was shown, not yet recorded, or what was
/// drawn, recorded.
enum Before {
    Shown(Shown),
    Drawn(Vec<Part>),
}

/// Where a widget moved would stick, let go now: a window of its own,
/// over the widget, shown while it is near an edge.
struct Hint {
    window: Window,
    surface: Option<Surface>,
    /// Where and how it was last drawn: drawn again only when that changes.
    drawn: Option<(RECT, bool, u32)>,
}

/// The desktop the panel had taken, lent to a widget torn off it: its
/// capture (without its pixels), as a bitmap at the scale it was made for,
/// and its tone.
pub struct Lent {
    pub capture: Capture,
    pub bitmap: (ID2D1Bitmap1, f32),
    pub tone: (f32, f32),
}

/// The desktop behind a widget, for the skins that see it.
struct Behind {
    capture: Capture,
    /// The capture as a bitmap, at the scale of the screen (physical px a
    /// DIP) it was made for.
    bitmap: Option<(ID2D1Bitmap1, f32)>,
    tone: (f32, f32),
    taken: Instant,
}

struct Widget {
    window: Window,
    gfx: Rc<Gfx>,
    surface: Option<Surface>,
    layers: PanelLayers,
    /// The screen it is on.
    contact: Contact,
    /// Its surface's corner on the screen (physical px).
    at: POINT,
    choice: Choice,
    /// While it is sized: the room its edges were dragged to (DIPs).
    room: Option<(f32, f32)>,
    /// Let go after it was sized or stuck: since when, and from what
    /// surface (DIPs), it settles onto what it shows.
    settle: Option<(Instant, (f32, f32))>,
    /// The sides of its surface what it shows keeps to (right, bottom).
    keeps_to: (bool, bool),
    drag: Option<Drag>,
    /// Flung: its speed (DIPs a millisecond), and how far it shows.
    fling: Option<(f32, f32)>,
    opacity: f32,
    stuck: Option<Side>,
    /// Where along it its middle is (physical px; none: where it was,
    /// restored from the settings): where the middle of what was stuck was.
    along: Option<i32>,
    /// What it showed before it was stuck to an edge, and how large it was
    /// then (DIPs on the screen; none, restored).
    before: Option<Choice>,
    before_size: Option<(f32, f32)>,
    /// Its skin's room round it for its shadow (DIPs, its own).
    margin: f32,
    /// Settling after it was sized by its left or top edge: where its right
    /// and bottom edges stay (physical px).
    anchor: Option<POINT>,
    pinned: bool,
    click_through: bool,
    /// Torn off the panel and not clear of where it came from yet: the
    /// edge the panel was at (none: moved away from it), and its screen.
    torn_from: Option<(Option<Side>, RECT)>,
    game_only: bool,
    /// Kept on the desktop, under every other window (one shown only with
    /// a game is above them all still).
    on_desktop: bool,
    /// What it showed last frame.
    last: Option<Shown>,
    /// Turning from one layout into the one it shows: since when, from
    /// what; what was drawn last as it turns, and the layer it is drawn on.
    morph: Option<(Instant, Before)>,
    drawn: Vec<Part>,
    morph_layer: Layer,
    hint: Option<Hint>,
    /// What is behind it, put together live where no one sees it (see
    /// `scene`): while it sees the desktop and shows.
    backdrop: Option<crate::ui::scene::Scene>,
    /// When the windows behind it were last looked at for it.
    arranged: Option<Instant>,
    /// Looking at what is behind it again is due (let go, the settings
    /// gone from the front).
    retake: bool,
    /// What is behind it arranged anew, its frame awaited.
    pending: Option<Request>,
    /// What is behind it is to be replaced at once, even in hand: the
    /// panel's, lent as it was torn off (`false`: its tone kept), or taken
    /// of its screen as it was (`true`: toned afresh).
    replace: Option<bool>,
    /// Counts the changes of screen: a capture asked for before the last
    /// is of the screen as it was, and let go of.
    generation: u64,
    /// Come to rest where it was not when it was saved (flung, or settling
    /// after it was sized): saved again.
    unsaved: bool,
    /// When the last capture asked for came back with nothing.
    failed: Option<Instant>,
    /// Drawn and shown on the screen at least once.
    shown_once: bool,
    /// How frosted it was, measured where it was (see `draw`).
    frost_at: Option<(RECT, f32)>,
    /// Light or dark is to be measured again where it comes to rest (see
    /// `retone`).
    tone_due: bool,
    /// How long the last frame's painting took, and how long handing it to the compositor then did.
    frame_took: gfx::Took,
    /// The form it shows and the room it was taken in while sized: a form
    /// showing more comes back only with room to spare.
    taken: Option<arrange::Shown>,
    /// The pointer is over it (its buttons show).
    hover: bool,
    /// Letting clicks through: since when the pointer has been over it, and
    /// whether it is on the button that ends that (the window then takes
    /// the click).
    lingered: Option<Instant>,
    reached: bool,
    /// The button the pointer is on, said under it.
    tip: Option<Button>,
    /// What of its lanes takes a press (what plays, the sound), where each
    /// is from the corner of what it shows (its own DIPs), as last drawn.
    hits: Vec<view::HitBox>,
    hits_from: (f32, f32),
    behind: Option<Behind>,
    /// When it is next drawn: at once while it moves, else after a rest (as
    /// its charts step), or sooner as the pointer comes or it is pressed.
    next_frame: Instant,
    /// The size its window would have stuck to `Side` (physical px), while
    /// it is moved near that edge (see `show_hint`).
    hint_size: Option<(Side, (i32, i32))>,
    /// Its surface, and what it shows, as last laid out (DIPs, its own at
    /// its zoom), and the zoom.
    surface_size: (f32, f32),
    shown: (f32, f32),
    zoom: f32,
    last_frame: Instant,
}

/// Every widget on the desktop, and those put away this session, the last
/// first back.
#[derive(Default)]
pub struct Widgets {
    list: Vec<Widget>,
    put_away: Vec<WidgetAt>,
    /// The settings window, while it is in front: the widgets give way to it.
    settings: Option<HWND>,
    /// Restored from the settings once a graphics device is there.
    restored: bool,
    /// Widgets to add as they are (see `adopt`).
    adopted: Vec<WidgetAt>,
    /// The system saying when other programs' windows change (see
    /// `WindowWatch`), while there are widgets.
    watching: Option<WindowWatch>,
    /// Where each widget was last (its window, physical px), shown.
    areas: Vec<(isize, RECT)>,
    /// Live refraction, as the widgets were last drawn (see `Look::live`).
    live: bool,
}

impl Widgets {
    /// Whether `hwnd` is one of the widgets'.
    pub fn owns(&self, hwnd: HWND) -> bool {
        self.list.iter().any(|w| w.window.hwnd == hwnd)
    }

    /// A widget torn off the panel: showing `form` at `scale` with its
    /// corner at `at` on `contact`'s screen, in hand, the pointer `grab`
    /// from its corner; on the desktop the panel had taken (`lent`: its
    /// capture, as a bitmap, and its tone), if it had, till it takes its own
    /// (on another thread: it is drawn at once). `from`: the edge the panel
    /// was at, if it was at one.
    pub fn tear(&mut self, form: Form, scale: f32, at: POINT, grab: POINT, lent: Option<Lent>, from: Option<Edge>) -> bool {
        let Some(contact) = monitor_at(at) else { return false };
        let choice = Choice { form, scale, extra: (0.0, 0.0) };
        let made = Instant::now();
        let Some(mut widget) = Widget::new(contact, at, choice) else { return false };
        let made = made.elapsed();
        if made > SLOW_FRAME {
            note(format!("widget: made in {:.1} ms", made.as_secs_f32() * 1000.0));
        }
        widget.drag = Some(Drag::Move { grab, trail: Vec::new(), torn: true, moved: true });
        let side = from.map(|edge| match edge {
            Edge::Left => Side::Left,
            Edge::Right => Side::Right,
            Edge::Top => Side::Top,
        });
        widget.torn_from = Some((side, contact.monitor));
        if let Some(Lent { capture, bitmap, tone }) = lent {
            widget.behind = Some(Behind { capture, bitmap: Some(bitmap), tone, taken: Instant::now() });
            widget.replace = Some(false);
        }
        widget.window.yield_to(self.settings);
        unsafe { SetCapture(widget.window.hwnd) };
        self.list.push(widget);
        true
    }

    /// Whether the widget `hwnd` is in hand (moved, sized, or its sound's track slid).
    pub fn held(&self, hwnd: HWND) -> bool {
        self.list.iter().any(|widget| widget.window.hwnd == hwnd && widget.drag.is_some())
    }

    /// The widget torn off the panel and in hand still, if one is.
    pub fn torn_in_hand(&self) -> Option<HWND> {
        self.list.iter().find(|widget| matches!(widget.drag, Some(Drag::Move { torn: true, .. }))).map(|widget| widget.window.hwnd)
    }

    /// Whether the widget torn off last is on the screen yet.
    pub fn torn_shown(&self) -> bool {
        self.list.last().is_some_and(|widget| widget.shown_once)
    }

    /// Whether the widget torn off last is there, waiting to show.
    pub fn torn_waiting(&self) -> bool {
        self.list.last().is_some_and(|widget| !widget.shown_once)
    }

    /// Brings back the widget put away last; whether there was one.
    pub fn recall(&mut self) -> bool {
        let Some(kept) = self.put_away.pop() else { return false };
        if let Some(widget) = Widget::restore(&kept) {
            widget.window.yield_to(self.settings);
            self.list.push(widget);
            self.save();
        }
        true
    }

    /// The settings window came to the front (`Some`) or left it: every
    /// widget gives way to it meanwhile, as the panel does, so that it is
    /// never covered.
    pub fn settings_front(&mut self, settings: Option<HWND>) {
        // Left the front: what was taken while it was there, to be taken again.
        if self.settings.is_some() && settings.is_none() {
            for widget in &mut self.list {
                widget.retake = true;
                widget.next_frame = Instant::now();
            }
        }
        self.settings = settings;
        for widget in &self.list {
            widget.window.yield_to(settings);
        }
    }

    /// A frame of what is behind `hwnd` has come (see `CAPTURED`): taken,
    /// if it is due, and drawn with at once.
    pub fn captured(&mut self, hwnd: HWND) {
        let live = self.live;
        if let Some(widget) = self.list.iter_mut().find(|w| w.window.hwnd == hwnd) {
            let px = widget.contact.scale * widget.zoom;
            widget.collect(live, px);
            widget.next_frame = Instant::now();
        }
    }

    /// The screens changed (one added or taken away, its size or scale):
    /// each widget on the screen it is on now, all of it on it once at rest,
    /// the desktop behind it taken again.
    pub fn screens_changed(&mut self) {
        for widget in &mut self.list {
            let r = widget.rect();
            let Some(contact) = monitor_at(POINT { x: (r.left + r.right) / 2, y: (r.top + r.bottom) / 2 }).or_else(|| monitor_at(widget.at)) else { continue };
            // Its screen as it was (a window placed for the first time hears
            // of its DPI too): what was taken of it stands.
            if contact == widget.contact {
                continue;
            }
            note("widget: its screen changed; the desktop taken again");
            widget.moved_to(contact);
            if widget.behind.is_some() {
                widget.replace = Some(true);
            }
            widget.hint_size = None;
            widget.next_frame = Instant::now();
        }
    }

    /// A widget to add as the others are restored (a 0.2.3 panel left on
    /// the desktop), saved with them.
    pub fn adopt(&mut self, kept: WidgetAt) {
        self.adopted.push(kept);
    }

    /// Whether any widget was put away this session.
    pub fn any_put_away(&self) -> bool {
        !self.put_away.is_empty()
    }

    /// The widgets kept in the settings (and a panel 0.2.3 left on the
    /// desktop, as a widget showing all of each lane), once.
    fn restore(&mut self, kept: &[WidgetAt]) {
        self.restored = true;
        self.list.extend(kept.iter().filter_map(Widget::restore));
        if !self.adopted.is_empty() {
            let adopted = std::mem::take(&mut self.adopted);
            self.list.extend(adopted.iter().filter_map(Widget::restore));
            self.save();
        }
        // Giving way to the settings window, if it is in front.
        for widget in &self.list {
            widget.window.yield_to(self.settings);
        }
    }

    /// Saves where every widget is and what it shows: by one thread, the
    /// newest of what was asked for since it last saved, so that an older
    /// list never lands after a newer one.
    fn save(&self) {
        static SAVER: std::sync::OnceLock<std::sync::mpsc::Sender<Vec<WidgetAt>>> = std::sync::OnceLock::new();
        let kept: Vec<WidgetAt> = self.list.iter().map(Widget::kept).collect();
        let saver = SAVER.get_or_init(|| {
            let (send, receive) = std::sync::mpsc::channel::<Vec<WidgetAt>>();
            std::thread::spawn(move || {
                while let Ok(mut kept) = receive.recv() {
                    while let Ok(newer) = receive.try_recv() {
                        kept = newer;
                    }
                    crate::app().change(|settings| settings.widgets = kept);
                }
            });
            send
        });
        let _ = saver.send(kept);
    }

    /// A press on a widget's window at `client` (physical px in it).
    pub fn press(&mut self, hwnd: HWND, client: POINT) {
        let Some(widget) = self.list.iter_mut().find(|w| w.window.hwnd == hwnd) else { return };
        widget.next_frame = Instant::now();
        widget.press(client);
    }

    /// The pointer over a widget's window at `client` (physical px in it):
    /// the shape of what a press there does.
    pub fn hover(&mut self, hwnd: HWND, client: POINT, lang: Lang) {
        let Some(widget) = self.list.iter_mut().find(|w| w.window.hwnd == hwnd) else { return };
        // Its buttons show as the pointer comes: drawn at once (moving over
        // it after, nothing that shows changes).
        if !widget.hover {
            widget.next_frame = Instant::now();
        }
        if widget.drag.is_some() {
            return;
        }
        let px = widget.contact.scale * widget.zoom;
        let margin = widget.margin_px();
        let (x, y) = ((client.x - margin) as f32 / px, (client.y - margin) as f32 / px);
        let (w, h) = widget.surface_size;
        // What the button under the pointer does, said under it.
        let over = widget.button_at(x, y);
        if over != widget.tip {
            widget.tip = over;
            // (Under the button itself, where that is on the screen: not wherever the pointer came onto it.)
            let about = over.and_then(|button| widget.buttons().into_iter().find(|(_, one)| *one == button)).map(|(centre, _)| {
                let on_screen = |dx: f32, dy: f32| ((widget.at.x as f32 + (centre.x + dx) * px).round() as i32, (widget.at.y as f32 + (centre.y + dy) * px).round() as i32);
                let ((left, top), (right, bottom)) = (on_screen(-BUTTON / 2.0, -BUTTON / 2.0), on_screen(BUTTON / 2.0, BUTTON / 2.0));
                RECT { left, top, right, bottom }
            });
            match (over, about) {
                (Some(button), Some(at)) => {
                    let lit = match button {
                        Button::Pin => widget.pinned,
                        Button::Through => widget.click_through,
                        Button::Game => game_shown(crate::app().controller.history.lock().unwrap().iter().last().map(std::slice::from_ref).unwrap_or_default()),
                        _ => false,
                    };
                    crate::ui::tip::show(button.says(lit, lang, false), at, crate::os::apps_dark());
                }
                _ => crate::ui::tip::hide(),
            }
        }
        let shape = if over.is_some() {
            windows::Win32::UI::WindowsAndMessaging::IDC_HAND
        } else {
            let (left, right, top, bottom) = edges_at(x, y, w, h);
            crate::panel::sizing(left, right, top, bottom)
        };
        widget.window.point(shape);
    }

    /// Let go of a press on a widget, or its capture lost.
    pub fn release(&mut self, hwnd: HWND, now: Instant) {
        let Some(index) = self.list.iter().position(|w| w.window.hwnd == hwnd) else { return };
        let widget = &mut self.list[index];
        widget.next_frame = now;
        match widget.release(now) {
            Released::Closed => self.put_away_at(index),
            Released::Changed => {}
            Released::Settings => {
                // (What its button said goes with the settings coming.)
                if self.list[index].tip.take().is_some() {
                    crate::ui::tip::hide();
                }
                crate::show_settings();
                return;
            }
            Released::Nothing => return,
        }
        self.save();
    }

    /// The menu of the widget at `hwnd` (a right click on it); what to do
    /// for the panel's side of it.
    pub fn menu(&mut self, hwnd: HWND, lang: Lang) -> Option<WidgetChoice> {
        let index = self.list.iter().position(|w| w.window.hwnd == hwnd)?;
        let choice = self.list[index].menu(lang)?;
        self.list[index].next_frame = Instant::now();
        match choice {
            WidgetChoice::Pin => self.list[index].pinned ^= true,
            WidgetChoice::ClickThrough => self.list[index].click_through ^= true,
            WidgetChoice::AboveOthers => {
                let widget = &mut self.list[index];
                widget.on_desktop ^= true;
                widget.window.set_on_desktop(widget.low());
            }
            WidgetChoice::Close => self.put_away_at(index),
            WidgetChoice::Settings => return Some(choice),
        }
        self.save();
        None
    }

    /// Follows the pointer, draws what is due, and says when next to.
    /// Each drawn at its next turn, not after its rest (what it shows has changed).
    pub fn draw_soon(&mut self) {
        let now = Instant::now();
        for widget in &mut self.list {
            widget.next_frame = now;
        }
    }

    pub fn tick(&mut self, now: Instant, look: &Look, kept: &[WidgetAt], history: &[Sample], seen: &Seen, controller: &crate::panel::Controller) -> Option<Duration> {
        if !self.restored {
            if gfx::current().is_err() {
                return None;
            }
            self.restore(kept);
        }
        if self.list.is_empty() {
            self.watching = None;
            SHOWN.store(0, std::sync::atomic::Ordering::Relaxed);
            return None;
        }
        if self.watching.is_none() {
            self.watching = Some(WindowWatch::start());
        }
        // Those kept on the desktop, just above it again wherever it went.
        if ORDER_CHANGED.swap(false, std::sync::atomic::Ordering::Relaxed) {
            for widget in &self.list {
                widget.window.sink();
            }
        }
        // Where they are, for what changes behind them (see `WindowWatch`);
        // one of them moved, sized, come or gone where it meets another
        // changes what is behind that one too, as another program's would.
        self.live = look.live && look.skin.sees_backdrop();
        let playing = game_shown(history);
        // Each shown, with its shadow and the room for its buttons round it.
        let shown: Vec<&Widget> = self.list.iter().filter(|w| !w.game_only || playing).collect();
        let areas: Vec<(isize, RECT)> = shown
            .iter()
            .map(|w| {
                let (r, m) = (w.rect(), w.margin_px());
                (w.window.hwnd.0 as isize, RECT { left: r.left - m, top: r.top - m, right: r.right + m, bottom: r.bottom + m })
            })
            .collect();
        // What each depends on: all it took round it, kept for it to be
        // moved over.
        let watched: Vec<(isize, RECT)> = shown
            .iter()
            .zip(&areas)
            .map(|(w, (hwnd, area))| {
                let kept = w.behind.as_ref().map_or(*area, |b| union(*area, b.capture.rect));
                (*hwnd, w.pending.as_ref().map_or(kept, |request| union(kept, request.area)))
            })
            .collect();
        let others = |hwnd: isize, r: RECT| watched.iter().any(|(w, a)| *w != hwnd && r.left < a.right && a.left < r.right && r.top < a.bottom && a.top < r.bottom);
        let moved = areas.iter().filter(|area| !self.areas.contains(area)).chain(self.areas.iter().filter(|area| !areas.contains(area)));
        let moved_by_another = moved.into_iter().any(|(hwnd, r)| others(*hwnd, *r));
        *WIDGET_AREAS.lock().unwrap() = watched.iter().map(|(_, r)| *r).collect();
        if moved_by_another {
            behind_changed();
        }
        self.areas = areas;
        SHOWN.store(self.list.len(), std::sync::atomic::Ordering::Relaxed);
        let mut due = REST_FRAME;
        let mut gone = Vec::new();
        for (index, widget) in self.list.iter_mut().enumerate() {
            // (Letting clicks through, but for the one on its button.)
            let through = widget.click_through && !widget.reached;
            widget.window.set_click_through(through);
            let hidden = widget.game_only && !playing;
            if hidden {
                widget.window.hide();
                // Nothing behind it looked at meanwhile: as it shows again, again.
                widget.backdrop = None;
                widget.pending = None;
                continue;
            }
            // Drawn when it is due: in hand, flung or turning, at once; else
            // after its rest, not each time the panel draws a frame.
            let due_now = widget.drag.is_some() || widget.fling.is_some() || now >= widget.next_frame;
            if !due_now {
                due = due.min(widget.next_frame.saturating_duration_since(now));
                continue;
            }
            if widget.drag.is_some() {
                widget.follow(now);
            }
            match widget.frame(now, look, history, seen, controller) {
                Frame::Gone => gone.push(index),
                Frame::Moving => {
                    widget.next_frame = now;
                    due = Duration::ZERO;
                }
                Frame::Rest | Frame::Waiting => {
                    let rest = if widget.lingered.is_some() { WATCHED } else { REST_FRAME };
                    // (Or sooner, a frame of what is behind it having come.)
                    let rest = widget.backdrop_wait(self.live).map_or(rest, |wait| rest.min(wait));
                    widget.next_frame = now + rest;
                    due = due.min(rest);
                }
            }
        }
        // Where they came to rest, kept.
        let rested = self.list.iter_mut().fold(false, |any, widget| std::mem::take(&mut widget.unsaved) | any);
        if rested && gone.is_empty() {
            self.save();
        }
        for index in gone.into_iter().rev() {
            self.put_away_at(index);
            self.save();
        }
        Some(due)
    }

    /// Puts the widget at `index` away: kept to be brought back, its window gone, and what one of its buttons said
    /// under the pointer gone with it (the pointer, no longer over it, would never take it away).
    fn put_away_at(&mut self, index: usize) {
        let widget = self.list.remove(index);
        if widget.tip.is_some() {
            crate::ui::tip::hide();
        }
        self.put_away.push(widget.kept());
        widget.window.destroy();
    }
}

/// What a widget's menu was used for.
pub enum WidgetChoice {
    Pin,
    ClickThrough,
    AboveOthers,
    Close,
    Settings,
}

enum Released {
    Nothing,
    Changed,
    Closed,
    Settings,
}

enum Frame {
    Rest,
    Moving,
    /// Not drawn: waiting on the desktop behind it (see `CAPTURED`).
    Waiting,
    Gone,
}

/// What is behind a widget arranged anew, its frame awaited: when, for
/// which screen (see `Widget::generation`), and whether of somewhere else
/// than the last.
struct Request {
    /// What it stands for (physical px).
    area: RECT,
    generation: u64,
    moved: bool,
}

/// Posted to a widget's window as a frame of what is behind it has come.
pub const CAPTURED: u32 = WM_APP + 12;

impl Widget {
    fn new(contact: Contact, at: POINT, choice: Choice) -> Option<Self> {
        let gfx = gfx::current().ok()?;
        let window = Window::new().ok()?;
        let surface = Surface::new(&gfx, window.hwnd).ok();
        Some(Widget {
            window,
            gfx,
            surface,
            layers: PanelLayers::default(),
            contact,
            at,
            choice,
            room: None,
            settle: None,
            keeps_to: (false, false),
            drag: None,
            fling: None,
            opacity: 1.0,
            stuck: None,
            along: None,
            before: None,
            before_size: None,
            margin: Theme::new(Skin::Paper, false).margin,
            anchor: None,
            pinned: false,
            click_through: false,
            torn_from: None,
            game_only: false,
            on_desktop: false,
            last: None,
            morph: None,
            drawn: Vec::new(),
            morph_layer: Layer::default(),
            hint: None,
            backdrop: None,
            arranged: None,
            retake: false,
            pending: None,
            replace: None,
            generation: 0,
            unsaved: false,
            failed: None,
            shown_once: false,
            frost_at: None,
            tone_due: false,
            frame_took: gfx::Took::default(),
            taken: None,
            hover: false,
            lingered: None,
            reached: false,
            tip: None,
            hits: Vec::new(),
            hits_from: (0.0, 0.0),
            behind: None,
            next_frame: Instant::now(),
            hint_size: None,
            surface_size: (0.0, 0.0),
            shown: (0.0, 0.0),
            zoom: 1.0,
            last_frame: Instant::now(),
        })
    }

    /// A widget as the settings keep it, on the screen nearest where it was.
    fn restore(kept: &WidgetAt) -> Option<Self> {
        let at = POINT { x: kept.at.0, y: kept.at.1 };
        let contact = monitor_at(at)?;
        let choice = Choice::from_kept(&kept.layout)?;
        let mut widget = Widget::new(contact, at, choice)?;
        widget.pinned = kept.pinned;
        widget.click_through = kept.click_through;
        widget.game_only = kept.game_only;
        widget.on_desktop = kept.on_desktop;
        widget.window.set_on_desktop(widget.low());
        widget.stuck = kept.stuck.map(Side::from_kept);
        widget.before = kept.before.as_ref().and_then(Choice::from_kept);
        Some(widget)
    }

    fn kept(&self) -> WidgetAt {
        WidgetAt {
            at: (self.at.x, self.at.y),
            layout: self.choice.kept(),
            pinned: self.pinned,
            click_through: self.click_through,
            game_only: self.game_only,
            on_desktop: self.on_desktop,
            stuck: self.stuck.map(Side::kept),
            before: self.before.map(Choice::kept),
        }
    }

    /// The desktop taken behind it, from `taken` on (`moved`: elsewhere
    /// than the last was).
    fn took(&mut self, capture: Option<Capture>, moved: bool, taken: Instant, px: f32) {
        let now = taken;
        // None could be taken (the lock screen): what it had stands as it
        // was, as old as it was, and is taken again once it has rested.
        let Some(capture) = capture else {
            self.failed = Some(Instant::now());
            self.retake |= self.behind.is_some();
            return;
        };
        self.failed = None;
        // One to be replaced, replaced by this.
        self.replace = None;
        self.frost_at = None;
        match (Some(capture), &mut self.behind) {
            // The same desktop as before, taken where the last was: the last
            // stands.
            (Some(capture), Some(behind)) if capture.digest == behind.capture.digest && capture.rect == behind.capture.rect && !moved => behind.taken = now,
            // Refreshed in place, the theme holds (as the panel's does); a
            // first or a new place's sets it.
            (Some(capture), behind) => {
                let tone = match behind {
                    Some(behind) if !moved => behind.tone,
                    _ => capture.luminance(self.rect(), px),
                };
                self.behind = Some(Behind { capture, bitmap: None, tone, taken: now });
            }
            (None, _) => {}
        }
    }

    /// On `contact`'s screen now: a capture asked for on another is of the
    /// screen as it was, and let go of.
    fn moved_to(&mut self, contact: Contact) {
        if contact != self.contact {
            self.contact = contact;
            self.generation += 1;
        }
    }

    /// How often what is behind is taken anew as it changes, with live
    /// refraction (without, it is taken only as it is arranged anew: see
    /// `collect`).
    fn backdrop_every(live: bool) -> Duration {
        if live { LIVE_INTERVAL } else { RETAKE_EVERY }
    }

    /// A frame of what is behind, read, put in place (if it is of its
    /// screen as it is); else one wanted, if one is due: the one awaited
    /// (arranged anew) at once, else as often as it is taken (see
    /// `backdrop_every`).
    fn collect(&mut self, live: bool, px: f32) {
        let Some(scene) = &self.backdrop else { return };
        let Some(capture) = scene.ready() else {
            // (Without live refraction, only the one awaited.)
            let again = live && self.behind.as_ref().is_some_and(|behind| behind.taken.elapsed() >= Self::backdrop_every(live));
            if self.pending.is_some() || self.behind.is_none() || again {
                scene.want();
            }
            return;
        };
        let now = Instant::now();
        // Without live refraction, what is behind is as it was when looked at, till it is looked at again (see
        // `frame`): the scene rests meanwhile.
        if !live {
            if let Some(scene) = &mut self.backdrop {
                scene.rest();
            }
        }
        match self.pending.take() {
            Some(request) if request.generation != self.generation => self.retake |= self.behind.is_some(),
            Some(request) => self.took(Some(capture), request.moved, now, px),
            None => self.took(Some(capture), false, now, px),
        }
    }

    /// With live refraction, how long till a frame of what is behind that
    /// has come is due to be taken (none: none has come; without, none is
    /// taken but the one awaited, which wakes it as it is read).
    fn backdrop_wait(&self, live: bool) -> Option<Duration> {
        self.backdrop.as_ref().filter(|scene| live && scene.fresh())?;
        Some(self.behind.as_ref().map_or(Duration::ZERO, |behind| Self::backdrop_every(live).saturating_sub(behind.taken.elapsed())))
    }

    /// What is taken of the desktop behind it: round it by half its width
    /// across and half its height up and down (room to be moved in, and for
    /// the blur past its edges), on its screen.
    fn backdrop_area(&self) -> RECT {
        let r = self.rect();
        let least = (64.0 * self.contact.scale) as i32;
        let (across, down) = (((r.right - r.left) / 2).max(least), ((r.bottom - r.top) / 2).max(least));
        clip(RECT { left: r.left - across, top: r.top - down, right: r.right + across, bottom: r.bottom + down }, self.contact.monitor)
    }

    /// Its surface on the screen (physical px).
    fn rect(&self) -> RECT {
        let px = self.contact.scale * self.zoom;
        let (w, h) = ((self.surface_size.0 * px).round() as i32, (self.surface_size.1 * px).round() as i32);
        RECT { left: self.at.x, top: self.at.y, right: self.at.x + w, bottom: self.at.y + h }
    }

    /// Its buttons' centres, from its surface's corner (DIPs, its own): in
    /// a row above its right end, close last.
    /// Whether its buttons are under it: where its screen has no room for them above it.
    fn buttons_under(&self) -> bool {
        let reach = ((BUTTON + 2.0 * BUTTON_INSET) * self.contact.scale * self.zoom) as i32;
        self.at.y - reach < self.contact.work.top
    }

    /// Where each of its buttons is, whether it shows or not: in a row
    /// ending at its right edge.
    fn buttons(&self) -> [(Point, Button); 5] {
        let (w, h) = self.surface_size;
        let r = BUTTON / 2.0;
        let y = if self.buttons_under() { h + BUTTON_INSET + r } else { -BUTTON_INSET - r };
        let count = buttons::ALL.len();
        std::array::from_fn(|place| (Point { x: w - r - (count - 1 - place) as f32 * (BUTTON + buttons::GAP), y }, buttons::ALL[place]))
    }

    /// The buttons that show now: all of them, the pointer over it; while
    /// it lets clicks through, the one that ends that, once the pointer has
    /// stayed over it a while.
    fn offered(&self) -> Vec<(Point, Button)> {
        let all = self.buttons();
        match (self.hover, self.click_through) {
            (false, _) => Vec::new(),
            (true, false) => all.to_vec(),
            (true, true) if self.lingered.is_some_and(|since| since.elapsed() >= buttons::LINGER) => all.into_iter().filter(|(_, button)| *button == Button::Through).collect(),
            (true, true) => Vec::new(),
        }
    }

    fn button_at(&self, x: f32, y: f32) -> Option<Button> {
        self.offered().into_iter().find(|(c, _)| (c.x - x).hypot(c.y - y) <= BUTTON / 2.0 + 2.0).map(|(_, b)| b)
    }

    fn press(&mut self, client: POINT) {
        let px = self.contact.scale * self.zoom;
        let margin = self.margin_px();
        let (x, y) = ((client.x - margin) as f32 / px, (client.y - margin) as f32 / px);
        let (w, h) = self.surface_size;
        if self.button_at(x, y).is_none() && (x < -EDGE || y < -EDGE || x > w + EDGE || y > h + EDGE) {
            note(format!("widget: press at ({x:.0}, {y:.0}) outside its {w:.0} x {h:.0}"));
            return;
        }
        let Some(from) = cursor_position() else { return };
        // Caught as it was flung or settling: kept where it was caught.
        self.unsaved |= self.settle.is_some() || self.fling.is_some();
        self.settle = None;
        self.fling = None;
        self.opacity = 1.0;
        self.anchor = None;
        // On the sound's track: it is slid along till let go (pinned or not: that is not moving it).
        let (lx, ly) = (x - self.hits_from.0, y - self.hits_from.1);
        if let Some((hx, _, wide, ..)) = self.hits.iter().find(|(hx, hy, w, h, hit)| *hit == view::Hit::Volume && lx >= *hx && lx < hx + w && ly >= *hy && ly < hy + h).copied() {
            self.drag = Some(Drag::Slide { from: hx + self.hits_from.0, wide });
            unsafe { SetCapture(self.window.hwnd) };
            self.slide();
            return;
        }
        let edges = edges_at(x, y, w, h);
        // Pinned, it is neither moved nor sized: only its buttons are pressed.
        if self.pinned && self.button_at(x, y).is_none() {
            note("widget: pressed while pinned");
            return;
        }
        if self.button_at(x, y).is_some() {
            // A press on a button: acted on as it is let go.
            self.drag = Some(Drag::Move { grab: POINT { x: from.x - self.at.x, y: from.y - self.at.y }, trail: Vec::new(), torn: false, moved: false });
        } else if edges.0 || edges.1 || edges.2 || edges.3 {
            let room = (self.surface_size.0 * self.zoom, self.surface_size.1 * self.zoom);
            self.drag = Some(Drag::Size { edges, from, room, corner: self.at });
            self.keeps_to = (edges.0, edges.2);
        } else {
            self.drag = Some(Drag::Move { grab: POINT { x: from.x - self.at.x, y: from.y - self.at.y }, trail: Vec::new(), torn: false, moved: false });
        }
        unsafe { SetCapture(self.window.hwnd) };
        // Above every window while it is in hand, kept on the desktop or not.
        self.window.lift(true);
        self.window.raise();
        // Over the others now where they meet: a widget that took the
        // desktop with them there takes it again.
        let (r, m) = (self.rect(), self.margin_px());
        own_window_changed(RECT { left: r.left - m, top: r.top - m, right: r.right + m, bottom: r.bottom + m });
        note(match &self.drag {
            Some(Drag::Size { edges, .. }) => format!("widget: sized by {edges:?} (left, right, top, bottom)"),
            Some(Drag::Move { .. }) => "widget: pressed to move".to_string(),
            Some(Drag::Slide { .. }) => "widget: the sound's track pressed".to_string(),
            None => "widget: pressed, nothing taken".to_string(),
        });
    }

    /// Follows the pointer while it is in hand.
    /// The sound as loud as where the pointer is along its track.
    fn slide(&self) {
        let (Some(Drag::Slide { from, wide }), Some(cursor)) = (&self.drag, cursor_position()) else { return };
        let x = (cursor.x - self.at.x) as f32 / (self.contact.scale * self.zoom);
        crate::playing::slide((((x - from) / wide).clamp(0.0, 1.0) * 100.0).round() as u8);
    }

    fn follow(&mut self, now: Instant) {
        if matches!(self.drag, Some(Drag::Slide { .. })) {
            self.slide();
            return;
        }
        let Some(cursor) = cursor_position() else { return };
        let scale = self.contact.scale;
        match &mut self.drag {
            Some(Drag::Move { grab, trail, moved, .. }) => {
                trail.push((now, cursor));
                trail.retain(|(at, _)| now.duration_since(*at) <= FLING_SPAN * 2);
                let to = POINT { x: cursor.x - grab.x, y: cursor.y - grab.y };
                if !*moved {
                    let (dx, dy) = ((to.x - self.at.x) as f32 / scale, (to.y - self.at.y) as f32 / scale);
                    if dx.hypot(dy) < MOVE_FROM {
                        return;
                    }
                    *moved = true;
                }
                // Pulled away from the edge it was stuck to: as it was before,
                // under the hand where it was taken.
                if let (Some(side), Some(before)) = (self.stuck, self.before) {
                    let work = self.contact.work;
                    let (w, h) = (self.surface_size.0 * self.zoom * scale, self.surface_size.1 * self.zoom * scale);
                    let away = match side {
                        Side::Left => to.x - work.left,
                        Side::Right => work.right - (to.x + w as i32),
                        Side::Top => to.y - work.top,
                        Side::Bottom => work.bottom - (to.y + h as i32),
                    } as f32
                        / scale;
                    if away > UNSTICK {
                        let (gx, gy) = (grab.x as f32 / w, grab.y as f32 / h);
                        self.stuck = None;
                        self.along = None;
                        self.keeps_to = (false, false);
                        self.before = None;
                        self.choice = before;
                        // The grab, at the same share of the size it had.
                        if let Some(size) = self.before_size.take() {
                            *grab = POINT { x: (gx * size.0 * scale) as i32, y: (gy * size.1 * scale) as i32 };
                        }
                    }
                }
                self.at = POINT { x: cursor.x - grab.x, y: cursor.y - grab.y };
                if let Some(contact) = monitor_at(cursor) {
                    self.moved_to(contact);
                }
                // Torn off and now well clear of the edge the panel was at
                // (off a panel at none, at once), or on another screen: in
                // hand as any widget is, to stick where it is brought to an
                // edge. (Once it has a size: how far it is from an edge is
                // not known before.)
                if matches!(self.drag, Some(Drag::Move { torn: true, .. })) && self.surface_size != (0.0, 0.0) {
                    let here = self.contact.monitor;
                    if self.torn_from.is_none_or(|(side, screen)| screen != here || side.is_none_or(|side| self.away_from(side) > UNSTICK)) {
                        self.torn_from = None;
                        if let Some(Drag::Move { torn, .. }) = &mut self.drag {
                            *torn = false;
                        }
                    }
                }
            }
            Some(Drag::Size { edges, from, room, corner }) => {
                let (dx, dy) = ((cursor.x - from.x) as f32 / scale, (cursor.y - from.y) as f32 / scale);
                let (l, r, t, b) = *edges;
                let width = (room.0 + if r { dx } else if l { -dx } else { 0.0 }).max(1.0);
                let height = (room.1 + if b { dy } else if t { -dy } else { 0.0 }).max(1.0);
                self.room = Some((width, height));
                self.stuck = None;
                self.along = None;
                // The edges not held stay where they were.
                let start = (room.0 * scale, room.1 * scale);
                let right = corner.x + start.0 as i32;
                let bottom = corner.y + start.1 as i32;
                self.at = POINT { x: if l { right } else { corner.x }, y: if t { bottom } else { corner.y } };
            }
            Some(Drag::Slide { .. }) | None => {}
        }
    }

    fn release(&mut self, now: Instant) -> Released {
        let _ = unsafe { ReleaseCapture() };
        // Back where it is kept, let go.
        self.window.lift(false);
        self.hide_hint();
        // (The capture's end after a release comes here too, with nothing.)
        let Some(drag) = self.drag.take() else { return Released::Nothing };
        match drag {
            // The sound's track let go: as loud as it was left, and looked at again.
            Drag::Slide { .. } => {
                if let Some(level) = crate::playing::now().and_then(|now| now.volume) {
                    crate::playing::act(crate::playing::Act::Volume(level));
                }
                Released::Nothing
            }
            Drag::Size { .. } => {
                // It settles from the surface it shows (the hand's room, given
                // as a band gives) onto what it shows, the edges not dragged
                // where they are.
                if self.room.take().is_some() {
                    self.settle = Some((now, self.surface_size));
                    let r = self.rect();
                    self.anchor = Some(POINT { x: r.right, y: r.bottom });
                }
                self.retone();
                Released::Changed
            }
            Drag::Move { trail, torn, moved, .. } => {
                if !moved {
                    // A press let go where it was: a button, if it was on one.
                    let px = self.contact.scale * self.zoom;
                    let Some(cursor) = cursor_position() else { return Released::Nothing };
                    let (x, y) = ((cursor.x - self.at.x) as f32 / px, (cursor.y - self.at.y) as f32 / px);
                    return match self.button_at(x, y) {
                        Some(Button::Pin) => {
                            self.pinned ^= true;
                            Released::Changed
                        }
                        Some(Button::Through) => {
                            self.click_through ^= true;
                            (self.lingered, self.reached) = (None, false);
                            Released::Changed
                        }
                        Some(Button::Close) => Released::Closed,
                        Some(Button::Settings) => Released::Settings,
                        Some(Button::Game) => {
                            // Showing a game (found or asked for): off; else on.
                            let history = crate::app().controller.history.lock().unwrap();
                            let shown = game_shown(history.iter().last().map(std::slice::from_ref).unwrap_or_default());
                            drop(history);
                            crate::source::set_game_mode(if shown { crate::presents::GameMode::Off } else { crate::presents::GameMode::On });
                            Released::Nothing
                        }
                        // No button: what of its lanes is there (what plays, the sound).
                        None => {
                            let (lx, ly) = (x - self.hits_from.0, y - self.hits_from.1);
                            match self.hits.iter().find(|(hx, hy, w, h, _)| lx >= *hx && lx < hx + w && ly >= *hy && ly < hy + h) {
                                Some((.., view::Hit::Media(key))) => crate::playing::act(crate::playing::Act::Key(*key)),
                                Some((.., view::Hit::Drive(place))) => crate::panel::open_drive(*place),
                                _ => {}
                            }
                            Released::Nothing
                        }
                    };
                }
                // A fling: fast enough as it was let go (a tear is put down).
                let scale = self.contact.scale;
                let recent: Vec<&(Instant, POINT)> = trail.iter().filter(|(at, _)| now.duration_since(*at) <= FLING_SPAN).collect();
                if let (false, Some(first), Some(last)) = (torn, recent.first(), recent.last()) {
                    let ms = last.0.duration_since(first.0).as_secs_f32() * 1000.0;
                    if ms > 10.0 {
                        let (vx, vy) = ((last.1.x - first.1.x) as f32 / scale / ms, (last.1.y - first.1.y) as f32 / scale / ms);
                        if vx.hypot(vy) > FLING {
                            note(format!("widget: flung at {:.1} DIP/ms", vx.hypot(vy)));
                            self.fling = Some((vx, vy));
                            return Released::Changed;
                        }
                    }
                }
                self.retone();
                // Torn off the panel and still at its edge, it is put down where it is let go.
                if !torn {
                    self.stick(now);
                }
                Released::Changed
            }
        }
    }

    /// Let go near an edge of its screen's work area: stuck to it, a rail
    /// on a side, a strip at the top or bottom; already stuck, flush with
    /// its edge again.
    fn stick(&mut self, now: Instant) {
        let near = self.nearest_side().filter(|(_, away)| *away < STICK).map(|(side, _)| side);
        let Some(side) = near.or(self.stuck) else { return };
        if self.stuck.is_none() {
            self.along = Some(self.middle_along(side));
            self.before = Some(self.choice);
            self.before_size = Some((self.surface_size.0 * self.zoom, self.surface_size.1 * self.zoom));
            let small = match side {
                Side::Left | Side::Right => Small::Rail,
                Side::Top | Side::Bottom => Small::Strip { rich: false },
            };
            self.choice = Choice { form: Form::Small(small), scale: 1.0, extra: (0.0, 0.0) };
            note(format!("widget: stuck to the {side:?} edge"));
            self.stuck = Some(side);
            self.settle = Some((now, self.surface_size));
        }
        self.keeps_to = (side == Side::Right, side == Side::Bottom);
    }

    /// The edge of its screen's work area it is nearest, and how near (DIPs).
    fn nearest_side(&self) -> Option<(Side, f32)> {
        [Side::Left, Side::Right, Side::Top, Side::Bottom].into_iter().map(|side| (side, self.away_from(side))).min_by(|a, b| a.1.total_cmp(&b.1))
    }

    /// How far it is from `side` of its screen's work area (DIPs).
    fn away_from(&self, side: Side) -> f32 {
        let (work, rect) = (self.contact.work, self.rect());
        let away = match side {
            Side::Left => rect.left - work.left,
            Side::Right => work.right - rect.right,
            Side::Top => rect.top - work.top,
            Side::Bottom => work.bottom - rect.bottom,
        };
        away as f32 / self.contact.scale
    }

    /// Where its middle is along `side` now (physical px).
    fn middle_along(&self, side: Side) -> i32 {
        let rect = self.rect();
        match side {
            Side::Left | Side::Right => (rect.top + rect.bottom) / 2,
            Side::Top | Side::Bottom => (rect.left + rect.right) / 2,
        }
    }

    /// Where a surface `size` large (physical px) stuck to `side`, its
    /// middle `along` it, has its corner, all on its screen; along the side
    /// where it is now for none.
    fn stuck_at(&self, side: Side, along: Option<i32>, size: (i32, i32)) -> POINT {
        let (work, at) = (self.contact.work, self.at);
        let (x, y) = match side {
            Side::Left => (work.left, at.y),
            Side::Right => (work.right - size.0, at.y),
            Side::Top => (at.x, work.top),
            Side::Bottom => (at.x, work.bottom - size.1),
        };
        let x = match (side, along) {
            (Side::Top | Side::Bottom, Some(middle)) => middle - size.0 / 2,
            _ => x,
        };
        let y = match (side, along) {
            (Side::Left | Side::Right, Some(middle)) => middle - size.1 / 2,
            _ => y,
        };
        let x = x.clamp(work.left, (work.right - size.0).max(work.left));
        let y = y.clamp(work.top, (work.bottom - size.1).max(work.top));
        POINT { x, y }
    }

    /// Shows where it would stick, as the skin draws it: `rect` (physical
    /// px), firmer once letting go would stick it there, `strength` as
    /// strong.
    fn show_hint(&mut self, rect: RECT, firm: bool, strength: f32, theme: &Theme) {
        if self.hint.is_none() {
            let Ok(mut window) = Window::new() else { return };
            window.set_click_through(true);
            // Never in what a widget sees behind it: it is where the widget
            // would go, not part of the desktop.
            crate::ui::scene::leave_out(window.hwnd, true);
            let surface = Surface::new(&self.gfx, window.hwnd).ok();
            self.hint = Some(Hint { window, surface, drawn: None });
        }
        let Some(hint) = self.hint.as_mut() else { return };
        let this = (rect, firm, strength.to_bits());
        if hint.drawn == Some(this) {
            return;
        }
        hint.drawn = Some(this);
        // Above the widget, which may be large enough to cover it: see-through
        // and letting the pointer through, it hides nothing that matters.
        hint.window.place(rect);
        hint.window.show_in_place();
        let Some(surface) = hint.surface.as_mut() else { return };
        let scale = self.contact.scale;
        let pixels = ((rect.right - rect.left).max(1) as u32, (rect.bottom - rect.top).max(1) as u32);
        let (w, h) = (pixels.0 as f32 / scale, pixels.1 as f32 / scale);
        let _ = surface.draw(&self.gfx, pixels, scale, |frame| {
            frame.origin(0.0, 0.0);
            paint_hint(frame, theme, (w, h), firm, strength);
        });
    }

    fn hide_hint(&mut self) {
        if let Some(hint) = self.hint.take() {
            crate::ui::scene::leave_out(hint.window.hwnd, false);
            hint.window.destroy();
        }
        self.hint_size = None;
    }

    /// Let go somewhere else on its screen: the desktop taken behind it
    /// serves still (it was taken without the widget); only how light it
    /// is where the widget now is, which the theme follows, is read again.
    fn retone(&mut self) {
        // Measured once it is where it comes to rest, on what is taken
        // there (see `frame`).
        self.tone_due = true;
        // What is behind it now is looked at once, the desktop having
        // changed meanwhile, or not; not when it was a moment ago (as it was
        // torn off).
        let fresh = self.behind.as_ref().is_some_and(|behind| behind.taken.elapsed() < RETAKE_AFTER);
        self.retake |= !fresh;
    }

    /// Whether it is kept under every other window.
    fn low(&self) -> bool {
        self.on_desktop && !self.game_only
    }

    fn margin_px(&self) -> i32 {
        (self.margin * self.contact.scale * self.zoom).ceil() as i32
    }

    /// Lays it out, places its window and draws it.
    fn frame(&mut self, now: Instant, look: &Look, history: &[Sample], seen: &Seen, controller: &crate::panel::Controller) -> Frame {
        let dt = now.duration_since(self.last_frame).as_secs_f32() * 1000.0;
        self.last_frame = now;
        // Each part of a frame timed, and a slow one noted.
        let started = Instant::now();
        let mut moving = self.drag.is_some() || self.settle.is_some();
        // Settling this frame (its last too): its far edges held.
        let settling = self.settle.is_some();
        // Flung: on along its way, slowing; gone once it has left the
        // screens, or at rest where it came to a stop.
        if let Some((vx, vy)) = self.fling {
            let scale = self.contact.scale;
            // As far as it goes slowing all the while (however long since the
            // last frame: a menu up meanwhile): v (1 - kᵗ) / -ln k.
            let keep = FRICTION.powf(dt);
            let travel = (1.0 - keep) / -FRICTION.ln();
            self.at.x += (vx * travel * scale) as i32;
            self.at.y += (vy * travel * scale) as i32;
            // Onto another screen on its way: on that one now.
            let r = self.rect();
            if let Some(contact) = monitor_at(POINT { x: (r.left + r.right) / 2, y: (r.top + r.bottom) / 2 }) {
                self.moved_to(contact);
            }
            let (vx, vy) = (vx * keep, vy * keep);
            self.fling = Some((vx, vy));
            let screens = unsafe {
                let (x, y) = (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN));
                RECT { left: x, top: y, right: x + GetSystemMetrics(SM_CXVIRTUALSCREEN), bottom: y + GetSystemMetrics(SM_CYVIRTUALSCREEN) }
            };
            let r = self.rect();
            let (w, h) = ((r.right - r.left).max(1) as f32, (r.bottom - r.top).max(1) as f32);
            let outside = (screens.left - r.left).max(r.right - screens.right).max(0) as f32 / w + (screens.top - r.top).max(r.bottom - screens.bottom).max(0) as f32 / h;
            self.opacity = (1.0 - outside * 1.4).clamp(0.0, 1.0);
            if self.opacity <= 0.0 {
                return Frame::Gone;
            }
            // Slowed to a stop: put away if it is mostly gone, else at rest,
            // all of it back on its screen.
            if vx.hypot(vy) < 0.15 {
                if self.opacity <= 0.5 {
                    return Frame::Gone;
                }
                self.fling = None;
                self.unsaved = true;
                self.opacity = 1.0;
                if let Some(contact) = monitor_at(POINT { x: (r.left + r.right) / 2, y: (r.top + r.bottom) / 2 }) {
                    self.moved_to(contact);
                }
                // Come to rest: light or dark as the desktop is there.
                self.retone();
            }
            moving = true;
        }
        let dark = theme::is_dark(look.prefs.theme, self.behind.as_ref().map(|b| b.tone.0).filter(|_| look.skin.sees_backdrop()));
        let theme = Theme::new(look.skin, dark);
        // Room round it for its shadow, and above it for its buttons.
        self.margin = theme.margin.max(BUTTON + 2.0 * BUTTON_INSET);
        let scene = crate::panel::scene_for(controller, look.prefs, &theme, look.lang, history, seen);
        // Laid out as it is wanted: what fills the room it is dragged to, else
        // what it shows.
        let work = self.contact.work;
        let work = ((work.right - work.left) as f32 / self.contact.scale, (work.bottom - work.top) as f32 / self.contact.scale);
        // Moved or flung, it shows what it showed (laid out again once at
        // rest, as the readings step); else laid out for what it shows, or
        // for the room it is sized to.
        let only_moved = matches!(self.drag, Some(Drag::Move { .. })) || self.fling.is_some();
        let shown = match self.last.clone().filter(|last| only_moved && last.form == self.choice.form) {
            Some(last) => last,
            None => {
                let heights = view::heights(&scene);
                let lanes: Vec<(&str, arrange::Heights)> = heights.iter().map(|(id, h)| (id.as_str(), *h)).collect();
                let readings = forms::count(&scene);
                let want = match (self.room, &self.drag) {
                    (Some(room), Some(Drag::Size { .. })) => {
                        Want::Room(room, Some(self.taken.filter(|taken| taken.form == self.choice.form).unwrap_or(arrange::Shown { form: self.choice.form, taken: room })))
                    }
                    _ => Want::Kept(self.choice),
                };
                let opening = arrange::Opening::new(&theme, Edge::Right, &lanes, readings, work, None, 1.0, want, seen.clone());
                let Some(choice) = opening.choice else { return Frame::Rest };
                if let Some(room) = self.room.filter(|_| matches!(self.drag, Some(Drag::Size { .. }))) {
                    self.taken = Some(arrange::Shown::after(self.taken, choice.form, room));
                }
                self.choice = choice;
                Shown { form: choice.form, zoom: opening.zoom, size: opening.size, layout: opening.layout.clone(), small: opening.small(), detail: opening.detail() }
            }
        };
        self.zoom = shown.zoom;
        self.shown = shown.size;
        // Another layout: what showed turns into it, from where it is now if
        // it was still turning.
        if let Some(last) = self.last.take().filter(|last| last.form != shown.form) {
            note(format!("widget: {} -> {}", last.form.key(), shown.form.key()));
            let before = if self.morph.is_some() { Before::Drawn(std::mem::take(&mut self.drawn)) } else { Before::Shown(last) };
            self.morph = Some((now, before));
        }
        self.last = Some(shown);
        if self.morph.as_ref().is_some_and(|(since, _)| now.duration_since(*since) >= MORPH) {
            self.morph = None;
            self.drawn.clear();
            self.morph_layer.release();
        }
        moving |= self.morph.is_some();
        // Its surface: the room the hand gives it, past what it shows only
        // part of the way; settling from that once let go; else what it shows.
        let give = |have: f32, fill: f32| if have <= fill { fill } else { fill + (have - fill) * GIVE };
        self.surface_size = match (self.room, self.settle) {
            (Some(room), _) => (give(room.0 / self.zoom, self.shown.0), give(room.1 / self.zoom, self.shown.1)),
            (None, Some((since, from))) => {
                let t = (now.saturating_duration_since(since).as_secs_f32() / SETTLE.as_secs_f32()).min(1.0);
                let eased = morph::ease(t);
                if t >= 1.0 {
                    self.settle = None;
                    self.unsaved = true;
                }
                (from.0 + (self.shown.0 - from.0) * eased, from.1 + (self.shown.1 - from.1) * eased)
            }
            _ => self.shown,
        };
        // Sized by its left or top edge, it grows and shrinks from the other.
        let px = self.contact.scale * self.zoom;
        if let Some(Drag::Size { edges: (l, _, t, _), from: _, room, corner }) = &self.drag {
            let start = ((room.0 * self.contact.scale) as i32, (room.1 * self.contact.scale) as i32);
            let size = ((self.surface_size.0 * px) as i32, (self.surface_size.1 * px) as i32);
            self.at = POINT { x: if *l { corner.x + start.0 - size.0 } else { corner.x }, y: if *t { corner.y + start.1 - size.1 } else { corner.y } };
        }
        // Settling after it was sized by its left or top edge: the other
        // edges held where they were let go.
        let size = ((self.surface_size.0 * px) as i32, (self.surface_size.1 * px) as i32);
        if let (None, true, Some(anchor), None) = (&self.drag, settling, self.anchor, self.stuck) {
            if self.keeps_to.0 {
                self.at.x = anchor.x - size.0;
            }
            if self.keeps_to.1 {
                self.at.y = anchor.y - size.1;
            }
        }
        // Stuck to an edge: flush with it, at its stop along it. Else, at
        // rest, all of it on its screen.
        if let (Some(side), None) = (self.stuck, &self.drag) {
            self.at = self.stuck_at(side, self.along, size);
        } else if self.drag.is_none() && self.fling.is_none() {
            let work = self.contact.work;
            self.at.x = self.at.x.clamp(work.left, (work.right - size.0).max(work.left));
            self.at.y = self.at.y.clamp(work.top, (work.bottom - size.1).max(work.top));
        }
        // The pointer over it, or (its buttons showing) over them.
        let reach = if self.hover { ((BUTTON + 2.0 * BUTTON_INSET) * px) as i32 } else { 0 };
        let (above, under) = if self.buttons_under() { (0, reach) } else { (reach, 0) };
        let cursor = cursor_position();
        let hovered = cursor.is_some_and(|c| {
            let r = self.rect();
            (r.left..r.right).contains(&c.x) && (r.top - above..r.bottom + under).contains(&c.y)
        });
        // Its buttons come or go: a widget that took the desktop with them
        // there takes it again.
        if hovered != self.hover {
            let (r, m) = (self.rect(), self.margin_px());
            own_window_changed(RECT { left: r.left - m, top: r.top - m, right: r.right + m, bottom: r.bottom + m });
        }
        self.hover = hovered;
        // Letting clicks through, it hears nothing of the pointer, which is
        // looked for here: over it a while, the button that ends it shows,
        // and with the pointer on that the window takes the click.
        self.lingered = if hovered && self.click_through { self.lingered.or(Some(now)) } else { None };
        self.reached = self.click_through && cursor.is_some_and(|c| self.button_at((c.x - self.at.x) as f32 / px, (c.y - self.at.y) as f32 / px) == Some(Button::Through));
        if !hovered && self.tip.take().is_some() {
            crate::ui::tip::hide();
        }
        // Moved near an edge, not stuck: where it would stick, as it would
        // be there, nearer and firmer the nearer it is.
        let near = match (&self.drag, self.stuck) {
            // Torn off the panel and still at its edge, it is put down where it is let go.
            (Some(Drag::Move { moved: true, torn: false, .. }), None) => self.nearest_side().filter(|(_, away)| *away < HINT_SEEN),
            _ => None,
        };
        match near {
            Some((side, away)) => {
                let small = match side {
                    Side::Left | Side::Right => Small::Rail,
                    Side::Top | Side::Bottom => Small::Strip { rich: false },
                };
                // Laid out once for the side it nears.
                let size = match self.hint_size.filter(|(near, _)| *near == side) {
                    Some((_, size)) => size,
                    None => {
                        let heights = view::heights(&scene);
                        let lanes: Vec<(&str, arrange::Heights)> = heights.iter().map(|(id, h)| (id.as_str(), *h)).collect();
                        let stuck = Want::Kept(Choice { form: Form::Small(small), scale: 1.0, extra: (0.0, 0.0) });
                        let there = arrange::Opening::new(&theme, Edge::Right, &lanes, forms::count(&scene), work, None, 1.0, stuck, seen.clone());
                        let scale = self.contact.scale;
                        let size = ((there.size.0 * there.zoom * scale) as i32, (there.size.1 * there.zoom * scale) as i32);
                        self.hint_size = Some((side, size));
                        size
                    }
                };
                let POINT { x, y } = self.stuck_at(side, Some(self.middle_along(side)), size);
                let firm = away < STICK;
                // In steps: drawn again as it changes visibly, not each frame.
                let strength = if firm { 1.0 } else { ((0.25 + 0.5 * (1.0 - (away - STICK) / (HINT_SEEN - STICK))) * 20.0).round() / 20.0 };
                self.show_hint(RECT { left: x, top: y, right: x + size.0, bottom: y + size.1 }, firm, strength, &theme);
            }
            None => self.hide_hint(),
        }
        let laid_out = started.elapsed();
        // The desktop behind, for the skins that see it, without the widget:
        // what is behind it put together where no one sees it (see `scene`),
        // before it first shows (the theme follows it), and arranged anew as
        // it moves past it, goes onto another screen, or the windows there
        // change; taken as it changes (see `collect`).
        let sees = look.skin.sees_backdrop();
        let live = look.live && sees;
        if !sees {
            self.backdrop = None;
            self.pending = None;
        }
        // A frame of it, come.
        self.collect(live, px);
        // Moved past what was taken round it (that part of it on its screen:
        // beyond, the capture is mirrored in its edge), or onto another screen;
        // in hand, near the edge of it already, so that the next is there by
        // the time it gets past (taken on another thread, the widget kept out
        // of it, nothing of it shows).
        let on_screen = clip(self.rect(), self.contact.monitor);
        let elsewhere = self.behind.as_ref().is_some_and(|behind| {
            let taken = behind.capture.rect;
            let ahead = if self.drag.is_some() { (on_screen.right - on_screen.left).max(on_screen.bottom - on_screen.top) / 6 } else { 0 };
            let past = on_screen.left - ahead < taken.left || on_screen.top - ahead < taken.top || on_screen.right + ahead > taken.right || on_screen.bottom + ahead > taken.bottom;
            let other_screen = clip(taken, self.contact.monitor) != taken;
            other_screen || past
        });
        let retake = self.retake && self.drag.is_none();
        // Other windows changed since they were looked at, and have settled;
        // not while it is in hand.
        let moved_behind = self.drag.is_none()
            && self.fling.is_none()
            && windows_changed().zip(self.arranged).is_some_and(|(changed, arranged)| changed > arranged && now.saturating_duration_since(changed) >= SETTLED);
        let due = self.behind.is_none() || self.replace.is_some() || elsewhere || retake || moved_behind;
        // None could be taken last time (the lock screen): not again at once.
        let rested = self.failed.is_none_or(|at| now.saturating_duration_since(at) >= RETAKE_EVERY);
        if sees && due && rested && self.pending.is_none() && !self.window.yielding() {
            // Why, for the journal (not each live refresh).
            let why = [(self.behind.is_none(), "first"), (self.replace.is_some(), "replacing what it had"), (elsewhere, "moved past what was taken"), (retake, "let go"), (moved_behind, "windows behind changed")]
                .iter()
                .filter(|(is, _)| *is)
                .map(|(_, why)| *why)
                .collect::<Vec<_>>()
                .join(", ");
            if !why.is_empty() {
                note_sparingly("retake", || format!("widget: the desktop taken again ({why})"));
            }
            self.retake = false;
            let area = self.backdrop_area();
            // Nothing behind it yet (a first, or the drawing device changed):
            // put together anew, which brings a first frame.
            if self.behind.is_none() || self.replace.is_some() {
                self.backdrop = None;
            }
            if self.backdrop.is_none() {
                let hwnd = self.window.hwnd.0 as isize;
                let wake = std::sync::Arc::new(move || {
                    let _ = unsafe { PostMessageW(Some(HWND(hwnd as *mut _)), CAPTURED, WPARAM(0), LPARAM(0)) };
                });
                self.backdrop = crate::ui::scene::Scene::new(area, wake, &self.gfx);
            }
            self.arranged = Some(now);
            // In hand, light or dark holds (measured again as it is let go:
            // see `retone`).
            let moved = (elsewhere && self.drag.is_none()) || self.replace == Some(true);
            match &mut self.backdrop {
                // (It could not be put together: as when the screen cannot be taken, tried again in a while.)
                None => self.took(None, moved, now, px),
                // A frame of it as it is now awaited; nothing changed, what it has stands.
                Some(scene) => {
                    if scene.arrange(area, self.window.hwnd) || self.behind.is_none() {
                        self.pending = Some(Request { area, generation: self.generation, moved });
                    } else if let Some(behind) = &mut self.behind {
                        behind.taken = now;
                        self.replace = None;
                    }
                }
            }
            // Watched from now (see `Widgets::tick`): what changes there is
            // seen to.
            WIDGET_AREAS.lock().unwrap().push(area);
        }
        // Let go or come to rest: light or dark as the desktop is where it
        // is now, measured on what was taken there (none under way).
        if self.tone_due && self.drag.is_none() && self.fling.is_none() && self.settle.is_none() && self.pending.is_none() {
            self.tone_due = false;
            let rect = self.rect();
            if let Some(behind) = &mut self.behind {
                behind.tone = behind.capture.luminance(rect, px);
            }
        }
        // Nothing yet behind a skin that sees it: drawn once it is there
        // (as it shows for the first time, it shows on it).
        if sees && self.behind.is_none() {
            return Frame::Waiting;
        }
        // Drawn light or dark as the desktop behind it is, now that it has
        // been taken: a first frame drawn before that would show the other.
        let dark = theme::is_dark(look.prefs.theme, self.behind.as_ref().map(|b| b.tone.0).filter(|_| sees));
        let theme = if dark == theme.dark { theme } else { Theme::new(look.skin, dark) };
        let scene = crate::panel::scene_for(controller, look.prefs, &theme, look.lang, history, seen);
        let taken = started.elapsed();
        self.draw(&theme, &scene, now);
        let whole = started.elapsed();
        if whole > SLOW_FRAME {
            let ms = |d: Duration| d.as_secs_f32() * 1000.0;
            let form = self.choice.form.key();
            let took = self.frame_took;
            note_sparingly("slow", || {
                format!("widget: a frame took {:.1} ms: laid out {:.1}, the desktop taken {:.1}, drawn {:.1} (its surface given in {:.1}, painted {:.1}, handed to the compositor {:.1}; {form})", ms(whole), ms(laid_out), ms(taken - laid_out), ms(whole - taken), ms(took.given), ms(took.painted), ms(took.handed))
            });
        }
        // (The lyrics gliding to the next line are drawn every frame too.)
        if moving || crate::playing::gliding() { Frame::Moving } else { Frame::Rest }
    }

    fn draw(&mut self, theme: &Theme, scene: &Scene, now: Instant) {
        let Some(shown) = self.last.clone() else { return };
        let Ok(current) = gfx::current() else { return };
        if !Rc::ptr_eq(&current, &self.gfx) || self.surface.is_none() {
            self.surface = None;
            self.surface = Surface::new(&current, self.window.hwnd).ok();
            if self.surface.is_none() {
                gfx::lost();
            }
            self.gfx = current;
            self.layers.release();
            self.morph_layer.release();
            // Its bitmap went with the device; its pixels are let go of:
            // the desktop is taken again (what is behind made on the new one).
            if self.behind.is_some() {
                note("widget: the drawing device changed; the desktop taken again");
            }
            self.behind = None;
            self.backdrop = None;
            self.pending = None;
        }
        let px = self.contact.scale * self.zoom;
        let margin = self.margin;
        let size = self.surface_size;
        let window = {
            let m = (margin * px).ceil() as i32;
            let r = self.rect();
            RECT { left: r.left - m, top: r.top - m, right: r.right + m, bottom: r.bottom + m }
        };
        // What it shows keeps to the sides not dragged; so does what it
        // showed, turning into it, at its own zoom.
        let keeps_to = self.keeps_to;
        let placed = |shown: &Shown| {
            let k = shown.zoom / self.zoom;
            (k, (if keeps_to.0 { size.0 - shown.size.0 * k } else { 0.0 }, if keeps_to.1 { size.1 - shown.size.1 * k } else { 0.0 }))
        };
        let lanes = view::lanes_at(scene, |_| shown.detail);
        let turning = self.morph.as_ref().map(|(since, _)| morph::ease(now.duration_since(*since).as_secs_f32() / MORPH.as_secs_f32()));
        let old = match &self.morph {
            Some((_, Before::Shown(old))) => Some((view::lanes_at(scene, |_| old.detail), placed(old), old.clone())),
            _ => None,
        };
        let halo = (theme.skin == Skin::Glass).then_some(theme.legibility);
        let (opacity, pinned, through, gfx) = (self.opacity, self.pinned, self.click_through, self.gfx.clone());
        let rough = matches!(self.drag, Some(Drag::Move { moved: true, .. })) || self.fling.is_some();
        let contact_scale = self.contact.scale;
        let offered = self.offered();
        // Frosted as the desktop where it is now asks (its theme holds, as
        // the panel's does): measured again as it moves a step.
        let here = self.rect();
        let step = (16.0 * contact_scale) as i32;
        let frost = match (&self.behind, self.frost_at) {
            (None, _) => 0.0,
            (Some(_), Some((at, frost))) if (at.left - here.left).abs() < step && (at.top - here.top).abs() < step && (at.right - here.right).abs() < step && (at.bottom - here.bottom).abs() < step => frost,
            (Some(behind), _) => {
                let (mean, spread) = behind.capture.luminance(here, contact_scale);
                let frost = (skins::frost(mean, spread, theme.dark) * 20.0).round() / 20.0;
                self.frost_at = Some((here, frost));
                frost
            }
        };
        let behind = &mut self.behind;
        let layers = &mut self.layers;
        let (before, drawn, morph_layer) = (self.morph.as_mut().map(|(_, before)| before), &mut self.drawn, &mut self.morph_layer);
        let Some(surface) = self.surface.as_mut() else { return };
        let pixels = ((window.right - window.left) as u32, (window.bottom - window.top) as u32);
        // Every layer of it drawn (see below).
        let mut whole = true;
        let mut pressed: Option<(Vec<view::HitBox>, (f32, f32))> = None;
        let drawn = surface.draw(&gfx, pixels, px, |frame| {
            frame.origin(0.0, 0.0);
            if opacity < 1.0 {
                let everything = D2D_RECT_F { left: -f32::MAX, top: -f32::MAX, right: f32::MAX, bottom: f32::MAX };
                let layer = D2D1_LAYER_PARAMETERS1 { contentBounds: everything, opacity, ..Default::default() };
                unsafe { frame.dc.PushLayer(&layer, None) };
            }
            let backdrop = behind.as_mut().and_then(|behind| {
                // Made once, at its screen's scale; its pixels then let go of
                // (a screen's worth of memory).
                if behind.bitmap.is_none() {
                    if let Ok(bitmap) = behind.capture.bitmap(&frame.dc, contact_scale) {
                        behind.bitmap = Some((bitmap, contact_scale));
                        behind.capture.forget_pixels();
                    }
                }
                let from = behind.capture.rect;
                let at = Vector2 { X: (window.left - from.left) as f32 / px + margin, Y: (window.top - from.top) as f32 / px + margin };
                Some((&behind.bitmap.as_ref()?.0, at, behind.capture.digest, px))
            });
            let at = (if keeps_to.0 { size.0 - shown.size.0 } else { 0.0 }, if keeps_to.1 { size.1 - shown.size.1 } else { 0.0 });
            let picture = render::Picture { scene, lanes: &lanes, layout: &shown.layout, small: shown.small.map(|small| (small, shown.size)), size, at, edge: None, backdrop, frost, rough };
            let local = Matrix3x2::translation(margin, margin);
            match (turning, before) {
                (Some(t), Some(before)) => {
                    // Turning: the skin's ground as it is now, and over it each
                    // reading between where it was and where it goes.
                    if let Err(error) = layers.draw_ground(frame, &picture, local, px) {
                        note(format!("widget: its ground not drawn: {error}"));
                        whole = false;
                    }
                    if let Some((old_lanes, (k, old_at), old)) = &old {
                        let small = old.small.map(|small| (small, old.size));
                        *before = Before::Drawn(render::record(frame, scene, old_lanes, &old.layout, small, *k, *old_at));
                    }
                    let now_drawn = render::record(frame, scene, &lanes, &shown.layout, picture.small, 1.0, at);
                    let Before::Drawn(from) = before else { return };
                    let parts = morph::between(from, &now_drawn, t);
                    // Within its frame: what is still on its way from a larger
                    // layout does not spill past it.
                    let area = (0.0, 0.0, size.0, size.1);
                    let turned = morph_layer.draw(frame, t.to_bits() as u64 + 1, local, area, px, halo, |frame| {
                        morph::draw(frame, &parts);
                        Ok(())
                    });
                    if let Err(error) = turned {
                        note(format!("widget: its turn between layouts not drawn: {error}"));
                        whole = false;
                    }
                    *drawn = parts;
                }
                _ => {
                    match layers.draw(frame, &picture, local, px) {
                        // (Its readings drawn again: where each thing to press now is.)
                        Ok(drawn) => pressed = drawn.hits.map(|hits| (hits, at)).or(pressed.take()),
                        Err(error) => {
                            note(format!("widget: not drawn: {error}"));
                            whole = false;
                        }
                    }
                }
            }
            frame.place(Matrix3x2::translation(margin, margin));
            // The pointer over it: its buttons (letting clicks through, the one that ends that).
            let front_is_game = game_shown(scene.history);
            for (centre, button) in offered {
                // Lit while on: pinned, letting clicks through, the window in front a game.
                let lit = (button == Button::Pin && pinned) || (button == Button::Through && through) || (button == Button::Game && front_is_game);
                buttons::paint(frame, centre, button, lit, theme.dark, theme.text3.alpha(0.5));
            }
            frame.origin(0.0, 0.0);
            if opacity < 1.0 {
                unsafe { frame.dc.PopLayer() };
            }
        });
        self.frame_took = surface.took;
        if let Some((hits, from)) = pressed {
            (self.hits, self.hits_from) = (hits, from);
        }
        if let Err(error) = &drawn {
            note(format!("widget: its surface not drawn: {error}"));
            gfx::lost();
            self.surface = None;
        }
        gfx.sweep();
        // Placed and shown once drawn: never the last picture in the new
        // place, nor an empty window, on the screen (not shown at all till
        // a frame was drawn whole).
        let whole = drawn.is_ok() && whole;
        if !self.shown_once && !whole {
            return;
        }
        self.window.place_keeping(window);
        self.window.show_in_place();
        if self.tip.is_some() {
            crate::ui::tip::above();
        }
        self.shown_once |= whole;
    }

    fn menu(&self, lang: Lang) -> Option<WidgetChoice> {
        // As the apps are, light or dark.
        match crate::ui::menu::show(&menu_items(lang, self.pinned, self.click_through, !self.on_desktop), crate::os::apps_dark())? {
            0 => Some(WidgetChoice::Pin),
            1 => Some(WidgetChoice::ClickThrough),
            2 => Some(WidgetChoice::AboveOthers),
            3 => Some(WidgetChoice::Close),
            _ => Some(WidgetChoice::Settings),
        }
    }
}

/// Where a widget let go would stick, `size` DIPs from the canvas's origin,
/// as `theme`'s skin draws it: firmer once letting go would stick it there,
/// `strength` as strong.
pub(crate) fn paint_hint(canvas: &dyn Canvas, theme: &Theme, (w, h): (f32, f32), firm: bool, strength: f32) {
    let (fill, line, width, dashed) = match theme.skin {
        // A pencilled outline on the paper's own colour.
        Skin::Paper => (theme.paper.alpha(if firm { 0.75 } else { 0.45 }), if firm { theme.text2 } else { theme.text3 }, 1.5, !firm),
        // As Windows' own snap preview: the accent's wash under a hairline.
        Skin::Fluent => {
            let accent = theme.ink("cpu").trace;
            (accent.alpha(if firm { 0.26 } else { 0.14 }), accent.alpha(if firm { 1.0 } else { 0.45 }), 1.0, false)
        }
        // A pane of glass, its rim catching the light.
        Skin::Glass => {
            let rim = if theme.dark { Color::hex(0xFFFFFF, 0.2) } else { Color::hex(0xFFFFFF, 0.7) };
            (theme.glass.alpha(if firm { 0.6 } else { 0.32 }), rim, 1.0, false)
        }
    };
    let (fill, line) = (fill.alpha(strength), line.alpha(strength));
    let inset = width / 2.0;
    let radius = theme.radius;
    canvas.fill_rounded(fill, inset, inset, w - width, h - width, radius);
    if dashed {
        canvas.stroke_dashed(&rounded(inset, inset, w - width, h - width, radius), line, width);
    } else {
        canvas.stroke_rounded(Fill::Solid(line), inset, inset, w - width, h - width, radius, width);
    }
}

/// A widget's menu: pinned or not, letting clicks through or not.
pub(crate) fn menu_items(lang: Lang, pinned: bool, through: bool, above: bool) -> [crate::ui::menu::Item<'static>; 5] {
    use crate::ui::menu::Item;
    let item = |zh, en, icon, checked, rule_before| Item { label: lang.pick(zh, en), icon: Some(icon), checked, rule_before };
    [
        item("固定位置和大小", "Pin where it is", Icon::Pin, pinned, false),
        item("鼠标穿透", "Let clicks through", Icon::Pointer, through, false),
        item("置于其他窗口之上", "Keep above other windows", Icon::Layers, above, false),
        item("收起", "Put away", Icon::Close, false, false),
        item("设置…", "Settings…", Icon::Settings, false, true),
    ]
}

/// The least rectangle holding both.
fn union(a: RECT, b: RECT) -> RECT {
    RECT { left: a.left.min(b.left), top: a.top.min(b.top), right: a.right.max(b.right), bottom: a.bottom.max(b.bottom) }
}

/// `r` within `bounds`.
fn clip(r: RECT, bounds: RECT) -> RECT {
    let left = r.left.clamp(bounds.left, bounds.right);
    let top = r.top.clamp(bounds.top, bounds.bottom);
    RECT { left, top, right: r.right.clamp(left, bounds.right), bottom: r.bottom.clamp(top, bounds.bottom) }
}

/// Whether a game shows (its button lit): one found and not turned off, or
/// one asked for by hand.
pub(crate) fn game_shown(history: &[Sample]) -> bool {
    use crate::presents::{game_mode, GameMode};
    match game_mode() {
        GameMode::On => true,
        GameMode::Off => false,
        GameMode::Auto => history.last().is_some_and(|s| s.game.is_some()),
    }
}

/// The outline of a rounded rectangle, as points round it, closed.
fn rounded(x: f32, y: f32, w: f32, h: f32, radius: f32) -> Vec<Point> {
    let r = radius.min(w / 2.0).min(h / 2.0).max(0.0);
    let corners = [(x + w - r, y + r, -0.25), (x + w - r, y + h - r, 0.0), (x + r, y + h - r, 0.25), (x + r, y + r, 0.5)];
    let mut points: Vec<Point> = corners
        .iter()
        .flat_map(|&(cx, cy, from)| {
            (0..=8).map(move |i| {
                let angle = std::f32::consts::TAU * (from + 0.25 * i as f32 / 8.0);
                Point { x: cx + r * angle.cos(), y: cy + r * angle.sin() }
            })
        })
        .collect();
    points.push(points[0]);
    points
}

/// What a widget's buttons do: the window in front taken for a game (one
/// played in a window, that would not be found), the settings, pinned,
/// put away.
#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_corner_is_taken_along_either_edge() {
        let (w, h) = (300.0, 200.0);
        // On the right edge near the top, and on the top edge near the right.
        assert_eq!(edges_at(w - 2.0, 15.0, w, h), (false, true, true, false));
        assert_eq!(edges_at(w - 15.0, 2.0, w, h), (false, true, true, false));
        assert_eq!(edges_at(2.0, h - 15.0, w, h), (true, false, false, true));
        // Away from the corners, one edge; inside, none.
        assert_eq!(edges_at(w - 2.0, 100.0, w, h), (false, true, false, false));
        assert_eq!(edges_at(150.0, h - 2.0, w, h), (false, false, false, true));
        assert_eq!(edges_at(15.0, 15.0, w, h), (false, false, false, false));
    }
}
