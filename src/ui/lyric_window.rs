//! The lyrics in windows of their own, on the desktop: a card (the cover
//! blurred behind them, the lines around the one sung now gliding past it,
//! that one filled word by word as it is sung, a spectrum along the bottom
//! moving with the music), a player of middle size (the cover, what plays,
//! the line sung now, its track and what pauses, plays and skips it, on the
//! same ground as the card's), and floating lyrics (one line, glowing, over
//! every window, as subtitles are). Each is shown while the lyrics module
//! is on and its own switch is; dragged where it is wanted, and kept there.
//! While something plays they are drawn every frame; at rest, now and then.

use std::cell::RefCell;
use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::core::{w, Interface};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT, D2D_SIZE_U};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Bitmap1, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_PROPERTIES1, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_LAYER_PARAMETERS1, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::DirectWrite::{IDWriteTextLayout, DWRITE_HIT_TEST_METRICS, DWRITE_LINE_METRICS};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::Foundation::{COLORREF, POINT};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, GetCursorPos, GetWindowLongPtrW, GetWindowRect, LoadCursorW, RegisterClassExW, SetCursor, SetLayeredWindowAttributes, SetWindowLongPtrW, SetWindowPos,
    ShowWindow, GWL_EXSTYLE, HTBOTTOM, HTBOTTOMLEFT, HTBOTTOMRIGHT, HTCAPTION, HTCLIENT, HTLEFT, HTRIGHT, HTTOP, HTTOPLEFT, HTTOPRIGHT, HWND_TOPMOST, IDC_ARROW, LWA_ALPHA, MA_NOACTIVATE,
    MINMAXINFO, SWP_NOACTIVATE, SWP_NOZORDER, SW_SHOWNOACTIVATE, WM_DPICHANGED, WM_EXITSIZEMOVE, WM_GETMINMAXINFO, WM_LBUTTONUP, WM_MOUSEACTIVATE, WM_NCHITTEST, WM_SETCURSOR, WM_SIZE, WNDCLASSEXW,
    WS_EX_LAYERED, WS_EX_NOACTIVATE, WS_EX_NOREDIRECTIONBITMAP, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

use super::buttons::{self, Button};
use super::canvas::{Align, Canvas, Color, Family, Fill, Font, Point};
use super::life::Lives;
use super::gfx::{self, rect, Frame, Gfx, Surface};
use crate::cover::Cover;
use crate::lyrics::Line;
use crate::playing::Sung;

/// Which of the two.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Style {
    Card,
    Medium,
    Strip,
    Subtitle,
}

impl Style {
    /// Its size (DIPs), and the room round it for its shadow.
    fn size(self) -> (f32, f32) {
        match self {
            Style::Card => (440.0, 330.0),
            Style::Medium => (420.0, 170.0),
            Style::Strip => (520.0, 56.0),
            Style::Subtitle => (980.0, 120.0),
        }
    }

    fn margin(self) -> f32 {
        match self {
            Style::Card | Style::Medium | Style::Strip => 30.0,
            Style::Subtitle => 0.0,
        }
    }

    /// The least it can be sized to (DIPs).
    fn least(self) -> (f32, f32) {
        match self {
            Style::Card => (320.0, 230.0),
            Style::Medium => (340.0, 150.0),
            Style::Strip => (360.0, 56.0),
            Style::Subtitle => (420.0, 100.0),
        }
    }

    /// The buttons it shows while the pointer is over it, in their row.
    fn buttons(self) -> &'static [Button] {
        match self {
            Style::Card | Style::Medium | Style::Strip => &[Button::Settings, Button::Pin, Button::Close],
            Style::Subtitle => &[Button::Settings, Button::Pin, Button::Through, Button::Close],
        }
    }

    /// Whether its buttons are above it, outside it (all but the floating lyrics, which are clear).
    fn buttons_outside(self) -> bool {
        self != Style::Subtitle
    }

    /// Its switch among the lyrics module's items.
    fn item(self) -> &'static str {
        match self {
            Style::Card => "card",
            Style::Medium => "medium",
            Style::Strip => "strip",
            Style::Subtitle => "subtitle",
        }
    }
}

/// How often one is drawn while something plays, and at rest.
const PLAYING_FRAME: Duration = Duration::from_millis(16);
const REST_FRAME: Duration = Duration::from_millis(250);

/// The card's words: the line sung now, the others, the translation, what it says above them (DIPs).
const NOW_FONT: Font = Font::new(Family::Segoe, 25.0, 700.0);
const TITLE_FONT: Font = Font::new(Family::Segoe, 12.0, 600.0);
const UNDER_FONT: Font = Font::new(Family::Segoe, 14.0, 500.0);
/// The player's: what plays, by whom, the line sung now and what is under it, how far in it is.
const PLAYER_TITLE: Font = Font::new(Family::Segoe, 15.0, 700.0);
const PLAYER_ABOUT: Font = Font::new(Family::Segoe, 12.0, 500.0);
const PLAYER_LINE: Font = Font::new(Family::Segoe, 18.0, 700.0);
const PLAYER_UNDER: Font = Font::new(Family::Segoe, 12.5, 500.0);
const PLAYER_TIME: Font = Font::new(Family::Segoe, 11.0, 500.0);
/// The strip's line sung now, and the least it is made to fit its room.
const STRIP_LINE: Font = Font::new(Family::Segoe, 16.0, 700.0);
const STRIP_LEAST: f32 = 12.5;
/// The player's three that are pressed: the side of each, and the system icon font's glyphs.
const CONTROL: f32 = 28.0;
const CONTROL_FONT: Font = Font::new(Family::Icons, 14.0, 400.0);
/// How fast a band of the spectrum falls, at most (of its whole height, a second): from full, in about 0.6 s.
const SPECTRUM_FALL: f32 = 1.7;
/// How long they take to come in over its spectrum (or go), seconds.
const CONTROLS_FADE: f32 = 0.18;
/// The floating lyrics' line and what is under it.
const FLOAT_FONT: Font = Font::new(Family::Segoe, 36.0, 800.0);
const FLOAT_UNDER: Font = Font::new(Family::Segoe, 18.0, 600.0);

impl super::life::Lives for Shown {
    fn device(&self) -> &Rc<Gfx> {
        &self.gfx
    }

    fn showing(&mut self) -> &mut Option<super::life::Showing> {
        &mut self.showing
    }

    /// What its surface holds, its ground, the cover behind it.
    fn let_go_of_shown(&mut self) {
        if let Some(surface) = &mut self.surface {
            surface.release(&self.gfx);
        }
        self.ground = Default::default();
        self.backdrop = None;
        self.fading = None;
    }

    /// Its surface.
    fn let_go_of_kept(&mut self) {
        self.surface = None;
    }
}

/// A window shown, and what it keeps from frame to frame.
struct Shown {
    style: Style,
    hwnd: HWND,
    gfx: Rc<Gfx>,
    surface: Option<Surface>,
    /// Counted as shown on its device while it is (see `life`).
    showing: Option<super::life::Showing>,
    scale: f32,
    /// Its size (DIPs, inside the room for its shadow).
    room: (f32, f32),
    /// Neither moved nor sized; letting clicks through (the floating lyrics alone).
    pinned: bool,
    through: bool,
    /// The pointer over it, since when; letting clicks through, the pointer has stayed long enough for the button
    /// that ends that to be pressed (it takes clicks meanwhile).
    over: Option<Instant>,
    reachable: bool,
    /// Where its buttons were drawn (DIPs, inside), and the one whose words show.
    hits: Vec<(f32, f32, f32, f32, Button)>,
    /// Where the player's three that are pressed were drawn, and its track (none: pressed there, it is played from
    /// that far along it).
    controls: Vec<(f32, f32, f32, f32, Option<crate::playing::Key>)>,
    tipped: Option<Button>,
    next_frame: Instant,
    /// The rows the lines have glided to (the line sung now's place, fractional while gliding), and how fast.
    rail: f32,
    rail_speed: f32,
    /// The lyrics it last showed (to tell another track's).
    lyrics: Option<usize>,
    last_frame: Instant,
    /// The cover behind the card, blurred, and the one it is fading from (with when that began).
    backdrop: Option<(usize, ID2D1Bitmap1)>,
    fading: Option<(ID2D1Bitmap1, Instant)>,
    /// The colours the lyrics are drawn in, easing toward the cover's.
    tint: [f32; 3],
    accent: [f32; 3],
    /// The card's ground (its shadow, the cover blurred, the colour washed over it, what plays), kept and drawn
    /// again only as it changes: what moves at every frame is drawn over it.
    ground: super::gfx::Layer,
    /// How far the player's controls have come in over its spectrum (0 to 1: in while the pointer is over it).
    controls_in: f32,
    /// Its spectrum as shown: up at once with the sound, down no faster than `SPECTRUM_FALL` (the music paused or
    /// stopped, it falls, as a meter's does, rather than vanishing).
    bands: [f32; crate::sound::BANDS],
}

thread_local! {
    static SHOWN: RefCell<Vec<Shown>> = const { RefCell::new(Vec::new()) };
}

/// Shows the windows wanted, takes away those no longer wanted, follows the pointer over them, and draws each
/// that is due; when next to come.
pub fn tick(now: Instant) -> Option<Duration> {
    let wanted: Vec<Style> = {
        let settings = crate::app().settings.lock().unwrap();
        let view = &settings.view;
        let on = super::prefs::module_on(view, "lyrics");
        [Style::Card, Style::Medium, Style::Strip, Style::Subtitle].into_iter().filter(|style| on && super::prefs::item_on(view, "lyrics", style.item())).collect()
    };
    SHOWN.with_borrow_mut(|shown| {
        shown.retain_mut(|one| {
            let keep = wanted.contains(&one.style);
            if !keep {
                if one.tipped.is_some() {
                    super::tip::hide();
                }
                // (All it drew with goes with its window: see `life`.)
                one.gone();
                unsafe {
                    let _ = DestroyWindow(one.hwnd);
                }
            }
            keep
        });
        for style in &wanted {
            if !shown.iter().any(|one| one.style == *style) {
                if let Some(made) = make(*style) {
                    shown.push(made);
                }
            }
        }
        let mut due: Option<Duration> = None;
        for one in shown.iter_mut() {
            one.follow_pointer(now);
            if now >= one.next_frame {
                let busy = one.draw(now);
                // (Text laid out and not drawn again lately let go of, as every window that draws does: drawn by
                // the lyrics alone, it would only grow.)
                one.gfx.sweep();
                one.next_frame = now + if busy { PLAYING_FRAME } else { REST_FRAME };
            }
            // (The pointer over it is followed closely.)
            let wait = one.next_frame.saturating_duration_since(now).min(if one.over.is_some() { Duration::from_millis(40) } else { Duration::MAX });
            due = Some(due.map_or(wait, |due| due.min(wait)));
        }
        due
    })
}

