//! A small window that asks before something leaves this PC: what would be
//! sent, whole, in a box that scrolls, and two buttons. Windows' own
//! message box would grow to the height of the text; this stays small
//! however long the text is.

use std::cell::Cell;

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateFontIndirectW, DeleteObject, GetSysColorBrush, COLOR_BTNFACE, HFONT};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForWindow, SystemParametersInfoForDpi};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, GetWindowRect, IsDialogMessageW, IsWindow, LoadCursorW, RegisterClassExW, SendMessageW, SetForegroundWindow,
    TranslateMessage, BS_DEFPUSHBUTTON, ES_AUTOVSCROLL, ES_MULTILINE, ES_READONLY, HMENU, IDC_ARROW, MSG, NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_SETFONT,
    WNDCLASSEXW, WS_BORDER, WS_CAPTION, WS_CHILD, WS_EX_DLGMODALFRAME, WS_POPUP, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};

/// The two buttons, as their window tells which was pressed.
const YES: isize = 1;
const NO: isize = 2;
/// The window's size and what is inside it (DIPs).
const WIDTH: i32 = 620;
const HEIGHT: i32 = 500;
const PAD: i32 = 16;
const INTRO: i32 = 54;
const BUTTON: (i32, i32) = (112, 32);

thread_local! {
    /// How the window that is up was answered, once it was.
    static ANSWER: Cell<Option<bool>> = const { Cell::new(None) };
}

unsafe extern "system" fn procedure(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        // A button (by its number), or Enter and Escape as a dialog has them.
        WM_COMMAND => {
            match (wparam.0 & 0xFFFF) as isize {
                YES => ANSWER.set(Some(true)),
                NO => ANSWER.set(Some(false)),
                _ => {}
            }
            LRESULT(0)
        }
        WM_CLOSE => {
            ANSWER.set(Some(false));
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

/// Shows `text` to be read (under `intro`, which says what it is) in a
/// window over `owner`, and asks: whether `yes` was the answer. `owner`
/// takes nothing till it is answered.
pub fn read_and_agree(owner: HWND, title: &str, intro: &str, text: &str, yes: &str, no: &str) -> bool {
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else { return false };
        let class = w!("GlanceAsk");
        // (Registered once; again is refused, which is as well.)
        RegisterClassExW(&WNDCLASSEXW { cbSize: size_of::<WNDCLASSEXW>() as u32, lpfnWndProc: Some(procedure), hInstance: module.into(), hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(), hbrBackground: GetSysColorBrush(COLOR_BTNFACE), lpszClassName: class, ..Default::default() });
        let dpi = GetDpiForWindow(owner).max(96) as i32;
        let px = |dips: i32| dips * dpi / 96;
        // In the middle of the window it is over.
        let mut over = RECT::default();
        let _ = GetWindowRect(owner, &mut over);
        let (x, y) = ((over.left + over.right - px(WIDTH)) / 2, (over.top + over.bottom - px(HEIGHT)) / 2);
        let Ok(window) = CreateWindowExW(WS_EX_DLGMODALFRAME, class, &HSTRING::from(title), WS_POPUP | WS_CAPTION | WS_SYSMENU, x, y, px(WIDTH), px(HEIGHT), Some(owner), None, Some(module.into()), None) else { return false };
        // The font Windows writes its own messages in, at this screen's scale.
        let mut metrics = NONCLIENTMETRICSW { cbSize: size_of::<NONCLIENTMETRICSW>() as u32, ..Default::default() };
        let font: HFONT = if SystemParametersInfoForDpi(SPI_GETNONCLIENTMETRICS.0, metrics.cbSize, Some((&mut metrics as *mut NONCLIENTMETRICSW).cast()), 0, dpi as u32).is_ok() { CreateFontIndirectW(&metrics.lfMessageFont) } else { HFONT::default() };
        let mut inside = RECT::default();
        let _ = windows::Win32::UI::WindowsAndMessaging::GetClientRect(window, &mut inside);
        let (width, height) = (inside.right, inside.bottom);
        let child = |kind: PCWSTR, words: &str, style: u32, at: (i32, i32, i32, i32), number: isize| {
            let made = CreateWindowExW(Default::default(), kind, &HSTRING::from(words), WS_CHILD | WS_VISIBLE | WINDOW_STYLE(style), at.0, at.1, at.2, at.3, Some(window), Some(HMENU(number as *mut _)), Some(module.into()), None).unwrap_or_default();
            SendMessageW(made, WM_SETFONT, Some(WPARAM(font.0 as usize)), Some(LPARAM(1)));
            made
        };
        child(w!("STATIC"), intro, 0, (px(PAD), px(PAD), width - 2 * px(PAD), px(INTRO)), 0);
        // (Lines as Windows' text boxes break them.)
        let lines = text.replace("\r\n", "\n").replace('\n', "\r\n");
        let top = px(PAD) + px(INTRO);
        let bottom = height - px(PAD) - px(BUTTON.1) - px(PAD);
        child(w!("EDIT"), &lines, (WS_BORDER | WS_VSCROLL | WS_TABSTOP).0 | (ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL) as u32, (px(PAD), top, width - 2 * px(PAD), bottom - top), 0);
        let across = height - px(PAD) - px(BUTTON.1);
        child(w!("BUTTON"), no, WS_TABSTOP.0, (width - px(PAD) - px(BUTTON.0), across, px(BUTTON.0), px(BUTTON.1)), NO);
        let agree = child(w!("BUTTON"), yes, WS_TABSTOP.0 | BS_DEFPUSHBUTTON as u32, (width - px(PAD) - 2 * px(BUTTON.0) - px(8), across, px(BUTTON.0), px(BUTTON.1)), YES);

        ANSWER.set(None);
        let _ = EnableWindow(owner, false);
        let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(window, windows::Win32::UI::WindowsAndMessaging::SW_SHOW);
        let _ = SetFocus(Some(agree));
        // Its own turn at the messages, till it is answered (or gone).
        let mut message = MSG::default();
        while ANSWER.get().is_none() && IsWindow(Some(window)).as_bool() && GetMessageW(&mut message, None, 0, 0).as_bool() {
            if !IsDialogMessageW(window, &message).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        let _ = EnableWindow(owner, true);
        let _ = DestroyWindow(window);
        let _ = SetForegroundWindow(owner);
        if !font.is_invalid() {
            let _ = DeleteObject(font.into());
        }
        ANSWER.take() == Some(true)
    }
}
