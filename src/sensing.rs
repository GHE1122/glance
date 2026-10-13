//! Glance's elevated half (`glance.exe --sensors`): reads the sensors that
//! only an administrator may (the board's chip, the processor's own
//! temperatures, the memory's, the drives'), the frames games present, and
//! tells them over the link to the rest of Glance, which runs as the user.
//! It does nothing else it is asked but what is checked in `link::Asked`,
//! and goes once the rest has gone (and has not come back for a while).

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::link::{self, Asked, Switch, Told, UpdateState, Want};
use crate::metrics::Sampler;

/// How long the rest may be gone (restarting, updating) before the sensors go too.
const WAITED_FOR: Duration = Duration::from_secs(20);

/// Runs the sensors till the rest has gone for good. Not elevated, or with the sensors already running for
/// this user, it does nothing.
pub fn run() {
    if !crate::elevation::is_elevated() {
        return;
    }
    let Some(pipes) = link::Sensors::open() else { return };
    // What an elevated start of Glance has always done: its task kept for later starts, the driver in place.
    crate::elevation::register_launch_task();
    crate::elevation::forget_download();
    if let Ok(exe) = std::env::current_exe() {
        crate::elevation::ensure_pawnio(&exe.with_file_name("resources").join("PawnIO_setup.exe"));
    }
    let mut sampler = Sampler::new();
    crate::presents::start();
    std::thread::spawn(hear_mouse);
    let want = Arc::new(Mutex::new(Want { interval_ms: 1000, refresh_slow: true, wsl: false, docker: false }));
    let pipes = Arc::new(pipes);
    loop {
        // The rest, come (or back): waited for no longer than it may be gone.
        let (sender, came) = mpsc::channel();
        std::thread::spawn({
            let pipes = pipes.clone();
            move || {
                let _ = sender.send(pipes.accept());
            }
        });
        let Ok(Some((told, asked))) = came.recv_timeout(WAITED_FOR) else { break };
        // (The rest's windows are no game's.)
        if let Some(pid) = link::client_of(&pipes) {
            crate::presents::set_partner(pid);
        }
        let telling = Arc::new(Mutex::new(told));
        let tell = |told: &Told| link::tell(&mut telling.lock().unwrap(), told);
        let hello = Told::Hello { version: env!("CARGO_PKG_VERSION").into(), info: sampler.info.clone(), found: sampler.info.found.clone() };
        let greeted = tell(&hello)
            && tell(&Told::Switched { switch: Switch::Autostart, on: crate::elevation::autostart_enabled() })
            && tell(&Told::Switched { switch: Switch::Remote, on: crate::phone::on() });
        // What it is asked, heard on a thread of its own; a report is said by the reading loop, the rest done here.
        let (report_wanted, report_asked) = mpsc::channel::<()>();
        let listener = std::thread::spawn({
            let want = want.clone();
            let telling = telling.clone();
            let mut asked = asked.try_clone().ok();
            move || {
                let tell = |told: &Told| link::tell(&mut telling.lock().unwrap(), told);
                while let Some(asking) = asked.as_mut().and_then(link::heard) {
                    match asking {
                        Asked::Want(wanted) => *want.lock().unwrap() = wanted.checked(),
                        Asked::GameMode(mode) => crate::presents::set_game_mode(mode.into()),
                        Asked::Report => {
                            let _ = report_wanted.send(());
                        }
                        Asked::Switch { switch: Switch::Autostart, on } => {
                            tell(&Told::Switched { switch: Switch::Autostart, on: crate::elevation::set_autostart(on) });
                        }
                        Asked::Switch { switch: Switch::Remote, on } => {
                            tell(&Told::Switched { switch: Switch::Remote, on: crate::phone::turn(on) });
                        }
                        Asked::Install(release) => {
                            tell(&Told::Updating(UpdateState::Downloading));
                            tell(&Told::Updating(crate::update::fetch_and_run_sent(&release)));
                        }
                    }
                }
            }
        });
        // The mouse's motion the rest cannot hear, told as it comes (a few milliseconds' worth at a time).
        let connected = Arc::new(std::sync::atomic::AtomicBool::new(true));
        let relay = std::thread::spawn({
            let telling = telling.clone();
            let connected = connected.clone();
            move || {
                while connected.load(std::sync::atomic::Ordering::Relaxed) {
                    std::thread::sleep(Duration::from_millis(8));
                    let (dx, dy) = std::mem::take(&mut *MOVED.lock().unwrap());
                    if (dx, dy) != (0, 0) && !link::tell(&mut telling.lock().unwrap(), &Told::Mouse { dx, dy }) {
                        break;
                    }
                }
            }
        });
        let mut next = Instant::now();
        let mut tick = 0u64;
        while greeted {
            // Till the next reading is due, a report asked for meanwhile is said at once; the listener gone (the
            // rest of Glance gone with it), the reading ends.
            let now = Instant::now();
            if now < next {
                match report_asked.recv_timeout(next - now) {
                    Ok(()) if !tell(&Told::Report(crate::diagnostics::sensor_lines())) => break,
                    Ok(()) | Err(mpsc::RecvTimeoutError::Timeout) => continue,
                    Err(mpsc::RecvTimeoutError::Disconnected) => break,
                }
            }
            let wanted = *want.lock().unwrap();
            let interval = Duration::from_millis(wanted.interval_ms as u64);
            // (Fallen behind by more than a reading, it goes on from now.)
            next = if next + interval < now { now + interval } else { next + interval };
            tick += 1;
            let refresh_slow = wanted.refresh_slow || tick % crate::HIDDEN_SLOW_TICKS == 0;
            if let Some(sample) = sampler.sample(refresh_slow, wanted.wsl, wanted.docker) {
                if !tell(&Told::Sample(Box::new(sample))) || !tell(&Told::Presenting { games: crate::presents::presenting(), mode: crate::presents::game_mode().into() }) {
                    break;
                }
            }
            if listener.is_finished() {
                break;
            }
        }
        // Gone: the pipes let go (which ends the listener's reading), for the rest to come again.
        connected.store(false, std::sync::atomic::Ordering::Relaxed);
        let _ = relay.join();
        drop(telling);
        drop(asked);
        pipes.let_go();
        // (Its read, which letting the pipes go does not end, given up on: it ends, and is waited for.)
        unsafe {
            use std::os::windows::io::AsRawHandle;
            let _ = windows::Win32::System::IO::CancelSynchronousIo(windows::Win32::Foundation::HANDLE(listener.as_raw_handle()));
        }
        let _ = listener.join();
    }
    crate::presents::stop();
}

