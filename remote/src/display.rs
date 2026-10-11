//! The screens: whether they are on (as Windows tells a window that asks to
//! hear of it), turning them off and on, and what each monitor answers over
//! its own control channel (DDC/CI), by which a monitor is told things its
//! cable's signal does not say.

use std::sync::atomic::{AtomicU8, Ordering};
use std::sync::Mutex;
use std::time::Duration;

use windows::core::{w, Result, BOOL, GUID};
use windows::Win32::Devices::Display::{
    DestroyPhysicalMonitors, GetNumberOfPhysicalMonitorsFromHMONITOR, GetPhysicalMonitorsFromHMONITOR, GetVCPFeatureAndVCPFeatureReply, SetVCPFeature, PHYSICAL_MONITOR,
};
use windows::Win32::Foundation::{HANDLE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, HDC, HMONITOR};
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
/// SC_MONITORPOWER's "on" and "off".
const ON: isize = -1;
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

/// The input each monitor showed as the screens were last turned off, by
/// its place among them (see `each_monitor`): a monitor with several PCs
/// plugged into it may go over to another while this one's signal is gone.
static INPUTS: Mutex<Vec<(usize, u32)>> = Mutex::new(Vec::new());
/// How long after the screens are asked on a monitor is given before it is
/// asked which input it shows.
const WAKING: Duration = Duration::from_millis(2500);

/// The screens off, until the mouse or the keyboard is touched, or `wake`.
pub fn sleep() -> Result<()> {
    let mut inputs = Vec::new();
    each_monitor(|index, _, monitor| inputs.extend(ask(monitor, INPUT_SOURCE).map(|(now, _)| (index, now & 0xFF))));
    *INPUTS.lock().unwrap() = inputs;
    power(OFF)
}

/// A monitor that went over to another input while the screens were off
/// is told to show this PC's again (one that still answers this PC: a
/// monitor that has let go of this cable altogether cannot be told).
fn back_to_this_pc() {
    let inputs = std::mem::take(&mut *INPUTS.lock().unwrap());
    each_monitor(|index, _, monitor| {
        if let Some((_, was)) = inputs.iter().find(|(at, _)| *at == index) {
            if ask(monitor, INPUT_SOURCE).is_some_and(|(now, _)| now & 0xFF != *was) {
                unsafe { SetVCPFeature(monitor, INPUT_SOURCE, *was) };
            }
        }
    });
}

/// The screens on again, as a touch of the mouse brings them: Windows is
/// told the display is wanted, and the mouse is moved by nothing (which is
/// input all the same; asking for the monitors' power alone does not wake
/// them on every system).
pub fn wake() -> Result<()> {
    unsafe {
        SetThreadExecutionState(ES_DISPLAY_REQUIRED);
        let nudge = INPUT { r#type: INPUT_MOUSE, Anonymous: INPUT_0 { mi: MOUSEINPUT { dwFlags: MOUSEEVENTF_MOVE, ..Default::default() } } };
        SendInput(&[nudge], size_of::<INPUT>() as i32);
    }
    std::thread::spawn(|| {
        std::thread::sleep(WAKING);
        back_to_this_pc();
    });
    power(ON)
}

/// A monitor, by what it calls itself, and what it answers over DDC/CI:
/// its brightness and the most it goes to, the input it shows, and its
/// power mode (each none where it does not answer).
pub struct Monitor {
    pub name: String,
    pub brightness: Option<(u32, u32)>,
    pub input: Option<u32>,
    pub power: Option<u32>,
}

/// VCP codes: brightness, input source, power mode.
const BRIGHTNESS: u8 = 0x10;
const INPUT_SOURCE: u8 = 0x60;
const POWER_MODE: u8 = 0xD6;

/// Goes through every monitor that is connected, in the order Windows has
/// them: `with` is handed each one's place in that order, its name and its
/// handle for DDC/CI.
fn each_monitor(mut with: impl FnMut(usize, String, HANDLE)) {
    unsafe extern "system" fn each(monitor: HMONITOR, _: HDC, _: *mut RECT, found: LPARAM) -> BOOL {
        unsafe { &mut *(found.0 as *mut Vec<HMONITOR>) }.push(monitor);
        true.into()
    }
    let mut screens: Vec<HMONITOR> = Vec::new();
    let _ = unsafe { EnumDisplayMonitors(None, None, Some(each), LPARAM(&mut screens as *mut _ as isize)) };
    let mut index = 0;
    for screen in screens {
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
            with(index, String::from_utf16_lossy(&name[..name.iter().position(|c| *c == 0).unwrap_or(name.len())]), one.hPhysicalMonitor);
            index += 1;
        }
        let _ = unsafe { DestroyPhysicalMonitors(&physical) };
    }
}

fn ask(monitor: HANDLE, code: u8) -> Option<(u32, u32)> {
    let (mut now, mut most) = (0u32, 0u32);
    (unsafe { GetVCPFeatureAndVCPFeatureReply(monitor, code, None, &mut now, Some(&mut most)) } != 0).then_some((now, most))
}

/// Every monitor that is connected, asked over DDC/CI.
pub fn monitors() -> Vec<Monitor> {
    let mut found = Vec::new();
    each_monitor(|_, name, monitor| {
        found.push(Monitor { name, brightness: ask(monitor, BRIGHTNESS), input: ask(monitor, INPUT_SOURCE).map(|(now, _)| now & 0xFF), power: ask(monitor, POWER_MODE).map(|(now, _)| now) })
    });
    found
}

/// Tells the monitor at `index` (see `each_monitor`) that its feature
/// `code` is `value`: whether it took it.
pub fn tell(index: usize, code: u8, value: u32) -> bool {
    let mut told = false;
    each_monitor(|at, _, monitor| {
        if at == index {
            told = unsafe { SetVCPFeature(monitor, code, value) } != 0;
        }
    });
    told
}
