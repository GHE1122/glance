//! A menu as Windows 11 draws its own (File Explorer's): a rounded sheet
//! with a soft shadow, a row for each choice with its icon, a light
//! highlight under the pointer, a tick for a choice that is on, thin rules
//! between groups. Shown at the pointer, it is up until a choice is made or
//! it is left (a click elsewhere, Esc, Alt, another window to the front),
//! as a system menu is.

use std::rc::Rc;

use windows::Win32::Foundation::{HWND, LPARAM, POINT, RECT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, VIRTUAL_KEY, VK_DOWN, VK_ESCAPE, VK_RETURN, VK_UP};
use windows::Win32::UI::WindowsAndMessaging::{
    GetCursorPos, GetForegroundWindow, IsIconic, PostMessageW, SetForegroundWindow, MSG, SC_RESTORE, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MBUTTONDOWN, WM_MOUSEMOVE, WM_RBUTTONDOWN, WM_RBUTTONUP,
    WM_SYSCOMMAND, WM_SYSKEYDOWN,
};

use super::canvas::{Align, Canvas, Color, Family, Fill, Font, Point};
use super::gfx::{self, Surface};
use super::icons::Icon;
use super::window::{Window, CAPTURE_LOST};

/// One choice: what it says, its icon, whether it is on (ticked), and
/// whether a rule sets it off from the choices before it.
pub struct Item<'a> {
    pub label: &'a str,
    pub icon: Option<Icon>,
    pub checked: bool,
    pub rule_before: bool,
}

/// Its measures (DIPs), as Windows 11's menus have them.
const ROW: f32 = 32.0;
const RULE: f32 = 9.0;
const INSET: f32 = 4.0;
const RADIUS: f32 = 8.0;
const HIGHLIGHT_RADIUS: f32 = 4.0;
const ICON: f32 = 16.0;
const TEXT_LEFT: f32 = 44.0;
const MIN_WIDTH: f32 = 200.0;
/// Room round the sheet for its shadow.
const SHADOW: f32 = 16.0;

/// Counts menus shown and gone: a capture of the desktop taken while one
/// was up may hold it (see `Panel::collect_behind`).
pub static SHOWN: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
const FONT: Font = Font::new(Family::Segoe, 14.0, 400.0);

/// What a menu made of a message for its window.
pub enum Handled {
    /// None of its own: for the window, as any other message.
    Not,
    /// Taken, the menu still up.
    Taken,
    /// Done with: the index of the choice made, if one was (none, left).
    Done(Option<usize>),
}

/// A menu up: shown at the pointer, or where it is asked, till a choice is made in it or it is left. Run by the loop
/// of the thread that showed it, as any other window of it is (its messages handed to `handle`, and `left` asked
/// after them): what else that thread shows goes on meanwhile. (Run by a loop of its own, it held back every other
/// window of the thread, the panel's live readings, the widgets' and the music full screen, till it was gone.)
pub struct Menu {
    gfx: Rc<gfx::Gfx>,
    window: Window,
    surface: Surface,
    items: Vec<Item<'static>>,
    sheet: Sheet,
    dark: bool,
    scale: f32,
    /// Its window's size (physical px) and where it is.
    size: (i32, i32),
    rect: RECT,
    lit: Option<usize>,
    /// What was in front before it, given the front back as it goes.
    before: HWND,
    /// Opened with the panel up (see `over_the_panel`).
    with_the_panel: bool,
    /// Shown till it goes (see `life`): counted before its surface is made, so that it is let go of after it.
    _showing: super::life::Showing,
}

impl Menu {
    /// Shows `items` at the pointer, light or `dark`.
    pub fn open(items: Vec<Item<'static>>, dark: bool, with_the_panel: bool) -> Option<Menu> {
        let mut cursor = POINT::default();
        unsafe { GetCursorPos(&mut cursor) }.ok()?;
        Menu::open_at(items, dark, cursor, false, with_the_panel)
    }

