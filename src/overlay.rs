//! The overlay's window: never taking the focus. Unlocked, it is dragged
//! where it is to stay; locked, it stays. Either way a right click on it
//! offers the lock, turning it off and the settings (so it takes the clicks that
//! land on it: a game holding the pointer is not one of them), and the
//! pointer over it brings a row of buttons for the same, as a widget's are.
//! Letting clicks through, it takes none: the pointer staying over it a
//! while brings the one button that ends that. Drawn by the panel's thread
//! at every sample.

use std::rc::Rc;
use std::time::Instant;

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, HMONITOR, MONITORINFO, MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTOPRIMARY};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, ReleaseCapture, SetCapture, VK_LBUTTON, VK_RBUTTON};
use windows::Win32::UI::WindowsAndMessaging::{GetCursorPos, GetSystemMetrics, KillTimer, SetTimer, SM_SWAPBUTTON, IDC_ARROW};

use std::collections::VecDeque;

use crate::reading::Sample;
use crate::settings::OverlaySettings;
use crate::ui::backdrop::Capture;
use crate::ui::buttons::{self, Button};
use crate::ui::canvas::{Color, Point};
use crate::ui::gfx::{self, Gfx, Surface};
use crate::ui::glass::Frosted;
use crate::ui::overlay::{self, Glass, Hand, Layout, Metrics, Shape, Tone, INSET, MARGIN};
use crate::ui::text::Lang;
use crate::ui::window::Window;


/// What its right-click menu offers.
pub enum Choice {
    Lock(bool),
    Through(bool),
    Close,
    Settings,
}

pub struct Overlay {
    gfx: Rc<Gfx>,
    /// Where it was last placed, with its shadow (physical px).
    placed: std::cell::Cell<RECT>,
    pub window: Window,
    /// Its content: the glass and what is on it.
    surface: Frosted,
    /// Its shadow, in a window of its own just beneath it that every click
    /// passes through, and that window's content (none after the device
    /// was lost, until it is made again on the new one).
    shade: Window,
    shade_surface: Option<(Rc<Gfx>, Surface)>,
    shown: bool,
    locked: bool,
    /// How clear its glass is (see `OverlaySettings::clear`), and whether
    /// clicks pass through it.
    clear: f32,
    through: bool,
    /// Its buttons, in a window of their own beside it: those that show
    /// now (none: the window is away), and its content.
    bar: Window,
    bar_surface: Option<(Rc<Gfx>, Surface)>,
    bar_shows: Vec<Button>,
    /// Since when the pointer has been over it or its buttons.
    over: Option<Instant>,
    /// Whether a game shows (its button lit), as the last sample had it:
    /// (the samples are held as it is drawn: not asked for again there).
    game: bool,
    /// Where its buttons' window was last put (none: away).
    bar_at: Option<RECT>,
    /// The button the pointer is on, said under it.
    tip: Option<Button>,
    /// How clear its glass was just made: said on it until it has been
    /// left so for a moment (see `SAID_MS`).
    said: Option<f32>,
    lang: Lang,
    /// Where its glass is on screen (physical pixels).
    rect: RECT,
    /// The screen it is on.
    screen: RECT,
    /// Its screen's scale, and the size it is drawn at (1 as designed).
    dpi: f32,
    size: f32,
    /// What it shows, the frame rate's recent course, and how: drawn again
    /// as it is dragged.
    drawn: (Vec<overlay::Reading>, Vec<Option<f32>>, Shape),
    /// How its glass is tinted, as the screen around it last asked.
    glass: Option<Glass>,
    /// Being dragged by the pointer: to move it, or by its edges, to size it.
    grab: Option<Grab>,
    /// The screen of the game played, kept while it presents no frames
    /// for a moment.
    game_screen: Option<[i32; 4]>,
    /// The panel's window while the panel is up: kept above the overlay.
    beneath: Option<HWND>,
}

/// A drag under way: where the pointer, the overlay and its size were as it
/// began, and which edges it holds (none: the whole overlay, to move it).
#[derive(Clone, Copy)]
struct Grab {
    from: POINT,
    /// The glass's place on screen as the drag began.
    glass: RECT,
    size: f32,
    edges: Edges,
}

