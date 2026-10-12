//! A small window that asks before something leaves this PC: what would be
//! sent, whole, in a box that scrolls, and two buttons. Windows' own
//! message box would grow to the height of the text; this stays small
//! however long the text is. It can take a few words of the reader's own,
//! and pictures, to go with what is sent.

use std::cell::{Cell, RefCell};

use windows::core::{w, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{CreateFontIndirectW, DeleteObject, GetMonitorInfoW, GetSysColorBrush, MonitorFromWindow, COLOR_BTNFACE, HFONT, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::UI::HiDpi::{GetDpiForWindow, SystemParametersInfoForDpi};
use windows::Win32::UI::Input::KeyboardAndMouse::{EnableWindow, GetKeyState, SetFocus, VK_CONTROL};

use super::picture::{self, Picture};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, GetWindowRect, IsDialogMessageW, IsWindow, LoadCursorW, RegisterClassExW, SendMessageW, SetForegroundWindow,
    TranslateMessage, BS_DEFPUSHBUTTON, WM_KEYDOWN, ES_AUTOVSCROLL, ES_MULTILINE, ES_READONLY, HMENU, IDC_ARROW, MSG, NONCLIENTMETRICSW, SPI_GETNONCLIENTMETRICS, WINDOW_STYLE, WM_CLOSE, WM_COMMAND, WM_SETFONT,
    WNDCLASSEXW, WS_BORDER, WS_CAPTION, WS_CHILD, WS_EX_DLGMODALFRAME, WS_POPUP, WS_SYSMENU, WS_TABSTOP, WS_VISIBLE, WS_VSCROLL,
};

/// The two buttons, as their window tells which was pressed; those that
/// take a picture; and the pictures taken, the first of them (pressed, a
/// picture is taken away again).
const YES: isize = 1;
const NO: isize = 2;
const ADD: isize = 3;
const PASTE: isize = 4;
const FIRST: isize = 10;
/// The window's size and what is inside it (DIPs).
const WIDTH: i32 = 620;
const HEIGHT: i32 = 500;
const PAD: i32 = 16;
const INTRO: i32 = 54;
const BUTTON: (i32, i32) = (112, 32);
/// How many characters may be written in the box for the reader's own words.
const WORDS_MOST: usize = 500;
/// How tall that box's name and the box are (DIPs).
const WORDS: (i32, i32) = (22, 64);
/// How many pictures may go with what is sent (the site takes no more), and
/// the room each one's likeness has (DIPs).
pub const PICTURES_MOST: usize = 3;
const LIKENESS: (i32, i32) = (84, 52);
/// STM_SETIMAGE and IMAGE_BITMAP; a picture kept whole in the middle of its
/// room, which tells when it is pressed (SS_BITMAP | SS_NOTIFY | SS_CENTERIMAGE).
const SET_IMAGE: u32 = 0x0172;
const SHOWS_A_PICTURE: u32 = 0x000E | 0x0100 | 0x0200;

/// What the reader gives of their own, while the window is up: the pictures,
/// the windows their likenesses are shown in (as large as `small`, pixels),
/// the buttons that take one, and what the file chooser calls pictures.
struct Given {
    pictures: Vec<Picture>,
    shown: [HWND; PICTURES_MOST],
    takers: [HWND; 2],
    small: (u32, u32),
    kinds: String,
}

impl Given {
    /// Takes `more`, as many as there is room for, and shows what is held.
    fn take(&mut self, more: Vec<Picture>) {
        self.pictures.extend(more);
        self.pictures.truncate(PICTURES_MOST);
        self.show();
    }

    /// Each picture held in its place, the places after them empty; and no more to be taken when all are full.
    fn show(&self) {
        unsafe {
            for (at, &shown) in self.shown.iter().enumerate() {
                let picture = self.pictures.get(at);
                SendMessageW(shown, SET_IMAGE, Some(WPARAM(0)), Some(LPARAM(picture.map_or(0, |picture| picture.small.0 as isize))));
                let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(shown, if picture.is_some() { windows::Win32::UI::WindowsAndMessaging::SW_SHOW } else { windows::Win32::UI::WindowsAndMessaging::SW_HIDE });
            }
            for taker in self.takers {
                let _ = EnableWindow(taker, self.pictures.len() < PICTURES_MOST);
            }
        }
    }
}

thread_local! {
    /// How the window that is up was answered, once it was.
    static ANSWER: Cell<Option<bool>> = const { Cell::new(None) };
    /// What the reader has given, where the window that is up takes anything.
    static GIVEN: RefCell<Option<Given>> = const { RefCell::new(None) };
}

/// The picture files the reader chooses (in Windows' own chooser, over
/// `owner`; `kinds` is what it calls them): their paths.
fn chosen(owner: HWND, kinds: &str) -> Vec<String> {
    use windows::Win32::UI::Controls::Dialogs::{GetOpenFileNameW, OFN_ALLOWMULTISELECT, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_HIDEREADONLY, OFN_NOCHANGEDIR, OPENFILENAMEW};
    let filter: Vec<u16> = format!("{kinds}\0*.png;*.jpg;*.jpeg;*.bmp;*.gif;*.webp\0\0").encode_utf16().collect();
    let mut paths = vec![0u16; 16 << 10];
    let mut asked = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: windows::core::PWSTR(paths.as_mut_ptr()),
        nMaxFile: paths.len() as u32,
        Flags: OFN_EXPLORER | OFN_ALLOWMULTISELECT | OFN_FILEMUSTEXIST | OFN_HIDEREADONLY | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    if !unsafe { GetOpenFileNameW(&mut asked) }.as_bool() {
        return Vec::new();
    }
    // One file chosen is its whole path; several are their folder, then each one's name.
    let named: Vec<String> = paths.split(|&c| c == 0).take_while(|part| !part.is_empty()).map(String::from_utf16_lossy).collect();
    match named.split_first() {
        Some((folder, names)) if !names.is_empty() => names.iter().map(|name| format!("{}\\{name}", folder.trim_end_matches('\\'))).collect(),
        _ => named,
    }
}

/// Does `with` with what the reader has given, where the window takes anything.
fn given(with: impl FnOnce(&mut Given)) {
    GIVEN.with_borrow_mut(|given| {
        if let Some(given) = given {
            with(given)
        }
    });
}

unsafe extern "system" fn procedure(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    match message {
        // A button (by its number), or Enter and Escape as a dialog has them.
        WM_COMMAND => {
            match (wparam.0 & 0xFFFF) as isize {
                YES => ANSWER.set(Some(true)),
                NO => ANSWER.set(Some(false)),
                ADD => {
                    // (The chooser has its own turn at the messages: nothing held while it is up.)
                    let asked = GIVEN.with_borrow(|given| given.as_ref().map(|given| (given.kinds.clone(), given.small)));
                    if let Some((kinds, small)) = asked {
                        let more = chosen(hwnd, &kinds).iter().filter_map(|path| picture::from_file(path, small)).collect();
                        given(|given| given.take(more));
                    }
                }
                PASTE => given(|given| given.take(picture::from_clipboard(hwnd, given.small))),
                // A picture pressed is taken away again.
                pressed if (FIRST..FIRST + PICTURES_MOST as isize).contains(&pressed) => given(|given| {
                    if ((pressed - FIRST) as usize) < given.pictures.len() {
                        given.pictures.remove((pressed - FIRST) as usize);
                        given.show();
                    }
                }),
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

/// What the window calls the reader's own part: the box for their words,
/// the two buttons that take a picture (from a file, from the clipboard),
/// and pictures, as the file chooser lists them.
pub struct Own<'a> {
    pub words: &'a str,
    pub add: &'a str,
    pub paste: &'a str,
    pub kinds: &'a str,
}

/// What the reader gave of their own: their words (none, perhaps) and
/// their pictures, each a JPEG.
pub struct Said {
    pub words: String,
    pub pictures: Vec<Vec<u8>>,
}

/// Shows `text` to be read (under `intro`, which says what it is) in a
/// window over `owner`, and asks: whether `yes` was the answer. `owner`
/// takes nothing till it is answered.
pub fn read_and_agree(owner: HWND, title: &str, intro: &str, text: &str, yes: &str, no: &str) -> bool {
    read_and_say(owner, title, intro, None, text, yes, no).is_some()
}

/// The same, and with `own`, the reader may write a few words over the text
/// and add pictures (from files, or the clipboard: Ctrl+V with a picture
/// there adds it): what they gave, if `yes` was the answer; none, if not.
pub fn read_and_say(owner: HWND, title: &str, intro: &str, own: Option<&Own>, text: &str, yes: &str, no: &str) -> Option<Said> {
    unsafe {
        let module = GetModuleHandleW(None).ok()?;
        let class = w!("GlanceAsk");
        // (Registered once; again is refused, which is as well.)
        RegisterClassExW(&WNDCLASSEXW { cbSize: size_of::<WNDCLASSEXW>() as u32, lpfnWndProc: Some(procedure), hInstance: module.into(), hCursor: LoadCursorW(None, IDC_ARROW).unwrap_or_default(), hbrBackground: GetSysColorBrush(COLOR_BTNFACE), lpszClassName: class, ..Default::default() });
        let dpi = GetDpiForWindow(owner).max(96) as i32;
        let px = |dips: i32| dips * dpi / 96;
        // Taller by the reader's own part, where it has one; in the middle of the window it is over.
        let own_part = if own.is_some() { WORDS.0 + WORDS.1 + PAD / 2 + LIKENESS.1 + PAD / 2 } else { 0 };
        let mut over = RECT::default();
        let _ = GetWindowRect(owner, &mut over);
        // (And within the screen that window is on, no taller than it: the text's box is what gives.)
        let mut screen = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(MonitorFromWindow(owner, MONITOR_DEFAULTTONEAREST), &mut screen);
        let room = screen.rcWork;
        let (wide, tall) = (px(WIDTH).min(room.right - room.left), px(HEIGHT + own_part).min(room.bottom - room.top));
        let (x, y) = (((over.left + over.right - wide) / 2).clamp(room.left, room.right - wide), ((over.top + over.bottom - tall) / 2).clamp(room.top, room.bottom - tall));
        let window = CreateWindowExW(WS_EX_DLGMODALFRAME, class, &HSTRING::from(title), WS_POPUP | WS_CAPTION | WS_SYSMENU, x, y, wide, tall, Some(owner), None, Some(module.into()), None).ok()?;
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
        let mut top = px(PAD) + px(INTRO);
        // The reader's own part, where they are asked for it: over what is sent, which it goes with.
        let written = own.map(|own| {
            child(w!("STATIC"), own.words, 0, (px(PAD), top, width - 2 * px(PAD), px(WORDS.0)), 0);
            let written = child(w!("EDIT"), "", (WS_BORDER | WS_VSCROLL | WS_TABSTOP).0 | (ES_MULTILINE | ES_AUTOVSCROLL) as u32, (px(PAD), top + px(WORDS.0), width - 2 * px(PAD), px(WORDS.1)), 0);
            SendMessageW(written, windows::Win32::UI::Controls::EM_SETLIMITTEXT, Some(WPARAM(WORDS_MOST)), None);
            top += px(WORDS.0) + px(WORDS.1) + px(PAD) / 2;
            // The buttons that take a picture, and the pictures after them.
            let middle = top + (px(LIKENESS.1) - px(BUTTON.1)) / 2;
            let takers = [
                child(w!("BUTTON"), own.add, WS_TABSTOP.0, (px(PAD), middle, px(BUTTON.0), px(BUTTON.1)), ADD),
                child(w!("BUTTON"), own.paste, WS_TABSTOP.0, (px(PAD) + px(BUTTON.0) + px(8), middle, px(BUTTON.0), px(BUTTON.1)), PASTE),
            ];
            let first = px(PAD) + 2 * (px(BUTTON.0) + px(8)) + px(8);
            let shown = std::array::from_fn(|at| child(w!("STATIC"), "", SHOWS_A_PICTURE, (first + at as i32 * (px(LIKENESS.0) + px(8)), top, px(LIKENESS.0), px(LIKENESS.1)), FIRST + at as isize));
            let given = Given { pictures: Vec::new(), shown, takers, small: (px(LIKENESS.0) as u32, px(LIKENESS.1) as u32), kinds: own.kinds.to_string() };
            given.show();
            GIVEN.set(Some(given));
            top += px(LIKENESS.1) + px(PAD) / 2;
            written
        });
        let bottom = height - px(PAD) - px(BUTTON.1) - px(PAD);
        child(w!("EDIT"), &lines, (WS_BORDER | WS_VSCROLL | WS_TABSTOP).0 | (ES_MULTILINE | ES_READONLY | ES_AUTOVSCROLL) as u32, (px(PAD), top, width - 2 * px(PAD), bottom - top), 0);
        let across = height - px(PAD) - px(BUTTON.1);
        child(w!("BUTTON"), no, WS_TABSTOP.0, (width - px(PAD) - px(BUTTON.0), across, px(BUTTON.0), px(BUTTON.1)), NO);
        let agree = child(w!("BUTTON"), yes, WS_TABSTOP.0 | BS_DEFPUSHBUTTON as u32, (width - px(PAD) - 2 * px(BUTTON.0) - px(8), across, px(BUTTON.0), px(BUTTON.1)), YES);

        ANSWER.set(None);
        let _ = EnableWindow(owner, false);
        let _ = windows::Win32::UI::WindowsAndMessaging::ShowWindow(window, windows::Win32::UI::WindowsAndMessaging::SW_SHOW);
        let _ = SetFocus(Some(written.unwrap_or(agree)));
        // Its own turn at the messages, till it is answered (or gone).
        let mut message = MSG::default();
        while ANSWER.get().is_none() && IsWindow(Some(window)).as_bool() && GetMessageW(&mut message, None, 0, 0).as_bool() {
            // Ctrl+V with a picture on the clipboard adds it, wherever in the window it is pressed.
            if written.is_some() && message.message == WM_KEYDOWN && message.wParam.0 == 'V' as usize && GetKeyState(VK_CONTROL.0 as i32) < 0 && picture::on_clipboard() {
                given(|given| given.take(picture::from_clipboard(window, given.small)));
                continue;
            }
            if !IsDialogMessageW(window, &message).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        }
        // (What was written, read before its box goes with the window.)
        let words = written.map_or(String::new(), |written| {
            let mut text = vec![0u16; WORDS_MOST + 1];
            let length = windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(written, &mut text).max(0) as usize;
            String::from_utf16_lossy(&text[..length]).trim().to_string()
        });
        let _ = EnableWindow(owner, true);
        let _ = DestroyWindow(window);
        let _ = SetForegroundWindow(owner);
        if !font.is_invalid() {
            let _ = DeleteObject(font.into());
        }
        let pictures = GIVEN.take().map_or(Vec::new(), |mut given| given.pictures.iter_mut().map(|picture| std::mem::take(&mut picture.jpeg)).collect());
        (ANSWER.take() == Some(true)).then_some(Said { words, pictures })
    }
}

#[cfg(test)]
mod tests {
    /// Puts the window up for a few seconds, to be looked at: `cargo test -- --ignored shows_the_window`.
    #[test]
    #[ignore]
    fn shows_the_window() {
        use windows::core::w;
        use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, FindWindowW, PostMessageW, WM_CLOSE, WS_POPUP, WS_VISIBLE};
        unsafe {
            let owner = CreateWindowExW(Default::default(), w!("STATIC"), w!(""), WS_POPUP | WS_VISIBLE, 300, 200, 900, 700, None, None, None, None).unwrap();
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_secs(8));
                let _ = PostMessageW(FindWindowW(w!("GlanceAsk"), None).ok(), WM_CLOSE, Default::default(), Default::default());
            });
            let own = super::Own { words: "想告诉我们什么？比如哪里显示得不对、哪个读数没有（可以不填）", add: "添加截图…", paste: "粘贴截图", kinds: "图片" };
            let text = (0..60).map(|line| format!("Line {line} of what is sent")).collect::<Vec<_>>().join("\n");
            assert!(super::read_and_say(owner, "发送诊断信息", "下面是将要发送给 Glance（glancepc.com）的全部内容：这台电脑的硬件、Glance 读到的数值和它最近的日志。不包含你的文件、账号或网络地址。你写的话和添加的截图会一起发送。", Some(&own), &text, "发送", "取消").is_none());
        }
    }
}
