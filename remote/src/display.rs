//! The screens: whether they are on (as Windows tells a window that asks to
//! hear of it), turning them off and on, and what each monitor answers over
//! its own control channel (DDC/CI), by which a monitor is told things its
//! cable's signal does not say.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows::core::{w, Result, BOOL, GUID};
use windows::Win32::Devices::Display::{
    DestroyPhysicalMonitors, GetNumberOfPhysicalMonitorsFromHMONITOR, GetPhysicalMonitorsFromHMONITOR, GetVCPFeatureAndVCPFeatureReply, SetVCPFeature, PHYSICAL_MONITOR,
};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO};
use windows::Win32::System::Power::{RegisterPowerSettingNotification, SetThreadExecutionState, ES_DISPLAY_REQUIRED, POWERBROADCAST_SETTING};
use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_MOVE, MOUSEINPUT};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetMessageW, RegisterClassW, DEVICE_NOTIFY_WINDOW_HANDLE, MSG, PBT_POWERSETTINGCHANGE, SC_MONITORPOWER,
    WINDOW_EX_STYLE, WINDOW_STYLE, WM_POWERBROADCAST, WM_SYSCOMMAND, WNDCLASSW,
};

/// GUID_CONSOLE_DISPLAY_STATE: the display off (0), on (1) or dimmed (2).
const DISPLAY_STATE: GUID = GUID::from_u128(0x6fe69556_704a_47a0_8f24_c28d936fda47);
/// Not told yet.
const UNKNOWN: u8 = u8::MAX;
/// SC_MONITORPOWER's "off".
const OFF: isize = 2;

/// The display's state as Windows last told it.
static STATE: AtomicU8 = AtomicU8::new(UNKNOWN);

unsafe extern "system" fn told(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == WM_POWERBROADCAST && wparam.0 == PBT_POWERSETTINGCHANGE as usize {
        let setting = unsafe { &*(lparam.0 as *const POWERBROADCAST_SETTING) };
        if setting.PowerSetting == DISPLAY_STATE && setting.DataLength >= 1 {
            STATE.store(setting.Data[0], Ordering::Relaxed);
        }
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// Listens, on a thread of its own, for the screens going off and coming on.
pub fn watch() {
    std::thread::spawn(|| unsafe {
        let class = WNDCLASSW { lpfnWndProc: Some(told), lpszClassName: w!("GlanceRemoteDisplay"), ..Default::default() };
        RegisterClassW(&class);
        let Ok(window) = CreateWindowExW(WINDOW_EX_STYLE(0), class.lpszClassName, w!(""), WINDOW_STYLE(0), 0, 0, 0, 0, None, None, None, None) else { return };
        // Told at once how it is now, then at every change.
        let _ = RegisterPowerSettingNotification(HANDLE(window.0), &DISPLAY_STATE, DEVICE_NOTIFY_WINDOW_HANDLE);
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            DispatchMessageW(&message);
        }
    });
}

/// Whether the screens are on (dimmed is on); none, before Windows has said.
pub fn on() -> Option<bool> {
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
    power(OFF)
}

/// The screens on again, exactly as a touch of the mouse brings them:
/// Windows is told the display is wanted, and the mouse is moved by
/// nothing (which is input all the same). Nothing more is asked of the
/// monitors: told besides to power on, and which input to show, a monitor
/// with several PCs plugged into it went over to another. One this helper
/// turned off by itself (see `turn`), which no touch of the mouse wakes, is
/// turned on.
pub fn wake() {
    unsafe {
        SetThreadExecutionState(ES_DISPLAY_REQUIRED);
        let nudge = INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi: MOUSEINPUT { dwFlags: MOUSEEVENTF_MOVE, ..Default::default() } } };
        SendInput(&[nudge], size_of::<INPUT>() as i32);
    }
    let turned_off = std::mem::take(&mut *TURNED_OFF.lock().unwrap());
    for index in turned_off {
        tell(index, POWER_MODE, POWER_ON);
    }
    *KNOWN.lock().unwrap() = None;
}

/// A monitor: what it calls itself, how many pixels it shows, and whether
/// it is on, as it answers over DDC/CI (none: it does not answer, and
/// cannot be turned off by itself).
#[derive(Clone)]
pub struct Monitor {
    pub name: String,
    pub size: (i32, i32),
    pub on: Option<bool>,
}