#[derive(Clone, Copy, Default, PartialEq)]
struct Edges {
    left: bool,
    right: bool,
    top: bool,
    bottom: bool,
}

impl Edges {
    fn any(self) -> bool {
        self.left || self.right || self.top || self.bottom
    }
}

/// How near its edge the pointer sizes it rather than moves it (DIPs).
const EDGE: f32 = 8.0;

/// How far around the overlay's shadow the screen is looked at, for how to
/// tint the glass (DIPs): what is behind it, blurred, is much like what is
/// around it.
const AROUND: f32 = 16.0;

/// While it is dragged, the pointer is followed this often (ms) by a timer
/// of this id: never in front, the overlay is told of the pointer only
/// while it is over it, and a quick drag leaves it.
pub const FOLLOW_TIMER: usize = 1;
const FOLLOW_MS: u32 = 8;

/// Where it is moved to, and how large it is, as a drag left it.
pub struct Placed {
    /// See `OverlaySettings::at`.
    pub at: (f32, f32),
    /// A point on the screen it is over.
    pub point: (i32, i32),
    pub size: f32,
}

/// A monitor's whole area, and its scale.
fn monitor_info(monitor: HMONITOR) -> (RECT, f32) {
    let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    let _ = unsafe { GetMonitorInfoW(monitor, &mut info) };
    let (mut dpi, mut unused) = (96u32, 96u32);
    let _ = unsafe { GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut unused) };
    (info.rcMonitor, dpi as f32 / 96.0)
}

/// The room glass `size` large has on `screen`, inside the inset: its
/// left, top, and how far it can move across and down.
fn room(screen: RECT, size: (i32, i32), scale: f32) -> (i32, i32, i32, i32) {
    let inset = (INSET * scale).round() as i32;
    let across = (screen.right - screen.left - 2 * inset - size.0).max(0);
    let down = (screen.bottom - screen.top - 2 * inset - size.1).max(0);
    (screen.left + inset, screen.top + inset, across, down)
}

/// Whether the primary button is down (the left one, or the right where
/// they are swapped): the one a drag is made with.
pub fn primary_down() -> bool {
    let button = if unsafe { GetSystemMetrics(SM_SWAPBUTTON) } != 0 { VK_RBUTTON } else { VK_LBUTTON };
    (unsafe { GetAsyncKeyState(button.0 as i32) }) < 0
}

/// The timer that ends the overlay's saying how clear its glass was made,
/// and how long after it was last changed (as the panel's size is said
/// only while it is being set).
pub const SAID_TIMER: usize = 7;
const SAID_MS: u32 = 600;
/// How near a corner the pointer sizes it by both edges there (DIPs): no
/// nearer than half its shorter side lets.
const CORNER: f32 = 16.0;

/// The room round the buttons in their window, and between it and the glass (DIPs).
const BAR_PAD: f32 = 3.0;
const BAR_GAP: f32 = 3.0;

fn cursor() -> Option<POINT> {
    let mut cursor = POINT::default();
    unsafe { GetCursorPos(&mut cursor) }.ok().map(|_| cursor)
}

impl Overlay {
    pub fn new() -> windows::core::Result<Self> {
        let gfx = gfx::current()?;
        let window = Window::see_through()?;
        let mut shade = match Window::new() {
            Ok(shade) => shade,
            Err(error) => {
                window.destroy();
                return Err(error);
            }
        };
        shade.set_click_through(true);
        let bar = match Window::new() {
            Ok(bar) => bar,
            Err(error) => {
                window.destroy();
                shade.destroy();
                return Err(error);
            }
        };
        let surface = match Frosted::new(&gfx, window.hwnd) {
            Ok(surface) => surface,
            Err(error) => {
                // Tried again at the next sample, with windows of its own.
                window.destroy();
                shade.destroy();
                bar.destroy();
                return Err(error);
            }
        };
        Ok(Overlay {
            gfx,
            window,
            surface,
            shade,
            shade_surface: None,
            shown: false,
            placed: std::cell::Cell::new(RECT::default()),
            locked: false,
            clear: 0.0,
            through: false,
            bar,
            bar_surface: None,
            bar_shows: Vec::new(),
            over: None,
            game: false,
            bar_at: None,
            tip: None,
            said: None,
            lang: Lang::En,
            rect: RECT::default(),
            screen: RECT::default(),
            dpi: 1.0,
            size: 1.0,
            drawn: (Vec::new(), Vec::new(), Shape { layout: Layout::Card, rows: 1, width: f32::INFINITY }),
            glass: None,
            grab: None,
            game_screen: None,
            beneath: None,
        })
    }

