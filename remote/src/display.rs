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
use windows::Win32::System::Threading::GetCurrentThreadId;
use windows::Win32::UI::Input::{GetRawInputData, GetRawInputDeviceInfoW, RegisterRawInputDevices, HRAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RIDEV_INPUTSINK, RIDEV_REMOVE, RIDI_DEVICENAME, RID_HEADER};
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
/// The window that listens (see `watch`), once there is one.
static LISTENER: AtomicIsize = AtomicIsize::new(0);
/// Asks the listening window to hear of what is touched (1) or to stop (0).
const HEAR_TOUCHES: u32 = WM_APP + 1;

/// What is touched while the screens are out, as Windows hands it to a
/// window that asks: which device it came from (not what was pressed).
/// The kinds of device whose touch wakes the screens: the mouse, the
/// keyboard, a remote's or a keyboard's media keys, a pen, a touch screen,
/// a touch pad (each a usage page and a usage, as HID names them).
const TOUCHED_BY: [(u16, u16); 6] = [(0x01, 0x02), (0x01, 0x06), (0x0C, 0x01), (0x0D, 0x02), (0x0D, 0x04), (0x0D, 0x05)];

/// The last touch heard of while listening: the device's handle, what it
/// is called, and when (see `actions::tick`).
static TOUCH: Mutex<Option<(isize, String, u32)>> = Mutex::new(None);

fn hear_touches(window: HWND, hear: bool) {
    let devices: Vec<RAWINPUTDEVICE> = TOUCHED_BY.iter().map(|(page, usage)| RAWINPUTDEVICE { usUsagePage: *page, usUsage: *usage, dwFlags: if hear { RIDEV_INPUTSINK } else { RIDEV_REMOVE }, hwndTarget: if hear { window } else { HWND::default() } }).collect();
    if hear {
        *TOUCH.lock().unwrap() = None;
    }
    if let Err(error) = unsafe { RegisterRawInputDevices(&devices, size_of::<RAWINPUTDEVICE>() as u32) } {
        crate::journal::note(format!("what is touched cannot be {}: {error}", if hear { "heard of" } else { "left unheard" }));
    }
}