/// VCP's power mode, and its values for on and for off (standby, from
/// which the monitor still listens).
const POWER_MODE: u8 = 0xD6;
const POWER_ON: u32 = 1;
const POWER_OFF: u32 = 4;
/// How long what the monitors answered is taken to stand: asking takes
/// each of them a moment.
const KNOWN_FOR: Duration = Duration::from_secs(5);

/// The monitors as they last answered, and when.
static KNOWN: Mutex<Option<(Instant, Vec<Monitor>)>> = Mutex::new(None);
/// The monitors this helper turned off by themselves, by their places.
static TURNED_OFF: Mutex<Vec<usize>> = Mutex::new(Vec::new());

/// Goes through every monitor that is connected, in the order Windows has
/// them: `with` is handed each one's place in that order, its name, its
/// size and its handle for DDC/CI.
fn each_monitor(mut with: impl FnMut(usize, String, (i32, i32), HANDLE)) {
    unsafe extern "system" fn each(monitor: HMONITOR, _: HDC, _: *mut RECT, found: LPARAM) -> BOOL {
        unsafe { &mut *(found.0 as *mut Vec<HMONITOR>) }.push(monitor);
        true.into()
    }
    let mut screens: Vec<HMONITOR> = Vec::new();
    let _ = unsafe { EnumDisplayMonitors(None, None, Some(each), LPARAM(&mut screens as *mut _ as isize)) };
    let mut index = 0;
    for screen in screens {
        let mut info = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = unsafe { GetMonitorInfoW(screen, &mut info) };
        let size = (info.rcMonitor.right - info.rcMonitor.left, info.rcMonitor.bottom - info.rcMonitor.top);
        let mut count = 0;
        if unsafe { GetNumberOfPhysicalMonitorsFromHMONITOR(screen, &mut count) }.is_err() || count == 0 {
            continue;
        }
        let mut physical = vec![PHYSICAL_MONITOR::default(); count as usize];
        if unsafe { GetPhysicalMonitorsFromHMONITOR(screen, &mut physical) }.is_err() {
            continue;
        }
        for one in &physical {
            let name = one.szPhysicalMonitorDescription;
            with(index, String::from_utf16_lossy(&name[..name.iter().position(|c| *c == 0).unwrap_or(name.len())]), size, one.hPhysicalMonitor);
            index += 1;
        }
        let _ = unsafe { DestroyPhysicalMonitors(&physical) };
    }
}

fn ask(monitor: HANDLE, code: u8) -> Option<u32> {
    let (mut now, mut most) = (0u32, 0u32);
    (unsafe { GetVCPFeatureAndVCPFeatureReply(monitor, code, None, &mut now, Some(&mut most)) } != 0).then_some(now)
}

/// Every monitor that is connected, as it answered within `KNOWN_FOR`.
pub fn monitors() -> Vec<Monitor> {
    let mut known = KNOWN.lock().unwrap();
    if let Some((_, monitors)) = known.as_ref().filter(|(at, _)| at.elapsed() < KNOWN_FOR) {
        return monitors.clone();
    }
    let mut found = Vec::new();
    each_monitor(|_, name, size, monitor| found.push(Monitor { name, size, on: ask(monitor, POWER_MODE).map(|mode| mode == POWER_ON) }));
    *known = Some((Instant::now(), found.clone()));
    found
}

/// Tells the monitor at `index` (see `each_monitor`) that its feature
/// `code` is `value`: whether it took it.
pub fn tell(index: usize, code: u8, value: u32) -> bool {
    let mut told = false;
    each_monitor(|at, _, _, monitor| {
        if at == index {
            told = unsafe { SetVCPFeature(monitor, code, value) } != 0;
        }
    });
    told
}

/// Turns the monitor at `index` off by itself, or on again: the others
/// stay as they are, and Windows keeps its windows where they were (to
/// Windows it is still there). Whether it took it.
pub fn turn(index: usize, on: bool) -> bool {
    let told = tell(index, POWER_MODE, if on { POWER_ON } else { POWER_OFF });
    let mut turned_off = TURNED_OFF.lock().unwrap();
    turned_off.retain(|at| *at != index);
    if told && !on {
        turned_off.push(index);
    }
    *KNOWN.lock().unwrap() = None;
    told
}
