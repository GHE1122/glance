//! The clipboard opened by Glance's windows. One window at a time has it open, each only a moment: another program's,
//! or another of Glance's threads (the station reads it as soon as it changes, see `station::clipboard`). Opened as
//! someone acts (pastes, copies), it is waited for while another holds it, as Windows gives no way to be told when it is
//! let go: tried again and again for a while, else given up.

use std::time::{Duration, Instant};
use windows::Win32::Foundation::HWND;
use windows::Win32::System::DataExchange::{CloseClipboard, OpenClipboard};

/// How long the clipboard held by another is waited for, and how often it is tried meanwhile.
const WAIT: Duration = Duration::from_millis(500);
const AGAIN: Duration = Duration::from_millis(10);

/// The clipboard open for a window of Glance's: closed as this is let go.
pub struct Opened(());

impl Drop for Opened {
    fn drop(&mut self) {
        unsafe {
            let _ = CloseClipboard();
        }
    }
}

/// The clipboard opened for `owner`, waited for while another holds it: none if it is held all that while.
pub fn open(owner: HWND) -> Option<Opened> {
    let began = Instant::now();
    loop {
        if let Some(opened) = try_open(owner) {
            return Some(opened);
        }
        if began.elapsed() >= WAIT {
            return None;
        }
        std::thread::sleep(AGAIN);
    }
}

/// The clipboard opened for `owner` if no other holds it just now (for one who comes back for it later).
pub fn try_open(owner: HWND) -> Option<Opened> {
    unsafe { OpenClipboard(Some(owner)) }.ok().map(|()| Opened(()))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Held by another thread, the clipboard is waited for and had once that lets it go, and not had by one who will
    /// not wait.
    #[test]
    fn waits_for_the_clipboard_held_by_another() {
        use std::sync::mpsc::channel;
        use windows::core::w;
        use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, HWND_MESSAGE, WINDOW_EX_STYLE, WINDOW_STYLE};
        // (A window of its own on each thread, as Glance's windows open it.)
        let window = || unsafe { CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"), w!(""), WINDOW_STYLE(0), 0, 0, 0, 0, Some(HWND_MESSAGE), None, None, None) }.unwrap();
        let (held, holding) = channel();
        let other = std::thread::spawn(move || {
            let owner = window();
            let opened = open(owner).expect("the clipboard");
            held.send(()).unwrap();
            std::thread::sleep(Duration::from_millis(150));
            drop(opened);
            unsafe {
                let _ = DestroyWindow(owner);
            }
        });
        holding.recv().unwrap();
        let owner = window();
        assert!(try_open(owner).is_none(), "held by the other thread");
        let began = Instant::now();
        let opened = open(owner);
        assert!(opened.is_some());
        assert!(began.elapsed() >= Duration::from_millis(100), "{:?}", began.elapsed());
        drop(opened);
        other.join().unwrap();
        unsafe {
            let _ = DestroyWindow(owner);
        }
    }
}
