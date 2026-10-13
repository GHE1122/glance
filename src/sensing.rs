//! Glance's elevated half (`glance.exe --sensors`): reads the sensors that
//! only an administrator may (the board's chip, the processor's own
//! temperatures, the memory's, the drives'), the frames games present, and
//! tells them over the link to the rest of Glance, which runs as the user.
//! It watches the edges that open the panel too (see `push`): it hears the
//! mouse whatever window is in front, which the rest does not.
//! It does nothing else it is asked but what is checked in `link::Asked`,
//! and goes once the rest has gone (and has not come back for a while).

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::link::{self, Asked, Switch, Told, UpdateState, Want};
use crate::push::{Edges, Pusher};
use crate::metrics::Sampler;

/// How long the rest may be gone (restarting, updating) before the sensors go too.
const WAITED_FOR: Duration = Duration::from_secs(20);
/// How long what came has to say who it is.
const HELLO_WITHIN: Duration = Duration::from_secs(5);
/// How long a reading may take before the sensors are taken to be stuck (a driver, a device, a system call that
/// does not come back): they end, and the rest of Glance starts them again.
const STUCK_AFTER: Duration = Duration::from_secs(30);

/// Runs the sensors till the rest has gone for good. Not elevated, or with the sensors already running for
/// this user, it does nothing.
pub fn run() {
    if !crate::elevation::is_elevated() {
        return;
    }
    let Some(pipes) = link::Sensors::open() else { return };
    // Its lines go to the rest of Glance, which writes the log (two processes writing one file lose lines).
    crate::journal::hold_for_the_app();
    // What an elevated start of Glance has always done: its task kept for later starts, the driver in place.
    crate::elevation::register_launch_task();
    crate::elevation::forget_download();
    if let Ok(exe) = std::env::current_exe() {
        crate::elevation::ensure_pawnio(&exe.with_file_name("resources").join("PawnIO_setup.exe"));
    }
    let mut sampler = Sampler::new();
    crate::presents::start();
    std::thread::Builder::new().name("glance sensors: edges".into()).spawn(watch_edges).expect("a thread started");
    std::thread::Builder::new().name("glance sensors: readings watched".into()).spawn(watch_readings).expect("a thread started");
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
        // What came says who it is first, within a few seconds (what says nothing is let go: it would keep the
        // rest out); it is checked (through the pipe) before it is told anything.
        let (said, hello) = mpsc::channel();
        let hearing = std::thread::spawn({
            let mut asked = asked.try_clone().ok();
            move || {
                let _ = said.send(asked.as_mut().and_then(link::heard));
            }
        });
        let first = hello.recv_timeout(HELLO_WITHIN).ok().flatten();
        if !hearing.is_finished() {
            unsafe {
                use std::os::windows::io::AsRawHandle;
                let _ = windows::Win32::System::IO::CancelSynchronousIo(windows::Win32::Foundation::HANDLE(hearing.as_raw_handle()));
            }
        }
        let _ = hearing.join();
        let checked = match first {
            Some(Asked::Hello { version }) if version == env!("CARGO_PKG_VERSION") => pipes.check_client(),
            // Another version's Glance: it starts the sensors of its own version once these have gone.
            Some(Asked::Hello { version }) => {
                crate::journal::note(format!("sensors: Glance {version} came, these are {}: going, for its own", env!("CARGO_PKG_VERSION")));
                break;
            }
            _ => Err("it said nothing first".into()),
        };
        let pid = match checked {
            Ok(pid) => pid,
            Err(why) => {
                crate::journal::note(format!("sensors: what came is not taken ({why})"));
                drop((told, asked));
                pipes.let_go();
                continue;
            }
        };
        // (The rest's windows are no game's.)
        crate::presents::set_partner(pid);
        let telling = Arc::new(Mutex::new(told));
        let tell = |told: &Told| link::tell(&mut telling.lock().unwrap(), told);
        let hello = Told::Hello { version: env!("CARGO_PKG_VERSION").into(), info: sampler.info.clone(), found: sampler.info.found.clone() };
        let greeted = tell(&hello)
            && tell(&Told::Buttons { down: crate::panel::buttons_down(), pressed: false })
            && tell(&Told::Switched { switch: Switch::Autostart, on: crate::elevation::autostart_enabled() })
            && tell(&Told::Switched { switch: Switch::Remote, on: crate::phone::on() });
        // What it is asked, heard on a thread of its own; a report is said by the reading loop, the rest done here.
        let (report_wanted, report_asked) = mpsc::channel::<()>();
        let listener = std::thread::Builder::new().name("glance sensors: asked".into()).spawn({
            let want = want.clone();
            let telling = telling.clone();
            let mut asked = asked.try_clone().ok();
            move || {
                let tell = |told: &Told| link::tell(&mut telling.lock().unwrap(), told);
                while let Some(asking) = asked.as_mut().and_then(link::heard) {
                    match asking {
                        // (Said once, first; heard again, it is nothing.)
                        Asked::Hello { .. } => {}
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
                        Asked::Edges(edges) => *WATCHED.lock().unwrap() = edges.checked(),
                        Asked::Activity(on) => HEARING_ACTIVITY.store(on, std::sync::atomic::Ordering::Relaxed),
                    }
                }
            }
        }).expect("a thread started");
        // The pushes seen from now on, told to it.
        *PUSHES_TO.lock().unwrap() = Some(telling.clone());
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
            if !crate::journal::take_held().into_iter().all(|line| tell(&Told::Note(line))) {
                break;
            }
            *READING_SINCE.lock().unwrap() = Some(Instant::now());
            let sample = sampler.sample(refresh_slow, wanted.wsl, wanted.docker);
            *READING_SINCE.lock().unwrap() = None;
            if let Some(sample) = sample {
                if !tell(&Told::Sample(Box::new(sample))) || !tell(&Told::Presenting { games: crate::presents::presenting(), mode: crate::presents::game_mode().into() }) {
                    break;
                }
            }
            if listener.is_finished() {
                break;
            }
        }
        // Gone: the pipes let go (which ends the listener's reading), for the rest to come again; no edge watched
        // till it says again which.
        *PUSHES_TO.lock().unwrap() = None;
        *WATCHED.lock().unwrap() = Edges::none();
        HEARING_ACTIVITY.store(false, std::sync::atomic::Ordering::Relaxed);
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

