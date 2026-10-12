//! A word under the pointer on what it is over: the system's own tooltip,
//! one for this thread, shown where it is told to be.

use std::cell::{Cell, RefCell};

use windows::core::{w, PCWSTR, PWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, POINT, WPARAM};
use windows::Win32::UI::Controls::{SetWindowTheme, TOOLTIPS_CLASSW, TTF_ABSOLUTE, TTF_TRACK, TTM_ADDTOOLW, TTM_SETMAXTIPWIDTH, TTM_TRACKACTIVATE, TTM_TRACKPOSITION, TTM_UPDATETIPTEXTW, TTS_ALWAYSTIP, TTS_NOPREFIX, TTTOOLINFOW};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, SendMessageW, SetWindowPos, HWND_TOPMOST, SWP_NOACTIVATE, SWP_NOMOVE, SWP_NOSIZE, WINDOW_STYLE, WS_EX_NOACTIVATE, WS_EX_TOPMOST, WS_EX_TRANSPARENT, WS_POPUP,
};

thread_local! {
    static TIP: Cell<Option<isize>> = const { Cell::new(None) };
    /// What it says now; empty, hidden.
    static SAID: RefCell<String> = const { RefCell::new(String::new()) };
}

/// How far under the pointer it shows (physical pixels), and how wide it gets before its words wrap.
const UNDER: i32 = 22;
const WIDEST: isize = 320;

fn tool(tip: HWND, text: &mut [u16]) -> TTTOOLINFOW {
    TTTOOLINFOW { cbSize: size_of::<TTTOOLINFOW>() as u32, uFlags: TTF_TRACK | TTF_ABSOLUTE, hwnd: tip, uId: 1, lpszText: PWSTR(text.as_mut_ptr()), ..Default::default() }
}

/// This thread's tooltip, made the first time it is wanted.
fn made(dark: bool) -> Option<HWND> {
    let tip = TIP.with(|kept| match kept.get() {
        Some(tip) => Some(HWND(tip as *mut _)),
        None => {
            let style = WINDOW_STYLE(WS_POPUP.0 | TTS_ALWAYSTIP | TTS_NOPREFIX);
            let tip = unsafe { CreateWindowExW(WS_EX_TOPMOST | WS_EX_TRANSPARENT | WS_EX_NOACTIVATE, TOOLTIPS_CLASSW, PCWSTR::null(), style, 0, 0, 0, 0, None, None, None, None) }.ok()?;
            let mut none = [0u16];
            unsafe {
                SendMessageW(tip, TTM_ADDTOOLW, Some(WPARAM(0)), Some(LPARAM(&tool(tip, &mut none) as *const _ as isize)));
                SendMessageW(tip, TTM_SETMAXTIPWIDTH, Some(WPARAM(0)), Some(LPARAM(WIDEST)));
            }
            kept.set(Some(tip.0 as isize));
            Some(tip)
        }
    })?;
    // (As the apps are: dark with them.)
    let _ = unsafe { SetWindowTheme(tip, if dark { w!("DarkMode_Explorer") } else { w!("Explorer") }, PCWSTR::null()) };
    Some(tip)
}

/// Says `text` under `at` (a point of the screen), till `hide`.
pub fn show(text: &str, at: POINT, dark: bool) {
    let Some(tip) = made(dark) else { return };
    let mut wide: Vec<u16> = text.encode_utf16().chain([0]).collect();
    let tool = tool(tip, &mut wide);
    unsafe {
        SendMessageW(tip, TTM_UPDATETIPTEXTW, Some(WPARAM(0)), Some(LPARAM(&tool as *const _ as isize)));
        SendMessageW(tip, TTM_TRACKPOSITION, Some(WPARAM(0)), Some(LPARAM(((at.y + UNDER) as isize) << 16 | (at.x as isize & 0xFFFF))));
        SendMessageW(tip, TTM_TRACKACTIVATE, Some(WPARAM(1)), Some(LPARAM(&tool as *const _ as isize)));
        // Over what it speaks of, which is above all itself.
        let _ = SetWindowPos(tip, Some(HWND_TOPMOST), 0, 0, 0, 0, SWP_NOACTIVATE | SWP_NOMOVE | SWP_NOSIZE);
    }
    SAID.with(|said| *said.borrow_mut() = text.to_string());
}

/// Says nothing more.
pub fn hide() {
    if SAID.with(|said| std::mem::take(&mut *said.borrow_mut())).is_empty() {
        return;
    }
    let Some(tip) = TIP.with(Cell::get) else { return };
    let tip = HWND(tip as *mut _);
    let mut none = [0u16];
    unsafe { SendMessageW(tip, TTM_TRACKACTIVATE, Some(WPARAM(0)), Some(LPARAM(&tool(tip, &mut none) as *const _ as isize))) };
}