    /// Shows the readings of the latest sample in `history` as `settings`
    /// ask, while `wanted`: while a game is played (`playing`), with its
    /// frames, on its screen; otherwise on the screen it was put on. Hidden
    /// while not wanted, and while there is nothing to show. Above all but
    /// `beneath` (the open panel's window).
    pub fn show(&mut self, history: &VecDeque<Sample>, settings: &OverlaySettings, lang: Lang, playing: bool, wanted: bool, beneath: Option<HWND>) {
        self.locked = settings.locked;
        let clear = settings.clear.clamp(0.0, 1.0);
        if self.shown && clear != self.clear {
            self.said = Some(clear);
            unsafe { SetTimer(Some(self.window.hwnd), SAID_TIMER, SAID_MS, None) };
        }
        self.clear = clear;
        self.through = settings.click_through;
        self.window.set_click_through(self.through);
        self.game = crate::widget::game_shown(history.back().map(std::slice::from_ref).unwrap_or_default());
        self.beneath = beneath;
        self.lang = lang;
        let sample = history.back();
        // The game played: one found, or one taken for a game by hand.
        let game = sample.and_then(|s| s.game.as_ref()).filter(|game| playing || game.by_hand);
        // (One taken by hand does not bring the overlay up: shown where it is up, its frames are.)
        let framed = playing || game.is_some();
        let readings = sample.filter(|_| wanted).map(|s| overlay::readings(s, game, framed, &settings.chosen(), lang)).unwrap_or_default();
        if readings.is_empty() {
            return self.hide();
        }
        let frames = overlay::frames(history.iter(), framed);
        // Held as the pointer has it while it is dragged.
        if self.grab.is_some() {
            self.drawn = (readings, frames, self.drawn.2);
            let (width, height) = self.dims();
            let r = self.rect;
            self.rect = RECT { left: r.left, top: r.top, right: r.left + width, bottom: r.top + height };
            return self.draw();
        }
        // On the screen it was put on; never put anywhere, over a game, the
        // game's screen.
        if let Some(screen) = game.and_then(|game| game.screen) {
            self.game_screen = Some(screen);
        }
        if !playing {
            self.game_screen = None;
        }
        let game_point = self.game_screen.map(|[left, top, right, bottom]| ((left + right) / 2, (top + bottom) / 2));
        let point = settings.screen.or(game_point);
        let monitor = match point {
            Some((x, y)) => unsafe { MonitorFromPoint(POINT { x, y }, MONITOR_DEFAULTTONEAREST) },
            None => unsafe { MonitorFromPoint(POINT::default(), MONITOR_DEFAULTTOPRIMARY) },
        };
        (self.screen, self.dpi) = monitor_info(monitor);
        self.size = settings.size.clamp(crate::panel::SIZES.0, crate::panel::SIZES.1);
        // As wide as its screen at most, inside the inset (DIPs at its size).
        let widest = (self.screen.right - self.screen.left) as f32 / self.scale() - 2.0 * INSET / self.size;
        self.drawn = (readings, frames, settings.shape(widest));
        let size = self.dims();
        let (left, top, across, down) = room(self.screen, size, self.dpi);
        let x = left + (settings.at.0.clamp(0.0, 1.0) * across as f32).round() as i32;
        let y = top + (settings.at.1.clamp(0.0, 1.0) * down as f32).round() as i32;
        self.rect = RECT { left: x, top: y, right: x + size.0, bottom: y + size.1 };
        self.look();
        self.draw();
    }