/// The mouse's motion heard here and not yet told (raw counts across and down).
static MOVED: Mutex<(i32, i32)> = Mutex::new((0, 0));

/// Hears the mouse's own motion (raw input, which reaches an elevated process whatever is in front), keeping what
/// it hears while the window in front is an elevated one: the rest of Glance, running as the user, hears nothing
/// of the mouse then (Windows keeps an elevated window's input from processes below it).
fn hear_mouse() {
    use windows::core::w;
    use windows::Win32::UI::Input::{GetRawInputData, RegisterRawInputDevices, HRAWINPUT, RAWINPUT, RAWINPUTDEVICE, RAWINPUTHEADER, RIDEV_INPUTSINK, RID_INPUT, RIM_TYPEMOUSE};
    use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, GetMessageW, HWND_MESSAGE, MSG, WM_INPUT};
    unsafe {
        let Ok(window) = CreateWindowExW(Default::default(), w!("STATIC"), w!(""), Default::default(), 0, 0, 0, 0, Some(HWND_MESSAGE), None, None, None) else { return };
        let mouse = RAWINPUTDEVICE { usUsagePage: 1, usUsage: 2, dwFlags: RIDEV_INPUTSINK, hwndTarget: window };
        if RegisterRawInputDevices(&[mouse], size_of::<RAWINPUTDEVICE>() as u32).is_err() {
            return;
        }
        let mut front = Front::default();
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            if message.message != WM_INPUT || !front.elevated() {
                continue;
            }
            let mut raw = RAWINPUT::default();
            let mut size = size_of::<RAWINPUT>() as u32;
            let read = GetRawInputData(HRAWINPUT(message.lParam.0 as *mut _), RID_INPUT, Some(&mut raw as *mut _ as *mut _), &mut size, size_of::<RAWINPUTHEADER>() as u32);
            // (Moves to a place, as a tablet's, say nothing of a push: the rest reads the pointer's place itself.)
            const ABSOLUTE: u16 = 1;
            if read == u32::MAX || raw.header.dwType != RIM_TYPEMOUSE.0 || raw.data.mouse.usFlags.0 & ABSOLUTE != 0 {
                continue;
            }
            let mut moved = MOVED.lock().unwrap();
            moved.0 = moved.0.saturating_add(raw.data.mouse.lLastX);
            moved.1 = moved.1.saturating_add(raw.data.mouse.lLastY);
        }
    }
}

/// The window in front, and whether its program runs elevated (looked at again only when another comes to the
/// front).
#[derive(Default)]
struct Front {
    window: isize,
    elevated: bool,
}

impl Front {
    fn elevated(&mut self) -> bool {
        use windows::Win32::Foundation::{CloseHandle, HANDLE};
        use windows::Win32::Security::{GetTokenInformation, TokenElevation, TOKEN_ELEVATION, TOKEN_QUERY};
        use windows::Win32::System::Threading::{OpenProcess, OpenProcessToken, PROCESS_QUERY_LIMITED_INFORMATION};
        use windows::Win32::UI::WindowsAndMessaging::{GetForegroundWindow, GetWindowThreadProcessId};
        let window = unsafe { GetForegroundWindow() };
        if window.0 as isize == self.window {
            return self.elevated;
        }
        self.window = window.0 as isize;
        let mut pid = 0u32;
        unsafe { GetWindowThreadProcessId(window, Some(&mut pid)) };
        self.elevated = pid != 0
            && pid != std::process::id()
            && unsafe {
                let Ok(process) = OpenProcess(PROCESS_QUERY_LIMITED_INFORMATION, false, pid) else { return false };
                let mut token = HANDLE::default();
                let mut elevation = TOKEN_ELEVATION::default();
                let mut size = 0u32;
                let raised = OpenProcessToken(process, TOKEN_QUERY, &mut token).is_ok()
                    && GetTokenInformation(token, TokenElevation, Some((&mut elevation as *mut TOKEN_ELEVATION).cast()), size_of::<TOKEN_ELEVATION>() as u32, &mut size).is_ok()
                    && elevation.TokenIsElevated != 0;
                if !token.is_invalid() {
                    let _ = CloseHandle(token);
                }
                let _ = CloseHandle(process);
                raised
            };
        self.elevated
    }
}
