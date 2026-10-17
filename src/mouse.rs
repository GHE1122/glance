//! The mouse and the screens it moves over, as both of Glance's programs read them: the pointer's place, the
//! buttons, a mouse's raw input, and whether the pointer is on an edge that opens the panel. Read here as this process
//! can (an elevated one, the sensors', reads them whatever window is in front).

use windows::Win32::Foundation::{POINT, RECT};
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, MonitorFromPoint, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::UI::HiDpi::{GetDpiForMonitor, MDT_EFFECTIVE_DPI};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_LBUTTON, VK_MBUTTON, VK_RBUTTON};
use windows::Win32::UI::Input::{GetRawInputData, HRAWINPUT, RAWINPUT, RAWINPUTHEADER, RID_INPUT, RIM_TYPEMOUSE};
use windows::Win32::UI::WindowsAndMessaging::{GetCursorInfo, GetCursorPos, CURSORINFO, CURSOR_SHOWING};

use crate::detector::Motion;
use crate::settings::Edge;

/// Strip at each end of the edge that does not trigger (logical px): the
/// height of a title bar's buttons and of the taskbar, which own the corners.
pub(crate) const CORNER_EXCLUSION: f64 = 48.0;

/// A screen: its whole, its work area (physical px), and its scale (physical
/// px per DIP).
#[derive(Clone, Copy, PartialEq)]
pub(crate) struct Contact {
    pub(crate) monitor: RECT,
    pub(crate) work: RECT,
    pub(crate) scale: f32,
}

pub(crate) fn cursor_position() -> Option<POINT> {
    let mut point = POINT::default();
    unsafe { GetCursorPos(&mut point) }.ok().map(|_| point)
}

/// Whether a mouse button is down, as this process reads it: an ordinary one reads "up" throughout while an elevated
/// window is in front.
pub(crate) fn buttons_held() -> bool {
    [VK_LBUTTON, VK_RBUTTON, VK_MBUTTON].iter().any(|key| unsafe { GetAsyncKeyState(key.0 as i32) } < 0)
}

pub(crate) fn cursor_showing() -> bool {
    let mut info = CURSORINFO { cbSize: size_of::<CURSORINFO>() as u32, ..Default::default() };
    unsafe { GetCursorInfo(&mut info) }.is_ok() && info.flags.0 & CURSOR_SHOWING.0 != 0
}

/// How the mouse moved, as it says itself: to a place, or by so many
/// counts across and down.
#[derive(Clone, Copy)]
pub(crate) enum Moved {
    To,
    By(i32, i32),
}

impl Moved {
    /// The move as a push into `edge`.
    pub(crate) fn toward(self, edge: Edge) -> Motion {
        let Moved::By(x, y) = self else { return Motion::Absolute };
        let (outward, along) = match edge {
            Edge::Left => (-x, y),
            Edge::Right => (x, y),
            Edge::Top => (-y, x),
        };
        Motion::Relative { outward, along }
    }
}

/// The raw input's flags for a button going down: the left, the right, the middle, the fourth and the fifth.
pub(crate) const ANY_BUTTON_DOWN: u16 = 0x0001 | 0x0004 | 0x0010 | 0x0040 | 0x0100;
/// A raw mouse report's flag for a move to a place (a tablet, a remote session), not by so many counts.
const MOUSE_MOVE_ABSOLUTE: u16 = 1;

/// Which buttons are down (a bit each: left, right, middle, the fourth and the fifth) after a raw report whose
/// buttons' flags are `flags`, `held` before it: each flag that went down set, each that went up cleared.
pub(crate) fn held_after(flags: u16, held: u8) -> u8 {
    (0..5).fold(held, |held, button| {
        let (down, up) = (1u16 << (2 * button), 1u16 << (2 * button + 1));
        let held = if flags & down != 0 { held | 1 << button } else { held };
        if flags & up != 0 { held & !(1 << button) } else { held }
    })
}