    /// The scale it is drawn at: its screen's, by its size.
    fn scale(&self) -> f32 {
        self.dpi * self.size
    }

    /// Its shadow's margin (physical pixels).
    fn margin(&self) -> i32 {
        (MARGIN * self.scale()).round() as i32
    }

    /// Where its shadow's window is: around the glass by the margin.
    fn shadow_rect(&self) -> RECT {
        let (m, r) = (self.margin(), self.rect);
        RECT { left: r.left - m, top: r.top - m, right: r.right + m, bottom: r.bottom + m }
    }

    /// The glass's width and height on screen, at its size (physical
    /// pixels), and its corners' radius.
    fn measures(&self) -> (i32, i32, f32) {
        let scale = self.scale();
        let (readings, _, shape) = &self.drawn;
        let width = |text: &str, font| self.gfx.measure(text, font);
        let line = |font| self.gfx.baseline(font);
        let metrics = Metrics { width: &width, line: &line };
        // Whole pixels already: rounded, not lifted a pixel by a float's error.
        let (width, height) = overlay::size(readings, *shape, scale, &metrics);
        ((width * scale).round() as i32, (height * scale).round() as i32, overlay::radius(readings, *shape, scale, &metrics) * scale)
    }

    fn dims(&self) -> (i32, i32) {
        let (width, height, _) = self.measures();
        (width, height)
    }

    /// Looks at the screen around it, and tints the glass for it (see
    /// `overlay::glass`); unseen (the lock screen has the display, or none
    /// of its screen is around it), for anything.
    fn look(&mut self) {
        let around = (AROUND * self.dpi).round() as i32;
        let (r, s) = (self.shadow_rect(), self.screen);
        // On its screen only: past the screen's edge is nothing.
        let area = RECT { left: (r.left - around).max(s.left), top: (r.top - around).max(s.top), right: (r.right + around).min(s.right), bottom: (r.bottom + around).min(s.bottom) };
        let seen = Capture::take(area);
        let mut behind = seen.map(|capture| capture.luminances(area, r, self.dpi.round().max(1.0) as i32)).unwrap_or_default();
        self.glass = Some(overlay::glass(&mut behind, self.glass));
    }

    /// Places its windows: the glass's where it is, above all but the open
    /// panel; its shadow's just beneath it.
    fn place(&self) {
        self.window.place_under(self.rect, self.beneath);
        self.shade.place_under(self.shadow_rect(), Some(self.window.hwnd));
        // Where it was and where it is: a widget that took the desktop with
        // it there takes it again.
        let now = self.shadow_rect();
        let was = self.placed.replace(now);
        if was != now {
            crate::widget::own_window_changed(was);
            crate::widget::own_window_changed(now);
        }
    }

    /// Draws what it shows where it is, set off in colour while it is dragged.
    fn draw(&mut self) {
        // This thread's device: a new one after the last was lost.
        let Ok(gfx) = gfx::current() else { return };
        self.gfx = gfx;
        // Placed above all again each time (a game may have taken the top),
        // but for the open panel.
        self.place();
        let scale = self.scale();
        let glass = Glass { presence: 1.0 - self.clear, ..self.glass.unwrap_or_else(|| overlay::glass(&mut [], None)) };
        let (width, height, radius) = self.measures();
        let (readings, frames, shape) = (&self.drawn.0, &self.drawn.1, self.drawn.2);
        let dragged = self.grab.is_some();
        let hand = if dragged { Hand::Dragged } else { Hand::Free };
        // Its glass is its tint alone: what is behind it is not blurred (the system's compositor would then draw
        // the game behind into it every frame, some frames in a hundred of the game's).
        self.surface.set_frost(0.0);
        let said = self.said.map(|clear| (if self.lang == Lang::Zh { "透明度" } else { "Clarity" }, format!("{:.0}", clear * 100.0)));
        let painted = self.surface.draw(&self.gfx, (width as u32, height as u32), scale, (0.0, 0.0, width as f32, height as f32, radius), |frame| {
            frame.crisp_text();
            overlay::paint(frame, readings, frames, shape, glass, hand, scale);
            if let Some((name, figure)) = &said {
                overlay::say(frame, (name, figure, "%"), (width as f32 / scale, height as f32 / scale), Glass { presence: 1.0, ..glass });
            }
        });
        // Its shadow, on a surface made on this device.
        let shade_surface = match self.shade_surface.take() {
            Some((made_on, surface)) if Rc::ptr_eq(&made_on, &self.gfx) => Ok(surface),
            _ => Surface::new(&self.gfx, self.shade.hwnd),
        };
        let (r, m) = (self.shadow_rect(), self.margin() as f32 / scale);
        let shaded = match shade_surface {
            Ok(mut surface) => {
                let drawn = surface.draw(&self.gfx, ((r.right - r.left) as u32, (r.bottom - r.top) as u32), scale, |frame| {
                    frame.origin(m, m);
                    overlay::shadow(frame, readings, shape, glass, dragged, scale);
                });
                self.shade_surface = Some((self.gfx.clone(), surface));
                drawn
            }
            Err(error) => Err(error),
        };
        if painted.is_err() || shaded.is_err() {
            // Made again on a new device at the next sample.
            gfx::lost();
            return;
        }
        self.gfx.sweep();
        if !self.shown {
            self.window.show();
            self.shade.show();
            self.place();
            self.shown = true;
        }
        // Its buttons, lit as it is now; and what one of them says, over it still.
        self.draw_bar();
        crate::ui::tip::above();
    }

