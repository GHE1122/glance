//! The frames programs present, from Event Tracing for Windows: DXGI's
//! Present_Start (Direct3D 10, 11 and 12) and Direct3D 9's; for a program
//! that presents through neither (OpenGL, Vulkan), what the graphics
//! kernel is handed of each frame (see `Fed`). Nothing is
//! asked of the programs themselves: no handle to a game's process, nothing
//! injected, so no anti-cheat has anything to see. A real-time session needs
//! administrator rights, which Glance has; without them there are no frames.

use std::collections::HashMap;
use std::sync::Mutex;
use std::thread;
use std::time::Duration;

use windows::core::{GUID, PCWSTR, PWSTR};
use windows::core::BOOL;
use windows::Win32::Foundation::{ERROR_ALREADY_EXISTS, ERROR_SUCCESS, HWND, LPARAM, POINT, RECT};
use windows::Win32::Graphics::Gdi::{
    EnumDisplaySettingsW, GetMonitorInfoW, MonitorFromPoint, MonitorFromWindow, DEVMODEW, ENUM_CURRENT_SETTINGS, HMONITOR, MONITORINFO, MONITORINFOEXW,
    MONITOR_DEFAULTTONEAREST, MONITOR_DEFAULTTONULL,
};
use windows::Win32::System::Diagnostics::Etw::{
    CloseTrace, ControlTraceW, EnableTraceEx2, OpenTraceW, ProcessTrace, StartTraceW, CONTROLTRACE_HANDLE, ENABLE_TRACE_PARAMETERS, ENABLE_TRACE_PARAMETERS_VERSION_2,
    EVENT_CONTROL_CODE_ENABLE_PROVIDER, EVENT_FILTER_DESCRIPTOR, EVENT_FILTER_TYPE_EVENT_ID, EVENT_RECORD,
    EVENT_TRACE_CONTROL_QUERY, EVENT_TRACE_CONTROL_STOP, EVENT_TRACE_LOGFILEW, EVENT_TRACE_PROPERTIES, EVENT_TRACE_NO_PER_PROCESSOR_BUFFERING, EVENT_TRACE_REAL_TIME_MODE, PROCESS_TRACE_MODE_EVENT_RECORD,
    PROCESS_TRACE_MODE_RAW_TIMESTAMP, PROCESS_TRACE_MODE_REAL_TIME, TRACE_LEVEL_INFORMATION, TRACE_LEVEL_VERBOSE, WNODE_FLAG_TRACED_GUID,
};
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::UI::WindowsAndMessaging::{
    EnumWindows, GetCursorPos, GetForegroundWindow, GetShellWindow, GetWindowRect, GetWindowTextW, GetWindowThreadProcessId, IsIconic, IsWindowVisible, IsZoomed,
};

use crate::frames::{FrameStats, Presents};

const DXGI: GUID = GUID::from_u128(0xCA11C036_0102_4A2D_A6AD_F03CFED5D3C9);
const D3D9: GUID = GUID::from_u128(0x783ACA0A_790E_4D7F_8451_AA850511C6B9);
const DXGKRNL: GUID = GUID::from_u128(0x802EC45A_1E99_4B83_9920_87C98277BA9D);
/// Present_Start, in each provider.
const DXGI_PRESENT: u16 = 42;
const D3D9_PRESENT: u16 = 1;
/// The graphics kernel's: a frame handed to the compositor (PresentHistory_Start or PresentHistoryDetailed_Start, one
/// for each), and a frame flipped to the screen without it (Flip_Info, FlipMultiPlaneOverlay_Info).
const KERNEL_PRESENTS: [u16; 4] = [0xAB, 0xD7, 0xA8, 0xFC];
/// The present models (D3DKMT_PRESENT_MODEL) that tell of no frame, as PresentMon passes them over: a window's GDI
/// drawing (REDIRECTED_GDI, REDIRECTED_GDI_SYSMEM) and a screen capture's fence (SCREENCAPTUREFENCE).
const NOT_FRAMES: [u64; 3] = [1, 5, 6];
/// DXGI_PRESENT_TEST: a present that asks whether the window shows, and draws nothing.
const DXGI_PRESENT_TEST: u64 = 0x1;
/// The session's flush timer counted in milliseconds, not seconds (Windows 8 and later).
const EVENT_TRACE_USE_MS_FLUSH_TIMER: u32 = 0x10;
/// Its keywords asked for: Base and Present (as PresentMon asks).
const DXGKRNL_KEYWORDS: u64 = 0x1 | 0x800_0000;

