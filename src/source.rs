//! Where Glance's readings come from: the sensors' own process, elevated,
//! over the link (see `sensing`); or, where there is none to be had (the
//! user did not let Glance run as administrator), a sampler of Glance's own,
//! reading what needs no rights. Glance itself runs as the user either way,
//! but where it was started elevated (an older task, a build run by hand),
//! when it reads the sensors itself, as it always did.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::link::{Asked, GameModeSent, Link, Told, Want};
use crate::metrics::Sampler;
use crate::presents::GameMode;
use crate::reading::{Sample, StaticInfo};

/// How long the sensors are waited for as they are started (an elevated start Windows asks the user about may
/// take a while to be answered).
const STARTED_WITHIN: Duration = Duration::from_secs(60);

/// What the readings come from.
pub enum Source {
    Linked(Link),
    Own { sampler: Sampler, next: Instant, tick: u64 },
}

/// The link's asking end, for what is asked of the sensors from any thread; whether there is a link.
static ASKING: Mutex<Option<std::fs::File>> = Mutex::new(None);
static LINKED: AtomicBool = AtomicBool::new(false);
/// What the sensors last said for a report, as it came.
static REPORTED: Mutex<Option<Vec<String>>> = Mutex::new(None);
/// What was last asked of them (asked again only when it changes).
static WANTED: Mutex<Option<Want>> = Mutex::new(None);

/// The readings' source, and what the machine is. Elevated already, Glance reads the sensors itself; else it
/// links to the sensors running, or starts them (through its task, or by asking the user) and links to them;
/// failing that, it reads what it can itself.
pub fn open() -> (StaticInfo, Source) {
    if !crate::elevation::is_elevated() {
        let linked = Link::connect(Duration::from_millis(300)).or_else(|| crate::elevation::start_sensors().then(|| Link::connect(STARTED_WITHIN)).flatten());
        if let Some(mut link) = linked {
            if let Some(Told::Hello { info, .. }) = link.receive() {
                if let Ok(asking) = link.asked.try_clone() {
                    *ASKING.lock().unwrap() = Some(asking);
                }
                LINKED.store(true, Ordering::Relaxed);
                crate::journal::note("readings: from the sensors' process");
                return (info, Source::Linked(link));
            }
        }
        crate::journal::note("readings: Glance's own, without administrator rights (the sensors' process was not had)");
    }
    let sampler = Sampler::new();
    crate::presents::start();
    (sampler.info.clone(), Source::Own { sampler, next: Instant::now(), tick: 0 })
}

/// Whether the readings come from the sensors' process.
pub fn linked() -> bool {
    LINKED.load(Ordering::Relaxed)
}

fn ask(asked: &Asked) {
    if let Some(asking) = ASKING.lock().unwrap().as_mut() {
        let _ = crate::link::ask_on(asking, asked);
    }
}

/// Whether a game shows, as the user sets it: here, and in the sensors' process, which finds the games.
pub fn set_game_mode(mode: GameMode) {
    crate::presents::set_game_mode(mode);
    ask(&Asked::GameMode(GameModeSent::from(mode)));
}

/// What the sensors have to say for a report: asked of their process (waited for a moment), or said here.
pub fn sensor_lines() -> Vec<String> {
    if !linked() {
        return crate::diagnostics::sensor_lines();
    }
    *REPORTED.lock().unwrap() = None;
    ask(&Asked::Report);
    let until = Instant::now() + Duration::from_secs(3);
    while Instant::now() < until {
        if let Some(lines) = REPORTED.lock().unwrap().take() {
            return lines;
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    vec!["Sensors: no answer from their process".into()]
}

impl Source {
    /// The next sample, as `want` asks for them: waited for (the sensors' process sends each as it reads it; Glance's
    /// own sampler reads at its time). None when there was none this time (a sample that could not be taken); the
    /// link lost, Glance reads what it can itself from then on.
    pub fn next(&mut self, want: Want) -> Option<Sample> {
        match self {
            Source::Linked(link) => {
                if *WANTED.lock().unwrap() != Some(want) {
                    *WANTED.lock().unwrap() = Some(want);
                    ask(&Asked::Want(want));
                }
                loop {
                    match link.receive() {
                        Some(Told::Sample(sample)) => return Some(*sample),
                        Some(Told::Presenting { games, mode }) => crate::presents::mirror(games, mode.into()),
                        Some(Told::Report(lines)) => *REPORTED.lock().unwrap() = Some(lines),
                        Some(Told::Mouse { dx, dy }) => crate::app().controller.relay_mouse(dx, dy),
                        Some(Told::Hello { .. }) => {}
                        None => {
                            crate::journal::note("readings: the sensors' process went; Glance reads what it can itself");
                            LINKED.store(false, Ordering::Relaxed);
                            *ASKING.lock().unwrap() = None;
                            let sampler = Sampler::new();
                            crate::presents::start();
                            *self = Source::Own { sampler, next: Instant::now(), tick: 0 };
                            return None;
                        }
                    }
                }
            }
            Source::Own { sampler, next, tick } => {
                let interval = Duration::from_millis(want.interval_ms as u64);
                *next += interval;
                // Fallen far behind (the machine slept): carried on from now rather than sampled again and again.
                let now = Instant::now();
                if *next + interval < now {
                    *next = now;
                }
                std::thread::sleep(next.saturating_duration_since(now));
                *tick += 1;
                sampler.sample(want.refresh_slow || *tick % crate::HIDDEN_SLOW_TICKS == 0, want.wsl, want.docker)
            }
        }
    }
}