    /// Where each of its buttons' middles is on the screen, whether it
    /// shows or not (physical pixels): a row ending at the glass's right
    /// edge, above it, or under it where its screen has no room above.
    fn bar_middles(&self) -> [POINT; 5] {
        let px = |dips: f32| (dips * self.dpi).round() as i32;
        let (tall, r) = (px(buttons::SIZE + 2.0 * BAR_PAD), self.rect);
        let top = if r.top - px(BAR_GAP) - tall < self.screen.top { r.bottom + px(BAR_GAP) } else { r.top - px(BAR_GAP) - tall };
        let count = buttons::ALL.len();
        std::array::from_fn(|place| POINT { x: r.right - px(BAR_PAD + buttons::SIZE / 2.0 + (count - 1 - place) as f32 * (buttons::SIZE + buttons::GAP)), y: top + tall / 2 })
    }

    /// The buttons' window: round those that show.
    fn bar_rect(&self) -> RECT {
        let half = ((buttons::SIZE / 2.0 + BAR_PAD) * self.dpi).round() as i32;
        let middles = self.bar_middles();
        let shown = |button: &Button| middles[buttons::ALL.iter().position(|one| one == button).unwrap()];
        let (first, last) = (self.bar_shows.first().map_or(middles[0], shown), self.bar_shows.last().map_or(middles[4], shown));
        RECT { left: first.x - half, top: first.y - half, right: last.x + half, bottom: last.y + half }
    }

    /// The button of those showing that a point of the screen is on.
    fn bar_button_at(&self, at: POINT) -> Option<Button> {
        let reach = (buttons::SIZE / 2.0 + 2.0) * self.dpi;
        let middles = self.bar_middles();
        self.bar_shows.iter().copied().find(|button| {
            let middle = middles[buttons::ALL.iter().position(|one| one == button).unwrap()];
            ((middle.x - at.x) as f32).hypot((middle.y - at.y) as f32) <= reach
        })
    }

    /// Whether `hwnd` is its buttons' window.
    pub fn owns_bar(&self, hwnd: HWND) -> bool {
        self.bar.hwnd == hwnd
    }