/// The session's name: one per Glance (a debug build has its own).
#[cfg(not(debug_assertions))]
const SESSION: &str = "Glance.Frames";
#[cfg(debug_assertions)]
const SESSION: &str = "Glance.Frames.Debug";

/// Each process's presents, by process id, in QPC ticks since boot.
static PRESENTS: Mutex<Option<HashMap<u32, Fed>>> = Mutex::new(None);

/// A process's frames, and where they are told from: its runtime's presents (DXGI, Direct3D 9) once it has made one,
/// and from then on those alone (the kernel is handed those frames too); till then, what the kernel is handed of
/// them, as PresentMon counts them (a driver may hand it more than one for a frame drawn: so many it shows).
#[derive(Default)]
struct Fed {
    frames: Presents,
    runtime: bool,
}
/// The moment of the latest event heard of from the session (QPC ticks):
/// frames are told of in batches, a while after they were presented, and
/// only up to here is it known what has been presented.
static HEARD: std::sync::atomic::AtomicI64 = std::sync::atomic::AtomicI64::new(0);
/// QPC ticks a second.
static FREQUENCY: Mutex<i64> = Mutex::new(0);

fn ticks(qpc: i64) -> Duration {
    let frequency = *FREQUENCY.lock().unwrap();
    Duration::from_nanos((qpc as i128 * 1_000_000_000 / frequency.max(1) as i128) as u64)
}


unsafe extern "system" fn on_event(record: *mut EVENT_RECORD) {
    let header = unsafe { &(*record).EventHeader };
    let id = header.EventDescriptor.Id;
    // Whether a frame is told of, and whether by its runtime.
    let told = match header.ProviderId {
        // (A present that only asks whether the window shows draws nothing: no frame, nor a sign of the runtime.)
        DXGI if id == DXGI_PRESENT => (field(record, "Flags").unwrap_or(0) & DXGI_PRESENT_TEST == 0).then_some(true),
        D3D9 if id == D3D9_PRESENT => Some(true),
        // (A window's GDI drawing, or a screen capture's fence, handed to the compositor is no frame of a game's.)
        DXGKRNL if KERNEL_PRESENTS.contains(&id) => (!field(record, "Model").is_some_and(|model| NOT_FRAMES.contains(&model))).then_some(false),
        _ => None,
    };
    // (Every event of the providers, a frame presented or not: they come in the order they happened, near enough.)
    HEARD.fetch_max(header.TimeStamp, std::sync::atomic::Ordering::Relaxed);
    let Some(runtime) = told else { return };
    let at = ticks(header.TimeStamp);
    let mut presents = PRESENTS.lock().unwrap();
    let Some(presents) = presents.as_mut() else { return };
    let fed = presents.entry(header.ProcessId).or_default();
    if runtime && !fed.runtime {
        // Its runtime's from now on: what the kernel told of it before goes.
        *fed = Fed { runtime: true, ..Default::default() };
    }
    if runtime == fed.runtime {
        fed.frames.push(at);
    }
}

/// The field `name` of an event (an integer of up to eight bytes), as its manifest has it.
fn field(record: *const EVENT_RECORD, name: &str) -> Option<u64> {
    use windows::Win32::System::Diagnostics::Etw::{TdhGetProperty, TdhGetPropertySize, PROPERTY_DATA_DESCRIPTOR};
    let wide: Vec<u16> = name.encode_utf16().chain([0]).collect();
    let descriptor = PROPERTY_DATA_DESCRIPTOR { PropertyName: wide.as_ptr() as u64, ArrayIndex: u32::MAX, Reserved: 0 };
    let mut size = 0u32;
    if unsafe { TdhGetPropertySize(record, None, &[descriptor], &mut size) } != ERROR_SUCCESS.0 || size == 0 || size > 8 {
        return None;
    }
    let mut value = [0u8; 8];
    (unsafe { TdhGetProperty(record, None, &[descriptor], &mut value[..size as usize]) } == ERROR_SUCCESS.0).then(|| u64::from_le_bytes(value))
}

