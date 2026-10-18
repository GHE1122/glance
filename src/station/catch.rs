//! Where what is dragged from another program is caught: windows of the station's own thread (OLE gives a drop to the
//! thread a window is of, and the panel's and the widgets' thread has none: see docs/station-and-notes.md, section
//! 七), put over each station shown (see `run::shows_at`) and along the screen edges that hold one, only while
//! something is dragged. Else there are none: they would take presses meant for what is under them (a maximized
//! window's scroll bar along the screen's edge).
//!
//! A drag is told from outside by the window that holds the pointer on the dragging program's thread: OLE's own
//! (`CLIPBRDWNDCLASS`), not a window moved or text selected.

use super::drop::{Dragged, Target};
use crate::settings::{Edge, Part};
use windows::core::w;
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::System::Ole::{IDropTarget, RegisterDragDrop, RevokeDragDrop};
use windows::Win32::UI::WindowsAndMessaging::*;

/// How thick a strip along a screen's edge is (physical px).
const STRIP: i32 = 6;

/// A window catching drops: where it is, its window, what it is for (an edge's strip opens the station there).
struct Catcher {
    rect: RECT,
    window: HWND,
    edge: Option<Edge>,
    _target: IDropTarget,
}

/// The catchers up, while something is dragged.
#[derive(Default)]
pub struct Catchers {
    up: Vec<Catcher>,
}

/// Whether a program is dragging something with OLE now: its thread (the one in front's) holding the pointer with
/// OLE's window.
pub fn dragging() -> bool {
    unsafe {
        let front = GetForegroundWindow();
        let thread = GetWindowThreadProcessId(front, None);
        let mut info = GUITHREADINFO { cbSize: std::mem::size_of::<GUITHREADINFO>() as u32, ..Default::default() };
        if thread == 0 || GetGUIThreadInfo(thread, &mut info).is_err() || info.hwndCapture.is_invalid() {
            return false;
        }
        let mut class = [0u16; 32];
        let length = GetClassNameW(info.hwndCapture, &mut class) as usize;
        String::from_utf16_lossy(&class[..length]) == "CLIPBRDWNDCLASS"
    }
}

/// Where drops are caught now: each station shown, and each edge of each screen that holds the workbench while it
/// shows a station, as a strip.
fn wanted() -> Vec<(RECT, Option<Edge>)> {
    let mut wanted: Vec<(RECT, Option<Edge>)> = super::run::areas().into_iter().map(|area| (area, None)).collect();
    let (places, station) = {
        let settings = crate::app().settings.lock().unwrap();
        (settings.places().clone(), crate::ui::prefs::module_on_in(&settings.view, "workbench", "station"))
    };
    if !station {
        return wanted;
    }
    for monitor in crate::screens::monitors() {
        let Some(id) = crate::screens::id_of(monitor) else { continue };
        for edge in [Edge::Left, Edge::Right, Edge::Top] {
            if places.part(&id, edge) != Some(Part::Workbench) {
                continue;
            }
            let strip = match edge {
                Edge::Left => RECT { right: monitor.left + STRIP, ..monitor },
                Edge::Right => RECT { left: monitor.right - STRIP, ..monitor },
                Edge::Top => RECT { bottom: monitor.top + STRIP, ..monitor },
            };
            wanted.push((strip, Some(edge)));
        }
    }
    wanted
}

unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

impl Catchers {
    /// Up where drops are wanted while something is dragged; none else. What each sees goes to `run::caught`.
    pub fn follow(&mut self) {
        let wanted = if dragging() { wanted() } else { Vec::new() };
        let same = wanted.len() == self.up.len() && wanted.iter().zip(&self.up).all(|((rect, edge), up)| *rect == up.rect && *edge == up.edge);
        if same {
            return;
        }
        self.clear();
        for (rect, edge) in wanted {
            if let Some(catcher) = make(rect, edge) {
                self.up.push(catcher);
            }
        }
    }

    /// None up.
    pub fn clear(&mut self) {
        for catcher in self.up.drain(..) {
            unsafe {
                let _ = RevokeDragDrop(catcher.window);
                let _ = DestroyWindow(catcher.window);
            }
        }
    }
}

/// A window catching drops over `rect` (screen px; along `edge`, where it is a strip there), above all, all but
/// invisible (hit by the pointer: a window wholly clear is passed over), telling `run::caught` what it sees.
fn make(rect: RECT, edge: Option<Edge>) -> Option<Catcher> {
    unsafe {
        let class = WNDCLASSW { lpfnWndProc: Some(procedure), lpszClassName: w!("GlanceStationCatcher"), ..Default::default() };
        RegisterClassW(&class);
        let window = CreateWindowExW(
            WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE | WS_EX_TOPMOST | WS_EX_LAYERED,
            class.lpszClassName,
            w!(""),
            WS_POPUP,
            rect.left,
            rect.top,
            rect.right - rect.left,
            rect.bottom - rect.top,
            None,
            None,
            None,
            None,
        )
        .ok()?;
        let _ = SetLayeredWindowAttributes(window, COLORREF(0), 1, LWA_ALPHA);
        let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
        let _ = SetWindowPos(window, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE);
        let target = Target::new(move |dragged: Dragged| super::run::caught(dragged, edge));
        if RegisterDragDrop(window, &target).is_err() {
            let _ = DestroyWindow(window);
            return None;
        }
        Some(Catcher { rect, window, edge, _target: target })
    }
}