    /// Draws the buttons that show, in their window where it belongs; none, puts it away.
    fn draw_bar(&mut self) {
        if self.bar_shows.is_empty() || !self.shown {
            if let Some((gfx, surface)) = &mut self.bar_surface {
                surface.release(gfx);
            }
            self.bar.hide();
            // (A widget that took the desktop with them there takes it again.)
            if let Some(was) = self.bar_at.take() {
                crate::widget::own_window_changed(was);
            }
            return;
        }
        let rect = self.bar_rect();
        let surface = match self.bar_surface.take() {
            Some((made_on, surface)) if Rc::ptr_eq(&made_on, &self.gfx) => Ok(surface),
            _ => Surface::new(&self.gfx, self.bar.hwnd),
        };
        let Ok(mut surface) = surface else { return };
        let dark = self.glass.is_none_or(|glass| glass.tone == Tone::Dark);
        let rim = if dark { Color::hex(0xFFFFFF, 0.3) } else { Color::hex(0x000000, 0.3) };
        let (shows, locked, through, game) = (&self.bar_shows, self.locked, self.through, self.game);
        let drawn = surface.draw(&self.gfx, ((rect.right - rect.left) as u32, (rect.bottom - rect.top) as u32), self.dpi, |frame| {
            for (place, button) in shows.iter().enumerate() {
                let lit = (*button == Button::Pin && locked) || (*button == Button::Through && through) || (*button == Button::Game && game);
                let centre = Point { x: BAR_PAD + buttons::SIZE / 2.0 + place as f32 * (buttons::SIZE + buttons::GAP), y: BAR_PAD + buttons::SIZE / 2.0 };
                buttons::paint(frame, centre, *button, lit, dark, rim);
            }
        });
        self.bar_surface = Some((self.gfx.clone(), surface));
        if drawn.is_err() {
            gfx::lost();
            return;
        }
        self.bar.place_under(rect, self.beneath);
        self.bar.show();
        crate::ui::tip::above();
        if self.bar_at.replace(rect) != Some(rect) {
            crate::widget::own_window_changed(rect);
        }
    }

    /// Looks for the pointer (letting clicks through, it hears nothing of
    /// it): over it or its buttons, they show; while it lets clicks
    /// through, only the one that ends that, and once the pointer has
    /// stayed a while. Called as often as the pointer is looked at.
    pub fn watch(&mut self) {
        let now = Instant::now();
        // (The glass, the whole row and what lies between them.)
        let reach = {
            let half = ((buttons::SIZE / 2.0 + BAR_PAD) * self.dpi).round() as i32;
            let (middles, r) = (self.bar_middles(), self.rect);
            RECT { left: r.left.min(middles[0].x - half), top: r.top.min(middles[0].y - half), right: r.right, bottom: r.bottom.max(middles[0].y + half) }
        };
        let inside = self.shown && self.grab.is_none() && cursor().is_some_and(|c| (reach.left..reach.right).contains(&c.x) && (reach.top..reach.bottom).contains(&c.y));
        self.over = if inside { self.over.or(Some(now)) } else { None };
        let shows = match (self.over, self.through) {
            (None, _) => Vec::new(),
            (Some(_), false) => buttons::ALL.to_vec(),
            (Some(since), true) if now.duration_since(since) >= buttons::LINGER => vec![Button::Through],
            (Some(_), true) => Vec::new(),
        };
        if shows != self.bar_shows {
            self.bar_shows = shows;
            self.draw_bar();
        }
        if !inside && self.tip.take().is_some() {
            crate::ui::tip::hide();
        }
    }

    /// The pointer moved over its buttons: what the one under it does, said under it.
    pub fn bar_hover(&mut self) {
        let Some(at) = cursor() else { return };
        let over = self.bar_button_at(at);
        self.bar.point(if over.is_some() { windows::Win32::UI::WindowsAndMessaging::IDC_HAND } else { IDC_ARROW });
        if over == self.tip {
            return;
        }
        self.tip = over;
        match over {
            Some(button) => {
                let lit = match button {
                    Button::Pin => self.locked,
                    Button::Through => self.through,
                    Button::Game => self.game,
                    _ => false,
                };
                // (Under the button itself: not wherever the pointer came onto it.)
                let half = (buttons::SIZE / 2.0 * self.dpi).round() as i32;
                let middle = self.bar_middles()[buttons::ALL.iter().position(|one| *one == button).unwrap()];
                let about = RECT { left: middle.x - half, top: middle.y - half, right: middle.x + half, bottom: middle.y + half };
                crate::ui::tip::show(button.says(lit, self.lang, true), about, crate::os::apps_dark());
            }
            None => crate::ui::tip::hide(),
        }
    }