    /// Shows `items` with a corner at `at` (physical px), its bottom there for `above` (as a menu from the taskbar
    /// opens), light or `dark`. None where it could not be shown, or was not let to the front.
    pub fn open_at(items: Vec<Item<'static>>, dark: bool, at: POINT, above: bool, with_the_panel: bool) -> Option<Menu> {
        let contact = crate::mouse::monitor_at(at)?;
        let gfx = gfx::current().ok()?;
        let showing = super::life::Showing::of(&gfx);
        let window = Window::activating().ok()?;
        let Ok(surface) = Surface::new(&gfx, window.hwnd) else {
            window.destroy();
            gfx::lost();
            return None;
        };
        let scale = contact.scale;
        let sheet = Sheet::new(&gfx, &items);
        // At the pointer, turned back where it would leave its screen.
        let (w, h) = (((sheet.width + 2.0 * SHADOW) * scale) as i32, ((sheet.height + 2.0 * SHADOW) * scale) as i32);
        let margin = (SHADOW * scale) as i32;
        let work = contact.work;
        let mut left = at.x - margin;
        if left + w - margin > work.right {
            left = at.x - w + margin;
        }
        let mut top = at.y - margin;
        if above || top + h - margin > work.bottom {
            top = at.y - h + margin;
        }
        let left = left.clamp(work.left - margin, (work.right - w + margin).max(work.left - margin));
        let top = top.clamp(work.top - margin, (work.bottom - h + margin).max(work.top - margin));
        let rect = RECT { left, top, right: left + w, bottom: top + h };
        let mut menu = Menu { gfx, window, surface, items, sheet, dark, scale, size: (w, h), rect, lit: None, before: HWND::default(), with_the_panel, _showing: showing };
        if !menu.draw() {
            menu.window.destroy();
            return None;
        }
        menu.window.place(rect);
        menu.window.show();
        // Up, and gone: a widget that took the desktop with it there takes it again.
        crate::widget::own_window_changed(rect);
        SHOWN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // In front, with the keyboard and the mouse, as a system menu is: its keys come to it, not to the program
        // behind; a click anywhere comes to it too; and it is left as anything else comes to the front.
        menu.before = unsafe { GetForegroundWindow() };
        // Not let to the front (Windows keeps it for the program in use): it could neither hear its keys nor know it
        // was left, so it goes.
        if !unsafe { SetForegroundWindow(menu.window.hwnd) }.as_bool() {
            crate::journal::note("menu: not let to the front; not shown");
            menu.close();
            return None;
        }
        unsafe { SetCapture(menu.window.hwnd) };
        Some(menu)
    }

    /// Whether `hwnd` is its window.
    pub fn owns(&self, hwnd: HWND) -> bool {
        self.window.hwnd == hwnd
    }

    /// Takes a message for its window, if it is one of its own.
    pub fn handle(&mut self, msg: &MSG) -> Handled {
        let (x, y) = ((msg.lParam.0 & 0xFFFF) as i16 as i32, ((msg.lParam.0 >> 16) & 0xFFFF) as i16 as i32);
        match msg.message {
            WM_MOUSEMOVE => {
                let row = self.row_at(x, y);
                self.light(row)
            }
            // A press outside leaves it; inside, the choice is made as the button comes up.
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN if !self.inside(x, y) => Handled::Done(None),
            WM_LBUTTONUP | WM_RBUTTONUP if self.inside(x, y) => self.row_at(x, y).map_or(Handled::Taken, |row| Handled::Done(Some(row))),
            WM_LBUTTONDOWN | WM_RBUTTONDOWN | WM_MBUTTONDOWN | WM_LBUTTONUP | WM_RBUTTONUP => Handled::Taken,
            WM_KEYDOWN => match VIRTUAL_KEY(msg.wParam.0 as u16) {
                VK_ESCAPE => Handled::Done(None),
                key @ (VK_UP | VK_DOWN) => {
                    let n = self.items.len();
                    let row = Some(match (self.lit, key == VK_UP) {
                        (None, true) => n - 1,
                        (None, false) => 0,
                        (Some(i), true) => (i + n - 1) % n,
                        (Some(i), false) => (i + 1) % n,
                    });
                    self.light(row)
                }
                VK_RETURN if self.lit.is_some() => Handled::Done(self.lit),
                _ => Handled::Taken,
            },
            // Alt, as with a system menu, or the mouse taken from it.
            WM_SYSKEYDOWN | CAPTURE_LOST => Handled::Done(None),
            _ => Handled::Not,
        }
    }

    /// Whether it was opened with the panel up: one that was not is left as the panel comes (by its shortcut, an
    /// edge), which would cover it and take what it was opened for.
    pub fn over_the_panel(&self) -> bool {
        self.with_the_panel
    }

    /// Its window.
    pub fn hwnd(&self) -> HWND {
        self.window.hwnd
    }

    /// Whether it has been left by something else coming to the front.
    pub fn left(&self) -> bool {
        (unsafe { GetForegroundWindow() }) != self.window.hwnd
    }

    /// Gone: the front given back where it was, unless something else has taken it.
    pub fn close(self) {
        let held = !self.left();
        unsafe {
            let _ = ReleaseCapture();
        }
        self.window.destroy();
        crate::widget::own_window_changed(self.rect);
        SHOWN.fetch_add(1, std::sync::atomic::Ordering::Relaxed);
        // (A game that went down, a fullscreen one stepping out as the menu came, is restored as a click on its
        // taskbar button would.)
        if held && !self.before.is_invalid() {
            unsafe {
                let _ = SetForegroundWindow(self.before);
                if IsIconic(self.before).as_bool() {
                    let _ = PostMessageW(Some(self.before), WM_SYSCOMMAND, WPARAM(SC_RESTORE as usize), LPARAM(0));
                }
            }
        }
    }

    /// The choice `row` lit (left, where it could not be drawn so).
    fn light(&mut self, row: Option<usize>) -> Handled {
        if row != self.lit {
            self.lit = row;
            if !self.draw() {
                return Handled::Done(None);
            }
        }
        Handled::Taken
    }