/// When the reading under way began (none: none is).
static READING_SINCE: Mutex<Option<Instant>> = Mutex::new(None);

/// Ends the sensors once a reading has been under way for longer than `STUCK_AFTER`, the rest of Glance told why
/// (on the pipe the reading does not hold meanwhile): stuck, they serve nothing, and the rest starts them again.
fn watch_readings() {
    loop {
        std::thread::sleep(STUCK_AFTER / 6);
        let since = *READING_SINCE.lock().unwrap();
        if since.is_some_and(|since| since.elapsed() > STUCK_AFTER) {
            let why = format!("sensors: a reading has taken over {} s; ending, to be started again", STUCK_AFTER.as_secs());
            // (Not waited for: a pipe the rest has stopped reading may hold it.)
            if let Some(Ok(mut telling)) = PUSHES_TO.lock().unwrap().as_ref().map(|telling| telling.try_lock()) {
                let _ = link::tell(&mut telling, &Told::Note(why));
            }
            std::process::exit(1);
        }
    }
}

/// The edges watched, as the rest last said; and where each push seen is told (none: the rest is not there).
static WATCHED: Mutex<Edges> = Mutex::new(Edges::none());
static PUSHES_TO: Mutex<Option<Arc<Mutex<std::fs::File>>>> = Mutex::new(None);
/// Whether the rest wants to hear what touches the PC (see `Asked::Activity`).
static HEARING_ACTIVITY: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);
/// What is heard of what touches the PC is told at most this often (a mouse reports a thousand times a second), and
/// at once when its kind changes (a touch after a report that said nothing, or the other way).
const ACTIVITY_EVERY: Duration = Duration::from_millis(50);

/// How often the pointer is looked at (for one that moves without the mouse's motion being heard: see
/// `Pusher::looked`), and how often again while it rests on an edge.
const LOOK_MS: u32 = 100;
const RESTING_MS: u32 = 16;
const LOOK_TIMER: usize = 1;
const RESTING_TIMER: usize = 2;