/// The session's properties, its name after them.
struct Properties(Vec<u8>);

impl Properties {
    fn new(name: &[u16]) -> Self {
        let size = size_of::<EVENT_TRACE_PROPERTIES>() + name.len() * 2;
        let mut buffer = vec![0u8; size];
        let properties = buffer.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES;
        unsafe {
            (*properties).Wnode.BufferSize = size as u32;
            (*properties).Wnode.Flags = WNODE_FLAG_TRACED_GUID;
            // Timestamps from the performance counter.
            (*properties).Wnode.ClientContext = 1;
            // One buffer for the session: events come in the order they
            // happened, not each processor's in turn; and delivered within
            // a tenth of a second (in milliseconds: delivered by the second,
            // the frame rate shown lagged the game's by about one).
            (*properties).LogFileMode = EVENT_TRACE_REAL_TIME_MODE | EVENT_TRACE_NO_PER_PROCESSOR_BUFFERING | EVENT_TRACE_USE_MS_FLUSH_TIMER;
            (*properties).FlushTimer = 100;
            // Room for a burst (the kernel tells of every program's frames, the compositor's among them, many a
            // second): an event with no room is lost, and a frame lost is a frame not counted, and a stutter that
            // was not there.
            (*properties).BufferSize = 64;
            (*properties).MinimumBuffers = 16;
            (*properties).MaximumBuffers = 64;
            (*properties).LoggerNameOffset = size_of::<EVENT_TRACE_PROPERTIES>() as u32;
        }
        Properties(buffer)
    }

    fn get(&mut self) -> *mut EVENT_TRACE_PROPERTIES {
        self.0.as_mut_ptr() as *mut EVENT_TRACE_PROPERTIES
    }
}

fn name() -> Vec<u16> {
    SESSION.encode_utf16().chain([0]).collect()
}

/// Starts listening, on a thread of its own, for as long as Glance runs
/// (`stop` ends it). Without administrator rights it does nothing, and no
/// program has frames.
pub fn start() {
    let mut frequency = 0i64;
    if unsafe { QueryPerformanceFrequency(&mut frequency) }.is_err() {
        return;
    }
    *FREQUENCY.lock().unwrap() = frequency;
    let name = name();
    let mut handle = CONTROLTRACE_HANDLE::default();
    let mut status = unsafe { StartTraceW(&mut handle, PCWSTR(name.as_ptr()), Properties::new(&name).get()) };
    // Left over from a Glance that ended without stopping it: taken over.
    if status == ERROR_ALREADY_EXISTS {
        stop();
        status = unsafe { StartTraceW(&mut handle, PCWSTR(name.as_ptr()), Properties::new(&name).get()) };
    }
    if status != ERROR_SUCCESS {
        crate::journal::note(format!("frames: no trace session ({})", status.0));
        return;
    }
    for provider in [DXGI, D3D9] {
        let _ = unsafe { EnableTraceEx2(handle, &provider, EVENT_CONTROL_CODE_ENABLE_PROVIDER.0, TRACE_LEVEL_INFORMATION as u8, u64::MAX, 0, 0, None) };
    }
    // The kernel's, only the events that tell of a frame (it tells of much else, many times a frame).
    #[repr(C)]
    struct Ids {
        filter_in: u8,
        reserved: u8,
        count: u16,
        events: [u16; 4],
    }
    let ids = Ids { filter_in: 1, reserved: 0, count: KERNEL_PRESENTS.len() as u16, events: KERNEL_PRESENTS };
    let mut descriptor = EVENT_FILTER_DESCRIPTOR { Ptr: &ids as *const Ids as u64, Size: size_of::<Ids>() as u32, Type: EVENT_FILTER_TYPE_EVENT_ID };
    let parameters = ENABLE_TRACE_PARAMETERS { Version: ENABLE_TRACE_PARAMETERS_VERSION_2, EnableFilterDesc: &mut descriptor, FilterDescCount: 1, ..Default::default() };
    let enabled = unsafe { EnableTraceEx2(handle, &DXGKRNL, EVENT_CONTROL_CODE_ENABLE_PROVIDER.0, TRACE_LEVEL_VERBOSE as u8, DXGKRNL_KEYWORDS, 0, 0, Some(&parameters)) };
    if enabled != ERROR_SUCCESS {
        crate::journal::note(format!("frames: the graphics kernel's not heard ({})", enabled.0));
    }
    *PRESENTS.lock().unwrap() = Some(HashMap::new());
    // How many events the session lost, told in the journal as it grows.
    thread::Builder::new()
        .name("glance: frames lost".into())
        .spawn({
            let name = name.clone();
            move || {
                let mut told = (0, 0);
                loop {
                    thread::sleep(Duration::from_secs(10));
                    let mut properties = Properties::new(&name);
                    let queried = unsafe { ControlTraceW(CONTROLTRACE_HANDLE::default(), PCWSTR(name.as_ptr()), properties.get(), EVENT_TRACE_CONTROL_QUERY) };
                    // (Stopped: it is no more.)
                    if queried != ERROR_SUCCESS {
                        return;
                    }
                    let lost = unsafe { ((*properties.get()).EventsLost, (*properties.get()).RealTimeBuffersLost) };
                    if lost != told {
                        crate::journal::note(format!("frames: the session has lost {} events and {} buffers of them in all", lost.0, lost.1));
                        told = lost;
                    }
                }
            }
        })
        .expect("a thread started");
    thread::Builder::new().name("glance: frames presented".into()).spawn(move || {
        let mut logfile = EVENT_TRACE_LOGFILEW { LoggerName: PWSTR(name.as_ptr() as *mut u16), ..Default::default() };
        // Timestamps left as the session takes them, from the performance
        // counter (otherwise turned into the system time).
        logfile.Anonymous1.ProcessTraceMode = PROCESS_TRACE_MODE_REAL_TIME | PROCESS_TRACE_MODE_EVENT_RECORD | PROCESS_TRACE_MODE_RAW_TIMESTAMP;
        logfile.Anonymous2.EventRecordCallback = Some(on_event);
        unsafe {
            let trace = OpenTraceW(&mut logfile);
            // Returns once the session is stopped.
            let _ = ProcessTrace(&[trace], None, None);
            let _ = CloseTrace(trace);
        }
    }).expect("a thread started");
}

