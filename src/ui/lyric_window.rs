//! The lyrics in windows of their own, on the desktop: a card (the cover
//! blurred behind them, the lines around the one sung now gliding past it,
//! that one filled word by word as it is sung, a spectrum along the bottom
//! moving with the music), and floating lyrics (one line, glowing, over
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
    Subtitle,
}

impl Style {
    /// Its size (DIPs), and the room round it for its shadow.
    fn size(self) -> (f32, f32) {
        match self {
            Style::Card => (440.0, 330.0),
            Style::Subtitle => (980.0, 120.0),
        }
    }

    fn margin(self) -> f32 {
        match self {
            Style::Card => 30.0,
            Style::Subtitle => 0.0,
        }
    }

    /// The least it can be sized to (DIPs).
    fn least(self) -> (f32, f32) {
        match self {
            Style::Card => (320.0, 230.0),
            Style::Subtitle => (420.0, 100.0),
        }
    }

    /// The buttons it shows while the pointer is over it, in their row.
    fn buttons(self) -> &'static [Button] {
        match self {
            Style::Card => &[Button::Settings, Button::Pin, Button::Close],
            Style::Subtitle => &[Button::Settings, Button::Pin, Button::Through, Button::Close],
        }
    }

    /// Its switch among the lyrics module's items.
    fn item(self) -> &'static str {
        match self {
            Style::Card => "card",
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
        [Style::Card, Style::Subtitle].into_iter().filter(|style| on && super::prefs::item_on(view, "lyrics", style.item())).collect()
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
        (x >= 0.0 && y >= 0.0 && x < self.room.0 && y < self.room.1).then_some((x, y))
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
        let y = 10.0 + size / 2.0;
        for (place, button) in shown.iter().rev().enumerate() {
            let x = w - 12.0 - size / 2.0 - place as f32 * (size + gap);
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
        Style::Subtitle => {
            if left {
                Some(HTLEFT)
            } else if right {
                Some(HTRIGHT)
            } else {
                None
            }
        }
        Style::Card => match (left, right, top, bottom) {
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
                if at.and_then(|at| one.button_at(at)).is_some() || one.pinned {
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
                if let Some(button) = one.pointer().and_then(|at| one.button_at(at)) {
                    one.press(button);
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
            if let Some((style, room)) = with(hwnd, |one| (one.style, one.room)) {
                crate::app().change(|settings| {
                    let kept = Some((at.left, at.top));
                    match style {
                        Style::Card => {
                            settings.lyric_card = kept;
                            settings.lyric_card_size = Some(room);
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

/// A window of `style`, where it was kept (else at the bottom of the main screen: the card at its right, the
/// floating lyrics in its middle).
fn make(style: Style) -> Option<Shown> {
    let gfx = gfx::current().ok()?;
    let (kept, size, pinned, through) = {
        let settings = crate::app().settings.lock().unwrap();
        match style {
            Style::Card => (settings.lyric_card, settings.lyric_card_size, settings.lyric_card_pinned, false),
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
        let moment = moment();
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
        moment.playing || settling || self.fading.is_some()
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
        let buttons_room = if self.over.is_some() { self.style.buttons().len() as f32 * (buttons::SIZE + buttons::GAP) } else { 0.0 };
        let (accent, tint) = (self.accent, self.tint);
        // The ground drawn again only as what it shows changes: its size, the cover (and how far the one before has
        // faded), the colours (to the 255th they are drawn in: they ease toward the cover's, nearer each frame),
        // what plays, the buttons' room.
        let key = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            (w.to_bits(), h.to_bits(), self.scale.to_bits(), backdrop.as_ref().map(|(key, _)| *key), (shown * 64.0).round() as u32).hash(&mut hasher);
            accent.iter().chain(&tint).map(|v| (v * 255.0).round() as u8).collect::<Vec<_>>().hash(&mut hasher);
            (&about, buttons_room.to_bits()).hash(&mut hasher);
            hasher.finish()
        };
        let margin = self.style.margin();
        let _ = self.ground.draw(frame, key, windows_numerics::Matrix3x2::identity(), (-margin, -margin, w + 2.0 * margin, h + 2.0 * margin), self.scale, None, |frame| {
            frame.shadow(Color::hex(0x000000, 0.4), 0.0, 0.0, w, h, radius, 8.0, 4.0);
            // All of it inside its rounded corners.
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
            if let Some((_, bitmap)) = &backdrop {
                if let Some(before) = &before {
                    unsafe { frame.dc.DrawBitmap(before, Some(&place), 1.0 - shown, D2D1_INTERPOLATION_MODE_LINEAR, None, None) };
                }
                unsafe { frame.dc.DrawBitmap(bitmap, Some(&place), shown, D2D1_INTERPOLATION_MODE_LINEAR, None, None) };
            }
            // Darkened for the words to be read on, the cover's colour washed from a corner.
            frame.fill(Color::hex(0x000000, 0.42), 0.0, 0.0, w, h);
            frame.fill_shape(&[Point { x: 0.0, y: 0.0 }, Point { x: w, y: 0.0 }, Point { x: w, y: h }, Point { x: 0.0, y: h }], Fill::Down { top: 0.0, from: colour(accent, 0.12), bottom: h, to: colour(tint, 0.0) });
            frame.text(&about, TITLE_FONT, Color::hex(0xFFFFFF, 0.62), pad, 16.0, w - 2.0 * pad - buttons_room, Align::Start);
            unsafe { frame.dc.PopLayer() };
            drop(std::mem::ManuallyDrop::into_inner(layer.geometricMask));
            Ok(())
        });
        // What moves, over it, inside the card (the lines and the spectrum keep clear of its rounded corners).
        unsafe { frame.dc.PushAxisAlignedClip(&rect(0.0, 0.0, w, h), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
        // The spectrum, along the bottom.
        let bars = 40;
        let floor = h - 14.0;
        let room = w - 2.0 * pad;
        let bar = room / bars as f32;
        for index in 0..bars {
            let at = index as f32 / (bars - 1) as f32 * (crate::sound::BANDS - 1) as f32;
            let (low, frac) = (at.floor() as usize, at.fract());
            let value = moment.sound.bands[low] * (1.0 - frac) + moment.sound.bands[(low + 1).min(crate::sound::BANDS - 1)] * frac;
            let tall = 2.0 + value.powf(1.6) * 30.0;
            let shade = toward(self.accent, self.tint, index as f32 / bars as f32);
            frame.fill_rounded(colour(shade, 0.22 + 0.4 * value), pad + index as f32 * bar + bar * 0.2, floor - tall, bar * 0.6, tall, bar * 0.3);
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
            frame.gfx.wrapped_height(if line.text.is_empty() { "♪" } else { &line.text }, Font { size: NOW_FONT.size * scale, ..NOW_FONT }, width) + if Some(index) == current && moment.translated && line.translation.is_some() { 20.0 } else { 0.0 }
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
                let text = if text.trim().is_empty() { "♪".to_string() } else { text };
                let layout = frame.gfx.wrapped(&text, font, width);
                let sung = sung_to(&parts, moment.position);
                let soft = (font.size * 0.45).clamp(6.0, 16.0);
                let bright = colour(toward([1.0, 1.0, 1.0], self.tint, 0.35), alpha);
                // What is sung glows in the cover's colour.
                frame.glow(colour(self.tint, 0.5 * alpha), 6.0, |frame| sung_part(frame, &layout, &text, font, x, y, width, sung, bright, soft));
                filled(frame, &layout, &text, font, x, y, width, sung, Color::hex(0xFFFFFF, 0.34 * alpha), bright, soft);
                if let Some(translation) = line.translation.as_ref().filter(|_| moment.translated) {
                    let under = frame.gfx.wrapped_height(&text, font, width);
                    frame.text(translation, UNDER_FONT, Color::hex(0xFFFFFF, 0.62 * alpha), x, y + under + 2.0, width, Align::Start);
                }
            } else {
                let text = if line.text.is_empty() { "♪" } else { line.text.as_str() };
                frame.text_wrapped(text, font, Color::hex(0xFFFFFF, 0.5 * alpha), x, y, width);
            }
        }
        frame.unclip();
    }

    /// The line sung now, large and glowing in the middle, filled as it is sung, rising in as it comes; under it
    /// its translation, or the next line.
    fn subtitle(&mut self, frame: &Frame, moment: &Moment, (w, h): (f32, f32)) {
        let Some(lines) = &moment.lines else {
            if let Some(said) = &moment.said {
                let wide = frame.measure(said, FLOAT_UNDER).min(w);
                outlined(frame, said, FLOAT_UNDER, Color::hex(0xFFFFFF, 0.85), (w - wide) / 2.0, h / 2.0 - 12.0, wide + 2.0);
            }
            return;
        };
        let Some(now) = moment.now else { return };
        let line = &lines[now];
        let (text, parts) = parts(line, lines.get(now + 1).map(|line| line.at));
        let text = if text.trim().is_empty() { "♪".to_string() } else { text };
        // It rises in over a quarter of a second as it comes.
        let came = moment.since.map_or(1.0, |since| (since.elapsed().as_secs_f32() / 0.28).min(1.0));
        let eased = 1.0 - (1.0 - came).powi(3);
        let font = FLOAT_FONT;
        let wide = frame.measure(&text, font).min(w - 20.0);
        let (x, y) = ((w - wide) / 2.0, 14.0 + (1.0 - eased) * 18.0);
        let layout = frame.gfx.wrapped(&text, font, wide + 2.0);
        let sung = sung_to(&parts, moment.position);
        let room = wide + 2.0;
        // A dark haze for it to be read on anything; a glow in the cover's colour round what is sung.
        frame.glow(Color::hex(0x000000, 0.55 * eased), 3.0, |frame| frame.text_wrapped(&text, font, Color::hex(0xFFFFFF, 1.0), x, y + 1.0, room));
        frame.glow(colour(self.accent, 0.75 * eased), 10.0, |frame| sung_part(frame, &layout, &text, font, x, y, room, sung, Color::hex(0xFFFFFF, 1.0), 14.0));
        filled(frame, &layout, &text, font, x, y, room, sung, Color::hex(0xFFFFFF, 0.6 * eased), colour(self.tint, eased), 14.0);
        let under = line.translation.clone().filter(|_| moment.translated).or_else(|| lines.get(now + 1).map(|line| line.text.clone())).unwrap_or_default();
        if !under.is_empty() {
            let wide = frame.measure(&under, FLOAT_UNDER).min(w - 20.0);
            outlined(frame, &under, FLOAT_UNDER, Color::hex(0xFFFFFF, 0.78 * eased), (w - wide) / 2.0, y + font.size * 1.45, wide + 2.0);
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

    /// Draws the card and the floating lyrics as they would show a known track (its words timed word by word, the
    /// cover of what this PC plays, a made-up spectrum), into raw files beside the target folder:
    /// `cargo test -- --ignored draws_the_lyrics --nocapture`.
    #[test]
    #[ignore]
    fn draws_the_lyrics() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        eprintln!("asking for the lyrics");
        let lines = Arc::new(crate::lyrics::amll(1901371647).expect("lyrics"));
        eprintln!("lyrics: {} lines", lines.len());
        let now = lines.iter().position(|line| line.words.len() > 4).unwrap() + 3;
        let line = &lines[now];
        let position = line.words[line.words.len() / 2].at + 0.1;
        let mut bands = [0.0; crate::sound::BANDS];
        for (i, band) in bands.iter_mut().enumerate() {
            *band = (0.85 - i as f32 * 0.025 + 0.15 * (i as f32 * 1.7).sin()).clamp(0.0, 1.0);
        }
        let cover = crate::cover::read().map(Arc::new);
        eprintln!("cover read: {}", cover.is_some());
        for style in [Style::Card, Style::Subtitle] {
            let moment = Moment {
                lines: Some(lines.clone()),
                now: Some(now),
                since: Some(Instant::now() - Duration::from_secs(2)),
                position,
                title: "孤勇者".into(),
                artist: "陈奕迅".into(),
                said: None,
                playing: true,
                cover: cover.clone(),
                sound: crate::sound::Heard { level: 0.8, bands, beat: 0.3 },
                translated: true,
            };
            let gfx = gfx::current().unwrap();
            let mut shown = Shown {
                style,
                hwnd: HWND::default(),
                gfx: gfx.clone(),
                surface: None,
                scale: 1.5,
                next_frame: Instant::now(),
                rail: now as f32,
                rail_speed: 0.0,
                lyrics: None,
                last_frame: Instant::now(),
                backdrop: None,
                fading: None,
                tint: cover.as_ref().map_or([1.0; 3], |c| readable(c.base, 0.72)),
                accent: cover.as_ref().map_or([0.5, 0.6, 1.0], |c| readable(c.accent, 0.62)),
            };
            let (w, h) = style.size();
            let margin = style.margin();
            let size = (((w + 2.0 * margin) * 1.5) as u32, ((h + 2.0 * margin) * 1.5) as u32);
            eprintln!("drawing {style:?}");
            let pixels = gfx
                .draw_offscreen(size, 1.5, |frame| {
                    // (On a mid grey, as a desktop might be.)
                    frame.fill(Color::hex(0x5A6470, 1.0), 0.0, 0.0, w + 2.0 * margin, h + 2.0 * margin);
                    frame.origin(margin, margin);
                    match style {
                        Style::Card => shown.card(frame, &moment, (w, h)),
                        Style::Subtitle => shown.subtitle(frame, &moment, (w, h)),
                    }
                })
                .unwrap();
            let out = format!("{}/target/lyrics-{:?}.raw", env!("CARGO_MANIFEST_DIR"), style);
            let mut file = size.0.to_le_bytes().to_vec();
            file.extend(size.1.to_le_bytes());
            file.extend(pixels);
            std::fs::write(&out, file).unwrap();
            println!("{out}");
        }
    }
}