    fn draw(&mut self) -> bool {
        let (sheet, items, dark, lit) = (&self.sheet, &self.items, self.dark, self.lit);
        let drawn = self.surface.draw(&self.gfx, (self.size.0 as u32, self.size.1 as u32), self.scale, |frame| {
            frame.origin(SHADOW, SHADOW);
            sheet.paint(frame, items, dark, lit);
        });
        if drawn.is_err() {
            gfx::lost();
        }
        drawn.is_ok()
    }

    /// Which choice a point in the window (physical px) is on.
    fn row_at(&self, x: i32, y: i32) -> Option<usize> {
        let (x, y) = (x as f32 / self.scale - SHADOW, y as f32 / self.scale - SHADOW);
        if x < 0.0 || x > self.sheet.width {
            return None;
        }
        self.sheet.tops.iter().position(|top| y >= *top && y < top + ROW)
    }

    fn inside(&self, x: i32, y: i32) -> bool {
        let (x, y) = (x as f32 / self.scale - SHADOW, y as f32 / self.scale - SHADOW);
        x >= 0.0 && x <= self.sheet.width && y >= 0.0 && y <= self.sheet.height
    }
}

/// A menu's sheet laid out for its items: its size (DIPs) and where each
/// row starts.
pub struct Sheet {
    pub width: f32,
    pub height: f32,
    tops: Vec<f32>,
}

impl Sheet {
    pub fn new(gfx: &gfx::Gfx, items: &[Item]) -> Self {
        // As wide as its longest choice.
        let widest = items.iter().map(|item| gfx.measure(item.label, FONT)).fold(0.0, f32::max);
        let width = (TEXT_LEFT + widest + 48.0).max(MIN_WIDTH);
        let tops: Vec<f32> = items
            .iter()
            .scan(INSET, |y, item| {
                *y += if item.rule_before { RULE } else { 0.0 };
                let top = *y;
                *y += ROW;
                Some(top)
            })
            .collect();
        let height = tops.last().map_or(INSET, |top| top + ROW) + INSET;
        Sheet { width, height, tops }
    }

    /// Draws it, its corner at the frame's origin (its shadow beyond),
    /// light or `dark`, the choice `lit` highlighted.
    pub fn paint(&self, frame: &gfx::Frame, items: &[Item], dark: bool, lit: Option<usize>) {
        let (width, height) = (self.width, self.height);
        let colours = Colours::new(dark);
        frame.shadow(colours.shadow, 0.0, 0.0, width, height, RADIUS, 12.0, 6.0);
        frame.fill_rounded(colours.sheet, 0.0, 0.0, width, height, RADIUS);
        frame.stroke_rounded(Fill::Solid(colours.stroke), 0.0, 0.0, width, height, RADIUS + 0.5, 1.0);
        for (i, (item, top)) in items.iter().zip(&self.tops).enumerate() {
            if item.rule_before {
                frame.fill(colours.rule, 0.0, top - RULE / 2.0 - 0.5, width, 1.0);
            }
            if lit == Some(i) {
                frame.fill_rounded(colours.highlight, INSET, *top, width - 2.0 * INSET, ROW, HIGHLIGHT_RADIUS);
            }
            let middle = top + ROW / 2.0;
            if let Some(icon) = item.icon {
                frame.icon(icon, Point { x: INSET + 16.0, y: middle }, ICON, colours.icon);
            }
            let (ink_top, ink_bottom) = frame.ink(item.label, FONT);
            frame.text(item.label, FONT, colours.text, TEXT_LEFT, middle - (ink_top + ink_bottom) / 2.0, width - TEXT_LEFT - 36.0, Align::Start);
            if item.checked {
                frame.icon(Icon::Check, Point { x: width - INSET - 18.0, y: middle }, ICON, colours.text);
            }
        }
    }
}

/// What a menu is drawn in, light or dark, as Windows 11's are.
struct Colours {
    sheet: Color,
    stroke: Color,
    shadow: Color,
    text: Color,
    icon: Color,
    highlight: Color,
    rule: Color,
}

impl Colours {
    fn new(dark: bool) -> Self {
        if dark {
            Colours {
                sheet: Color::hex(0x2C2C2C, 0.98),
                stroke: Color::hex(0x000000, 0.2),
                shadow: Color::hex(0x000000, 0.35),
                text: Color::hex(0xFFFFFF, 1.0),
                icon: Color::hex(0xFFFFFF, 0.86),
                highlight: Color::hex(0xFFFFFF, 0.06),
                rule: Color::hex(0xFFFFFF, 0.08),
            }
        } else {
            Colours {
                sheet: Color::hex(0xF9F9F9, 0.98),
                stroke: Color::hex(0x000000, 0.08),
                shadow: Color::hex(0x000000, 0.16),
                text: Color::hex(0x000000, 0.9),
                icon: Color::hex(0x000000, 0.72),
                highlight: Color::hex(0x000000, 0.04),
                rule: Color::hex(0x000000, 0.08),
            }
        }
    }
}