/// Ends the session: it would outlive Glance otherwise.
pub fn stop() {
    let name = name();
    let _ = unsafe { ControlTraceW(CONTROLTRACE_HANDLE::default(), PCWSTR(name.as_ptr()), Properties::new(&name).get(), EVENT_TRACE_CONTROL_STOP) };
}

/// A program presenting frames, as one of its windows shows it: the
/// topmost of its windows that is visible and not minimized.
#[derive(Clone, serde::Serialize, serde::Deserialize)]
pub struct Presenting {
    pub pid: u32,
    pub stats: FrameStats,
    /// Its window's title (what the game calls itself).
    pub title: String,
    /// Whether its window covers its whole screen without being maximized
    /// (a game, borderless or in exclusive fullscreen: a maximized window on
    /// a screen with no taskbar covers it too, and is no game).
    pub fills_screen: bool,
    /// Whether that screen is the one the pointer is on.
    pub under_pointer: bool,
    /// Whether its window is the one in front.
    pub in_front: bool,
    pub refresh_hz: Option<u32>,
    /// Its screen, in physical pixels: left, top, right, bottom.
    pub screen: Option<[i32; 4]>,
}

/// Whether a game shows, as a widget's game button sets it.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum GameMode {
    /// As found: a game whose window covers its screen.
    Auto,
    /// Asked for by hand: the window in front counts as a game whether or
    /// not it covers its screen (a game played in a window).
    On,
    /// Turned off by hand: no game shows, found or not, until the game
    /// found has gone (then `Auto` again).
    Off,
}

static GAME_MODE: std::sync::atomic::AtomicU8 = std::sync::atomic::AtomicU8::new(0);

pub fn game_mode() -> GameMode {
    match GAME_MODE.load(std::sync::atomic::Ordering::Relaxed) {
        1 => GameMode::On,
        2 => GameMode::Off,
        _ => GameMode::Auto,
    }
}

pub fn set_game_mode(mode: GameMode) {
    GAME_MODE.store(mode as u8, std::sync::atomic::Ordering::Relaxed);
}