/// Lets clicks through `hwnd` (to what is behind it), or no longer.
fn let_through(hwnd: HWND, through: bool) {
    unsafe {
        let style = GetWindowLongPtrW(hwnd, GWL_EXSTYLE);
        let style = if through { style | WS_EX_TRANSPARENT.0 as isize } else { style & !(WS_EX_TRANSPARENT.0 as isize) };
        SetWindowLongPtrW(hwnd, GWL_EXSTYLE, style);
    }
}

impl Shown {
    /// Where the pointer is (DIPs, inside), if it is over it.
    fn pointer(&self) -> Option<(f32, f32)> {
        let (mut cursor, mut at) = (POINT::default(), RECT::default());
        unsafe {
            GetCursorPos(&mut cursor).ok()?;
            GetWindowRect(self.hwnd, &mut at).ok()?;
        }
        let margin = self.style.margin();
        let (x, y) = ((cursor.x - at.left) as f32 / self.scale - margin, (cursor.y - at.top) as f32 / self.scale - margin);
        // (And on the row of its buttons above it, while they show: the pointer goes there from it.)
        let above = if self.over.is_some() && self.style.buttons_outside() { buttons::SIZE + 2.0 * buttons::GAP } else { 0.0 };
        (x >= 0.0 && y >= -above && x < self.room.0 && y < self.room.1).then_some((x, y))
    }

    fn button_at(&self, (x, y): (f32, f32)) -> Option<Button> {
        self.hits.iter().find(|(bx, by, bw, bh, _)| x >= *bx && x < bx + bw && y >= *by && y < by + bh).map(|hit| hit.4)
    }

    /// The pointer come, gone, or resting on a button (whose words then show); letting clicks through, the
    /// button that ends that made reachable once it has stayed.
    fn follow_pointer(&mut self, now: Instant) {
        let pointer = self.pointer();
        if pointer.is_some() != self.over.is_some() {
            self.over = pointer.map(|_| now);
            self.next_frame = now;
        }
        if self.through {
            let reach = self.over.is_some_and(|since| now.duration_since(since) >= buttons::LINGER);
            if reach != self.reachable {
                self.reachable = reach;
                let_through(self.hwnd, !reach);
                self.next_frame = now;
            }
        }
        let on = pointer.and_then(|at| self.button_at(at));
        if on != self.tipped {
            self.tipped = on;
            match on {
                Some(button) => {
                    let lit = (button == Button::Pin && self.pinned) || (button == Button::Through && self.through);
                    let hit = self.hits.iter().find(|hit| hit.4 == button).copied().unwrap();
                    let mut at = RECT::default();
                    unsafe {
                        let _ = GetWindowRect(self.hwnd, &mut at);
                    }
                    let margin = self.style.margin();
                    let px = |v: f32| (v * self.scale) as i32;
                    let about = RECT { left: at.left + px(margin + hit.0), top: at.top + px(margin + hit.1), right: at.left + px(margin + hit.0 + hit.2), bottom: at.top + px(margin + hit.1 + hit.3) };
                    super::tip::show(button.says(lit, crate::update::language(), false), about, true);
                    super::tip::above();
                }
                None => super::tip::hide(),
            }
        }
    }

    /// `button` pressed.
    fn press(&mut self, button: Button) {
        let style = self.style;
        match button {
            Button::Close => {
                super::tip::hide();
                crate::app().change(|settings| super::prefs::set_item(&mut settings.view, "lyrics", style.item(), false));
            }
            Button::Pin => {
                self.pinned = !self.pinned;
                let pinned = self.pinned;
                crate::app().change(|settings| match style {
                    Style::Card => settings.lyric_card_pinned = pinned,
                    Style::Medium => settings.lyric_medium_pinned = pinned,
                    Style::Strip => settings.lyric_strip_pinned = pinned,
                    Style::Subtitle => settings.lyric_subtitle_pinned = pinned,
                });
            }
            Button::Through => {
                self.through = !self.through;
                self.reachable = false;
                let through = self.through;
                let_through(self.hwnd, through);
                crate::app().change(|settings| settings.lyric_subtitle_through = through);
            }
            Button::Settings => crate::show_settings(),
            Button::Game => {}
        }
        self.tipped = None;
        super::tip::hide();
        self.next_frame = Instant::now();
    }

    /// The row of buttons in its top right corner, while the pointer is over it (letting clicks through, the one
    /// that ends that, once the pointer has stayed); where each is, kept for the pointer.
    fn paint_buttons(&mut self, frame: &Frame, (w, _h): (f32, f32)) {
        self.hits.clear();
        if self.over.is_none() {
            return;
        }
        let shown: Vec<Button> = if self.through {
            if !self.reachable {
                return;
            }
            vec![Button::Through]
        } else {
            self.style.buttons().to_vec()
        };
        let (size, gap) = (buttons::SIZE, buttons::GAP);
        // (On the strip's middle: it is no taller than a row of them and the room round it.)
        // Above its top right corner, outside it, as a widget's are (the floating lyrics, clear, keep theirs inside).
        let outside = self.style.buttons_outside();
        let y = if outside { -(gap + size / 2.0) } else { 10.0 + size / 2.0 };
        for (place, button) in shown.iter().rev().enumerate() {
            let x = w - if outside { 0.0 } else { 12.0 } - size / 2.0 - place as f32 * (size + gap);
            let lit = (*button == Button::Pin && self.pinned) || (*button == Button::Through && self.through);
            buttons::paint(frame, Point { x, y }, *button, lit, true, Color::hex(0xFFFFFF, 0.25));
            self.hits.push((x - size / 2.0, y - size / 2.0, size, size, *button));
        }
    }
}

/// Which edge of `one` a point (DIPs, inside) is on, for it to be sized there: the card at any edge or corner, the
/// floating lyrics at their ends.
fn edge_at(one: &Shown, (x, y): (f32, f32)) -> Option<u32> {
    let near = 7.0;
    let (w, h) = one.room;
    let (left, right, top, bottom) = (x < near, x > w - near, y < near, y > h - near);
    match one.style {
        Style::Subtitle | Style::Strip => {
            if left {
                Some(HTLEFT)
            } else if right {
                Some(HTRIGHT)
            } else {
                None
            }
        }
        Style::Card | Style::Medium => match (left, right, top, bottom) {
            (true, _, true, _) => Some(HTTOPLEFT),
            (_, true, true, _) => Some(HTTOPRIGHT),
            (true, _, _, true) => Some(HTBOTTOMLEFT),
            (_, true, _, true) => Some(HTBOTTOMRIGHT),
            (true, ..) => Some(HTLEFT),
            (_, true, ..) => Some(HTRIGHT),
            (_, _, true, _) => Some(HTTOP),
            (_, _, _, true) => Some(HTBOTTOM),
            _ => None,
        },
    }
}

/// How near an edge of its screen's work area the strip, let go, keeps to it (DIPs).
const STRIP_SNAP: f32 = 20.0;

/// Where a window at `at` (physical px), drawn `margin` in from its edges (the room for its shadow), goes to keep to
/// the edges of its screen's work area it is within `near` of: those edges flush with what it draws.
fn snapped(at: RECT, margin: i32, near: i32) -> (i32, i32) {
    use windows::Win32::Graphics::Gdi::{MonitorFromRect, MONITOR_DEFAULTTONEAREST};
    let drawn = RECT { left: at.left + margin, top: at.top + margin, right: at.right - margin, bottom: at.bottom - margin };
    let mut screen = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    if !unsafe { GetMonitorInfoW(MonitorFromRect(&drawn, MONITOR_DEFAULTTONEAREST), &mut screen) }.as_bool() {
        return (at.left, at.top);
    }
    let work = screen.rcWork;
    let along = |from: i32, to: i32, low: i32, high: i32| {
        if (from - low).abs() <= near {
            low - from
        } else if (to - high).abs() <= near {
            high - to
        } else {
            0
        }
    };
    (at.left + along(drawn.left, drawn.right, work.left, work.right), at.top + along(drawn.top, drawn.bottom, work.top, work.bottom))
}

/// Does `act` with the window shown as `hwnd`, while nothing else holds them.
fn with<R>(hwnd: HWND, act: impl FnOnce(&mut Shown) -> R) -> Option<R> {
    SHOWN.with(|shown| shown.try_borrow_mut().ok().and_then(|mut shown| shown.iter_mut().find(|one| one.hwnd == hwnd).map(act)))
}