    /// One of its buttons pressed: what is asked for, where it is another's to do.
    pub fn bar_press(&mut self) -> Option<Choice> {
        let button = self.bar_button_at(cursor()?)?;
        if self.tip.take().is_some() {
            crate::ui::tip::hide();
        }
        // (Its buttons come again as the pointer stays: at once, or, letting clicks through, after a while.)
        self.over = None;
        match button {
            Button::Game => {
                // Showing a game (found or asked for): off; else on.
                let shown = self.game;
                crate::presents::set_game_mode(if shown { crate::presents::GameMode::Off } else { crate::presents::GameMode::On });
                None
            }
            Button::Settings => Some(Choice::Settings),
            Button::Pin => Some(Choice::Lock(!self.locked)),
            Button::Through => Some(Choice::Through(!self.through)),
            Button::Close => Some(Choice::Close),
        }
    }

    /// The edges of it the pointer at `at` is on, if it is near them.
    fn edges_at(&self, at: POINT) -> Edges {
        let near = (EDGE * self.dpi).round() as i32;
        let r = self.rect;
        let corner = ((CORNER * self.dpi).round() as i32).min((r.right - r.left).min(r.bottom - r.top) / 2).max(near);
        // How far in from each edge; near one, or in a corner, near both
        // of the corner's.
        let (left, right, top, bottom) = (at.x - r.left, r.right - 1 - at.x, at.y - r.top, r.bottom - 1 - at.y);
        let on = |own: i32, a: i32, b: i32| own < near || (own < corner && (a < corner || b < corner));
        Edges { left: on(left, top, bottom), right: on(right, top, bottom), top: on(top, left, right), bottom: on(bottom, left, right) }
    }

    /// The pointer's shape over edges `edges` (none: the whole overlay).
    fn point_at(&self, edges: Edges) {
        self.window.point(crate::panel::sizing(edges.left, edges.right, edges.top, edges.bottom));
    }

    /// The left button went down on it: unless it is locked, it is moved, or
    /// if the pointer is on its edges, sized, until it is let go.
    pub fn press(&mut self) {
        let Some(from) = cursor().filter(|_| !self.locked) else { return };
        let edges = self.edges_at(from);
        self.grab = Some(Grab { from, glass: self.rect, size: self.size, edges });
        // (Its buttons are away while it is in hand.)
        self.watch();
        unsafe {
            SetCapture(self.window.hwnd);
            SetTimer(Some(self.window.hwnd), FOLLOW_TIMER, FOLLOW_MS, None);
        }
        self.draw();
    }

    /// Whether the button it is dragged by is still down.
    pub fn held(&self) -> bool {
        self.grab.is_some() && primary_down()
    }

    /// The pointer moved over it, or while it is dragged: it moves, or grows
    /// and shrinks as a whole from the corner opposite the edges held (its
    /// shape is its readings').
    pub fn moved(&mut self) {
        self.watch();
        let Some(at) = cursor() else { return };
        let Some(grab) = self.grab else {
            if self.locked {
                self.window.point(IDC_ARROW);
            } else {
                self.point_at(self.edges_at(at));
            }
            return;
        };
        self.point_at(grab.edges);
        let (dx, dy) = (at.x - grab.from.x, at.y - grab.from.y);
        let g = grab.glass;
        if !grab.edges.any() {
            let rect = RECT { left: g.left + dx, top: g.top + dy, right: g.right + dx, bottom: g.bottom + dy };
            // Followed at every tick: moved only when the pointer has.
            if rect != self.rect {
                self.rect = rect;
                self.place();
            }
            return;
        }
        let (width, height) = ((g.right - g.left) as f32, (g.bottom - g.top) as f32);
        let e = grab.edges;
        let across = (e.left || e.right).then(|| (width + if e.right { dx } else { -dx } as f32) / width);
        let down = (e.top || e.bottom).then(|| (height + if e.bottom { dy } else { -dy } as f32) / height);
        let ratio = match (across, down) {
            (Some(a), Some(d)) => a.max(d),
            (Some(a), None) => a,
            (None, Some(d)) => d,
            (None, None) => 1.0,
        };
        let size = (grab.size * ratio).clamp(crate::panel::SIZES.0, crate::panel::SIZES.1);
        if size == self.size {
            return;
        }
        self.size = size;
        let (width, height) = self.dims();
        let x = if e.left { g.right - width } else { g.left };
        let y = if e.top { g.bottom - height } else { g.top };
        self.rect = RECT { left: x, top: y, right: x + width, bottom: y + height };
        self.draw();
    }