/// The other process of Glance's (the sensors' process's partner, or the other way about): its windows are no
/// game's either.
static PARTNER: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);

pub fn set_partner(pid: u32) {
    PARTNER.store(pid, std::sync::atomic::Ordering::Relaxed);
}

/// Whether `pid` is one of Glance's own processes.
fn ours(pid: u32) -> bool {
    pid == std::process::id() || pid == PARTNER.load(std::sync::atomic::Ordering::Relaxed)
}

/// The games presenting and the game mode, as the sensors' process last told them (where it reads them): what
/// `presenting` and `game_mode` say here then.
static MIRRORED: Mutex<Option<Vec<Presenting>>> = Mutex::new(None);

pub fn mirror(games: Vec<Presenting>, mode: GameMode) {
    *MIRRORED.lock().unwrap() = Some(games);
    set_game_mode(mode);
}

/// The sensors' process gone: the games it saw are not what is seen here from now on.
pub fn forget_mirrored() {
    *MIRRORED.lock().unwrap() = None;
}

/// Whether frames are counted: told by the sensors' program, or traced here (where Glance may: not, left unelevated).
pub fn counting() -> bool {
    MIRRORED.lock().unwrap().is_some() || PRESENTS.lock().unwrap().is_some()
}

/// Every program presenting frames now, by its topmost window, topmost
/// first; programs that have stopped presenting are forgotten. Looked at
/// through their windows alone: no program's process is opened.
pub fn presenting() -> Vec<Presenting> {
    if let Some(games) = MIRRORED.lock().unwrap().clone() {
        return games;
    }
    let mut now = 0i64;
    if unsafe { QueryPerformanceCounter(&mut now) }.is_err() {
        return Vec::new();
    }
    let now = ticks(now);
    let stats: HashMap<u32, FrameStats> = {
        let mut presents = PRESENTS.lock().unwrap();
        let Some(presents) = presents.as_mut() else { return Vec::new() };
        let heard = ticks(HEARD.load(std::sync::atomic::Ordering::Relaxed));
        // (A program that presents nothing for a while is kept while its frames are: its next frame is as long as the
        // wait, and goes with the others into its 1% low.)
        presents.retain(|_, fed| fed.frames.kept_at(now));
        presents.iter().filter_map(|(pid, fed)| Some((*pid, fed.frames.stats(now, heard)?))).collect()
    };
    if stats.is_empty() {
        return Vec::new();
    }
    let front = unsafe { GetForegroundWindow() };
    let mut cursor = POINT::default();
    let pointer_screen = unsafe { GetCursorPos(&mut cursor) }.ok().map(|_| unsafe { MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST) });
    // The top-level windows, topmost first.
    let mut windows: Vec<HWND> = Vec::new();
    unsafe extern "system" fn gather(window: HWND, windows: LPARAM) -> BOOL {
        unsafe { (*(windows.0 as *mut Vec<HWND>)).push(window) };
        BOOL(1)
    }
    let _ = unsafe { EnumWindows(Some(gather), LPARAM(&mut windows as *mut Vec<HWND> as isize)) };
    let mut found: Vec<Presenting> = Vec::new();
    for window in windows {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
        let Some(&frames) = stats.get(&pid) else { continue };
        if ours(pid) || found.iter().any(|seen| seen.pid == pid) || !unsafe { IsWindowVisible(window) }.as_bool() || unsafe { IsIconic(window) }.as_bool() {
            continue;
        }
        let (fills_screen, refresh_hz, monitor) = screen_of(window);
        let mut title = [0u16; 256];
        let length = unsafe { GetWindowTextW(window, &mut title) };
        found.push(Presenting {
            pid,
            stats: frames,
            title: String::from_utf16_lossy(&title[..length.max(0) as usize]).trim().to_string(),
            fills_screen,
            under_pointer: monitor.is_some() && monitor.map(|(monitor, _)| monitor) == pointer_screen,
            in_front: window == front,
            refresh_hz,
            screen: monitor.map(|(_, r)| [r.left, r.top, r.right, r.bottom]),
        });
    }
    found
}

