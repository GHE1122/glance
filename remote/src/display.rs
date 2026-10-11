//! The screens: whether they are on (as Windows tells a window that asks to
//! hear of it), turning them off and on, and what each monitor answers over
//! its own control channel (DDC/CI), by which a monitor is told things its
//! cable's signal does not say.
//!
//! There are two ways to put them out. Windows' own stops the signal to
//! every monitor: each then sleeps, as it would left alone. But a monitor
//! that has another computer plugged into it may go over to that one and
//! light up again, and one that sleeps deeply is gone from Windows till it
//! wakes, its windows moved to the others. The other way keeps the signal:
//! each monitor is turned off by itself where it can be, and covered in
//! black (and made as dim as it goes) where it cannot; to Windows nothing
//! has changed, and it is this helper that wakes them at a touch.
//!
//! Out either way, they stay out till the PC is touched. Not everything
//! Windows takes for a touch is one: a keyboard that is a mouse and a set
//! of media keys besides will now and then report, as each of those, that
//! nothing is moved and nothing pressed, and Windows wakes the screens at
//! that. So what is touched is heard of here (which device, and what it
//! said), and a report that says nothing is not a touch: screens that came
//! on at one are put out again.

use std::sync::atomic::{AtomicIsize, AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows::core::{w, Result, BOOL, GUID};
use windows::Win32::Devices::Display::{
    DestroyPhysicalMonitors, DisplayConfigGetDeviceInfo, GetDisplayConfigBufferSizes, GetNumberOfPhysicalMonitorsFromHMONITOR, GetPhysicalMonitorsFromHMONITOR,
    GetVCPFeatureAndVCPFeatureReply, QueryDisplayConfig, SetVCPFeature, DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_LVDS, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED, DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME, DISPLAYCONFIG_DEVICE_INFO_HEADER,
    DISPLAYCONFIG_MODE_INFO, DISPLAYCONFIG_PATH_INFO, DISPLAYCONFIG_SOURCE_DEVICE_NAME, DISPLAYCONFIG_TARGET_DEVICE_NAME, PHYSICAL_MONITOR, QDC_ONLY_ACTIVE_PATHS,
};
use windows::Win32::Foundation::{CloseHandle, GENERIC_READ, GENERIC_WRITE, HANDLE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Storage::FileSystem::{CreateFileW, FILE_SHARE_READ, FILE_SHARE_WRITE, OPEN_EXISTING};
use windows::Win32::System::IO::DeviceIoControl;
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, GetStockObject, BLACK_BRUSH, HBRUSH, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW};
use windows::Win32::System::Power::{RegisterPowerSettingNotification, SetThreadExecutionState, ES_CONTINUOUS, ES_DISPLAY_REQUIRED, POWERBROADCAST_SETTING};
use windows::Win32::System::Threading::{CreateEventW, GetCurrentThreadId, ResetEvent, SetEvent};

use crate::touch::Heard;
use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_MOVE, MOUSEINPUT};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, PostMessageW, PostThreadMessageW, RegisterClassW, SetCursor, DEVICE_NOTIFY_WINDOW_HANDLE, MSG, PBT_POWERSETTINGCHANGE,
    SC_MONITORPOWER, WINDOW_EX_STYLE, WINDOW_STYLE, WM_APP, WM_DISPLAYCHANGE, WM_INPUT, WM_POWERBROADCAST, WM_QUIT, WM_SETCURSOR, WM_SYSCOMMAND, WNDCLASSW, WS_EX_NOACTIVATE, WS_EX_TOOLWINDOW, WS_EX_TOPMOST, WS_POPUP,
    WS_VISIBLE,
};

/// GUID_CONSOLE_DISPLAY_STATE: the display off (0), on (1) or dimmed (2).
const DISPLAY_STATE: GUID = GUID::from_u128(0x6fe69556_704a_47a0_8f24_c28d936fda47);
/// Not told yet.
const UNKNOWN: u8 = u8::MAX;
/// SC_MONITORPOWER's "off".
const OFF: isize = 2;

/// The display's state as Windows last told it.
static STATE: AtomicU8 = AtomicU8::new(UNKNOWN);
/// When Windows last said the screens had gone off (see `actions::tick`), once it has.
static WENT_OFF: Mutex<Option<u32>> = Mutex::new(None);
/// The window that listens (see `watch`), once there is one.
static LISTENER: AtomicIsize = AtomicIsize::new(0);
/// Asks the listening window to hear of what is touched (1) or to stop (0).
const HEAR_TOUCHES: u32 = WM_APP + 1;