unsafe extern "system" fn procedure(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        // On a button: pressed. Pinned: held still. At an edge: sized. Anywhere else: moved.
        WM_NCHITTEST => {
            let hit = with(hwnd, |one| {
                let at = one.pointer();
                if at.and_then(|at| one.button_at(at)).is_some() || at.and_then(|at| one.control_at(at)).is_some() || one.pinned {
                    return HTCLIENT;
                }
                at.and_then(|at| edge_at(one, at)).unwrap_or(HTCAPTION)
            });
            LRESULT(hit.unwrap_or(HTCAPTION) as isize)
        }
        WM_MOUSEACTIVATE => LRESULT(MA_NOACTIVATE as isize),
        // (Over its body, which moves it, the plain arrow, as over any window and as over the panel and widgets.)
        WM_SETCURSOR if (lparam.0 & 0xFFFF) as u32 == HTCAPTION || (lparam.0 & 0xFFFF) as u32 == HTCLIENT => {
            unsafe { SetCursor(LoadCursorW(None, IDC_ARROW).ok()) };
            LRESULT(1)
        }
        WM_LBUTTONUP => {
            with(hwnd, |one| {
                let at = one.pointer();
                if let Some(button) = at.and_then(|at| one.button_at(at)) {
                    one.press(button);
                } else if let Some((at, control)) = at.and_then(|at| Some((at, one.control_at(at)?))) {
                    one.control(at, control);
                }
            });
            LRESULT(0)
        }
        WM_GETMINMAXINFO => {
            let least = with(hwnd, |one| {
                let (w, h) = one.style.least();
                let margin = one.style.margin();
                (((w + 2.0 * margin) * one.scale) as i32, ((h + 2.0 * margin) * one.scale) as i32)
            });
            if let Some((w, h)) = least {
                let info = unsafe { &mut *(lparam.0 as *mut MINMAXINFO) };
                info.ptMinTrackSize = POINT { x: w, y: h };
            }
            LRESULT(0)
        }
        // Sized: what it shows laid out afresh at once (the panel's own turn waits while it is sized by hand).
        WM_SIZE => {
            with(hwnd, |one| {
                let margin = one.style.margin();
                let (w, h) = ((lparam.0 & 0xFFFF) as f32 / one.scale - 2.0 * margin, ((lparam.0 >> 16) & 0xFFFF) as f32 / one.scale - 2.0 * margin);
                if w > 0.0 && h > 0.0 && (w, h) != one.room {
                    one.room = (w, h);
                    one.draw(Instant::now());
                }
            });
            LRESULT(0)
        }
        // Moved or sized: kept so.
        WM_EXITSIZEMOVE => {
            let mut at = RECT::default();
            unsafe {
                let _ = GetWindowRect(hwnd, &mut at);
            }
            if let Some((style, room, scale)) = with(hwnd, |one| (one.style, one.room, one.scale)) {
                // The strip, let go near an edge of its screen's work area, keeps to it: its edge flush with that one.
                if style == Style::Strip {
                    let snapped = snapped(at, (style.margin() * scale).round() as i32, (STRIP_SNAP * scale) as i32);
                    if snapped != (at.left, at.top) {
                        unsafe {
                            let _ = SetWindowPos(hwnd, None, snapped.0, snapped.1, 0, 0, SWP_NOZORDER | SWP_NOACTIVATE | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE);
                        }
                        (at.right, at.bottom) = (at.right + snapped.0 - at.left, at.bottom + snapped.1 - at.top);
                        (at.left, at.top) = snapped;
                    }
                }
                crate::app().change(|settings| {
                    let kept = Some((at.left, at.top));
                    match style {
                        Style::Card => {
                            settings.lyric_card = kept;
                            settings.lyric_card_size = Some(room);
                        }
                        Style::Medium => {
                            settings.lyric_medium = kept;
                            settings.lyric_medium_size = Some(room);
                        }
                        Style::Strip => {
                            settings.lyric_strip = kept;
                            settings.lyric_strip_size = Some(room);
                        }
                        Style::Subtitle => {
                            settings.lyric_subtitle = kept;
                            settings.lyric_subtitle_size = Some(room);
                        }
                    }
                });
            }
            LRESULT(0)
        }
        WM_DPICHANGED => {
            let suggested = unsafe { *(lparam.0 as *const RECT) };
            with(hwnd, |one| one.scale = (wparam.0 & 0xFFFF) as f32 / 96.0);
            unsafe {
                let _ = SetWindowPos(hwnd, None, suggested.left, suggested.top, suggested.right - suggested.left, suggested.bottom - suggested.top, SWP_NOZORDER | SWP_NOACTIVATE);
            }
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

/// A window of `style`, where it was kept (else on the main screen: the card at its bottom right, the player at its
/// top right, the floating lyrics in the middle of its bottom).
fn make(style: Style) -> Option<Shown> {
    let gfx = gfx::current().ok()?;
    let (kept, size, pinned, through) = {
        let settings = crate::app().settings.lock().unwrap();
        match style {
            Style::Card => (settings.lyric_card, settings.lyric_card_size, settings.lyric_card_pinned, false),
            Style::Medium => (settings.lyric_medium, settings.lyric_medium_size, settings.lyric_medium_pinned, false),
            Style::Strip => (settings.lyric_strip, settings.lyric_strip_size, settings.lyric_strip_pinned, false),
            Style::Subtitle => (settings.lyric_subtitle, settings.lyric_subtitle_size, settings.lyric_subtitle_pinned, settings.lyric_subtitle_through),
        }
    };
    unsafe {
        let module = GetModuleHandleW(None).ok()?;
        let class = w!("GlanceLyrics");
        RegisterClassExW(&WNDCLASSEXW { cbSize: size_of::<WNDCLASSEXW>() as u32, lpfnWndProc: Some(procedure), hInstance: module.into(), lpszClassName: class, ..Default::default() });
        let mut screen = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(MonitorFromPoint(Default::default(), MONITOR_DEFAULTTOPRIMARY), &mut screen);
        let work = screen.rcWork;
        let (w, h) = size.unwrap_or(style.size());
        let margin = style.margin();
        // (Sized at the main screen's scale; a move to another screen sizes it again.)
        let scale = windows::Win32::UI::HiDpi::GetDpiForSystem() as f32 / 96.0;
        let (pw, ph) = (((w + 2.0 * margin) * scale) as i32, ((h + 2.0 * margin) * scale) as i32);
        let (x, y) = kept.unwrap_or(match style {
            Style::Card => (work.right - pw - (12.0 * scale) as i32, work.bottom - ph - (12.0 * scale) as i32),
            Style::Medium => (work.right - pw - (12.0 * scale) as i32, work.top + (12.0 * scale) as i32),
            Style::Strip => ((work.left + work.right - pw) / 2, work.top + (12.0 * scale) as i32),
            Style::Subtitle => ((work.left + work.right - pw) / 2, work.bottom - ph - (60.0 * scale) as i32),
        });
        // The floating lyrics above all, and layered (as a window that lets clicks through must be).
        let over_all = if style == Style::Subtitle { WS_EX_TOPMOST | WS_EX_LAYERED } else { Default::default() };
        let hwnd = CreateWindowExW(WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_NOREDIRECTIONBITMAP | over_all, class, w!("Glance"), WS_POPUP, x, y, pw, ph, None, None, Some(module.into()), None).ok()?;
        if style == Style::Subtitle {
            let _ = SetLayeredWindowAttributes(hwnd, COLORREF(0), 255, LWA_ALPHA);
            if through {
                let_through(hwnd, true);
            }
        }
        // Made where it was kept, on a screen whose scale may be another than the main one's it was sized at: sized
        // again at its own (else what it draws, at that scale, would not fit in it).
        let made_at = scale;
        let scale = GetDpiForWindow(hwnd).max(96) as f32 / 96.0;
        if scale != made_at {
            let (pw, ph) = (((w + 2.0 * margin) * scale) as i32, ((h + 2.0 * margin) * scale) as i32);
            let _ = SetWindowPos(hwnd, None, 0, 0, pw, ph, SWP_NOZORDER | SWP_NOACTIVATE | windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE);
        }
        let surface = Surface::new(&gfx, hwnd).ok();
        let _ = ShowWindow(hwnd, SW_SHOWNOACTIVATE);
        if style == Style::Subtitle {
            let _ = SetWindowPos(hwnd, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOACTIVATE | windows::Win32::UI::WindowsAndMessaging::SWP_NOMOVE | windows::Win32::UI::WindowsAndMessaging::SWP_NOSIZE);
        }
        let now = Instant::now();
        Some(Shown {
            style,
            hwnd,
            gfx,
            surface,
            scale,
            room: (w, h),
            pinned,
            through,
            over: None,
            reachable: false,
            hits: Vec::new(),
            controls: Vec::new(),
            tipped: None,
            next_frame: now,
            rail: 0.0,
            rail_speed: 0.0,
            lyrics: None,
            last_frame: now,
            backdrop: None,
            fading: None,
            tint: [1.0, 1.0, 1.0],
            accent: [0.55, 0.65, 1.0],
            ground: Default::default(),
            controls_in: 0.0,
            bands: [0.0; crate::sound::BANDS],
            showing: None,
        })
    }
}

/// What it shows this frame: the lyrics and the line sung now, how far into the track it is, the track, its
/// cover, the sound.
struct Moment {
    lines: Option<Arc<Vec<Line>>>,
    now: Option<usize>,
    since: Option<Instant>,
    position: f64,
    /// How long the track is (seconds), and whether it can be played from elsewhere along it; none where its player
    /// does not say.
    length: Option<(f64, bool)>,
    title: String,
    artist: String,
    /// Why there are no lines, where there are none.
    said: Option<String>,
    playing: bool,
    cover: Option<Arc<Cover>>,
    sound: crate::sound::Heard,
    translated: bool,
}

fn moment() -> Moment {
    let heard = crate::playing::now();
    let lang = crate::update::language();
    let translated = super::prefs::item_on(&crate::app().settings.lock().unwrap().view, "lyrics", "translation");
    let (title, artist, playing) = heard.as_ref().map_or_else(Default::default, |heard| (heard.title.clone(), heard.artist.clone(), heard.playing));
    let mut moment = Moment {
        lines: None,
        now: None,
        since: None,
        position: crate::playing::position().unwrap_or(0.0),
        length: heard.as_ref().and_then(|heard| heard.timeline).map(|timeline| (timeline.length, timeline.seekable)),
        cover: (!title.is_empty()).then(|| crate::cover::of(&title, &artist)).flatten(),
        sound: if playing { crate::sound::heard() } else { crate::sound::Heard::default() },
        title,
        artist,
        said: None,
        playing,
        translated,
    };
    match heard.as_ref().and_then(|heard| heard.sung.clone()) {
        Some(Sung::Lines { lines, now, since }) => {
            moment.lines = Some(lines);
            moment.now = now;
            moment.since = Some(since);
        }
        Some(Sung::Untimed { netease: true }) => moment.said = Some(lang.pick("网易云音乐没有提供播放进度 · 安装 InfLink-rs 插件后可以显示歌词", "NetEase Cloud Music does not say how far in it is · with the InfLink-rs plugin it does").into()),
        Some(Sung::Untimed { netease: false }) => moment.said = Some(lang.pick("这个播放器没有提供播放进度", "This player does not say how far into the track it is").into()),
        Some(Sung::Asking) => moment.said = Some(lang.pick("正在查找歌词…", "Finding the lyrics…").into()),
        Some(Sung::Unknown) => moment.said = Some(lang.pick("没有找到这首歌的歌词", "No lyrics were found for this track").into()),
        None => moment.said = Some(lang.pick("没有正在播放的歌曲", "Nothing is playing").into()),
    }
    moment
}

/// How a line is sung, part by part: each part's place in the text drawn (UTF-16, from and to) and its time (from
/// and to, seconds). Word by word where that is known; else each character of Chinese or Japanese, each word of
/// other writing (weighed by its length), sharing nine tenths of the line's time.
fn parts(line: &Line, next: Option<f64>) -> (String, Vec<(usize, usize, f64, f64)>) {
    if !line.words.is_empty() {
        let mut text = String::new();
        let mut parts = Vec::new();
        for word in &line.words {
            let from = text.encode_utf16().count();
            text.push_str(&word.text);
            parts.push((from, text.encode_utf16().count(), word.at, word.end.max(word.at + 0.01)));
        }
        return (text, parts);
    }
    let text = line.text.clone();
    let end = line.end.or(next).unwrap_or(line.at + 4.0).max(line.at + 0.3);
    let span = (end - line.at) * 0.9;
    // The pieces: a character of the scripts written without spaces, a run of anything else up to a space.
    let mut pieces: Vec<(usize, usize, f64)> = Vec::new();
    let mut at = 0;
    let mut run: Option<usize> = None;
    let close = |pieces: &mut Vec<(usize, usize, f64)>, run: &mut Option<usize>, to: usize| {
        if let Some(from) = run.take() {
            pieces.push((from, to, 1.0 + 0.15 * (to - from) as f64));
        }
    };
    for c in text.chars() {
        let units = c.len_utf16();
        let wide = matches!(c as u32, 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF);
        if wide {
            close(&mut pieces, &mut run, at);
            pieces.push((at, at + units, 1.0));
        } else if c.is_whitespace() {
            close(&mut pieces, &mut run, at);
        } else if run.is_none() {
            run = Some(at);
        }
        at += units;
    }
    close(&mut pieces, &mut run, at);
    let weight: f64 = pieces.iter().map(|p| p.2).sum::<f64>().max(1e-6);
    let mut t = line.at;
    let parts = pieces
        .into_iter()
        .map(|(from, to, w)| {
            let took = span * w / weight;
            let part = (from, to, t, t + took);
            t += took;
            part
        })
        .collect();
    (text, parts)
}

/// How much of the text drawn is sung at `t`: its place, in UTF-16 units, fractional within the part being sung.
fn sung_to(parts: &[(usize, usize, f64, f64)], t: f64) -> f32 {
    let mut done = 0.0;
    for (from, to, at, end) in parts {
        if t < *at {
            break;
        }
        done = if t >= *end { *to as f32 } else { *from as f32 + (*to - *from) as f32 * ((t - at) / (end - at)) as f32 };
    }
    done
}

/// How far through it is (0 to 1) at `t`: a line without words (the music between), from its start to its end or
/// the next line's.
fn between(line: &Line, next: Option<f64>, t: f64) -> f32 {
    let end = line.end.or(next).unwrap_or(line.at + 4.0).max(line.at + 0.3);
    ((t - line.at) / (end - line.at)).clamp(0.0, 1.0) as f32
}

/// The music between lines, where a line of words `size` DIPs tall would be (its box's corner at `(x, y)`): three
/// dots in `dim`, each coming to `bright` (and a little larger) as its third of it goes (`done`, 0 to 1): how many
/// are lit, how much of it has gone.
fn interlude(frame: &Frame, x: f32, y: f32, size: f32, done: f32, dim: Color, bright: Color) {
    let radius = size * 0.12;
    let centre_y = y + size * 0.68;
    for index in 0..3 {
        let lit = (done * 3.0 - index as f32).clamp(0.0, 1.0);
        let lit = lit * lit * (3.0 - 2.0 * lit);
        let color = Color { r: dim.r + (bright.r - dim.r) * lit, g: dim.g + (bright.g - dim.g) * lit, b: dim.b + (bright.b - dim.b) * lit, a: dim.a + (bright.a - dim.a) * lit };
        if color.a > 0.0 {
            frame.fill_circle(color, Point { x: x + radius * 1.2 + index as f32 * radius * 3.6, y: centre_y }, radius * (1.0 + 0.25 * lit));
        }
    }
}

/// A line drawn in `dim`, and over it in `bright` as far as it is sung (see `sung_part`).
#[allow(clippy::too_many_arguments)]
fn filled(frame: &Frame, layout: &IDWriteTextLayout, text: &str, font: Font, x: f32, y: f32, width: f32, sung: f32, dim: Color, bright: Color, soft: f32) {
    frame.text_wrapped(text, font, dim, x, y, width);
    sung_part(frame, layout, text, font, x, y, width, sung, bright, soft);
}

/// As much of a line as is sung (`sung`, UTF-16 units), in `bright`, its edge soft: row by row as `layout` breaks
/// it, the row being sung up to the place sung, the rows before it whole.
#[allow(clippy::too_many_arguments)]
fn sung_part(frame: &Frame, layout: &IDWriteTextLayout, text: &str, font: Font, x: f32, y: f32, width: f32, sung: f32, bright: Color, soft: f32) {
    if sung <= 0.0 {
        return;
    }
    let mut count = 0;
    let _ = unsafe { layout.GetLineMetrics(None, &mut count) };
    let mut rows = vec![DWRITE_LINE_METRICS::default(); count as usize];
    let _ = unsafe { layout.GetLineMetrics(Some(&mut rows), &mut count) };
    // Where the place sung is: its row's top and the x it is at (between the characters about it).
    let place = |unit: usize| {
        let (mut px, mut py, mut metrics) = (0.0, 0.0, DWRITE_HIT_TEST_METRICS::default());
        let _ = unsafe { layout.HitTestTextPosition(unit as u32, false, &mut px, &mut py, &mut metrics) };
        (px, metrics.top)
    };
    let whole = sung.floor() as usize;
    let (x0, top) = place(whole);
    let (x1, top1) = place(whole + 1);
    let at = if (top1 - top).abs() < 0.5 { x0 + (x1 - x0) * sung.fract() } else { x0 };
    let mut row_top = 0.0;
    for row in &rows {
        let height = row.height;
        if row_top + height <= top + 0.5 {
            frame.clip(x - 4.0, y + row_top, width + 8.0, height);
            frame.text_wrapped(text, font, bright, x, y, width);
            frame.unclip();
        } else if row_top <= top + 0.5 {
            // The row being sung: whole to the place, then fading over the soft edge, in steps.
            frame.clip(x - 4.0, y + row_top, at + 4.0, height);
            frame.text_wrapped(text, font, bright, x, y, width);
            frame.unclip();
            for step in 0..4 {
                let from = at + soft * step as f32 / 4.0;
                frame.clip(x + from, y + row_top, soft / 4.0 + 0.5, height);
                frame.text_wrapped(text, font, bright.alpha(1.0 - (step as f32 + 0.5) / 4.0), x, y, width);
                frame.unclip();
            }
            break;
        }
        row_top += height;
    }
}

/// `colour` as a canvas colour, as opaque as `alpha`.
fn colour([r, g, b]: [f32; 3], alpha: f32) -> Color {
    Color { r, g, b, a: alpha }
}

/// `a` eased toward `b` by `t`.
fn toward(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// A colour light enough to read on the dark (its hue kept; its lightness raised to at least `least`).
fn readable(rgb: [f32; 3], least: f32) -> [f32; 3] {
    let (h, s, l) = crate::cover::hsl(rgb);
    crate::cover::rgb(h, s.min(0.85), l.max(least))
}

/// A cover's pixels blurred (three box blurs, of `radius` pixels, across then down), for behind the card.
fn blurred(pixels: &[u8], side: usize, radius: usize) -> Vec<u8> {
    let mut image: Vec<f32> = pixels.iter().map(|&b| b as f32).collect();
    let mut other = image.clone();
    for _ in 0..3 {
        box_pass(&image, &mut other, side, radius, true);
        box_pass(&other, &mut image, side, radius, false);
    }
    image.into_iter().map(|v| v.round().clamp(0.0, 255.0) as u8).collect()
}

/// One box blur of a square BGRA image, across its rows or down its columns (the edges held).
fn box_pass(from: &[f32], to: &mut [f32], side: usize, radius: usize, across: bool) {
    let at = |line: usize, i: usize| if across { (line * side + i) * 4 } else { (i * side + line) * 4 };
    let span = (2 * radius + 1) as f32;
    for line in 0..side {
        for channel in 0..4 {
            let mut sum: f32 = (0..=2 * radius).map(|k| from[at(line, (k as isize - radius as isize).clamp(0, side as isize - 1) as usize) + channel]).sum();
            for i in 0..side {
                to[at(line, i) + channel] = sum / span;
                let out = (i as isize - radius as isize).clamp(0, side as isize - 1) as usize;
                let into = (i + radius + 1).min(side - 1);
                sum += from[at(line, into) + channel] - from[at(line, out) + channel];
            }
        }
    }
}

impl Shown {
    /// Draws a frame; whether it is to be drawn again soon (something plays, or it is still settling).
    fn draw(&mut self, now: Instant) -> bool {
        let mut moment = moment();
        let step = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        // The colours, toward the cover's (a little each frame).
        if let Some(cover) = &moment.cover {
            let ease = (step * 3.0).min(1.0);
            self.tint = toward(self.tint, readable(cover.base, 0.72), ease);
            self.accent = toward(self.accent, readable(cover.accent, 0.62), ease);
        }
        // The rail, sprung toward the line sung now (as Apple Music's lyrics move: quick, then settling).
        let target = moment.now.map_or(-1.0, |now| now as f32);
        let key = moment.lines.as_ref().map(|lines| Arc::as_ptr(lines) as usize);
        if key != self.lyrics {
            self.lyrics = key;
            self.rail = target;
            self.rail_speed = 0.0;
        }
        let (stiffness, damping, mass) = (142.0, 28.0, 0.82);
        let mut left = step;
        while left > 0.0 {
            let dt = left.min(1.0 / 240.0);
            let force = -stiffness * (self.rail - target) - damping * self.rail_speed;
            self.rail_speed += force / mass * dt;
            self.rail += self.rail_speed * dt;
            left -= dt;
        }
        let settling = (self.rail - target).abs() > 0.002 || self.rail_speed.abs() > 0.002;
        // The player's controls, in toward the pointer over it and out after it.
        let controls_to = if self.over.is_some() && (!self.through || self.reachable) { 1.0 } else { 0.0 };
        // (Never past where they are going: come in, they stay in.)
        self.controls_in = if controls_to > self.controls_in { (self.controls_in + step / CONTROLS_FADE).min(controls_to) } else { (self.controls_in - step / CONTROLS_FADE).max(controls_to) };
        let coming = self.controls_in != controls_to;
        // The spectrum, falling where the sound has fallen (drawn from what it shows, not what is heard).
        for (shown, heard) in self.bands.iter_mut().zip(moment.sound.bands) {
            *shown = if heard >= *shown { heard } else { (*shown - step * SPECTRUM_FALL).max(heard) };
        }
        moment.sound.bands = self.bands;
        let falling = !moment.playing && self.bands.iter().any(|&band| band > 0.0);
        if self.surface.is_none() {
            // (What it drew with was the device's before, if it was another: made again on the new one.)
            self.gone();
            if let Ok(gfx) = gfx::current() {
                self.gfx = gfx;
            }
            self.surface = Surface::new(&self.gfx, self.hwnd).ok();
        }
        let Some(mut surface) = self.surface.take() else { return false };
        self.shown();
        let (w, h) = self.room;
        let margin = self.style.margin();
        let size = (((w + 2.0 * margin) * self.scale) as u32, ((h + 2.0 * margin) * self.scale) as u32);
        let gfx = self.gfx.clone();
        let scale = self.scale;
        let drawn = surface.draw(&gfx, size, scale, |frame| {
            frame.origin(margin, margin);
            match self.style {
                Style::Card => self.card(frame, &moment, (w, h)),
                Style::Medium => self.medium(frame, &moment, (w, h)),
                Style::Strip => self.strip(frame, &moment, (w, h)),
                Style::Subtitle => {
                    // Pointed at, where it is shows, for it to be taken and moved.
                    if self.over.is_some() && !self.through {
                        frame.fill_rounded(Color::hex(0x000000, 0.28), 0.0, 0.0, w, h, 12.0);
                    }
                    self.subtitle(frame, &moment, (w, h));
                }
            }
            self.paint_buttons(frame, (w, h));
        });
        if drawn.is_ok() {
            self.surface = Some(surface);
        } else {
            gfx::lost();
        }
        moment.playing || settling || coming || falling || self.fading.is_some()
    }

    /// The cover behind the card, blurred: made once for each cover (the one before fading out over a moment).
    fn backdrop(&mut self, frame: &Frame, cover: &Arc<Cover>) {
        let key = Arc::as_ptr(cover) as usize;
        if self.backdrop.as_ref().is_some_and(|(of, _)| *of == key) {
            return;
        }
        let side = crate::cover::SIDE as usize;
        let pixels = blurred(&cover.pixels, side, 9);
        let properties = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
            ..Default::default()
        };
        if let Ok(bitmap) = unsafe { frame.dc.CreateBitmap(D2D_SIZE_U { width: side as u32, height: side as u32 }, Some(pixels.as_ptr().cast()), side as u32 * 4, &properties) } {
            if let Some((_, before)) = self.backdrop.take() {
                self.fading = Some((before, Instant::now()));
            }
            self.backdrop = Some((key, bitmap));
        }
    }

    fn card(&mut self, frame: &Frame, moment: &Moment, (w, h): (f32, f32)) {
        let radius = 14.0;
        let pad = 26.0;
        // The cover, blurred, larger than the card; the one before fading out.
        if let Some(cover) = &moment.cover {
            self.backdrop(frame, cover);
        }
        let shown = self.fading.as_ref().map_or(1.0, |(_, since)| (since.elapsed().as_secs_f32() / 1.2).min(1.0));
        let before = self.fading.as_ref().map(|(before, _)| before.clone());
        if shown >= 1.0 {
            self.fading = None;
        }
        let backdrop = self.backdrop.as_ref().map(|(key, bitmap)| (*key, bitmap.clone()));
        // What plays, along the top.
        let about = [moment.title.as_str(), moment.artist.as_str()].into_iter().filter(|s| !s.is_empty()).collect::<Vec<_>>().join("  ·  ");
        let (accent, tint) = (self.accent, self.tint);
        // The ground drawn again only as what it shows changes: its size, the cover (and how far the one before has
        // faded), the colours (to the 255th they are drawn in: they ease toward the cover's, nearer each frame),
        // what plays, the buttons' room.
        let key = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            (w.to_bits(), h.to_bits(), self.scale.to_bits(), backdrop.as_ref().map(|(key, _)| *key), (shown * 64.0).round() as u32).hash(&mut hasher);
            accent.iter().chain(&tint).map(|v| (v * 255.0).round() as u8).collect::<Vec<_>>().hash(&mut hasher);
            about.hash(&mut hasher);
            hasher.finish()
        };
        let margin = self.style.margin();
        let ground = Ground { size: (w, h), radius, backdrop: backdrop.map(|(_, bitmap)| bitmap), before, shown, accent, tint };
        let _ = self.ground.draw(frame, key, windows_numerics::Matrix3x2::identity(), (-margin, -margin, w + 2.0 * margin, h + 2.0 * margin), self.scale, None, |frame| {
            ground.paint(frame, |frame| frame.text(&about, TITLE_FONT, Color::hex(0xFFFFFF, 0.62), pad, 16.0, w - 2.0 * pad, Align::Start))
        });
        // What moves, over it, inside the card (the lines and the spectrum keep clear of its rounded corners).
        unsafe { frame.dc.PushAxisAlignedClip(&rect(0.0, 0.0, w, h), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
        // The spectrum, along the bottom; the pointer over it, what goes back, pauses or plays, and skips, and its track,
        // in its place (as the player's).
        let floor = h - 14.0;
        self.controls.clear();
        let shown_in = self.controls_shown();
        if shown_in < 1.0 {
            spectrum(frame, moment, (self.accent, self.tint), (pad, floor, w - 2.0 * pad, 32.0), 40, 0.75 * (1.0 - shown_in));
        }
        if shown_in > 0.0 {
            let row = floor - CONTROL;
            let x = self.keys(frame, moment, pad - (CONTROL - 16.0) / 2.0, row, shown_in);
            self.progress(frame, moment, x + 10.0, w - pad, row, shown_in);
        }
        // The lines, about the place the line sung now glides to.
        let top = 44.0;
        let bottom = floor - 48.0;
        let width = w - 2.0 * pad;
        match &moment.lines {
            Some(lines) => self.rail_of_lines(frame, moment, lines, pad, top, bottom, width),
            None => {
                let said = moment.said.clone().unwrap_or_default();
                let height = frame.gfx.wrapped_height(&said, UNDER_FONT, width);
                frame.text_wrapped(&said, UNDER_FONT, Color::hex(0xFFFFFF, 0.7), pad, (top + bottom - height) / 2.0, width);
            }
        }
        unsafe { frame.dc.PopAxisAlignedClip() };
    }

    /// The cover blurred behind it, its colour washed over it (as the card's ground), and on it the cover itself at
    /// the start; beside it what plays and by whom, the line sung now filled as it is sung with what is under it,
    /// and along the bottom the spectrum, or (the pointer over it) what pauses, plays and skips it and its track.
    fn medium(&mut self, frame: &Frame, moment: &Moment, (w, h): (f32, f32)) {
        let radius = 14.0;
        let pad = 18.0;
        if let Some(cover) = &moment.cover {
            self.backdrop(frame, cover);
        }
        let shown = self.fading.as_ref().map_or(1.0, |(_, since)| (since.elapsed().as_secs_f32() / 1.2).min(1.0));
        let before = self.fading.as_ref().map(|(before, _)| before.clone());
        if shown >= 1.0 {
            self.fading = None;
        }
        let backdrop = self.backdrop.as_ref().map(|(key, bitmap)| (*key, bitmap.clone()));
        // The cover as tall as the room inside, no wider than a third of it.
        let side = (h - 2.0 * pad).min(w * 0.34).max(0.0);
        let left = pad + side + 16.0;
        let width = (w - left - pad).max(0.0);
        let (accent, tint) = (self.accent, self.tint);
        let cover = moment.cover.clone();
        let (title, about) = (moment.title.clone(), moment.artist.clone());
        // The ground, with the cover and what plays on it, drawn again only as one of them changes (see `card`).
        let key = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            (w.to_bits(), h.to_bits(), self.scale.to_bits(), backdrop.as_ref().map(|(key, _)| *key), (shown * 64.0).round() as u32).hash(&mut hasher);
            accent.iter().chain(&tint).map(|v| (v * 255.0).round() as u8).collect::<Vec<_>>().hash(&mut hasher);
            (&title, &about, cover.as_ref().map(|cover| Arc::as_ptr(cover) as usize)).hash(&mut hasher);
            hasher.finish()
        };
        let margin = self.style.margin();
        let ground = Ground { size: (w, h), radius, backdrop: backdrop.map(|(_, bitmap)| bitmap), before, shown, accent, tint };
        let _ = self.ground.draw(frame, key, windows_numerics::Matrix3x2::identity(), (-margin, -margin, w + 2.0 * margin, h + 2.0 * margin), self.scale, None, |frame| {
            ground.paint(frame, |frame| {
                let corner = (radius - pad / 2.0).max(6.0);
                frame.shadow(Color::hex(0x000000, 0.35), pad, pad, side, side, corner, 10.0, 3.0);
                match &cover {
                    Some(cover) => {
                        let picture = super::canvas::Picture { key: Arc::as_ptr(cover) as usize as u64, side: crate::cover::SIDE, pixels: cover.pixels.clone() };
                        frame.picture(&picture, pad, pad, side, corner, 1.0);
                    }
                    // None (its player gives none, or it is being read): a plate with a note on it.
                    None => {
                        frame.fill_rounded(Color::hex(0xFFFFFF, 0.08), pad, pad, side, side, corner);
                        let note = Font::new(Family::Icons, side * 0.36, 400.0);
                        let (ink_top, ink_bottom) = frame.ink("\u{E8D6}", note);
                        let wide = frame.measure("\u{E8D6}", note);
                        frame.text("\u{E8D6}", note, Color::hex(0xFFFFFF, 0.45), pad + (side - wide) / 2.0, pad + side / 2.0 - (ink_top + ink_bottom) / 2.0, wide + 2.0, Align::Start);
                    }
                }
                let room = width;
                frame.text(&fitted(frame, &title, PLAYER_TITLE, room), PLAYER_TITLE, Color::hex(0xFFFFFF, 0.95), left, pad - 2.0, room + 2.0, Align::Start);
                frame.text(&fitted(frame, &about, PLAYER_ABOUT, room), PLAYER_ABOUT, Color::hex(0xFFFFFF, 0.6), left, pad + 19.0, room + 2.0, Align::Start);
            })
        });
        // The line sung now, rising in as it comes, filled as it is sung; under it its translation or the next.
        let line_top = pad + 46.0;
        unsafe { frame.dc.PushAxisAlignedClip(&rect(left - 4.0, line_top - 4.0, width + 8.0, 48.0), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
        match (&moment.lines, moment.now) {
            (Some(lines), Some(now)) => {
                let line = &lines[now];
                let next = lines.get(now + 1).map(|line| line.at);
                let (text, parts) = parts(line, next);
                let came = moment.since.map_or(1.0, |since| (since.elapsed().as_secs_f32() / 0.28).min(1.0));
                let eased = 1.0 - (1.0 - came).powi(3);
                let y = line_top + (1.0 - eased) * 10.0;
                // (What is sung near white, the cover's colour in it, as the card's line sung now: clearly apart from what is not.)
                let (dim, bright) = (Color::hex(0xFFFFFF, 0.36 * eased), colour(toward(tint, [1.0; 3], 0.65), eased));
                let font = if text.trim().is_empty() {
                    interlude(frame, left, y, PLAYER_LINE.size, between(line, next, moment.position), dim, bright);
                    PLAYER_LINE
                } else {
                    // Too long for its room: smaller, down to the size of what is under it, then cut where it wraps.
                    let font = Font { size: (PLAYER_LINE.size * (width / frame.measure(&text, PLAYER_LINE).max(1.0))).clamp(PLAYER_UNDER.size + 1.5, PLAYER_LINE.size), ..PLAYER_LINE };
                    let layout = frame.gfx.wrapped(&text, font, width);
                    let sung = sung_to(&parts, moment.position);
                    // Its first row only (what wraps would lie over what is under it).
                    let mut rows = [DWRITE_LINE_METRICS::default()];
                    let mut count = 0;
                    let _ = unsafe { layout.GetLineMetrics(Some(&mut rows), &mut count) };
                    frame.clip(left - 4.0, y - 4.0, width + 8.0, rows[0].height + 4.0);
                    filled(frame, &layout, &text, font, left, y, width, sung, dim, bright, 12.0);
                    frame.unclip();
                    font
                };
                let under = line.translation.clone().filter(|_| moment.translated).or_else(|| lines.get(now + 1).map(|line| line.text.clone())).unwrap_or_default();
                frame.text(&fitted(frame, &under, PLAYER_UNDER, width), PLAYER_UNDER, Color::hex(0xFFFFFF, 0.5 * eased), left, line_top + font.size * 1.4 + 2.0, width + 2.0, Align::Start);
            }
            // None sung yet: a note, filled as the music before the first line goes on.
            (Some(lines), None) => {
                let first = lines.first().map_or(0.0, |line| line.at);
                let done = if first > 0.0 { (moment.position / first).clamp(0.0, 1.0) as f32 } else { 0.0 };
                interlude(frame, left, line_top, PLAYER_LINE.size, done, Color::hex(0xFFFFFF, 0.36), colour(toward(tint, [1.0; 3], 0.65), 1.0));
            }
            // None to be had: why, in their place.
            (None, _) => frame.text_wrapped(&moment.said.clone().unwrap_or_default(), PLAYER_UNDER, Color::hex(0xFFFFFF, 0.62), left, line_top + 2.0, width),
        }
        unsafe { frame.dc.PopAxisAlignedClip() };
        // Along the bottom, the spectrum moving with the music; the pointer over it, what goes back, pauses or plays,
        // and skips, its track and how long is left, in its place (each fading as the other comes; pressed only
        // while shown).
        self.controls.clear();
        let row = pad + side - CONTROL;
        let shown_in = self.controls_shown();
        if shown_in < 1.0 {
            spectrum(frame, moment, (accent, tint), (left, row + CONTROL - 2.0, width, CONTROL - 4.0), 28, 1.0 - shown_in);
        }
        if shown_in <= 0.0 {
            return;
        }
        let x = self.keys(frame, moment, left - (CONTROL - 16.0) / 2.0, row, shown_in);
        self.progress(frame, moment, x + 10.0, left + width, row, shown_in);
    }

    /// What plays's track, from `from` to `to` on `row`, with how long is left at its end, `fade` of it shown
    /// (thicker under the pointer; pressed where it is to be played from, once the controls are half come).
    fn progress(&mut self, frame: &Frame, moment: &Moment, from: f32, to: f32, row: f32, fade: f32) {
        let Some((length, seekable)) = moment.length.filter(|(length, _)| *length > 0.0) else { return };
        let pointer = self.pointer();
        let left_over = format!("-{}", super::text::clock((length - moment.position).max(0.0).round() as u32));
        let time_wide = frame.measure(&left_over, PLAYER_TIME) + 2.0;
        let track = (to - time_wide - 10.0 - from).max(0.0);
        let lit = pointer.is_some_and(|(px, py)| px >= from && px < from + track && py >= row && py < row + CONTROL);
        let middle = row + CONTROL / 2.0;
        let fraction = (moment.position / length).clamp(0.0, 1.0) as f32;
        let thick = if seekable && lit { 5.0 } else { 3.0 };
        frame.fill_rounded(Color::hex(0xFFFFFF, 0.18 * fade), from, middle - thick / 2.0, track, thick, thick / 2.0);
        frame.fill_rounded(colour(self.accent, 0.95 * fade), from, middle - thick / 2.0, track * fraction, thick, thick / 2.0);
        frame.text(&left_over, PLAYER_TIME, Color::hex(0xFFFFFF, 0.55 * fade), to - time_wide, middle - PLAYER_TIME.size * 0.7, time_wide, Align::End);
        if seekable && self.controls_in >= 0.5 {
            self.controls.push((from, row, track, CONTROL, None));
        }
    }

    /// How far the controls have come in over the spectrum (0 to 1), eased.
    fn controls_shown(&self) -> f32 {
        self.controls_in * self.controls_in * (3.0 - 2.0 * self.controls_in)
    }

    /// What goes back, pauses or plays, and skips, in a row from `x` at `row`, `fade` of them shown (each lit under
    /// the pointer, pressed only once they are half come); where the row ends.
    fn keys(&mut self, frame: &Frame, moment: &Moment, mut x: f32, row: f32, fade: f32) -> f32 {
        use crate::playing::Key;
        let collect = self.controls_in >= 0.5;
        let pointer = self.pointer();
        for (key, glyph) in [(Key::Previous, "\u{E892}"), (Key::Toggle, if moment.playing { "\u{E769}" } else { "\u{E768}" }), (Key::Next, "\u{E893}")] {
            let on = pointer.is_some_and(|(px, py)| px >= x && px < x + CONTROL && py >= row && py < row + CONTROL);
            if on {
                frame.fill_rounded(Color::hex(0xFFFFFF, 0.14 * fade), x, row, CONTROL, CONTROL, CONTROL / 2.0);
            }
            let font = if key == Key::Toggle { Font { size: 16.0, ..CONTROL_FONT } } else { CONTROL_FONT };
            let (ink_top, ink_bottom) = frame.ink(glyph, font);
            let wide = frame.measure(glyph, font);
            frame.text(glyph, font, Color::hex(0xFFFFFF, (if on { 1.0 } else { 0.85 }) * fade), x + (CONTROL - wide) / 2.0, row + CONTROL / 2.0 - (ink_top + ink_bottom) / 2.0, wide + 2.0, Align::Start);
            if collect {
                self.controls.push((x, row, CONTROL, CONTROL, Some(key)));
            }
            x += CONTROL + 2.0;
        }
        x
    }

    /// The strip: the cover small at the start, the line sung now filled as it is sung (what plays where there is
    /// none), and at the end the spectrum, or (the pointer over it) what goes back, pauses or plays, and skips,
    /// before its window's buttons.
    fn strip(&mut self, frame: &Frame, moment: &Moment, (w, h): (f32, f32)) {
        let radius = 14.0f32.min(h / 2.0);
        let pad = 10.0;
        if let Some(cover) = &moment.cover {
            self.backdrop(frame, cover);
        }
        let shown = self.fading.as_ref().map_or(1.0, |(_, since)| (since.elapsed().as_secs_f32() / 1.2).min(1.0));
        let before = self.fading.as_ref().map(|(before, _)| before.clone());
        if shown >= 1.0 {
            self.fading = None;
        }
        let backdrop = self.backdrop.as_ref().map(|(key, bitmap)| (*key, bitmap.clone()));
        let side = (h - 2.0 * pad).max(0.0);
        let left = pad + side + 12.0;
        let (accent, tint) = (self.accent, self.tint);
        let cover = moment.cover.clone();
        let key = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            (w.to_bits(), h.to_bits(), self.scale.to_bits(), backdrop.as_ref().map(|(key, _)| *key), (shown * 64.0).round() as u32).hash(&mut hasher);
            accent.iter().chain(&tint).map(|v| (v * 255.0).round() as u8).collect::<Vec<_>>().hash(&mut hasher);
            cover.as_ref().map(|cover| Arc::as_ptr(cover) as usize).hash(&mut hasher);
            hasher.finish()
        };
        let margin = self.style.margin();
        let ground = Ground { size: (w, h), radius, backdrop: backdrop.map(|(_, bitmap)| bitmap), before, shown, accent, tint };
        let _ = self.ground.draw(frame, key, windows_numerics::Matrix3x2::identity(), (-margin, -margin, w + 2.0 * margin, h + 2.0 * margin), self.scale, None, |frame| {
            ground.paint(frame, |frame| {
                let corner = 8.0f32.min(side / 4.0);
                match &cover {
                    Some(cover) => {
                        let picture = super::canvas::Picture { key: Arc::as_ptr(cover) as usize as u64, side: crate::cover::SIDE, pixels: cover.pixels.clone() };
                        frame.picture(&picture, pad, pad, side, corner, 1.0);
                    }
                    None => {
                        frame.fill_rounded(Color::hex(0xFFFFFF, 0.08), pad, pad, side, side, corner);
                        let note = Font::new(Family::Icons, side * 0.42, 400.0);
                        let (ink_top, ink_bottom) = frame.ink("\u{E8D6}", note);
                        let wide = frame.measure("\u{E8D6}", note);
                        frame.text("\u{E8D6}", note, Color::hex(0xFFFFFF, 0.45), pad + (side - wide) / 2.0, pad + side / 2.0 - (ink_top + ink_bottom) / 2.0, wide + 2.0, Align::Start);
                    }
                }
            })
        });
        // The end: the spectrum at rest; the pointer over it, the three that are pressed, before the window's buttons.
        self.controls.clear();
        let keys_wide = 3.0 * CONTROL + 4.0;
        let row = (h - CONTROL) / 2.0;
        let shown_in = self.controls_shown();
        let spectrum_wide = keys_wide.max(84.0);
        let end = w - pad;
        if shown_in < 1.0 {
            spectrum(frame, moment, (accent, tint), (w - pad - spectrum_wide, h / 2.0 + 10.0, spectrum_wide, 20.0), 14, 1.0 - shown_in);
        }
        if shown_in > 0.0 {
            self.keys(frame, moment, end - keys_wide, row, shown_in);
        }
        // The line sung now, in the room between; what plays, where none is.
        let width = (end - spectrum_wide.max(keys_wide) - 12.0 - left).max(0.0);
        unsafe { frame.dc.PushAxisAlignedClip(&rect(left - 4.0, 0.0, width + 8.0, h), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
        match (&moment.lines, moment.now) {
            (Some(lines), Some(now)) => {
                let line = &lines[now];
                let next = lines.get(now + 1).map(|line| line.at);
                let (text, parts) = parts(line, next);
                let came = moment.since.map_or(1.0, |since| (since.elapsed().as_secs_f32() / 0.28).min(1.0));
                let eased = 1.0 - (1.0 - came).powi(3);
                let (dim, bright) = (Color::hex(0xFFFFFF, 0.36 * eased), colour(toward(tint, [1.0; 3], 0.65), eased));
                if text.trim().is_empty() {
                    let y = h / 2.0 - STRIP_LINE.size * 0.7 + (1.0 - eased) * 8.0;
                    interlude(frame, left, y, STRIP_LINE.size, between(line, next, moment.position), dim, bright);
                } else {
                    let font = Font { size: (STRIP_LINE.size * (width / frame.measure(&text, STRIP_LINE).max(1.0))).clamp(STRIP_LEAST, STRIP_LINE.size), ..STRIP_LINE };
                    let y = h / 2.0 - font.size * 0.7 + (1.0 - eased) * 8.0;
                    // (One row: where it would wrap, cut where it wraps.)
                    let layout = frame.gfx.wrapped(&text, font, 100_000.0);
                    let sung = sung_to(&parts, moment.position);
                    filled(frame, &layout, &text, font, left, y, 100_000.0, sung, dim, bright, 12.0);
                }
            }
            _ => {
                let said = [moment.title.as_str(), moment.artist.as_str()].into_iter().filter(|one| !one.is_empty()).collect::<Vec<_>>().join(" · ");
                let said = if said.is_empty() { moment.said.clone().unwrap_or_default() } else { said };
                frame.text(&fitted(frame, &said, PLAYER_ABOUT, width), PLAYER_ABOUT, Color::hex(0xFFFFFF, 0.8), left, h / 2.0 - PLAYER_ABOUT.size * 0.7, width + 2.0, Align::Start);
            }
        }
        unsafe { frame.dc.PopAxisAlignedClip() };
    }

    /// The player's control at `at` (DIPs, inside), if one is there: `Some(None)`, its track.
    fn control_at(&self, (x, y): (f32, f32)) -> Option<Option<crate::playing::Key>> {
        self.controls.iter().find(|(cx, cy, cw, ch, _)| x >= *cx && x < cx + cw && y >= *cy && y < cy + ch).map(|control| control.4)
    }

    /// The player's `control` pressed at `at`: its key; its track, played from as far along it.
    fn control(&mut self, (x, _): (f32, f32), control: Option<crate::playing::Key>) {
        use crate::playing::{act, Act};
        match control {
            Some(key) => act(Act::Key(key)),
            None => {
                let Some((cx, _, cw, _, _)) = self.controls.iter().find(|control| control.4.is_none()).copied() else { return };
                let length = crate::playing::now().and_then(|now| now.timeline).map_or(0.0, |timeline| timeline.length);
                act(Act::Seek(((x - cx) / cw.max(1.0)).clamp(0.0, 1.0) as f64 * length));
            }
        }
        self.next_frame = Instant::now();
    }

    /// The lines before and after the one sung now, smaller and fainter the farther they are from it, the rail
    /// gliding; that one filled as it is sung, its translation under it.
    #[allow(clippy::too_many_arguments)]
    fn rail_of_lines(&self, frame: &Frame, moment: &Moment, lines: &[Line], x: f32, top: f32, bottom: f32, width: f32) {
        let anchor = top + (bottom - top) * 0.38;
        let current = moment.now;
        // Each line's size and how it shows, by how far it is from the rail.
        let scale_at = |d: f32| {
            let d = d.abs();
            if d <= 1.0 { 1.0 - 0.32 * d } else { (0.68 * 0.9f32.powf(d - 1.0)).clamp(0.5, 0.68) }
        };
        let opacity_at = |d: f32| {
            if d >= 0.0 {
                if d <= 1.0 { 1.0 - 0.28 * d } else { (0.72 - 0.18 * (d - 1.0)).max(0.3) }
            } else {
                let d = -d;
                if d <= 1.0 { 1.0 - 0.48 * d } else { (0.52 - 0.12 * (d - 1.0)).max(0.22) }
            }
        };
        let gap = 10.0;
        let height_of = |index: usize, scale: f32| {
            let line = &lines[index];
            frame.gfx.wrapped_height(if line.text.trim().is_empty() { "♪" } else { &line.text }, Font { size: NOW_FONT.size * scale, ..NOW_FONT }, width) + if Some(index) == current && moment.translated && line.translation.is_some() { 20.0 } else { 0.0 }
        };
        let rail = self.rail;
        let base = rail.floor().max(-1.0);
        // The lines from a few before the rail to a few after, each placed from the one the rail is nearest.
        let first = (base as isize - 4).max(0) as usize;
        let last = ((base as isize + 6).max(0) as usize).min(lines.len());
        if first >= last {
            return;
        }
        // Their places (the middle of each), laid one after another at their sizes, then shifted so the rail's
        // place is at the anchor.
        let mut middles = Vec::with_capacity(last - first);
        let mut y = 0.0;
        for index in first..last {
            let scale = scale_at(index as f32 - rail);
            let tall = height_of(index, scale);
            middles.push((index, y + tall / 2.0, scale, tall));
            y += tall + gap;
        }
        let at_rail = {
            let below = middles.iter().find(|m| m.0 as f32 >= rail).or(middles.last()).map_or(0.0, |m| m.1);
            let above = middles.iter().rev().find(|m| (m.0 as f32) <= rail).map_or(below, |m| m.1);
            let t = rail - rail.floor();
            if rail < first as f32 { below } else { above + (below - above) * t }
        };
        frame.clip(x - 8.0, top, width + 16.0, bottom - top + 12.0);
        for (index, middle, scale, tall) in middles {
            let d = index as f32 - rail;
            let y = anchor + middle - at_rail - tall / 2.0;
            if y + tall < top - 4.0 || y > bottom + 12.0 {
                continue;
            }
            // Faded toward the rail's edges.
            let edge = ((y - top) / 28.0).clamp(0.0, 1.0).min(((bottom + 12.0 - (y + tall)) / 34.0).clamp(0.0, 1.0));
            let alpha = opacity_at(d) * edge;
            let line = &lines[index];
            let font = Font { size: NOW_FONT.size * scale, ..NOW_FONT };
            if Some(index) == current {
                let next = lines.get(index + 1).map(|line| line.at);
                let (text, parts) = parts(line, next);
                let bright = colour(toward([1.0, 1.0, 1.0], self.tint, 0.35), alpha);
                let dim = Color::hex(0xFFFFFF, 0.34 * alpha);
                if text.trim().is_empty() {
                    let done = between(line, next, moment.position);
                    // What has gone glows in the cover's colour.
                    frame.glow(colour(self.tint, 0.5 * alpha), 6.0, |frame| interlude(frame, x, y, font.size, done, Color::hex(0, 0.0), bright));
                    interlude(frame, x, y, font.size, done, dim, bright);
                } else {
                    let layout = frame.gfx.wrapped(&text, font, width);
                    let sung = sung_to(&parts, moment.position);
                    let soft = (font.size * 0.45).clamp(6.0, 16.0);
                    // What is sung glows in the cover's colour.
                    frame.glow(colour(self.tint, 0.5 * alpha), 6.0, |frame| sung_part(frame, &layout, &text, font, x, y, width, sung, bright, soft));
                    filled(frame, &layout, &text, font, x, y, width, sung, dim, bright, soft);
                }
                if let Some(translation) = line.translation.as_ref().filter(|_| moment.translated) {
                    let under = frame.gfx.wrapped_height(&text, font, width);
                    frame.text(translation, UNDER_FONT, Color::hex(0xFFFFFF, 0.62 * alpha), x, y + under + 2.0, width, Align::Start);
                }
            } else {
                let shown = Color::hex(0xFFFFFF, 0.5 * alpha);
                if line.text.trim().is_empty() {
                    interlude(frame, x, y, font.size, 0.0, shown, shown);
                } else {
                    frame.text_wrapped(&line.text, font, shown, x, y, width);
                }
            }
        }
        frame.unclip();
    }

    /// The line sung now, large and glowing in the middle, filled as it is sung, rising in as it comes; under it
    /// its translation, or the next line.
    fn subtitle(&mut self, frame: &Frame, moment: &Moment, (w, h): (f32, f32)) {
        // The pointer over them (and able to press there, letting clicks through), what goes back, pauses or plays,
        // and skips, at the start of the line (its window's buttons are at its end); the words kept clear of them,
        // either way, centred as they are.
        self.controls.clear();
        let shown_in = self.controls_shown();
        let keys_wide = 3.0 * CONTROL + 4.0;
        let room = w - 20.0 - 2.0 * (keys_wide + 12.0) * shown_in;
        if shown_in > 0.0 {
            self.keys(frame, moment, 10.0, 14.0 + FLOAT_FONT.size * 0.7 - CONTROL / 2.0, shown_in);
        }
        let Some(lines) = &moment.lines else {
            if let Some(said) = &moment.said {
                let wide = frame.measure(said, FLOAT_UNDER).min(room + 20.0);
                outlined(frame, said, FLOAT_UNDER, Color::hex(0xFFFFFF, 0.85), (w - wide) / 2.0, h / 2.0 - 12.0, wide + 2.0);
            }
            return;
        };
        let Some(now) = moment.now else { return };
        let line = &lines[now];
        let next = lines.get(now + 1).map(|line| line.at);
        let (text, parts) = parts(line, next);
        // It rises in over a quarter of a second as it comes.
        let came = moment.since.map_or(1.0, |since| (since.elapsed().as_secs_f32() / 0.28).min(1.0));
        let eased = 1.0 - (1.0 - came).powi(3);
        let font = FLOAT_FONT;
        let y = 14.0 + (1.0 - eased) * 18.0;
        let (dim, bright) = (Color::hex(0xFFFFFF, 0.6 * eased), colour(self.tint, eased));
        if text.trim().is_empty() {
            let x = (w - font.size * 1.15) / 2.0;
            let done = between(line, next, moment.position);
            // A dark haze for it to be seen on anything; a glow in the cover's colour round what has gone.
            frame.glow(Color::hex(0x000000, 0.55 * eased), 3.0, |frame| interlude(frame, x, y + 1.0, font.size, 0.0, Color::hex(0xFFFFFF, 1.0), Color::hex(0xFFFFFF, 1.0)));
            frame.glow(colour(self.accent, 0.75 * eased), 10.0, |frame| interlude(frame, x, y, font.size, done, Color::hex(0, 0.0), Color::hex(0xFFFFFF, 1.0)));
            interlude(frame, x, y, font.size, done, dim, bright);
        } else {
            let wide = frame.measure(&text, font).min(room);
            let x = (w - wide) / 2.0;
            let layout = frame.gfx.wrapped(&text, font, wide + 2.0);
            let sung = sung_to(&parts, moment.position);
            let room = wide + 2.0;
            // A dark haze for it to be read on anything; a glow in the cover's colour round what is sung.
            frame.glow(Color::hex(0x000000, 0.55 * eased), 3.0, |frame| frame.text_wrapped(&text, font, Color::hex(0xFFFFFF, 1.0), x, y + 1.0, room));
            frame.glow(colour(self.accent, 0.75 * eased), 10.0, |frame| sung_part(frame, &layout, &text, font, x, y, room, sung, Color::hex(0xFFFFFF, 1.0), 14.0));
            filled(frame, &layout, &text, font, x, y, room, sung, dim, bright, 14.0);
        }
        let under = line.translation.clone().filter(|_| moment.translated).or_else(|| lines.get(now + 1).map(|line| line.text.clone())).unwrap_or_default();
        if !under.is_empty() {
            let wide = frame.measure(&under, FLOAT_UNDER).min(room + 2.0);
            outlined(frame, &under, FLOAT_UNDER, Color::hex(0xFFFFFF, 0.78 * eased), (w - wide) / 2.0, y + font.size * 1.45, wide + 2.0);
        }
    }
}

/// The spectrum moving with the music: `bars` of them across `width` from `x`, standing on `floor`, `tall` at most,
/// shaded from the accent to the tint, `alpha` of it shown.
fn spectrum(frame: &Frame, moment: &Moment, (accent, tint): ([f32; 3], [f32; 3]), (x, floor, width, tall): (f32, f32, f32, f32), bars: usize, alpha: f32) {
    let bar = width / bars as f32;
    for index in 0..bars {
        let at = index as f32 / (bars - 1) as f32 * (crate::sound::BANDS - 1) as f32;
        let (low, frac) = (at.floor() as usize, at.fract());
        let value = moment.sound.bands[low] * (1.0 - frac) + moment.sound.bands[(low + 1).min(crate::sound::BANDS - 1)] * frac;
        let height = 2.0 + value.powf(1.6) * (tall - 2.0);
        let shade = toward(accent, tint, index as f32 / bars as f32);
        frame.fill_rounded(colour(shade, (0.3 + 0.45 * value) * alpha), x + index as f32 * bar + bar * 0.2, floor - height, bar * 0.6, height, bar * 0.3);
    }
}

/// The ground of the card and of the player: its shadow, the cover blurred behind (the one before fading out) and
/// darkened, the cover's colour washed from a corner; all inside its rounded corners, with what `on` draws.
struct Ground {
    size: (f32, f32),
    radius: f32,
    backdrop: Option<ID2D1Bitmap1>,
    before: Option<ID2D1Bitmap1>,
    shown: f32,
    accent: [f32; 3],
    tint: [f32; 3],
}

impl Ground {
    fn paint(&self, frame: &Frame, on: impl FnOnce(&Frame)) -> windows::core::Result<()> {
        let ((w, h), radius) = (self.size, self.radius);
        frame.shadow(Color::hex(0x000000, 0.4), 0.0, 0.0, w, h, radius, 8.0, 4.0);
        let mask = unsafe { frame.gfx.factory.CreateRoundedRectangleGeometry(&D2D1_ROUNDED_RECT { rect: rect(0.0, 0.0, w, h), radiusX: radius, radiusY: radius }) }?;
        let layer = D2D1_LAYER_PARAMETERS1 {
            contentBounds: rect(0.0, 0.0, w, h),
            geometricMask: std::mem::ManuallyDrop::new(mask.cast().ok()),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: windows_numerics::Matrix3x2::identity(),
            opacity: 1.0,
            ..Default::default()
        };
        unsafe { frame.dc.PushLayer(&layer, None) };
        frame.fill(Color::hex(0x101014, 1.0), 0.0, 0.0, w, h);
        let grown = 1.35;
        let (bw, bh) = (w.max(h) * grown, w.max(h) * grown);
        let place = rect((w - bw) / 2.0, (h - bh) / 2.0, bw, bh);
        if let Some(bitmap) = &self.backdrop {
            if let Some(before) = &self.before {
                unsafe { frame.dc.DrawBitmap(before, Some(&place), 1.0 - self.shown, D2D1_INTERPOLATION_MODE_LINEAR, None, None) };
            }
            unsafe { frame.dc.DrawBitmap(bitmap, Some(&place), self.shown, D2D1_INTERPOLATION_MODE_LINEAR, None, None) };
        }
        // Darkened for the words to be read on, the cover's colour washed from a corner.
        frame.fill(Color::hex(0x000000, 0.42), 0.0, 0.0, w, h);
        frame.fill_shape(&[Point { x: 0.0, y: 0.0 }, Point { x: w, y: 0.0 }, Point { x: w, y: h }, Point { x: 0.0, y: h }], Fill::Down { top: 0.0, from: colour(self.accent, 0.12), bottom: h, to: colour(self.tint, 0.0) });
        on(frame);
        unsafe { frame.dc.PopLayer() };
        drop(std::mem::ManuallyDrop::into_inner(layer.geometricMask));
        Ok(())
    }
}

/// `text`, or as much of its start as is no wider than `room` with an ellipsis after it.
fn fitted(frame: &Frame, text: &str, font: Font, room: f32) -> String {
    if frame.measure(text, font) <= room {
        return text.to_string();
    }
    let mut kept: Vec<char> = text.chars().collect();
    loop {
        kept.pop();
        let cut = format!("{}…", kept.iter().collect::<String>().trim_end());
        if kept.is_empty() || frame.measure(&cut, font) <= room {
            return cut;
        }
    }
}

/// Words with a dark edge under them, to be read on whatever is behind.
fn outlined(frame: &Frame, text: &str, font: Font, colour: Color, x: f32, y: f32, width: f32) {
    for (dx, dy) in [(0.0, 1.2), (0.8, 0.8), (-0.8, 0.8)] {
        frame.text(text, font, Color::hex(0x000000, 0.4 * colour.a), x + dx, y + dy, width, Align::Start);
    }
    frame.text(text, font, colour, x, y, width, Align::Start);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lyrics::Word;

    fn line(text: &str, words: Vec<Word>) -> Line {
        Line { at: 10.0, end: None, text: text.into(), words, backing: None, translation: None }
    }

    #[test]
    fn a_line_sung_part_by_part() {
        // Word by word: as the words say.
        let worded = line("ab cd", vec![Word { at: 10.0, end: 11.0, text: "ab ".into() }, Word { at: 11.0, end: 12.0, text: "cd".into() }]);
        let (text, worded) = parts(&worded, None);
        assert_eq!(text, "ab cd");
        assert_eq!(sung_to(&worded, 10.5), 1.5);
        assert_eq!(sung_to(&worded, 11.5), 4.0);
        assert_eq!(sung_to(&worded, 20.0), 5.0);
        assert_eq!(sung_to(&worded, 9.0), 0.0);
        // By line: each Chinese character a part, the time shared.
        let (_, shared) = parts(&line("你好 hi", Vec::new()), Some(14.0));
        assert_eq!(shared.iter().map(|p| (p.0, p.1)).collect::<Vec<_>>(), [(0, 1), (1, 2), (3, 5)]);
        assert!((shared.last().unwrap().3 - (10.0 + 4.0 * 0.9)).abs() < 1e-9);
    }
}

#[cfg(test)]
#[cfg(feature = "studio")]
mod pictures {
    use super::*;
    use crate::lyrics::Word;

    /// Draws the card, the player and the floating lyrics as they would show a made-up track (its lines timed word
    /// by word, a cover of two colours, a made-up spectrum), with the pointer away and over them, into PNG files in
    /// the target folder: `cargo test --features studio -- --ignored draws_the_lyrics --nocapture`.
    #[test]
    #[ignore]
    fn draws_the_lyrics() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let worded = |at: f64, text: &str| {
            let pieces: Vec<&str> = text.split_inclusive(' ').collect();
            let words = pieces.iter().enumerate().map(|(i, piece)| Word { at: at + i as f64 * 0.4, end: at + (i + 1) as f64 * 0.4, text: piece.to_string() }).collect();
            Line { at, end: Some(at + pieces.len() as f64 * 0.4), text: text.into(), words, backing: None, translation: None }
        };
        let mut lines = vec![
            worded(10.0, "Somewhere far beyond the city lights "),
            worded(14.0, "我们 在 黑夜里 奔跑 "),
            worded(18.0, "Every heartbeat sounds like thunder "),
            worded(22.0, "直到 晨光 照亮 归途 "),
            worded(26.0, "And the morning finds us singing "),
        ];
        lines[1].translation = Some("We run through the night".into());
        let lines = Arc::new(lines);
        let now = 1;
        let position = lines[now].words[2].at + 0.2;
        let mut bands = [0.0; crate::sound::BANDS];
        for (i, band) in bands.iter_mut().enumerate() {
            *band = (0.85 - i as f32 * 0.025 + 0.15 * (i as f32 * 1.7).sin()).clamp(0.0, 1.0);
        }
        // A cover: a warm sky over a dark sea, a pale sun in it.
        let side = crate::cover::SIDE as usize;
        let mut pixels = vec![0u8; side * side * 4];
        for y in 0..side {
            for x in 0..side {
                let (fx, fy) = (x as f32 / side as f32, y as f32 / side as f32);
                let sun = (((fx - 0.62).powi(2) + (fy - 0.4).powi(2)).sqrt() < 0.12) as u8 as f32;
                let [r, g, b] = if fy < 0.6 { [0.95 - 0.3 * fy, 0.45 + 0.2 * fy, 0.35 + 0.1 * fx] } else { [0.08, 0.18 + 0.1 * fx, 0.32] };
                let at = (y * side + x) * 4;
                pixels[at] = ((b * (1.0 - sun) + 0.85 * sun) * 255.0) as u8;
                pixels[at + 1] = ((g * (1.0 - sun) + 0.95 * sun) * 255.0) as u8;
                pixels[at + 2] = ((r * (1.0 - sun) + 1.0 * sun) * 255.0) as u8;
                pixels[at + 3] = 255;
            }
        }
        let cover = Arc::new(Cover { pixels: Arc::new(pixels), base: [0.85, 0.5, 0.4], accent: [0.95, 0.6, 0.35] });
        let out = format!("{}/target/lyrics", env!("CARGO_MANIFEST_DIR"));
        std::fs::create_dir_all(&out).unwrap();
        for style in [Style::Card, Style::Medium, Style::Strip, Style::Subtitle] {
            for over in [false, true] {
                let moment = Moment {
                    lines: Some(lines.clone()),
                    now: Some(now),
                    since: Some(Instant::now() - Duration::from_secs(2)),
                    position,
                    length: Some((214.0, true)),
                    title: "Night Runners".into(),
                    artist: "Some Artist".into(),
                    said: None,
                    playing: true,
                    cover: Some(cover.clone()),
                    sound: crate::sound::Heard { level: 0.8, bands, beat: 0.3 },
                    translated: true,
                };
                let gfx = gfx::current().unwrap();
                let at = Instant::now();
                let mut shown = Shown {
                    style,
                    hwnd: HWND::default(),
                    gfx: gfx.clone(),
                    surface: None,
                    showing: None,
                    scale: 1.5,
                    room: style.size(),
                    pinned: false,
                    through: false,
                    over: over.then_some(at),
                    reachable: false,
                    hits: Vec::new(),
                    controls: Vec::new(),
                    tipped: None,
                    next_frame: at,
                    rail: now as f32,
                    rail_speed: 0.0,
                    lyrics: None,
                    last_frame: at,
                    backdrop: None,
                    fading: None,
                    tint: readable(cover.base, 0.72),
                    accent: readable(cover.accent, 0.62),
                    ground: Default::default(),
                    controls_in: if over { 1.0 } else { 0.0 },
                    bands,
                };
                let (w, h) = style.size();
                let margin = style.margin();
                let size = (((w + 2.0 * margin) * 1.5) as u32, ((h + 2.0 * margin) * 1.5) as u32);
                let pixels = gfx
                    .draw_offscreen(size, 1.5, |frame| {
                        // (On a mid grey, as a desktop might be.)
                        frame.fill(Color::hex(0x5A6470, 1.0), 0.0, 0.0, w + 2.0 * margin, h + 2.0 * margin);
                        frame.origin(margin, margin);
                        match style {
                            Style::Card => shown.card(frame, &moment, (w, h)),
                            Style::Medium => shown.medium(frame, &moment, (w, h)),
                            Style::Strip => shown.strip(frame, &moment, (w, h)),
                            Style::Subtitle => shown.subtitle(frame, &moment, (w, h)),
                        }
                        shown.paint_buttons(frame, (w, h));
                    })
                    .unwrap();
                let path = std::path::PathBuf::from(format!("{out}/{style:?}{}.png", if over { "-over" } else { "" }));
                crate::studio::save_png(&path, size.0, size.1, &pixels).unwrap();
                println!("{}", path.display());
            }
        }
    }
}
