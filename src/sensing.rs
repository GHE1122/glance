//! Glance's elevated half (`glance.exe --sensors`): reads the sensors that
//! only an administrator may (the board's chip, the processor's own
//! temperatures, the memory's, the drives'), the frames games present, and
//! tells them over the link to the rest of Glance, which runs as the user.
//! It does nothing else it is asked but what is checked in `link::Asked`,
//! and goes once the rest has gone (and has not come back for a while).

use std::sync::mpsc;
use std::sync::{Arc, Mutex};
use std::time::{Duration, Instant};

use crate::link::{self, Asked, Told, Want};
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
        let Ok(Some((mut told, asked))) = came.recv_timeout(WAITED_FOR) else { break };
        // (The rest's windows are no game's.)
        if let Some(pid) = link::client_of(&pipes) {
            crate::presents::set_partner(pid);
        }
        let hello = Told::Hello { version: env!("CARGO_PKG_VERSION").into(), info: sampler.info.clone(), found: sampler.info.found.clone() };
        if !link::tell(&mut told, &hello) {
            pipes.let_go((told, asked));
            continue;
        }
        // What it is asked, heard on a thread of its own; a report asked for is told by the reading loop.
        let (report_wanted, report_asked) = mpsc::channel::<()>();
        let listener = std::thread::spawn({
            let want = want.clone();
            let mut asked = asked.try_clone().ok();
            move || {
                while let Some(asking) = asked.as_mut().and_then(link::heard) {
                    match asking {
                        Asked::Want(wanted) => *want.lock().unwrap() = wanted.checked(),
                        Asked::GameMode(mode) => crate::presents::set_game_mode(mode.into()),
                        Asked::Report => {
                            let _ = report_wanted.send(());
                        }
                    }
                }
            }
        });
        let mut next = Instant::now();
        let mut tick = 0u64;
        let gone = loop {
            let wanted = *want.lock().unwrap();
            let interval = Duration::from_millis(wanted.interval_ms as u64);
            next += interval;
            let now = Instant::now();
            if next + interval < now {
                next = now;
            }
            // (A report asked for meanwhile is told at once.)
            if report_asked.recv_timeout(next.saturating_duration_since(now)).is_ok() && !link::tell(&mut told, &Told::Report(crate::diagnostics::sensor_lines())) {
                break true;
            }
            if Instant::now() < next {
                continue;
            }
            tick += 1;
            let refresh_slow = wanted.refresh_slow || tick % crate::HIDDEN_SLOW_TICKS == 0;
            if let Some(sample) = sampler.sample(refresh_slow, wanted.wsl, wanted.docker) {
                if !link::tell(&mut told, &Told::Sample(Box::new(sample))) || !link::tell(&mut told, &Told::Presenting { games: crate::presents::presenting(), mode: crate::presents::game_mode().into() }) {
                    break true;
                }
            }
            if listener.is_finished() {
                break true;
            }
        };
        if gone {
            pipes.let_go((told, asked));
            let _ = listener.join();
        }
    }
    crate::presents::stop();
}