/// A device as it is named in the log: what kind it is and whose (its bus,
/// maker and model), without the numbers that tell one unit from another.
fn device(handle: HANDLE) -> String {
    if handle.is_invalid() || handle.0.is_null() {
        return "no device (a program made it)".into();
    }
    let mut name = [0u16; 512];
    let mut length = name.len() as u32;
    let got = unsafe { GetRawInputDeviceInfoW(Some(handle), RIDI_DEVICENAME, Some(name.as_mut_ptr().cast()), &mut length) };
    if got == u32::MAX || got == 0 {
        return "a device that does not say what it is".into();
    }
    let name = text(&name);
    name.trim_start_matches(r"\\?\").split('#').take(2).collect::<Vec<_>>().join(" ")
}

fn touched(input: LPARAM) {
    let mut header = RAWINPUTHEADER::default();
    let mut size = size_of::<RAWINPUTHEADER>() as u32;
    if unsafe { GetRawInputData(HRAWINPUT(input.0 as *mut _), RID_HEADER, Some((&mut header as *mut RAWINPUTHEADER).cast()), &mut size, size_of::<RAWINPUTHEADER>() as u32) } == u32::MAX {
        return;
    }
    let mut touch = TOUCH.lock().unwrap();
    let handle = header.hDevice.0 as isize;
    let now = crate::actions::tick();
    match touch.as_mut().filter(|(last, _, _)| *last == handle) {
        Some(same) => same.2 = now,
        None => *touch = Some((handle, format!("{} ({})", device(header.hDevice), ["a mouse", "a keyboard", "another kind of device"][header.dwType.min(2) as usize]), now)),
    }
}

/// What last touched this PC while the screens were out, and how long ago
/// (ms): for the log, to say what woke them.
pub fn last_touch() -> String {
    match TOUCH.lock().unwrap().as_ref() {
        Some((_, device, at)) => format!("last touched {} ms before, by {device}", crate::actions::tick().wrapping_sub(*at)),
        None => "nothing was touched".into(),
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
                    crate::journal::note("Windows: the screens are off");
                    hear_touches(window, true);
                } else {
                    crate::journal::note(format!("Windows: the screens are on ({})", last_touch()));
                    // (Still wanted while this helper keeps them dark its own way.)
                    if dark_since().is_none() {
                        hear_touches(window, false);
                    }
                }
            }
        }
    }
    match message {
        WM_INPUT => touched(lparam),
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
    if dark_since().is_some() {
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

/// The screens kept dark by this helper: since when (see `actions::tick`),
/// which monitors and how, and the thread that holds the covers up.
struct Dark {
    since: u32,
    monitors: Vec<Darkened>,
    covering: (u32, std::thread::JoinHandle<()>),
}

static DARK: Mutex<Option<Dark>> = Mutex::new(None);
/// How long a monitor told to turn off is given to say it has, and how
/// often it is asked meanwhile.
const TURNING_OFF: Duration = Duration::from_millis(2000);
const ASKED_EVERY: Duration = Duration::from_millis(200);

/// Since when this helper has kept the screens dark, if it does.
pub fn dark_since() -> Option<u32> {
    DARK.lock().unwrap().as_ref().map(|dark| dark.since)
}

unsafe extern "system" fn cover(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    // No pointer over it: black is all of it.
    if message == WM_SETCURSOR {
        unsafe { SetCursor(None) };
        return LRESULT(1);
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// Holds a black window over each of `places` (and the signal on: Windows
/// is not to stop it for want of a touch) till it is told to end; its
/// thread's number is sent once the covers are up.
fn hold_covers(places: Vec<(i32, i32, i32, i32)>, up: std::sync::mpsc::Sender<u32>) {
    unsafe {
        let class = WNDCLASSW { lpfnWndProc: Some(cover), lpszClassName: w!("GlanceRemoteCover"), hbrBackground: HBRUSH(GetStockObject(BLACK_BRUSH).0), ..Default::default() };
        RegisterClassW(&class);
        SetThreadExecutionState(ES_CONTINUOUS | ES_DISPLAY_REQUIRED);
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
        SetThreadExecutionState(ES_CONTINUOUS);
    }
}

/// The screens out with their signal kept: each monitor turned off by
/// itself where it says it has been; the others covered in black and made
/// as dim as they go. What was done to each is returned (to be kept: see
/// `restore`); already dark, nothing more is done but that what is touched
/// just now is not minded.
pub fn darken() -> Vec<Darkened> {
    let mut dark = DARK.lock().unwrap();
    if let Some(dark) = dark.as_mut() {
        dark.since = crate::actions::tick();
        return dark.monitors.clone();
    }
    let mut monitors = Vec::new();
    let mut covered = Vec::new();
    each_monitor(|index, name, place, built_in, monitor| {
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
            covered.push(place);
        }
        crate::journal::note(format!("monitor {index} {name}: {}", if off { "turned off by itself".to_string() } else { format!("covered in black{}", if was.is_some() { " and dimmed" } else { "" }) }));
        monitors.push(Darkened { index, name, off, brightness: was });
    });
    let (up, is_up) = std::sync::mpsc::channel();
    let holding = std::thread::spawn(move || hold_covers(covered, up));
    let Ok(thread) = is_up.recv() else { return monitors };
    let listener = LISTENER.load(Ordering::SeqCst);
    if listener != 0 {
        let _ = unsafe { PostMessageW(Some(HWND(listener as *mut _)), HEAR_TOUCHES, WPARAM(1), LPARAM(0)) };
    }
    *KNOWN.lock().unwrap() = None;
    *dark = Some(Dark { since: crate::actions::tick(), monitors: monitors.clone(), covering: (thread, holding) });
    monitors
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
    let (thread, holding) = dark.covering;
    let _ = unsafe { PostThreadMessageW(thread, WM_QUIT, WPARAM(0), LPARAM(0)) };
    let _ = holding.join();
    restore(&dark.monitors);
    let listener = LISTENER.load(Ordering::SeqCst);
    if listener != 0 && STATE.load(Ordering::Relaxed) != 0 {
        let _ = unsafe { PostMessageW(Some(HWND(listener as *mut _)), HEAR_TOUCHES, WPARAM(0), LPARAM(0)) };
    }
    true
}

/// The screens on again, exactly as a touch of the mouse brings them:
/// Windows is told the display is wanted, and the mouse is moved by
/// nothing (which is input all the same). Nothing more is asked of the
/// monitors: told besides to power on, and which input to show, a monitor
/// with several PCs plugged into it went over to another. One this helper
/// turned off by itself (see `turn`), which no touch of the mouse wakes, is
/// turned on.
pub fn wake() {
    lighten();
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
