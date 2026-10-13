//! What touches the PC while the phone remote has the screens out, heard
//! for the remote: it runs as the user, and Windows lets it hear nothing of
//! what goes to a window of a program that is elevated (see `touch`). The
//! sensors' process is, and hears it all: while there is one, it is asked
//! to listen; without one, Glance listens itself, and hears what it can.
//! The remote says when it has the screens out (an event of its own making,
//! set meanwhile); only then is anything listened for, and what is heard
//! (which device, when, and whether it said anything was moved or pressed:
//! never what) is left in a piece of memory shared by name, as Glance
//! leaves its readings. Listening here, the mouse is heard by the panel's
//! window, which hears it for the edges (a process hears a kind of device
//! at one window only), and passes it on (see `mouse_input`).

use std::sync::Mutex;
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
/// What is heard here is left for the remote at most this often (a mouse
/// reports a thousand times a second), and at once when its kind changes
/// (a touch after a report that said nothing, or the other way).
const TOLD_EVERY: Duration = Duration::from_millis(50);

/// Where what is heard is left for the remote.
static SHARED: Mutex<Option<crate::remote::Shared>> = Mutex::new(None);

/// What has been heard here since listening began, and what of it the
/// remote was last told (what kind, and when).
struct Listening {
    heard: Heard,
    last: Option<(bool, Instant)>,
}

/// While it is listened for here: what has been heard.
static HEARD: Mutex<Option<Listening>> = Mutex::new(None);

/// Whether the remote has the screens out now.
fn out() -> bool {
    let Ok(said) = (unsafe { OpenEventW(SYNCHRONIZATION_SYNCHRONIZE, false, w!("Local\\Glance.Remote.Out")) }) else { return false };
    let out = unsafe { WaitForSingleObject(said, 0) } == WAIT_OBJECT_0;
    let _ = unsafe { CloseHandle(said) };
    out
}

/// Leaves for the remote the last touch and the last report that said nothing (none: nothing heard).
fn tell(touched: &Option<(String, u32)>, nothing: &Option<(String, u32)>) {
    let one = |heard: &Option<(String, u32)>| heard.as_ref().map(|(device, at)| json!([device, at]));
    if let Some(shared) = SHARED.lock().unwrap().as_mut() {
        shared.write(json!({ "touched": one(touched), "nothing": one(nothing) }).to_string().as_bytes());
    }
}

/// What the sensors' process heard (see `link::Told::Activity`), left for the remote.
pub fn heard(touched: Option<(String, u32)>, nothing: Option<(String, u32)>) {
    tell(&touched, &nothing);
}

/// A WM_INPUT heard here, taken while it is listened for here.
fn take(input: LPARAM) {
    let mut heard = HEARD.lock().unwrap();
    let Some(listening) = heard.as_mut() else { return };
    if let Some(said) = listening.heard.take(input, unsafe { GetTickCount() }) {
        let now = Instant::now();
        if listening.last.is_none_or(|(kind, at)| kind != said || now.duration_since(at) >= TOLD_EVERY) {
            tell(&listening.heard.touched, &listening.heard.nothing);
            listening.last = Some((said, now));
        }
    }
}

/// The mouse's raw input, as the panel's window hears it (see the module's note).
pub fn mouse_input(input: LPARAM) {
    take(input);
}

unsafe extern "system" fn told(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == WM_INPUT {
        take(lparam);
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// Listens, on a thread of its own, for as long as Glance runs; nothing
/// is heard (and Windows is asked for nothing) but while the remote has
/// the screens out: by the sensors' process while there is one, else here.
pub fn start() {
    std::thread::spawn(|| unsafe {
        let Some(shared) = crate::remote::Shared::open(w!("Local\\Glance.Touches")) else { return };
        *SHARED.lock().unwrap() = Some(shared);
        let class = WNDCLASSW { lpfnWndProc: Some(told), lpszClassName: w!("GlanceTouches"), ..Default::default() };
        RegisterClassW(&class);
        let Ok(window) = CreateWindowExW(WINDOW_EX_STYLE(0), class.lpszClassName, w!(""), WINDOW_STYLE(0), 0, 0, 0, 0, Some(HWND_MESSAGE), None, None, None) else { return };
        // Whether it is heard here; what the sensors' process was last asked, and over which link.
        let mut hearing = false;
        let mut asked: Option<(bool, u64)> = None;
        let mut looked = Instant::now() - ASKED_EVERY;
        loop {
            if looked.elapsed() >= ASKED_EVERY {
                looked = Instant::now();
                let wanted = out();
                match crate::source::link() {
                    Some(link) => {
                        if hearing && crate::touch::hear_but_the_mouse(window, false).is_ok() {
                            hearing = false;
                            *HEARD.lock().unwrap() = None;
                        }
                        // Asked again on a new link (it knows nothing of what the last was asked); nothing heard
                        // of what it is asked for till it says.
                        if asked != Some((wanted, link)) {
                            crate::source::hear_activity(wanted);
                            asked = Some((wanted, link));
                            tell(&None, &None);
                        }
                    }
                    None => {
                        // The link gone: nothing it heard stands.
                        if asked.take().is_some() {
                            tell(&None, &None);
                        }
                        if wanted != hearing && crate::touch::hear_but_the_mouse(window, wanted).is_ok() {
                            hearing = wanted;
                            // (Each time anew: what was heard the last time is not of this one.)
                            *HEARD.lock().unwrap() = wanted.then(|| Listening { heard: Heard::default(), last: None });
                            tell(&None, &None);
                        }
                    }
                }
            }
            MsgWaitForMultipleObjects(None, false, ASKED_EVERY.saturating_sub(looked.elapsed()).as_millis() as u32, QS_ALLINPUT);
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                DispatchMessageW(&message);
            }
        }
    });
}
