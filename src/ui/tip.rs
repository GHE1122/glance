//! A word under the pointer on what it is over: a small plate in a window
//! of its own, one for this thread, that every click passes through.

use std::cell::RefCell;
use std::rc::Rc;

use windows::Win32::Foundation::{HWND, POINT, RECT};
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Accessibility::{SetWinEventHook, HWINEVENTHOOK};
use windows::Win32::UI::WindowsAndMessaging::{EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE, OBJID_WINDOW, WINEVENT_OUTOFCONTEXT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

use super::canvas::{Align, Canvas, Color, Family, Fill, Font};
use super::gfx::{self, Gfx, Surface};
use super::life::{Lives, Showing};
use super::window::Window;

struct Tip {
    window: Window,
    /// The window whose part it speaks of: hidden or gone, the tip goes with it (see `owner_went`).
    owner: HWND,
    /// The device it draws with, and its surface on it.
    gfx: Rc<Gfx>,
    surface: Option<Surface>,
    /// Counted as shown on its device while it is (see `life`).
    showing: Option<Showing>,
    /// What it says where (empty: away), not drawn again for the same.
    said: (String, RECT),
}

impl Lives for Tip {
    fn device(&self) -> &Rc<Gfx> {
        &self.gfx
    }

    fn showing(&mut self) -> &mut Option<Showing> {
        &mut self.showing
    }

    /// What its surface holds.
    fn let_go_of_shown(&mut self) {
        if let Some(surface) = &mut self.surface {
            surface.release(&self.gfx);
        }
    }

    /// Its surface.
    fn let_go_of_kept(&mut self) {
        self.surface = None;
    }
}

thread_local! {
    static TIP: RefCell<Option<Tip>> = const { RefCell::new(None) };
}

/// How far from what it speaks of it shows, its height, and the room beside its words (DIPs).
const UNDER: f32 = 8.0;
const TALL: f32 = 28.0;
const PAD: f32 = 10.0;
/// How wide its words go before they are put on more lines, and how tall they go at most, the rest left off (DIPs).
const WIDEST: f32 = 360.0;
const TALLEST: f32 = 150.0;

/// As much of `text` as `TALLEST` holds on lines `WIDEST` across (its padding off), and "…" where some is left off:
/// the most of its characters that fit (found by halving).
fn held(gfx: &Gfx, text: &str, font: Font) -> String {
    let across = WIDEST - 2.0 * PAD;
    let room = TALLEST - (TALL - font.size * 1.3);
    if gfx.wrapped_height(text, font, across) <= room {
        return text.to_string();
    }
    let chars: Vec<char> = text.chars().collect();
    let cut = |count: usize| chars[..count].iter().collect::<String>().trim_end().to_string() + "…";
    let (mut fits, mut over) = (0, chars.len());
    while over - fits > 1 {
        let middle = (fits + over) / 2;
        if gfx.wrapped_height(&cut(middle), font, across) <= room { fits = middle } else { over = middle }
    }
    cut(fits)
}

/// A window of this thread hidden or destroyed (EVENT_OBJECT_HIDE, EVENT_OBJECT_DESTROY): the one the tip speaks of,
/// the tip hidden.
unsafe extern "system" fn owner_went(_: HWINEVENTHOOK, _: u32, hwnd: HWND, object: i32, _: i32, _: u32, _: u32) {
    if object == OBJID_WINDOW.0 && TIP.with(|kept| kept.borrow().as_ref().is_some_and(|tip| tip.owner == hwnd)) {
        hide();
    }
}

/// Says `text` under `about` (what it speaks of, where that is on the
/// screen), its middle under its middle, on its screen, till `hide` or till
/// `owner` (the window `about` is a part of) is hidden or destroyed: always
/// as far from it, wherever on it the pointer is.
pub fn show(owner: HWND, text: &str, about: RECT, dark: bool) {
    let at = POINT { x: (about.left + about.right) / 2, y: (about.top + about.bottom) / 2 };
    let Ok(gfx) = gfx::current() else { return };
    TIP.with(|kept| {
        let mut kept = kept.borrow_mut();
        if kept.is_none() {
            let Ok(mut window) = Window::new() else { return };
            window.set_click_through(true);
            // Told of each of this thread's windows hidden or destroyed, for as long as the thread lives: the tip is
            // put away with the window it speaks of, whatever took that away (a widget closed, the panel put away as
            // the pointer was on it), not only as the pointer leaves.
            unsafe {
                SetWinEventHook(EVENT_OBJECT_DESTROY, EVENT_OBJECT_HIDE, None, Some(owner_went), std::process::id(), GetCurrentThreadId(), WINEVENT_OUTOFCONTEXT);
            }
            *kept = Some(Tip { window, owner, gfx: gfx.clone(), surface: None, showing: None, said: (String::new(), RECT::default()) });
        }
        let tip = kept.as_mut().unwrap();
        tip.owner = owner;
        // (What it drew with on another device goes with it: see `life`.)
        if !Rc::ptr_eq(&tip.gfx, &gfx) {
            tip.gone();
            tip.gfx = gfx.clone();
        }
        // Its screen, and how many pixels a DIP is there.
        let monitor = unsafe { MonitorFromPoint(at, MONITOR_DEFAULTTONEAREST) };
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let (mut dpi, mut unused) = (96, 96);
        unsafe {
            let _ = GetMonitorInfoW(monitor, &mut info);
            let _ = GetDpiForMonitor(monitor, MDT_EFFECTIVE_DPI, &mut dpi, &mut unused);
        }
        let (scale, screen) = (dpi as f32 / 96.0, info.rcMonitor);
        let font = Font::new(Family::Segoe, 12.0, 400.0);
        // On one line where it fits; else on as many as it takes `WIDEST` across, as many as `TALLEST` holds, what is
        // past them left off.
        let one_line = gfx.measure(text, font) <= WIDEST - 2.0 * PAD;
        let said = if one_line { text.to_string() } else { held(&gfx, text, font) };
        let (wide, tall) = if one_line { (gfx.measure(text, font) + 2.0 * PAD, TALL) } else { (WIDEST, gfx.wrapped_height(&said, font, WIDEST - 2.0 * PAD) + 2.0 * (TALL - font.size * 1.3) / 2.0) };
        let (width, height) = ((wide * scale).ceil() as i32, (tall * scale).ceil() as i32);
        // Under it, its middle on its middle; all of it on the screen; over it where there is no room under.
        let left = (at.x - width / 2).min(screen.right - width).max(screen.left);
        let under = about.bottom + (UNDER * scale) as i32;
        let top = if under + height > screen.bottom { about.top - (UNDER * scale) as i32 - height } else { under };
        let rect = RECT { left, top, right: left + width, bottom: top + height };
        if tip.said.0 == said && tip.said.1 == rect {
            return;
        }
        let surface = match tip.surface.take() {
            Some(surface) => Ok(surface),
            None => Surface::new(&gfx, tip.window.hwnd),
        };
        let Ok(mut surface) = surface else { return };
        tip.shown();
        let (plate, rim, ink) = if dark { (Color::hex(0x2B2B2B, 0.98), Color::hex(0xFFFFFF, 0.16), Color::hex(0xFFFFFF, 0.92)) } else { (Color::hex(0xF9F9F9, 0.98), Color::hex(0x000000, 0.14), Color::hex(0x1A1A1A, 1.0)) };
        let drawn = surface.draw(&gfx, (width as u32, height as u32), scale, |frame| {
            frame.fill_rounded(plate, 0.0, 0.0, wide, tall, 6.0);
            frame.stroke_rounded(Fill::Solid(rim), 0.0, 0.0, wide, tall, 6.0, 1.0);
            if one_line {
                let (ink_top, ink_bottom) = frame.ink(&said, font);
                frame.text(&said, font, ink, PAD, tall / 2.0 - (ink_top + ink_bottom) / 2.0, wide - 2.0 * PAD + 2.0, Align::Start);
            } else {
                frame.text_wrapped(&said, font, ink, PAD, (TALL - font.size * 1.3) / 2.0, wide - 2.0 * PAD);
            }
        });
        // (The device gone, its surface is made anew on the next.)
        if let Err(error) = &drawn {
            if !gfx::failed("tip", Some(error)) {
                tip.surface = Some(surface);
            }
            return;
        }
        tip.surface = Some(surface);
        tip.window.place(rect);
        tip.window.show();
        tip.said = (said, rect);
    });
}

/// Puts it over the windows again, while it says something: what it speaks
/// of puts itself above all each time it is drawn.
pub fn above() {
    TIP.with(|kept| {
        if let Some(tip) = kept.borrow().as_ref().filter(|tip| !tip.said.0.is_empty()) {
            tip.window.show();
        }
    });
}

/// Says nothing more.
pub fn hide() {
    TIP.with(|kept| {
        if let Some(tip) = kept.borrow_mut().as_mut().filter(|tip| !tip.said.0.is_empty()) {
            tip.window.hide();
            tip.said.0.clear();
            tip.hidden();
        }
    });
}

#[cfg(test)]
mod tests {
    use windows::Win32::UI::WindowsAndMessaging::{GetWindowRect, IsWindowVisible};

    /// Words too many for a tip: as many as fit its tallest on its widest lines, "…" after them; a name that fits on
    /// its lines whole, whole.
    #[test]
    fn a_long_tip_is_cut_to_its_room() {
        let gfx = crate::ui::gfx::Gfx::new().expect("graphics");
        let font = super::Font::new(super::Family::Segoe, 12.0, 400.0);
        let long = "先看一下安装脚本的用法，再备份设置： Ran 1 shell command 用正式版配置编译 0.4，同时备份设置： ".repeat(12);
        let held = super::held(&gfx, &long, font);
        assert!(held.ends_with('…') && held.chars().count() < long.chars().count());
        assert!(gfx.wrapped_height(&held, font, super::WIDEST - 2.0 * super::PAD) <= super::TALLEST - (super::TALL - font.size * 1.3));
        let name = "Bliss_v2.1.1_(Chocapic13_Shaders_edit_with_a_long_tail_of_words).zip";
        assert_eq!(super::held(&gfx, name, font), name);
    }

    /// On a desktop: the tip comes where it is asked for, and goes.
    #[test]
    #[ignore = "puts a tip up on this PC's desktop"]
    fn shows_and_hides() {
        super::show(windows::Win32::Foundation::HWND::default(), "鼠标穿透已开启，点一下关闭", windows::Win32::Foundation::RECT { left: 590, top: 390, right: 610, bottom: 410 }, true);
        let window = super::TIP.with(|kept| kept.borrow().as_ref().map(|tip| tip.window.hwnd)).expect("no tip was made");
        let mut rect = Default::default();
        unsafe { GetWindowRect(window, &mut rect).unwrap() };
        println!("tip at {rect:?}");
        assert!(unsafe { IsWindowVisible(window) }.as_bool() && rect.right - rect.left > 100 && rect.top > 400);
        std::thread::sleep(std::time::Duration::from_secs(2));
        super::hide();
        assert!(!unsafe { IsWindowVisible(window) }.as_bool());
        // (Its window and device are not taken down with the test's thread: that waits on a desktop that is gone.)
        super::TIP.with(|kept| std::mem::forget(kept.borrow_mut().take()));
    }

    /// Lets the thread take in what it was told (the hook's calls among it).
    fn pump() {
        use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE};
        let mut message = MSG::default();
        unsafe {
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
    }

    /// On a desktop: the window a tip speaks of hidden, or destroyed, without the pointer leaving what it said it of
    /// (a widget closed from its menu, the panel put away), the tip goes with it. Another of the thread's windows
    /// hidden, it stays.
    #[test]
    #[ignore = "puts a tip and windows up on this PC's desktop"]
    fn goes_with_its_window() {
        use crate::ui::window::Window;
        let about = windows::Win32::Foundation::RECT { left: 590, top: 390, right: 610, bottom: 410 };
        let (owner, other) = (Window::new().expect("a window"), Window::new().expect("a window"));
        owner.show();
        other.show();
        super::show(owner.hwnd, "关闭", about, true);
        let tip = super::TIP.with(|kept| kept.borrow().as_ref().map(|tip| tip.window.hwnd)).expect("no tip was made");
        let shown = || unsafe { IsWindowVisible(tip) }.as_bool();
        other.hide();
        pump();
        let other_hidden = shown();
        owner.hide();
        pump();
        let owner_hidden = shown();
        owner.show();
        super::show(owner.hwnd, "关闭", about, true);
        let again = shown();
        owner.destroy();
        pump();
        let owner_gone = shown();
        // (Let go of before anything is asserted: taken down with a failed test's thread, it waits on a desktop gone.)
        super::TIP.with(|kept| std::mem::forget(kept.borrow_mut().take()));
        assert!(other_hidden, "another window hidden took the tip away");
        assert!(!owner_hidden, "not hidden with its window");
        assert!(again);
        assert!(!owner_gone, "not gone with its window");
    }
}