/// A mouse's raw input: how it moved, and its buttons' flags (what went down and up with it).
pub(crate) fn read_mouse(handle: HRAWINPUT) -> Option<(Moved, u16)> {
    let mut raw = RAWINPUT::default();
    let mut size = size_of::<RAWINPUT>() as u32;
    let read = unsafe {
        GetRawInputData(handle, RID_INPUT, Some(&mut raw as *mut _ as *mut _), &mut size, size_of::<RAWINPUTHEADER>() as u32)
    };
    if read == u32::MAX || raw.header.dwType != RIM_TYPEMOUSE.0 {
        return None;
    }
    let mouse = unsafe { raw.data.mouse };
    let buttons = unsafe { mouse.Anonymous.Anonymous.usButtonFlags };
    if mouse.usFlags.0 & MOUSE_MOVE_ABSOLUTE != 0 {
        return Some((Moved::To, buttons));
    }
    Some((Moved::By(mouse.lLastX, mouse.lLastY), buttons))
}

/// The whole desktop, every screen within it (physical px): the bounds of what is on any screen.
pub(crate) fn desktop() -> RECT {
    use windows::Win32::UI::WindowsAndMessaging::{GetSystemMetrics, SM_CXVIRTUALSCREEN, SM_CYVIRTUALSCREEN, SM_XVIRTUALSCREEN, SM_YVIRTUALSCREEN};
    let (left, top) = unsafe { (GetSystemMetrics(SM_XVIRTUALSCREEN), GetSystemMetrics(SM_YVIRTUALSCREEN)) };
    let (width, height) = unsafe { (GetSystemMetrics(SM_CXVIRTUALSCREEN), GetSystemMetrics(SM_CYVIRTUALSCREEN)) };
    RECT { left, top, right: left + width, bottom: top + height }
}

pub(crate) fn monitor_at(point: POINT) -> Option<Contact> {
    let handle = unsafe { MonitorFromPoint(point, MONITOR_DEFAULTTONEAREST) };
    let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
    unsafe { GetMonitorInfoW(handle, &mut info) }.ok().ok()?;
    let (mut dpi, mut dpi_y) = (0, 0);
    unsafe { GetDpiForMonitor(handle, MDT_EFFECTIVE_DPI, &mut dpi, &mut dpi_y) }.ok()?;
    Some(Contact { monitor: info.rcMonitor, work: info.rcWork, scale: dpi as f32 / 96.0 })
}

/// Describes the monitor under the cursor if the cursor is pressed against its
/// trigger edge with intent to point: visible, no button held (as this process
/// reads them: the edges are watched where the mouse is heard), clear of the
/// corners, and where the edge stops it (it does not pass on to another
/// screen there).
pub(crate) fn edge_contact(cursor: POINT, edge: Edge) -> Option<Contact> {
    let contact = monitor_at(cursor)?;
    let monitor = contact.monitor;
    let corner = (CORNER_EXCLUSION * contact.scale as f64).round() as i32;
    let along_height = (monitor.top + corner..monitor.bottom - corner).contains(&cursor.y);
    let (on_edge, along, clear_of_corners) = match edge {
        Edge::Left => (cursor.x <= monitor.left, cursor.y, along_height),
        Edge::Right => (cursor.x >= monitor.right - 1, cursor.y, along_height),
        Edge::Top => (cursor.y <= monitor.top, cursor.x, (monitor.left + corner..monitor.right - corner).contains(&cursor.x)),
    };
    if !on_edge || !clear_of_corners || buttons_held() || !cursor_showing() {
        return None;
    }
    (!crate::screens::passes(monitor, edge, along)).then_some(contact)
}

#[cfg(test)]
mod tests {
    /// The buttons held follow each report's flags: down, up, and several in one report.
    #[test]
    fn buttons_held_follow_the_reports() {
        let (left_down, left_up, right_down, right_up) = (0x0001, 0x0002, 0x0004, 0x0008);
        let held = super::held_after(right_down, 0);
        assert_eq!(held, 0b10);
        let held = super::held_after(left_down, held);
        assert_eq!(held, 0b11);
        assert_eq!(super::held_after(right_up | left_up, held), 0);
        // A report with no button in it leaves them as they were.
        assert_eq!(super::held_after(0, 0b10), 0b10);
    }
}
