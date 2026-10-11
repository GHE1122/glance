//! What touches the PC while the phone remote has the screens out, heard
//! here for the remote: it is not elevated, and Windows lets it hear
//! nothing of what goes to a window of a program that is (see `touch`);
//! Glance is, and hears it all. The remote says when it has the screens
//! out (an event of its own making, set meanwhile); only then does Glance
//! listen, and what it hears (which device, when, and whether it said
//! anything was moved or pressed: never what) it leaves in a piece of
//! memory shared by name, as it leaves its readings.

use std::time::{Duration, Instant};

use serde_json::json;
use windows::core::w;
use windows::Win32::Foundation::{CloseHandle, HWND, LPARAM, LRESULT, WAIT_OBJECT_0, WPARAM};
use windows::Win32::System::SystemInformation::GetTickCount;
use windows::Win32::System::Threading::{OpenEventW, WaitForSingleObject, SYNCHRONIZATION_SYNCHRONIZE};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DispatchMessageW, MsgWaitForMultipleObjects, PeekMessageW, RegisterClassW, HWND_MESSAGE, MSG, PM_REMOVE, QS_ALLINPUT, WINDOW_EX_STYLE, WINDOW_STYLE, WM_INPUT, WNDCLASSW,
};

use crate::touch::Heard;

/// How often the remote is asked whether it has the screens out.
const ASKED_EVERY: Duration = Duration::from_secs(1);
/// What is heard is left for the remote at most this often (a mouse
/// reports a thousand times a second), and at once when its kind changes
/// (a touch after a report that said nothing, or the other way).
const TOLD_EVERY: Duration = Duration::from_millis(50);

/// What has been heard since listening began, where it is left for the
/// remote, and what of it the remote was last told (what kind, and when).
struct Listening {
    heard: Heard,
    shared: crate::remote::Shared,
    last: Option<(bool, Instant)>,
}

thread_local! {
    static HEARD: std::cell::RefCell<Listening> = panic!("heard only on the thread that listens");
}

/// Whether the remote has the screens out now.
fn out() -> bool {
    let Ok(said) = (unsafe { OpenEventW(SYNCHRONIZATION_SYNCHRONIZE, false, w!("Local\\Glance.Remote.Out")) }) else { return false };
    let out = unsafe { WaitForSingleObject(said, 0) } == WAIT_OBJECT_0;
    let _ = unsafe { CloseHandle(said) };
    out
}

fn tell(heard: &Heard, shared: &mut crate::remote::Shared) {
    let one = |heard: &Option<(String, u32)>| heard.as_ref().map(|(device, at)| json!([device, at]));
    shared.write(json!({ "touched": one(&heard.touched), "nothing": one(&heard.nothing) }).to_string().as_bytes());
}

unsafe extern "system" fn told(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == WM_INPUT {
        HEARD.with_borrow_mut(|listening| {
            if let Some(said) = listening.heard.take(lparam, unsafe { GetTickCount() }) {
                let now = Instant::now();
                if listening.last.is_none_or(|(kind, at)| kind != said || now.duration_since(at) >= TOLD_EVERY) {
                    tell(&listening.heard, &mut listening.shared);
                    listening.last = Some((said, now));
                }
            }
        });
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// Listens, on a thread of its own, for as long as Glance runs; nothing
/// is heard (and Windows is asked for nothing) but while the remote has
/// the screens out.
pub fn start() {
    std::thread::spawn(|| unsafe {
        let Some(shared) = crate::remote::Shared::open(w!("Local\\Glance.Touches")) else { return };
        HEARD.set(Listening { heard: Heard::default(), shared, last: None });
        let class = WNDCLASSW { lpfnWndProc: Some(told), lpszClassName: w!("GlanceTouches"), ..Default::default() };
        RegisterClassW(&class);
        let Ok(window) = CreateWindowExW(WINDOW_EX_STYLE(0), class.lpszClassName, w!(""), WINDOW_STYLE(0), 0, 0, 0, 0, Some(HWND_MESSAGE), None, None, None) else { return };
        let mut hearing = false;
        let mut asked = Instant::now() - ASKED_EVERY;
        loop {
            if asked.elapsed() >= ASKED_EVERY {
                asked = Instant::now();
                let wanted = out();
                if wanted != hearing && crate::touch::hear(window, wanted).is_ok() {
                    hearing = wanted;
                    // (Each time anew: what was heard the last time is not of this one.)
                    HEARD.with_borrow_mut(|listening| {
                        listening.heard = Heard::default();
                        listening.last = None;
                        tell(&listening.heard, &mut listening.shared);
                    });
                }
            }
            MsgWaitForMultipleObjects(None, false, ASKED_EVERY.saturating_sub(asked.elapsed()).as_millis() as u32, QS_ALLINPUT);
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                DispatchMessageW(&message);
            }
        }
    });
}
