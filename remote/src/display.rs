//! The screens: whether they are on (as Windows tells a window that asks to
//! hear of it), turning them off and on, and what each monitor answers over
//! its own control channel (DDC/CI), by which a monitor is told things its
//! cable's signal does not say.

use std::sync::atomic::{AtomicU8, Ordering};
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
use windows::Win32::Graphics::Gdi::{EnumDisplayMonitors, GetMonitorInfoW, HDC, HMONITOR, MONITORINFO, MONITORINFOEXW};
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
/// The monitors this helper turned off by themselves, by their places.
static TURNED_OFF: Mutex<Vec<usize>> = Mutex::new(Vec::new());

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
    let mut known = KNOWN.lock().unwrap();
    if let Some((_, monitors)) = known.as_ref().filter(|(at, _)| at.elapsed() < KNOWN_FOR) {
        return monitors.clone();
    }
    let mut found = Vec::new();
    each_monitor(|_, name, place, built_in, monitor| {
        // A laptop's own screen says how bright it is through Windows, not over DDC/CI.
        let bright = brightness(monitor).or_else(|| built_in.then(panel_brightness).flatten());
        found.push(Monitor { name, place, on: ask(monitor, POWER_MODE).map(|mode| mode == POWER_ON), brightness: bright })
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
