//! What plays on this PC and how loud the sound is, for the module that
//! shows them and lets them be changed: the track paused, played and
//! skipped, the sound turned up and down and muted. Read and done as the
//! phone remote reads and does them (the same code: see the remote's
//! `media` and `actions`), by Glance itself, whether the remote is on or
//! not; looked at every second while the module is on, and at once after
//! anything is asked.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

/// How it is now.
#[derive(Clone, Default, PartialEq, Debug)]
pub struct Now {
    /// What plays, by whom and in which program: each empty where it is not said (all, with nothing playing).
    pub title: String,
    pub artist: String,
    pub app: String,
    pub playing: bool,
    /// How loud the sound is (0 to 100), where it could be read, and whether it is muted.
    pub volume: Option<u8>,
    pub muted: bool,
}

/// One of the things pressed.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Key {
    Previous,
    Toggle,
    Next,
    Mute,
}

/// What is asked for.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Act {
    Key(Key),
    /// The sound as loud as this (0 to 100).
    Volume(u8),
}

static NOW: Mutex<Option<Now>> = Mutex::new(None);
/// Counted up each time what `NOW` holds changes: what is drawn of it is drawn again then.
static STAMP: AtomicU64 = AtomicU64::new(0);

/// How it is, as last looked at; none while the module is off (or before the first look).
pub fn now() -> Option<Now> {
    NOW.lock().unwrap().clone()
}

/// A number that changes whenever `now` does.
pub fn stamp() -> u64 {
    STAMP.load(Ordering::Relaxed)
}

fn keep(now: Option<Now>) {
    let mut kept = NOW.lock().unwrap();
    if *kept != now {
        *kept = now;
        STAMP.fetch_add(1, Ordering::Relaxed);
    }
}

/// Just asked to pause or to play: whether it is taken to be playing till
/// when. (A program says that it has paused a little after it was asked
/// to, and some say the other thing once in between: what was asked for
/// shows meanwhile, and what the program says after.)
#[cfg(windows)]
static ASKED: Mutex<Option<(std::time::Instant, bool)>> = Mutex::new(None);
/// How long what was asked for stands for what is said.
#[cfg(windows)]
const SETTLES: std::time::Duration = std::time::Duration::from_millis(1500);

#[cfg(windows)]
fn look() {
    let playing = crate::media::playing();
    let sound = crate::actions::volume();
    let (title, artist, app, is_playing) = playing.map_or_else(Default::default, |playing| (playing.title, playing.artist, playing.app, playing.playing));
    let is_playing = match *ASKED.lock().unwrap() {
        Some((till, asked)) if std::time::Instant::now() < till => asked,
        _ => is_playing,
    };
    keep(Some(Now { title, artist, app, playing: is_playing, volume: sound.map(|(level, _)| level), muted: sound.is_some_and(|(_, muted)| muted) }));
}

/// Looks every second while `wanted` says the module is on; forgets what it saw while it is off.
#[cfg(windows)]
pub fn watch(wanted: impl Fn() -> bool + Send + 'static) {
    std::thread::spawn(move || loop {
        if wanted() {
            look();
        } else {
            keep(None);
        }
        std::thread::sleep(std::time::Duration::from_secs(1));
    });
}

/// Does `act`, on a thread of its own, and looks at how it is then: at
/// once, and again a moment later (a program says that it has paused a
/// little after it was asked to).
#[cfg(windows)]
pub fn act(act: Act) {
    std::thread::spawn(move || {
        match act {
            Act::Key(Key::Previous) => {
                let _ = crate::media::ask("previous");
            }
            Act::Key(Key::Toggle) => {
                // Shown as asked for at once.
                if let Some(now) = now() {
                    *ASKED.lock().unwrap() = Some((std::time::Instant::now() + SETTLES, !now.playing));
                    keep(Some(Now { playing: !now.playing, ..now }));
                }
                let _ = crate::media::ask("toggle");
            }
            Act::Key(Key::Next) => {
                let _ = crate::media::ask("next");
            }
            Act::Key(Key::Mute) => {
                let muted = crate::actions::volume().is_some_and(|(_, muted)| muted);
                let _ = crate::actions::set_volume(None, Some(!muted));
            }
            Act::Volume(level) => drop(crate::actions::set_volume(Some(level), None)),
        }
        // (The last, once what was asked for no longer stands for what is said.)
        for wait in [0, 150, 500, 900] {
            std::thread::sleep(std::time::Duration::from_millis(wait));
            look();
        }
    });
}

/// The sound as loud as `level` while its track is dragged along: shown so
/// at once, and set (it is looked at again when the track is let go: see
/// `act`).
#[cfg(windows)]
pub fn slide(level: u8) {
    let shown = now().map(|now| Now { volume: Some(level), ..now });
    if shown.is_none() || shown == now() {
        return;
    }
    keep(shown);
    std::thread::spawn(move || drop(crate::actions::set_volume(Some(level), None)));
}

#[cfg(not(windows))]
pub fn act(_: Act) {}

/// For the studio's pictures: as if this played.
#[cfg(feature = "studio")]
pub fn pretend(now: Now) {
    keep(Some(now));
}