/// What this helper has heard touch the PC while it listened.
static HEARD: std::sync::LazyLock<Mutex<Heard>> = std::sync::LazyLock::new(Mutex::default);
/// Set for as long as the screens are out at this helper's asking: Glance,
/// which hears what this helper cannot (see `touch`), listens meanwhile.
static OUT_SAID: AtomicIsize = AtomicIsize::new(0);

fn hear_touches(window: HWND, hear: bool) {
    if hear {
        *HEARD.lock().unwrap() = Heard::default();
    }
    if let Err(error) = crate::touch::hear(window, hear) {
        crate::journal::note(format!("what is touched cannot be {}: {error}", if hear { "heard of" } else { "left unheard" }));
    }
}

/// The last touch, and the last report that said nothing, each by its
/// device and when: as this helper heard them or as Glance did, whichever
/// is the later.
fn heard() -> (Option<(String, u32)>, Option<(String, u32)>) {
    let now = crate::actions::tick();
    let later = |own: Option<(String, u32)>, told: Option<(String, u32)>| match (own, told) {
        (Some(own), Some(told)) => Some(if now.wrapping_sub(own.1) <= now.wrapping_sub(told.1) { own } else { told }),
        (own, told) => own.or(told),
    };
    let (touched, nothing) = crate::reads::touches();
    let own = HEARD.lock().unwrap();
    (later(own.touched.clone(), touched), later(own.nothing.clone(), nothing))
}

/// What was last heard from while the screens were out, and how long ago
/// (ms): for the log, to say what woke them.
pub fn last_touch() -> String {
    let ago = |at: u32| crate::actions::tick().wrapping_sub(at);
    let (touched, nothing) = heard();
    let touch = touched.map(|(device, at)| format!("last touched {} ms before, by {device}", ago(at)));
    let nothing = nothing.map(|(device, at)| format!("a report that said nothing {} ms before, from {device}", ago(at)));
    match (touch, nothing) {
        (None, None) => "nothing was heard from".into(),
        (touch, nothing) => [touch, nothing].into_iter().flatten().collect::<Vec<_>>().join("; "),
    }
}

/// The screens put out at this helper's asking, either way: since when
/// (see `actions::tick`), and the moment of an input Windows had that was
/// found to be a report that said nothing, till the screens it woke are
/// put out again.
struct Out {
    since: u32,
    nothing_at: Option<u32>,
}

static OUT: Mutex<Option<Out>> = Mutex::new(None);
/// How long after the screens are put out a touch is not yet minded
/// (whoever asked may still be letting go of the mouse), and how long
/// after Windows has had an input what was heard of it is waited for.
const SETTLING: u32 = 3000;
const HEARD_WITHIN: u32 = 300;

fn out(since: u32) {
    *OUT.lock().unwrap() = Some(Out { since, nothing_at: None });
    say_out(true);
}

fn say_out(out: bool) {
    let said = HANDLE(OUT_SAID.load(Ordering::SeqCst) as *mut _);
    if !said.0.is_null() {
        let _ = unsafe { if out { SetEvent(said) } else { ResetEvent(said) } };
    }
}

/// Whether the screens are out at this helper's asking.
pub fn is_out() -> bool {
    OUT.lock().unwrap().is_some()
}

/// What has happened to screens this helper put out.
#[derive(Debug, PartialEq)]
pub enum Woken {
    /// Nothing: they are out still (or were not put out).
    No,
    /// The PC was touched.
    Touched,
    /// Windows put them on at a report that said nothing was touched: they are to be put out again.
    ByNothing,
    /// Windows put them on, and nothing was heard from at all.
    Unasked,
}

