//! Where Glance's readings come from: the sensors' own process, elevated,
//! over the link (see `sensing`); or, where there is none to be had (the
//! user did not let Glance run as administrator), a sampler of Glance's own,
//! reading what needs no rights. Glance itself runs as the user either way,
//! but where it was started elevated (an older task, a build run by hand),
//! when it reads the sensors itself, as it always did.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use crate::link::{Asked, GameModeSent, Link, Switch, Told, Want};
use crate::metrics::Sampler;
use crate::presents::GameMode;
use crate::push::Edges;
use crate::reading::{Sample, StaticInfo};

/// How long the sensors are waited for as they are started (an elevated start Windows asks the user about may
/// take a while to be answered).
const STARTED_WITHIN: Duration = Duration::from_secs(60);
/// How long they are waited for as they are started again through their task, the link lost (no one is asked).
const STARTED_AGAIN_WITHIN: Duration = Duration::from_secs(15);
/// How long a link must have lasted for its sensors, gone, to be started again: those that fail again and again
/// as they start are not started without end.
const LASTED_TO_START_AGAIN: Duration = Duration::from_secs(60);

/// What the readings come from.
pub enum Source {
    Linked(Link),
    Own { sampler: Sampler, next: Instant, tick: u64 },
}

/// The link's asking end, for what is asked of the sensors from any thread; whether there is a link.
static ASKING: Mutex<Option<std::fs::File>> = Mutex::new(None);
static LINKED: AtomicBool = AtomicBool::new(false);
/// Since when the link there is has been there.
static LINKED_SINCE: Mutex<Option<Instant>> = Mutex::new(None);
/// How many links there have been (one asked something before the latest knows nothing of it).
static LINKS: std::sync::atomic::AtomicU64 = std::sync::atomic::AtomicU64::new(0);
/// What the sensors last said for a report, as it came.
static REPORTED: Mutex<Option<Vec<String>>> = Mutex::new(None);
/// What was last asked of them (asked again only when it changes): the readings, and the edges they watch.
static WANTED: Mutex<Option<Want>> = Mutex::new(None);
static EDGES: Mutex<Option<Edges>> = Mutex::new(None);
/// How the switches that need their rights are, as they last told (none: not told yet), and the turn of each
/// told (for one waiting on an answer).
static SWITCHES: Mutex<[(Option<bool>, u64); 2]> = Mutex::new([(None, 0), (None, 0)]);

fn slot(switch: Switch) -> usize {
    match switch {
        Switch::Autostart => 0,
        Switch::Remote => 1,
    }
}

/// Whether Glance starts at sign-in: as the sensors' process says (their task was made elevated), else as read
/// here.
pub fn autostart() -> bool {
    if linked() {
        if let Some(on) = SWITCHES.lock().unwrap()[0].0 {
            return on;
        }
    }
    crate::elevation::autostart_enabled()
}

/// Turns a switch that needs the sensors' rights: by their process (its answer waited for, a while), or here.
/// Whether it is on then.
pub fn switch(switch: Switch, on: bool) -> bool {
    if !linked() {
        return match switch {
            Switch::Autostart => crate::elevation::set_autostart(on),
            Switch::Remote => crate::phone::turn(on),
        };
    }
    let turn = SWITCHES.lock().unwrap()[slot(switch)].1;
    ask(&Asked::Switch { switch, on });
    let until = Instant::now() + Duration::from_secs(20);
    while Instant::now() < until {
        let (now, told) = SWITCHES.lock().unwrap()[slot(switch)];
        if told != turn {
            return now.unwrap_or(false);
        }
        std::thread::sleep(Duration::from_millis(50));
    }
    SWITCHES.lock().unwrap()[slot(switch)].0.unwrap_or(false)
}

/// Asks the sensors' process to download and run the release found newer.
pub fn install(release: crate::link::ReleaseSent) {
    ask(&Asked::Install(release));
}

/// The readings' source, and what the machine is. Elevated already, Glance reads the sensors itself; else it
/// links to the sensors running, or starts them (through its task, or by asking the user) and links to them;
/// failing that, it reads what it can itself.
pub fn open() -> (StaticInfo, Source) {
    if !crate::elevation::is_elevated() {
        // The sensors running, and of this version; else (none running, or another version's, which goes as it
        // hears this one) this version's started.
        let running = Link::connect(Duration::from_millis(300));
        let turned_away = running.is_some();
        let mut linked = greeted(running);
        if linked.is_none() {
            if turned_away {
                // (Its pipes free once it has gone.)
                std::thread::sleep(Duration::from_secs(2));
            }
            linked = crate::elevation::start_sensors().then(|| greeted(Link::connect(STARTED_WITHIN))).flatten();
        }
        if let Some((link, info)) = linked {
            linked_to(&link);
            crate::journal::note("readings: from the sensors' process");
            return (info, Source::Linked(link));
        }
        crate::journal::note("readings: Glance's own, without administrator rights (the sensors' process was not had)");
    }
    let sampler = Sampler::new();
    crate::presents::start();
    (sampler.info.clone(), Source::Own { sampler, next: Instant::now(), tick: 0 })
}

