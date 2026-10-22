//! A window of the panel's thread taking the keyboard for words written in it where they show (a note stuck on the
//! desktop, written on its widget: see `widget`), any of its windows, for whatever writes there. Such a window never
//! takes the focus (see `window`) but while it does this: it comes to the front then, and what writes in it answers its
//! messages (the keys, the input method's, the pointer's over the words, its own timers), given it whatever the window
//! is in the middle of (an input method answers inside the message that asked it). Esc, or another program's window
//! come in front, gives the keys back (told as `LEFT`, for its owner to see to): Esc where what writes there does not
//! keep it for itself (a link being written there, left by it).

use std::cell::RefCell;
use std::rc::Rc;

use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, WPARAM};
use windows::Win32::UI::Input::KeyboardAndMouse::{GetDoubleClickTime, VK_ESCAPE};
use windows::Win32::UI::WindowsAndMessaging::{
    DispatchMessageW, GetMessageTime, GetSystemMetrics, PostMessageW, MSG, SM_CXDOUBLECLK, SM_CYDOUBLECLK, WA_INACTIVE, WM_ACTIVATE, WM_APP, WM_CHAR, WM_DEADCHAR, WM_IME_CHAR, WM_IME_COMPOSITION, WM_IME_ENDCOMPOSITION, WM_IME_STARTCOMPOSITION,
    WM_KEYDOWN, WM_KEYUP, WM_LBUTTONDBLCLK, WM_LBUTTONDOWN, WM_SYSCHAR, WM_SYSKEYDOWN, WM_SYSKEYUP, WM_TIMER, WM_UNICHAR, TranslateMessage,
};

/// Posted to a window taking the keys as they are to be given back: Esc pressed (WPARAM 1), or it no longer in front
/// (0: another of its thread's, a menu, may have come in front, see to it as that goes).
pub const LEFT: u32 = WM_APP + 17;

/// What answers a window's messages while it takes the keys: none, a message not its, else what it answered (none: the
/// system's to answer as well, an input method's).
pub type Answer = Rc<dyn Fn(HWND, u32, WPARAM, LPARAM) -> Option<Option<LRESULT>>>;

thread_local! {
    /// The windows taking the keys, and what answers each.
    static TAKING: RefCell<Vec<(isize, Answer)>> = const { RefCell::new(Vec::new()) };
    /// The windows whose words took a message since this was last asked (see `took`).
    static TOOK: RefCell<Vec<isize>> = const { RefCell::new(Vec::new()) };
}

/// `hwnd` taking the keys, its messages answered by `answer` from now (see the top).
pub fn take(hwnd: HWND, answer: Answer) {
    TAKING.with_borrow_mut(|taking| {
        taking.retain(|(window, _)| *window != hwnd.0 as isize);
        taking.push((hwnd.0 as isize, answer));
    });
}

/// `hwnd` taking the keys no more.
pub fn give_back(hwnd: HWND) {
    TAKING.with_borrow_mut(|taking| taking.retain(|(window, _)| *window != hwnd.0 as isize));
}

/// Whether `hwnd` takes the keys.
pub fn taking(hwnd: HWND) -> bool {
    TAKING.with_borrow(|taking| taking.iter().any(|(window, _)| *window == hwnd.0 as isize))
}

/// A message for `hwnd` (from its window's procedure): answered where it takes the keys and it is the words' (see
/// `Answer`), or Esc (not theirs), or the window gone from the front (told as `LEFT`). None: the window's own.
pub fn answer(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
    // (Cloned out first: what answers may be asked again from inside its answer.)
    let answer = TAKING.with_borrow(|taking| taking.iter().find(|(window, _)| *window == hwnd.0 as isize).map(|(_, answer)| answer.clone()))?;
    if message == WM_ACTIVATE && (wparam.0 & 0xFFFF) as u32 == WA_INACTIVE {
        let _ = unsafe { PostMessageW(Some(hwnd), LEFT, WPARAM(0), LPARAM(0)) };
    }
    let escaped = message == WM_KEYDOWN && wparam.0 == usize::from(VK_ESCAPE.0);
    let Some(answered) = answer(hwnd, message, wparam, lparam) else {
        if escaped {
            let _ = unsafe { PostMessageW(Some(hwnd), LEFT, WPARAM(1), LPARAM(0)) };
            return Some(LRESULT(0));
        }
        return None;
    };
    TOOK.with_borrow_mut(|took| {
        if !took.contains(&(hwnd.0 as isize)) {
            took.push(hwnd.0 as isize);
        }
    });
    answered
}

/// The windows whose words took a message since this was last asked: theirs to see to what was written (dressed, kept,
/// drawn again).
pub fn took() -> Vec<HWND> {
    TOOK.with_borrow_mut(std::mem::take).into_iter().map(|window| HWND(window as *mut _)).collect()
}