/// What has happened to the screens this helper put out, since it did.
pub fn woken() -> Woken {
    let kept = kept_dark();
    let mut out = OUT.lock().unwrap();
    let Some(out) = out.as_mut() else { return Woken::No };
    let (now, input) = (crate::actions::tick(), crate::actions::last_input());
    let after = |later: u32, than: u32| later.wrapping_sub(than) as i32;
    // On again, as Windows has them: they went off after they were put out, and are on now. (Kept dark by this
    // helper, they are on to Windows all along; and till Windows says they are off, they have not yet gone.)
    let went_off = WENT_OFF.lock().unwrap().is_some_and(|at| after(at, out.since) >= 0);
    let lit = !kept && went_off && !matches!(STATE.load(Ordering::Relaxed), 0 | UNKNOWN);
    if after(input, out.since) > SETTLING as i32 && out.nothing_at != Some(input) {
        // (What was heard of it reaches this a moment after Windows has had it.)
        if after(now, input) < HEARD_WITHIN as i32 {
            return Woken::No;
        }
        let (touched, nothing) = heard();
        let then = |heard: &Option<(String, u32)>| heard.as_ref().is_some_and(|(_, at)| after(*at, input) >= -(HEARD_WITHIN as i32));
        // A touch: whatever was heard to say something, and whatever was not heard of at all.
        if then(&touched) || !then(&nothing) {
            return Woken::Touched;
        }
        out.nothing_at = Some(input);
        crate::journal::note(format!("not taken for a touch: {}", last_touch()));
    }
    if !lit {
        return Woken::No;
    }
    if out.nothing_at.take().is_some() {
        Woken::ByNothing
    } else {
        Woken::Unasked
    }
}

/// The monitors Windows has now, for the log: each one's name and place.
fn listed() -> String {
    let mut found = Vec::new();
    each_monitor(|index, name, place, _, _| found.push(format!("{index} {name} {}x{} at {},{}", place.2, place.3, place.0, place.1)));
    found.join("; ")
}