    /// Let go: where it now is and how large, as kept; none unless it was
    /// being dragged.
    pub fn release(&mut self) -> Option<Placed> {
        self.grab.take()?;
        unsafe {
            let _ = KillTimer(Some(self.window.hwnd), FOLLOW_TIMER);
            let _ = ReleaseCapture();
        }
        let glass = self.rect;
        let centre = POINT { x: (glass.left + glass.right) / 2, y: (glass.top + glass.bottom) / 2 };
        (self.screen, self.dpi) = monitor_info(unsafe { MonitorFromPoint(centre, MONITOR_DEFAULTTONEAREST) });
        // Tinted for where it was put.
        self.look();
        self.draw();
        let (left, top, across, down) = room(self.screen, (glass.right - glass.left, glass.bottom - glass.top), self.dpi);
        let share = |at: i32, room: i32| if room > 0 { (at as f32 / room as f32).clamp(0.0, 1.0) } else { 0.0 };
        Some(Placed { at: (share(glass.left - left, across), share(glass.top - top, down)), point: (centre.x, centre.y), size: self.size })
    }

    /// Its glass left as clear as it was made for a moment: no longer said.
    pub fn hush(&mut self) {
        unsafe {
            let _ = KillTimer(Some(self.window.hwnd), SAID_TIMER);
        }
        if self.said.take().is_some() && self.shown {
            self.draw();
        }
    }

    /// Its menu, and what was chosen in it (the focus put back where it
    /// was by the menu itself).
    pub fn menu(&self) -> Option<Choice> {
        self.choose()
    }

    fn choose(&self) -> Option<Choice> {
        let zh = self.lang == Lang::Zh;
        const LOCK: usize = 1;
        const CLOSE: usize = 2;
        const SETTINGS: usize = 3;
        const THROUGH: usize = 4;
        use crate::ui::icons::Icon;
        use crate::ui::menu::{self, Item};
        let items = [
            Item { label: if zh { "锁定位置" } else { "Lock position" }, icon: Some(Icon::Pin), checked: self.locked, rule_before: false },
            Item { label: if zh { "鼠标穿透" } else { "Let clicks through" }, icon: Some(Icon::Pointer), checked: self.through, rule_before: false },
            Item { label: if zh { "关闭悬浮窗" } else { "Close the overlay" }, icon: Some(Icon::Close), checked: false, rule_before: false },
            Item { label: if zh { "设置…" } else { "Settings…" }, icon: Some(Icon::Settings), checked: false, rule_before: true },
        ];
        let chosen = match menu::show(&items, crate::os::apps_dark())? {
            0 => LOCK,
            1 => THROUGH,
            2 => CLOSE,
            _ => SETTINGS,
        };
        match chosen {
            LOCK => Some(Choice::Lock(!self.locked)),
            THROUGH => Some(Choice::Through(!self.through)),
            CLOSE => Some(Choice::Close),
            SETTINGS => Some(Choice::Settings),
            _ => None,
        }
    }

    fn hide(&mut self) {
        if self.grab.take().is_some() {
            unsafe {
                let _ = ReleaseCapture();
            }
        }
        if !self.shown {
            return;
        }
        self.shown = false;
        self.window.hide();
        self.shade.hide();
        self.bar_shows.clear();
        self.over = None;
        self.draw_bar();
        if self.tip.take().is_some() {
            crate::ui::tip::hide();
        }
        crate::widget::own_window_changed(self.shadow_rect());
        self.placed.set(RECT::default());
        // The drawing memory is given back while it is away.
        self.surface.release();
        if let Some((gfx, surface)) = &mut self.shade_surface {
            surface.release(gfx);
        }
    }
}