/// `link`, if the sensors at its other end said hello: with what the machine is.
fn greeted(link: Option<Link>) -> Option<(Link, StaticInfo)> {
    let link = link?;
    // (They may be starting: installing their driver, looking at the hardware first; not waited for for ever.)
    match crate::link::receive_within::<Told>(&link.told, STARTED_WITHIN) {
        Some(Told::Hello { info, .. }) => Some((link, info)),
        _ => None,
    }
}

/// From now on what is asked goes over `link`; nothing asked of the sensors before is taken to be known to them.
fn linked_to(link: &Link) {
    if let Ok(asking) = link.asked.try_clone() {
        *ASKING.lock().unwrap() = Some(asking);
    }
    *WANTED.lock().unwrap() = None;
    *EDGES.lock().unwrap() = None;
    *LINKED_SINCE.lock().unwrap() = Some(Instant::now());
    LINKS.fetch_add(1, Ordering::Relaxed);
    LINKED.store(true, Ordering::Relaxed);
}

/// The link there is now, by its count (see `LINKS`); none, there is none.
pub fn link() -> Option<u64> {
    linked().then(|| LINKS.load(Ordering::Relaxed))
}

/// Asks the sensors' process to hear what touches the PC, or no longer.
pub fn hear_activity(on: bool) {
    ask(&Asked::Activity(on));
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
    /// own sampler reads at its time). None when there was none this time (a sample that could not be taken). The
    /// link lost, the sensors are started again through their task (if it had lasted: see `LASTED_TO_START_AGAIN`);
    /// failing that, Glance reads what it can itself from then on, and watches the edges itself. The sensors'
    /// process watches `edges` meanwhile, and tells each push it sees.
    pub fn next(&mut self, want: Want, edges: Edges) -> Option<Sample> {
        match self {
            Source::Linked(link) => {
                if *WANTED.lock().unwrap() != Some(want) {
                    *WANTED.lock().unwrap() = Some(want);
                    ask(&Asked::Want(want));
                }
                let mut told = EDGES.lock().unwrap();
                if told.as_ref() != Some(&edges) {
                    ask(&Asked::Edges(edges.clone()));
                    *told = Some(edges);
                }
                drop(told);
                loop {
                    match link.receive() {
                        Some(Told::Sample(sample)) => return Some(*sample),
                        Some(Told::Presenting { games, mode }) => crate::presents::mirror(games, mode.into()),
                        Some(Told::Report(lines)) => *REPORTED.lock().unwrap() = Some(lines),
                        Some(Told::Pushed { edge, at }) => crate::app().controller.pushed(edge, at),
                        Some(Told::Switched { switch, on }) => {
                            let mut switches = SWITCHES.lock().unwrap();
                            let kept = &mut switches[slot(switch)];
                            *kept = (Some(on), kept.1 + 1);
                        }
                        Some(Told::Updating(state)) => crate::update::set_state(state),
                        Some(Told::Note(line)) => crate::journal::note(line),
                        Some(Told::Buttons { down, pressed }) => crate::panel::heard_buttons(down, pressed),
                        Some(Told::Activity { touched, nothing }) => crate::touches::heard(touched, nothing),
                        Some(Told::Hello { .. }) => {}
                        None => {
                            LINKED.store(false, Ordering::Relaxed);
                            *ASKING.lock().unwrap() = None;
                            let lasted = LINKED_SINCE.lock().unwrap().take().is_some_and(|since| since.elapsed() >= LASTED_TO_START_AGAIN);
                            if lasted && crate::elevation::start_sensors_unasked() {
                                if let Some((again, _)) = greeted(Link::connect(STARTED_AGAIN_WITHIN)) {
                                    crate::journal::note("readings: the sensors' process went, and was started again");
                                    linked_to(&again);
                                    *link = again;
                                    return None;
                                }
                            }
                            crate::journal::note("readings: the sensors' process went; Glance reads what it can itself");
                            // What it said last is not so any more: the games it saw, an update it was taking.
                            crate::presents::forget_mirrored();
                            crate::update::link_lost();
                            crate::app().controller.hear_own();
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