unsafe extern "system" fn told(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == WM_POWERBROADCAST && wparam.0 == PBT_POWERSETTINGCHANGE as usize {
        let setting = unsafe { &*(lparam.0 as *const POWERBROADCAST_SETTING) };
        if setting.PowerSetting == DISPLAY_STATE && setting.DataLength >= 1 {
            let (was, now) = (STATE.swap(setting.Data[0], Ordering::Relaxed), setting.Data[0]);
            // (Dimmed is on.) Out, what is touched is heard of, to say what wakes them.
            if (was == 0) != (now == 0) && was != UNKNOWN {
                if now == 0 {
                    *WENT_OFF.lock().unwrap() = Some(crate::actions::tick());
                    crate::journal::note("Windows: the screens are off");
                    hear_touches(window, true);
                } else {
                    crate::journal::note(format!("Windows: the screens are on ({})", last_touch()));
                    // (Still wanted while this helper has them out: to tell what woke them, and by its own way what is to.)
                    if !is_out() {
                        hear_touches(window, false);
                    }
                }
            }
        }
    }
    match message {
        WM_INPUT => {
            HEARD.lock().unwrap().take(lparam, crate::actions::tick());
        }
        WM_DISPLAYCHANGE => crate::journal::note(format!("the monitors changed: {}", listed())),
        HEAR_TOUCHES => hear_touches(window, wparam.0 == 1),
        _ => {}
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// Listens, on a thread of its own, for the screens going off and coming on.
pub fn watch() {
    std::thread::spawn(|| unsafe {
        let class = WNDCLASSW { lpfnWndProc: Some(told), lpszClassName: w!("GlanceRemoteDisplay"), ..Default::default() };
        RegisterClassW(&class);
        let Ok(window) = CreateWindowExW(WINDOW_EX_STYLE(0), class.lpszClassName, w!(""), WINDOW_STYLE(0), 0, 0, 0, 0, None, None, None, None) else { return };
        LISTENER.store(window.0 as isize, Ordering::SeqCst);
        // (Set by hand and cleared by hand; not set to begin with.)
        if let Ok(said) = CreateEventW(None, true, false, w!("Local\\Glance.Remote.Out")) {
            OUT_SAID.store(said.0 as isize, Ordering::SeqCst);
        }
        // Told at once how it is now, then at every change.
        let _ = RegisterPowerSettingNotification(HANDLE(window.0), &DISPLAY_STATE, DEVICE_NOTIFY_WINDOW_HANDLE);
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            DispatchMessageW(&message);
        }
    });
}

/// Whether the screens are on (dimmed is on); none, before Windows has said.
/// Kept dark by this helper, they are off, whatever Windows has them as.
pub fn on() -> Option<bool> {
    if kept_dark() {
        return Some(false);
    }
    match STATE.load(Ordering::Relaxed) {
        UNKNOWN => None,
        state => Some(state != 0),
    }
}

/// Asks, of a window of this program's own, that the monitors be powered
/// so (as Windows asks it to be done).
fn power(how: isize) -> Result<()> {
    unsafe {
        let window = CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"), w!(""), WINDOW_STYLE(0), 0, 0, 0, 0, None, None, None, None)?;
        DefWindowProcW(window, WM_SYSCOMMAND, WPARAM(SC_MONITORPOWER as usize), LPARAM(how));
        DestroyWindow(window)
    }
}

/// The screens off, until the mouse or the keyboard is touched, or `wake`.
pub fn sleep() -> Result<()> {
    out(crate::actions::tick());
    power(OFF)
}

/// A monitor put out with its signal kept: which (its place among them and
/// its name), whether it was turned off by itself (else it is covered in
/// black), and how bright it was, where it was made dim under the cover.
#[derive(Clone, Debug, PartialEq, serde::Serialize, serde::Deserialize)]
pub struct Darkened {
    pub index: usize,
    pub name: String,
    pub off: bool,
    pub brightness: Option<u8>,
}

/// A thread that holds covers up, by its number: told to end by that, and waited for.
type Covering = (u32, std::thread::JoinHandle<()>);

/// The screens kept dark by this helper: which monitors and how, and the
/// thread that holds the covers up.
struct Dark {
    monitors: Vec<Darkened>,
    covering: Covering,
}

static DARK: Mutex<Option<Dark>> = Mutex::new(None);
/// The monitors put out one by one (see `turn`), each with the thread
/// that holds its cover up, where it is covered.
static SINGLY: Mutex<Vec<(Darkened, Option<Covering>)>> = Mutex::new(Vec::new());
/// How long a monitor told to turn off is given to say it has, and how
/// often it is asked meanwhile.
const TURNING_OFF: Duration = Duration::from_millis(2000);
const ASKED_EVERY: Duration = Duration::from_millis(200);

/// Whether this helper is keeping the screens dark with their signal kept.
pub fn kept_dark() -> bool {
    DARK.lock().unwrap().is_some()
}

unsafe extern "system" fn cover(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // No pointer over it: black is all of it.
    if message == WM_SETCURSOR {
        unsafe { SetCursor(None) };
        return LRESULT(1);
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// Holds a black window over each of `places` till it is told to end
/// (and, with `signal_kept`, the signal on meanwhile: Windows is not to
/// stop it for want of a touch); its thread's number is sent once the
/// covers are up.
fn hold_covers(places: Vec<(i32, i32, i32, i32)>, signal_kept: bool, up: std::sync::mpsc::Sender<u32>) {
    unsafe {
        let class = WNDCLASSW { lpfnWndProc: Some(cover), lpszClassName: w!("GlanceRemoteCover"), hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0), ..Default::default() };
        RegisterClassW(&class);
        if signal_kept {
            SetThreadExecutionState(ES_CONTINUOUS | ES_DISPLAY_REQUIRED);
        }
        let covers: Vec<HWND> = places
            .iter()
            .filter_map(|(left, top, width, height)| CreateWindowExW(WS_EX_TOPMOST | WS_EX_TOOLWINDOW | WS_EX_NOACTIVATE, class.lpszClassName, w!(""), WS_POPUP | WS_VISIBLE, *left, *top, *width, *height, None, None, None, None).ok())
            .collect();
        if covers.len() != places.len() {
            crate::journal::note(format!("only {} of {} covers could be put up", covers.len(), places.len()));
        }
        let _ = up.send(GetCurrentThreadId());
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            DispatchMessageW(&message);
        }
        for one in covers {
            let _ = DestroyWindow(one);
        }
        if signal_kept {
            SetThreadExecutionState(ES_CONTINUOUS);
        }
    }
}

/// Covers put up over `places` on a thread of their own: the thread, once they are up.
fn covering(places: Vec<(i32, i32, i32, i32)>, signal_kept: bool) -> Option<Covering> {
    let (up, is_up) = std::sync::mpsc::channel();
    let holding = std::thread::spawn(move || hold_covers(places, signal_kept, up));
    Some((is_up.recv().ok()?, holding))
}

fn uncover(covering: Covering) {
    let _ = unsafe { PostThreadMessageW(covering.0, WM_QUIT, WPARAM(0), LPARAM(0)) };
    let _ = covering.1.join();
}

/// Puts one monitor out with its signal kept: turned off by itself where
/// it then says it is; else made as dim as it goes, to be covered in black
/// (whether it is to be covered comes with what was done to it).
fn put_out(index: usize, name: String, built_in: bool, monitor: HANDLE) -> Darkened {
    // Told to turn off, it is off once it no longer says it is on (one that says nothing of its power is not told).
    let mut off = false;
    if ask(monitor, POWER_MODE).is_some() && unsafe { SetVCPFeature(monitor, POWER_MODE, POWER_OFF) } != 0 {
        let told = Instant::now();
        while !off && told.elapsed() < TURNING_OFF {
            std::thread::sleep(ASKED_EVERY);
            off = ask(monitor, POWER_MODE) != Some(POWER_ON);
        }
    }
    let mut was = None;
    if !off {
        was = brightness(monitor).or_else(|| built_in.then(panel_brightness).flatten());
        let dimmed = match asked(monitor, BRIGHTNESS) {
            Some(_) => unsafe { SetVCPFeature(monitor, BRIGHTNESS, 0) != 0 },
            None => built_in && set_panel_brightness(0),
        };
        if !dimmed {
            was = None;
        }
    }
    crate::journal::note(format!("monitor {index} {name}: {}", if off { "turned off by itself".to_string() } else { format!("covered in black{}", if was.is_some() { " and dimmed" } else { "" }) }));
    Darkened { index, name, off, brightness: was }
}

/// Every monitor this helper has put out with its signal kept, all of
/// them together or one by one: as each is to be put back (see `restore`).
pub fn held() -> Vec<Darkened> {
    let mut held: Vec<Darkened> = DARK.lock().unwrap().as_ref().map(|dark| dark.monitors.clone()).unwrap_or_default();
    held.extend(SINGLY.lock().unwrap().iter().map(|(one, _)| one.clone()));
    held
}

/// The screens out with their signal kept: each monitor turned off by
/// itself where it says it has been; the others covered in black and made
/// as dim as they go (one put out by itself before stays as it is).
/// Already dark, nothing more is done but that what is touched just now is
/// not minded.
pub fn darken() {
    let mut dark = DARK.lock().unwrap();
    if dark.is_some() {
        out(crate::actions::tick());
        return;
    }
    let singly: Vec<usize> = SINGLY.lock().unwrap().iter().map(|(one, _)| one.index).collect();
    let mut monitors = Vec::new();
    let mut covered = Vec::new();
    each_monitor(|index, name, place, built_in, monitor| {
        if singly.contains(&index) {
            return;
        }
        let done = put_out(index, name, built_in, monitor);
        if !done.off {
            covered.push(place);
        }
        monitors.push(done);
    });
    let Some(covering) = covering(covered, true) else { return };
    let listener = LISTENER.load(Ordering::SeqCst);
    if listener != 0 {
        let _ = unsafe { PostMessageW(Some(HWND(listener as *mut _)), HEAR_TOUCHES, WPARAM(1), LPARAM(0)) };
    }
    *KNOWN.lock().unwrap() = None;
    *dark = Some(Dark { monitors, covering });
    out(crate::actions::tick());
}

/// Each of `monitors` as it was before it was put out (see `darken`): on
/// again, as bright as it was. A monitor is known by its place and its
/// name; one that has moved among the others since, by its name.
pub fn restore(monitors: &[Darkened]) {
    let mut there = Vec::new();
    each_monitor(|index, name, _, _, _| there.push((index, name)));
    for one in monitors {
        let Some((index, _)) = there.iter().find(|(index, name)| *index == one.index && *name == one.name).or_else(|| there.iter().find(|(_, name)| *name == one.name)) else {
            crate::journal::note(format!("monitor {} {} is not there to be put on again", one.index, one.name));
            continue;
        };
        if one.off && !tell(*index, POWER_MODE, POWER_ON) {
            crate::journal::note(format!("monitor {index} {} did not take being turned on", one.name));
        }
        if let Some(percent) = one.brightness {
            if !dim(*index, percent) {
                crate::journal::note(format!("monitor {index} {} did not take its brightness back", one.name));
            }
        }
    }
    *KNOWN.lock().unwrap() = None;
}

/// The screens this helper kept dark, on again: whether it had.
fn lighten() -> bool {
    let Some(dark) = DARK.lock().unwrap().take() else { return false };
    uncover(dark.covering);
    restore(&dark.monitors);
    true
}

/// The screens on again, exactly as a touch of the mouse brings them:
/// Windows is told the display is wanted, and the mouse is moved by
/// nothing (which is input all the same). Nothing more is asked of the
/// monitors: told besides to power on, and which input to show, a monitor
/// with several PCs plugged into it went over to another. One this helper
/// put out by itself (see `turn`), which no touch of the mouse wakes, is
/// put on again.
pub fn wake() {
    *OUT.lock().unwrap() = None;
    say_out(false);
    lighten();
    // (Out to Windows too, what is touched is listened for till Windows says they are on.)
    let listener = LISTENER.load(Ordering::SeqCst);
    if listener != 0 && STATE.load(Ordering::Relaxed) != 0 {
        let _ = unsafe { PostMessageW(Some(HWND(listener as *mut _)), HEAR_TOUCHES, WPARAM(0), LPARAM(0)) };
    }
    unsafe {
        SetThreadExecutionState(ES_DISPLAY_REQUIRED);
        let nudge = INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi: MOUSEINPUT { dwFlags: MOUSEEVENTF_MOVE, ..Default::default() } } };
        SendInput(&[nudge], size_of::<INPUT>() as i32);
    }
    let singly = std::mem::take(&mut *SINGLY.lock().unwrap());
    let mut back = Vec::new();
    for (one, covering) in singly {
        if let Some(covering) = covering {
            uncover(covering);
        }
        back.push(one);
    }
    restore(&back);
}