/// The window in front, as a program that presents nothing just now: for
/// a game taken by hand (see `GameMode::On`), whose frames are then none a
/// second, not unknown. None where what is in front is Glance's own, or
/// the desktop's and the taskbar's (no program's window).
pub fn in_front() -> Option<Presenting> {
    let front = unsafe { GetForegroundWindow() };
    let pid_of = |window: HWND| {
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
        pid
    };
    let pid = pid_of(front);
    if front.is_invalid() || pid == 0 || ours(pid) || pid == pid_of(unsafe { GetShellWindow() }) {
        return None;
    }
    let (fills_screen, refresh_hz, monitor) = screen_of(front);
    let mut cursor = POINT::default();
    let pointer_screen = unsafe { GetCursorPos(&mut cursor) }.ok().map(|_| unsafe { MonitorFromPoint(cursor, MONITOR_DEFAULTTONEAREST) });
    let mut title = [0u16; 256];
    let length = unsafe { GetWindowTextW(front, &mut title) };
    Some(Presenting {
        pid,
        stats: FrameStats { fps: 0.0, low: None, longest_ms: 0.0 },
        title: String::from_utf16_lossy(&title[..length.max(0) as usize]).trim().to_string(),
        fills_screen,
        under_pointer: monitor.is_some() && monitor.map(|(monitor, _)| monitor) == pointer_screen,
        in_front: true,
        refresh_hz,
        screen: monitor.map(|(_, r)| [r.left, r.top, r.right, r.bottom]),
    })
}

/// Whether `window` covers its monitor, not maximized, the monitor's
/// refresh rate, and the monitor.
fn screen_of(window: HWND) -> (bool, Option<u32>, Option<(HMONITOR, RECT)>) {
    unsafe {
        let monitor = MonitorFromWindow(window, MONITOR_DEFAULTTONULL);
        let mut info = MONITORINFOEXW { monitorInfo: MONITORINFO { cbSize: size_of::<MONITORINFOEXW>() as u32, ..Default::default() }, ..Default::default() };
        if monitor.is_invalid() || !GetMonitorInfoW(monitor, &mut info.monitorInfo).as_bool() {
            return (false, None, None);
        }
        let mut rect = RECT::default();
        let fills = !IsZoomed(window).as_bool() && GetWindowRect(window, &mut rect).is_ok() && {
            let screen = info.monitorInfo.rcMonitor;
            rect.left <= screen.left && rect.top <= screen.top && rect.right >= screen.right && rect.bottom >= screen.bottom
        };
        let mut mode = DEVMODEW { dmSize: size_of::<DEVMODEW>() as u16, ..Default::default() };
        let refresh = EnumDisplaySettingsW(PCWSTR(info.szDevice.as_ptr()), ENUM_CURRENT_SETTINGS, &mut mode)
            .as_bool()
            .then_some(mode.dmDisplayFrequency)
            // 0 and 1 stand for the hardware's default.
            .filter(|&hz| hz > 1);
        (fills, refresh, Some((monitor, info.monitorInfo.rcMonitor)))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// For a look at a game in front: run elevated, with the game in front
    /// (`PRESENTS_OUT` names the file the readings go to).
    #[test]
    #[ignore = "needs administrator rights and a game running"]
    fn reads_the_presenting_windows() {
        use std::io::Write;
        let mut out = std::fs::File::create(std::env::var("PRESENTS_OUT").unwrap()).unwrap();
        start();
        for second in 0..60 {
            thread::sleep(Duration::from_secs(1));
            let mut now = 0i64;
            unsafe { QueryPerformanceCounter(&mut now) }.unwrap();
            let now = ticks(now);
            let mut rates: Vec<(u32, f32)> = PRESENTS
                .lock()
                .unwrap()
                .as_ref()
                .map(|presents| presents.iter().filter_map(|(pid, fed)| Some((*pid, fed.frames.stats(now, now)?.fps))).collect())
                .unwrap_or_default();
            rates.sort_by(|a, b| b.1.total_cmp(&a.1));
            let windows: Vec<String> = presenting()
                .iter()
                .map(|p| format!("pid {} '{}' fps {:.1} fills {} pointer {} refresh {:?}", p.pid, p.title, p.stats.fps, p.fills_screen, p.under_pointer, p.refresh_hz))
                .collect();
            writeln!(out, "{second:>2}s presenting {rates:?}\n    windows: {windows:?}").unwrap();
        }
        stop();
    }
}