/// Watches the edges that open the panel, on a thread of its own: the mouse's own motion (raw input, which reaches
/// an elevated process whatever window is in front) and the pointer's place; each push seen told to the rest. And
/// the mouse's buttons, told as they change; and, while the rest asks, what touches the PC (every kind of device the
/// phone remote wakes the screens for), all heard by the one window (a process hears a kind of device at one
/// window only).
fn watch_edges() {
    use windows::core::w;
    use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, WPARAM};
    use windows::Win32::System::LibraryLoader::GetModuleHandleW;
    use windows::Win32::System::SystemInformation::GetTickCount;
    use windows::Win32::UI::Input::{RegisterRawInputDevices, HRAWINPUT, RAWINPUTDEVICE, RIDEV_INPUTSINK};
    use windows::Win32::UI::WindowsAndMessaging::{
        CreateWindowExW, DefWindowProcW, DispatchMessageW, GetMessageW, KillTimer, RegisterClassW, SetTimer, MSG, WINDOW_STYLE, WM_DISPLAYCHANGE, WM_INPUT, WM_TIMER, WNDCLASSW, WS_EX_TOOLWINDOW,
    };

    // A window never shown, not a message-only one: it hears that the screens changed, as those do not.
    unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
        if message == WM_DISPLAYCHANGE {
            crate::screens::changed();
        }
        unsafe { DefWindowProcW(window, message, wparam, lparam) }
    }
    let say = |told: &Told| {
        if let Some(telling) = PUSHES_TO.lock().unwrap().as_ref() {
            let _ = link::tell(&mut telling.lock().unwrap(), told);
        }
    };
    let tell = |pushed: Option<crate::push::Pushed>| {
        let Some((at, _, edge)) = pushed else { return };
        say(&Told::Pushed { edge, at: (at.x, at.y) });
    };
    // Whether a button was down as last said.
    let mut down = crate::panel::buttons_down();
    // What touches the PC, while it is heard: what was heard, and what kind was told when.
    let mut activity: Option<(crate::touch::Heard, Option<(bool, Instant)>)> = None;
    let tell_activity = |heard: &crate::touch::Heard| {
        let named = |heard: &Option<(String, u32)>| heard.as_ref().map(|(device, at)| (device.chars().take(link::DEVICE_MOST).collect(), *at));
        say(&Told::Activity { touched: named(&heard.touched), nothing: named(&heard.nothing) });
    };
    unsafe {
        let Ok(module) = GetModuleHandleW(None) else { return };
        let class = WNDCLASSW { lpfnWndProc: Some(procedure), hInstance: module.into(), lpszClassName: w!("GlanceEdges"), ..Default::default() };
        RegisterClassW(&class);
        let Ok(window) = CreateWindowExW(WS_EX_TOOLWINDOW, class.lpszClassName, w!(""), WINDOW_STYLE(0), 0, 0, 0, 0, None, None, Some(module.into()), None) else { return };
        let mouse = RAWINPUTDEVICE { usUsagePage: 1, usUsage: 2, dwFlags: RIDEV_INPUTSINK, hwndTarget: window };
        if RegisterRawInputDevices(&[mouse], size_of::<RAWINPUTDEVICE>() as u32).is_err() {
            return;
        }
        SetTimer(Some(window), LOOK_TIMER, LOOK_MS, None);
        let mut pusher = Pusher::default();
        let mut resting = false;
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            let now = Instant::now();
            match message.message {
                WM_INPUT => {
                    if let Some((moved, buttons)) = crate::panel::read_mouse(HRAWINPUT(message.lParam.0 as *mut _)) {
                        if let Some(cursor) = crate::panel::cursor_position() {
                            tell(pusher.heard(&WATCHED.lock().unwrap(), moved, cursor, now));
                        }
                        // The buttons: a press as the device reports it, whether one is down as read now (this
                        // process reads them truly).
                        let pressed = buttons & crate::panel::ANY_BUTTON_DOWN != 0;
                        let now_down = crate::panel::buttons_down();
                        if pressed || now_down != down {
                            down = now_down;
                            say(&Told::Buttons { down, pressed });
                        }
                    }
                    if let Some((heard, last)) = &mut activity {
                        if let Some(said) = heard.take(message.lParam, GetTickCount()) {
                            if last.is_none_or(|(kind, at)| kind != said || now.duration_since(at) >= ACTIVITY_EVERY) {
                                tell_activity(heard);
                                *last = Some((said, now));
                            }
                        }
                    }
                }
                WM_TIMER if message.wParam.0 == LOOK_TIMER => {
                    // What touches the PC heard from now on, or no longer, as the rest asks; each time anew (what
                    // was heard the last time is not of this one).
                    let wanted = HEARING_ACTIVITY.load(std::sync::atomic::Ordering::Relaxed);
                    // (The mouse is heard here throughout.)
                    if wanted != activity.is_some() && crate::touch::hear_but_the_mouse(window, wanted).is_ok() {
                        activity = wanted.then(|| {
                            let heard = crate::touch::Heard::default();
                            tell_activity(&heard);
                            (heard, None)
                        });
                    }
                    if let Some(cursor) = crate::panel::cursor_position() {
                        pusher.looked(&WATCHED.lock().unwrap(), cursor, now);
                    }
                }
                WM_TIMER if message.wParam.0 == RESTING_TIMER => {
                    if let Some(cursor) = crate::panel::cursor_position() {
                        tell(pusher.rested(&WATCHED.lock().unwrap(), cursor, now));
                    }
                }
                _ => {}
            }
            if pusher.resting() != resting {
                resting = pusher.resting();
                if resting {
                    SetTimer(Some(window), RESTING_TIMER, RESTING_MS, None);
                } else {
                    let _ = KillTimer(Some(window), RESTING_TIMER);
                }
            }
            DispatchMessageW(&message);
        }
    }
}