/// A monitor: what it calls itself, where it is on the desktop (left, top,
/// width, height: physical px), and, as it answers over DDC/CI, whether it
/// is on and how bright it is (percent of its most); none where it does
/// not answer: that it cannot be asked to change by itself.
#[derive(Clone)]
pub struct Monitor {
    pub name: String,
    pub place: (i32, i32, i32, i32),
    pub on: Option<bool>,
    pub brightness: Option<u8>,
}

fn text(wide: &[u16]) -> String {
    String::from_utf16_lossy(&wide[..wide.iter().position(|c| *c == 0).unwrap_or(wide.len())])
}

/// Each display output in use (as `\\.\DISPLAY1`) with the name the
/// monitor on it gives itself ("DELL S2725QS"): what Windows' own display
/// settings call it.
fn names() -> Vec<(String, String, bool)> {
    let (mut paths, mut modes) = (0u32, 0u32);
    if unsafe { GetDisplayConfigBufferSizes(QDC_ONLY_ACTIVE_PATHS, &mut paths, &mut modes) }.is_err() {
        return Vec::new();
    }
    let mut path = vec![DISPLAYCONFIG_PATH_INFO::default(); paths as usize];
    let mut mode = vec![DISPLAYCONFIG_MODE_INFO::default(); modes as usize];
    if unsafe { QueryDisplayConfig(QDC_ONLY_ACTIVE_PATHS, &mut paths, path.as_mut_ptr(), &mut modes, mode.as_mut_ptr(), None) }.is_err() {
        return Vec::new();
    }
    path.truncate(paths as usize);
    path.iter()
        .filter_map(|path| {
            let mut source = DISPLAYCONFIG_SOURCE_DEVICE_NAME::default();
            source.header = DISPLAYCONFIG_DEVICE_INFO_HEADER { r#type: DISPLAYCONFIG_DEVICE_INFO_GET_SOURCE_NAME, size: size_of::<DISPLAYCONFIG_SOURCE_DEVICE_NAME>() as u32, adapterId: path.sourceInfo.adapterId, id: path.sourceInfo.id };
            let mut target = DISPLAYCONFIG_TARGET_DEVICE_NAME::default();
            target.header = DISPLAYCONFIG_DEVICE_INFO_HEADER { r#type: DISPLAYCONFIG_DEVICE_INFO_GET_TARGET_NAME, size: size_of::<DISPLAYCONFIG_TARGET_DEVICE_NAME>() as u32, adapterId: path.targetInfo.adapterId, id: path.targetInfo.id };
            let got = unsafe { DisplayConfigGetDeviceInfo(&mut source.header) == 0 && DisplayConfigGetDeviceInfo(&mut target.header) == 0 };
            let name = text(&target.monitorFriendlyDeviceName);
            // A screen that is part of the PC itself (a laptop's): wired inside it.
            let built_in = [DISPLAYCONFIG_OUTPUT_TECHNOLOGY_INTERNAL, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_LVDS, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_DISPLAYPORT_EMBEDDED, DISPLAYCONFIG_OUTPUT_TECHNOLOGY_UDI_EMBEDDED].contains(&path.targetInfo.outputTechnology);
            got.then(|| (text(&source.viewGdiDeviceName), name, built_in))
        })
        .collect()
}

/// VCP's power mode, and its values for on and for off (standby, from
/// which the monitor still listens).
const POWER_MODE: u8 = 0xD6;
const POWER_ON: u32 = 1;
const POWER_OFF: u32 = 4;
/// VCP's brightness (luminance): from nothing to the most the monitor says.
const BRIGHTNESS: u8 = 0x10;
/// How long what the monitors answered is taken to stand: asking takes
/// each of them a moment.
const KNOWN_FOR: Duration = Duration::from_secs(5);

/// The monitors as they last answered, and when.
static KNOWN: Mutex<Option<(Instant, Vec<Monitor>)>> = Mutex::new(None);

/// Goes through every monitor that is connected, in the order Windows has
/// them: `with` is handed each one's place in that order, its name, where
/// it is on the desktop, whether it is the PC's own screen, and its handle for DDC/CI.
fn each_monitor(mut with: impl FnMut(usize, String, (i32, i32, i32, i32), bool, HANDLE)) {
    let names = names();
    unsafe extern "system" fn each(monitor: HMONITOR, _: HDC, _: *mut RECT, found: LPARAM) -> BOOL {
        unsafe { &mut *(found.0 as *mut Vec<HMONITOR>) }.push(monitor);
        true.into()
    }
    let mut screens: Vec<HMONITOR> = Vec::new();
    let _ = unsafe { EnumDisplayMonitors(None, None, Some(each), LPARAM(&mut screens as *mut _ as isize)) };
    let mut index = 0;
    for screen in screens {
        let mut info = MONITORINFOEXW::default();
        info.monitorInfo.cbSize = size_of::<MONITORINFOEXW>() as u32;
        let _ = unsafe { GetMonitorInfoW(screen, &mut info as *mut _ as *mut MONITORINFO) };
        let at = info.monitorInfo.rcMonitor;
        let place = (at.left, at.top, at.right - at.left, at.bottom - at.top);
        let output = text(&info.szDevice);
        let known = names.iter().find(|(source, _, _)| *source == output);
        let named = known.map(|(_, name, _)| name.clone()).filter(|name| !name.is_empty());
        let built_in = known.is_some_and(|(_, _, built_in)| *built_in);
        let mut count = 0;
        if unsafe { GetNumberOfPhysicalMonitorsFromHMONITOR(screen, &mut count) }.is_err() || count == 0 {
            continue;
        }
        let mut physical = vec![PHYSICAL_MONITOR::default(); count as usize];
        if unsafe { GetPhysicalMonitorsFromHMONITOR(screen, &mut physical) }.is_err() {
            continue;
        }
        for one in &physical {
            // As the monitor names itself; one that does not, as its driver does.
            let described = one.szPhysicalMonitorDescription;
            with(index, named.clone().unwrap_or_else(|| text(&described)), place, built_in, one.hPhysicalMonitor);
            index += 1;
        }
        let _ = unsafe { DestroyPhysicalMonitors(&physical) };
    }
}

/// A monitor's feature `code`: what it is now, and the most it can be.
fn asked(monitor: HANDLE, code: u8) -> Option<(u32, u32)> {
    let (mut now, mut most) = (0u32, 0u32);
    (unsafe { GetVCPFeatureAndVCPFeatureReply(monitor, code, None, &mut now, Some(&mut most)) } != 0).then_some((now, most))
}

fn ask(monitor: HANDLE, code: u8) -> Option<u32> {
    asked(monitor, code).map(|(now, _)| now)
}

/// How bright a monitor is, as a percent of its most.
fn brightness(monitor: HANDLE) -> Option<u8> {
    asked(monitor, BRIGHTNESS).filter(|(_, most)| *most > 0).map(|(now, most)| ((now.min(most) * 100 + most / 2) / most) as u8)
}

/// Windows' own way to a laptop's screen: how bright it is, asked and set
/// (IOCTL_VIDEO_QUERY_DISPLAY_BRIGHTNESS and _SET_).
const PANEL_BRIGHTNESS: u32 = 0x0023_0498;
const SET_PANEL_BRIGHTNESS: u32 = 0x0023_049C;

fn panel() -> Option<HANDLE> {
    unsafe { CreateFileW(windows::core::w!(r"\\.\LCD"), (GENERIC_READ | GENERIC_WRITE).0, FILE_SHARE_READ | FILE_SHARE_WRITE, None, OPEN_EXISTING, Default::default(), None) }.ok()
}

/// How bright the PC's own screen is (percent), as it is powered now.
fn panel_brightness() -> Option<u8> {
    let panel = panel()?;
    // Which of the two that follow is in force (1, on mains; 2, on battery), on mains, on battery.
    let mut answer = [0u8; 3];
    let mut length = 0u32;
    let read = unsafe { DeviceIoControl(panel, PANEL_BRIGHTNESS, None, 0, Some(answer.as_mut_ptr().cast()), 3, Some(&mut length), None) };
    let _ = unsafe { CloseHandle(panel) };
    (read.is_ok() && length == 3).then(|| if answer[0] == 2 { answer[2] } else { answer[1] })
}

/// Makes the PC's own screen as bright as `percent`, on mains and on battery alike.
fn set_panel_brightness(percent: u8) -> bool {
    let Some(panel) = panel() else { return false };
    let asked = [3u8, percent, percent];
    let set = unsafe { DeviceIoControl(panel, SET_PANEL_BRIGHTNESS, Some(asked.as_ptr().cast()), 3, None, 0, None, None) };
    let _ = unsafe { CloseHandle(panel) };
    set.is_ok()
}

/// Every monitor that is connected, as it answered within `KNOWN_FOR`.
pub fn monitors() -> Vec<Monitor> {
    // (Those put out one by one first: `turn` holds them while it changes what is known.)
    let singly: Vec<usize> = SINGLY.lock().unwrap().iter().map(|(one, _)| one.index).collect();
    let mut known = KNOWN.lock().unwrap();
    if let Some((_, monitors)) = known.as_ref().filter(|(at, _)| at.elapsed() < KNOWN_FOR) {
        return monitors.clone();
    }
    let mut found = Vec::new();
    each_monitor(|index, name, place, built_in, monitor| {
        // A laptop's own screen says how bright it is through Windows, not over DDC/CI.
        let bright = brightness(monitor).or_else(|| built_in.then(panel_brightness).flatten());
        // On, unless this helper has put it out by itself or it says it is off (one that says nothing of its power is on).
        let on = !singly.contains(&index) && ask(monitor, POWER_MODE).is_none_or(|mode| mode == POWER_ON);
        found.push(Monitor { name, place, on: Some(on), brightness: bright })
    });
    *known = Some((Instant::now(), found.clone()));
    found
}

/// Tells the monitor at `index` (see `each_monitor`) that its feature
/// `code` is `value`: whether it took it.
pub fn tell(index: usize, code: u8, value: u32) -> bool {
    let mut told = false;
    each_monitor(|at, _, _, _, monitor| {
        if at == index {
            told = unsafe { SetVCPFeature(monitor, code, value) } != 0;
        }
    });
    told
}

/// Makes the monitor at `index` as bright as `percent` of its most:
/// whether it took it.
pub fn dim(index: usize, percent: u8) -> bool {
    let mut told = false;
    each_monitor(|at, _, _, built_in, monitor| {
        if at == index {
            if let Some((_, most)) = asked(monitor, BRIGHTNESS).filter(|(_, most)| *most > 0) {
                told = unsafe { SetVCPFeature(monitor, BRIGHTNESS, (percent.min(100) as u32 * most + 50) / 100) } != 0;
            } else if built_in {
                told = set_panel_brightness(percent.min(100));
            }
        }
    });
    *KNOWN.lock().unwrap() = None;
    told
}

/// Puts the monitor at `index` out by itself, or on again: the others
/// stay as they are, and Windows keeps its windows where they were (to
/// Windows it is still there). Out, it is turned off where it then says
/// it is, and covered in black (made as dim as it goes) where not. Whether
/// there is such a monitor.
pub fn turn(index: usize, on: bool) -> bool {
    let mut singly = SINGLY.lock().unwrap();
    let held = singly.iter().position(|(one, _)| one.index == index);
    let mut there = false;
    match (on, held) {
        (false, Some(_)) => there = true,
        (false, None) => {
            let mut done = None;
            each_monitor(|at, name, place, built_in, monitor| {
                if at == index {
                    done = Some((put_out(at, name, built_in, monitor), place));
                }
            });
            if let Some((done, place)) = done {
                let cover = if done.off { None } else { covering(vec![place], false) };
                singly.push((done, cover));
                there = true;
            }
        }
        (true, Some(held)) => {
            let (one, cover) = singly.remove(held);
            if let Some(cover) = cover {
                uncover(cover);
            }
            restore(&[one]);
            there = true;
        }
        // (Off by other means than this helper's: told to turn on.)
        (true, None) => there = tell(index, POWER_MODE, POWER_ON),
    }
    *KNOWN.lock().unwrap() = None;
    there
}