/// `message`, taken from the thread's queue, handed on where it is a window's taking the keys, of those its words take
/// there (the keys, the characters they make, the input method's, its timers): the keys' characters made (but Esc's,
/// which gives the keys back) and given to it as it is, nothing else done with it. Whether it was.
pub fn queued(message: &MSG) -> bool {
    if !(matches!(message.message, WM_KEYDOWN | WM_KEYUP | WM_SYSKEYDOWN | WM_SYSKEYUP | WM_CHAR | WM_DEADCHAR | WM_SYSCHAR | WM_UNICHAR | WM_IME_CHAR | WM_IME_STARTCOMPOSITION | WM_IME_COMPOSITION | WM_IME_ENDCOMPOSITION | WM_TIMER) && taking(message.hwnd)) {
        return false;
    }
    unsafe {
        if !(message.message == WM_KEYDOWN && message.wParam.0 == usize::from(VK_ESCAPE.0)) {
            let _ = TranslateMessage(message);
        }
        DispatchMessageW(message);
    }
    true
}

/// The presses on a window that tells none twice in a row apart (its class asks for no double clicks: see `window`):
/// where and when the last was, to tell a second as Windows would.
#[derive(Default)]
pub struct Clicks {
    last: Option<(u32, POINT)>,
}

impl Clicks {
    /// A press at `point` (in the window): `WM_LBUTTONDBLCLK` where it is the second of two within the time and the room
    /// Windows gives a double click (as it would tell it to a window that asks), else `WM_LBUTTONDOWN`.
    pub fn press(&mut self, point: POINT) -> u32 {
        let now = unsafe { GetMessageTime() } as u32;
        let (wide, tall) = unsafe { (GetSystemMetrics(SM_CXDOUBLECLK), GetSystemMetrics(SM_CYDOUBLECLK)) };
        let second = self.last.is_some_and(|(at, from)| now.wrapping_sub(at) <= unsafe { GetDoubleClickTime() } && (point.x - from.x).abs() <= wide / 2 && (point.y - from.y).abs() <= tall / 2);
        // (A third press begins anew: Windows tells a double click, then a press.)
        self.last = if second { None } else { Some((now, point)) };
        if second { WM_LBUTTONDBLCLK } else { WM_LBUTTONDOWN }
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::cell::RefCell;
    use windows::Win32::UI::WindowsAndMessaging::{PeekMessageW, SendMessageW, PM_REMOVE};

    /// A second press near the first, soon after, told as a double click (as Windows tells one to a window that asks),
    /// a third begins anew, one further away than a double click's room, a press.
    #[test]
    fn a_second_press_near_the_first_is_a_double_click() {
        let mut clicks = Clicks::default();
        let at = POINT { x: 40, y: 40 };
        assert_eq!(clicks.press(at), WM_LBUTTONDOWN);
        assert_eq!(clicks.press(POINT { x: 41, y: 40 }), WM_LBUTTONDBLCLK);
        assert_eq!(clicks.press(at), WM_LBUTTONDOWN);
        let room = unsafe { GetSystemMetrics(SM_CXDOUBLECLK) };
        assert_eq!(clicks.press(POINT { x: 40 + room, y: 40 }), WM_LBUTTONDOWN);
    }

    /// A window taking the keys: its messages answered by what writes in it, and told as taken. Esc, and the window gone
    /// from the front, give the keys back (told as `LEFT`, Esc not the words'). Given back, nothing answered. Esc the
    /// words keep for themselves (a link being written left by it) leaves the keys taken.
    #[test]
    fn esc_or_another_window_in_front_gives_the_keys_back() {
        let window = crate::ui::window::Window::new().unwrap();
        let hwnd = window.hwnd;
        let heard = Rc::new(RefCell::new(Vec::new()));
        let hears = heard.clone();
        let keeps_esc = Rc::new(std::cell::Cell::new(false));
        let keeps = keeps_esc.clone();
        take(
            hwnd,
            Rc::new(move |_, message, wparam, _| {
                if message == WM_KEYDOWN && wparam.0 == usize::from(VK_ESCAPE.0) && !keeps.get() {
                    return None;
                }
                hears.borrow_mut().push(message);
                Some(Some(LRESULT(0)))
            }),
        );
        let send = |message: u32, wparam: usize| unsafe { SendMessageW(hwnd, message, Some(WPARAM(wparam)), Some(LPARAM(0))) };
        send(WM_CHAR, usize::from(b'a'));
        send(WM_KEYDOWN, usize::from(VK_ESCAPE.0));
        send(WM_ACTIVATE, WA_INACTIVE as usize);
        assert_eq!(*heard.borrow(), vec![WM_CHAR, WM_ACTIVATE]);
        assert_eq!(took(), vec![hwnd]);
        let mut message = MSG::default();
        let mut left = Vec::new();
        while unsafe { PeekMessageW(&mut message, Some(hwnd), LEFT, LEFT, PM_REMOVE) }.as_bool() {
            left.push(message.wParam.0);
        }
        assert_eq!(left, vec![1, 0]);
        keeps_esc.set(true);
        send(WM_KEYDOWN, usize::from(VK_ESCAPE.0));
        assert_eq!(*heard.borrow(), vec![WM_CHAR, WM_ACTIVATE, WM_KEYDOWN]);
        let _ = took();
        assert!(!unsafe { PeekMessageW(&mut message, Some(hwnd), LEFT, LEFT, PM_REMOVE) }.as_bool());
        give_back(hwnd);
        send(WM_CHAR, usize::from(b'b'));
        assert_eq!(heard.borrow().len(), 3);
        assert!(took().is_empty());
    }
}
