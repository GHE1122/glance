//! A word under the pointer on what it is over: a small plate in a window
//! of its own, one for this thread, that every click passes through.

use std::cell::RefCell;
use std::rc::Rc;

use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};

use super::canvas::{Align, Canvas, Color, Family, Fill, Font};
use super::gfx::{self, Gfx, Surface};
use super::life::{Lives, Showing};
use super::window::Window;

struct Tip {
    window: Window,
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

/// Says `text` under `about` (what it speaks of, where that is on the
/// screen), its middle under its middle, on its screen, till `hide`: always
/// as far from it, wherever on it the pointer is.
pub fn show(text: &str, about: RECT, dark: bool) {
    let at = POINT { x: (about.left + about.right) / 2, y: (about.top + about.bottom) / 2 };
    let Ok(gfx) = gfx::current() else { return };
    TIP.with(|kept| {
        let mut kept = kept.borrow_mut();
        if kept.is_none() {
            let Ok(mut window) = Window::new() else { return };
            window.set_click_through(true);
            *kept = Some(Tip { window, gfx: gfx.clone(), surface: None, showing: None, said: (String::new(), RECT::default()) });
        }
        let tip = kept.as_mut().unwrap();
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
        let (wide, tall) = (gfx.measure(text, font) + 2.0 * PAD, TALL);
        let (width, height) = ((wide * scale).ceil() as i32, (tall * scale).ceil() as i32);
        // Under it, its middle on its middle; all of it on the screen; over it where there is no room under.
        let left = (at.x - width / 2).min(screen.right - width).max(screen.left);
        let under = about.bottom + (UNDER * scale) as i32;
        let top = if under + height > screen.bottom { about.top - (UNDER * scale) as i32 - height } else { under };
        let rect = RECT { left, top, right: left + width, bottom: top + height };
        if tip.said.0 == text && tip.said.1 == rect {
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
            let (ink_top, ink_bottom) = frame.ink(text, font);
            frame.text(text, font, ink, PAD, tall / 2.0 - (ink_top + ink_bottom) / 2.0, wide - 2.0 * PAD + 2.0, Align::Start);
        });
        tip.surface = Some(surface);
        if drawn.is_err() {
            gfx::lost();
            return;
        }
        tip.window.place(rect);
        tip.window.show();
        tip.said = (text.to_string(), rect);
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

    /// On a desktop: the tip comes where it is asked for, and goes.
    #[test]
    #[ignore = "puts a tip up on this PC's desktop"]
    fn shows_and_hides() {
        super::show("鼠标穿透已开启，点一下关闭", windows::Win32::Foundation::RECT { left: 590, top: 390, right: 610, bottom: 410 }, true);
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
}
