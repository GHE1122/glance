//! What plays on this PC and how loud the sound is, for the module that
//! shows them and lets them be changed: the track paused, played and
//! skipped, the sound turned up and down and muted. Read and done as the
//! phone remote reads and does them (the same code: see the remote's
//! `media` and `actions`), by Glance itself, whether the remote is on or
//! not; looked at every second while the module is on, and at once after
//! anything is asked. With them, while lyrics are shown, the line sung now
//! (looked at again as the next is due).

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
    /// What is sung, while lyrics are shown and a track plays.
    pub sung: Option<Sung>,
}

/// What is sung of the track that plays.
#[derive(Clone, PartialEq, Debug)]
pub enum Sung {
    /// Its player does not say how far into it it is, so no line can be told from another; `netease`, NetEase
    /// Cloud Music (which says, with the InfLink-rs plugin).
    Untimed { netease: bool },
    /// Its lyrics are being asked for.
    Asking,
    /// None were found.
    Unknown,
    /// Its lyrics, the line sung now (none before the first), and since when it is the one (what is drawn of
    /// them glides to it for `GLIDE` after).
    Lines { lines: Lyrics, now: Option<usize>, since: std::time::Instant },
}

/// How long the lyrics take to glide from one line to the next.
pub const GLIDE: std::time::Duration = std::time::Duration::from_millis(420);

/// Whether the lyrics are gliding to the line sung now: what shows them is drawn every frame meanwhile.
pub fn gliding() -> bool {
    NOW.lock().unwrap().as_ref().and_then(|now| now.sung.as_ref()).is_some_and(|sung| matches!(sung, Sung::Lines { since, .. } if since.elapsed() < GLIDE))
}

/// What is told that the line sung has changed (the windows that show it are drawn at once then).
#[cfg(windows)]
static TOLD: std::sync::OnceLock<Box<dyn Fn() + Send + Sync>> = std::sync::OnceLock::new();

#[cfg(windows)]
use crate::lyrics::Lyrics;
#[cfg(not(windows))]
pub type Lyrics = std::sync::Arc<Vec<()>>;

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

/// Whether lyrics are shown (and so asked for), as `watch` was last told.
#[cfg(windows)]
static LYRICS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// How far into the track it was as last looked at (seconds), when that was, and whether it was playing.
#[cfg(windows)]
static POSITION: Mutex<Option<(f64, std::time::Instant, bool)>> = Mutex::new(None);

/// How far into the track it is now (seconds), from the last look on by the time since while it plays; none where
/// its player does not say.
#[cfg(windows)]
pub fn position() -> Option<f64> {
    POSITION.lock().unwrap().map(|(at, when, playing)| if playing { at + when.elapsed().as_secs_f64() } else { at })
}

/// Looks at how it is; how long till the next line of the lyrics is due, where one is.
#[cfg(windows)]
fn look() -> Option<std::time::Duration> {
    let playing = crate::media::playing();
    *POSITION.lock().unwrap() = playing.as_ref().and_then(|playing| playing.timeline.map(|(at, _)| (at, std::time::Instant::now(), playing.playing)));
    let sound = crate::actions::volume();
    let (sung, next) = match playing.as_ref().filter(|playing| LYRICS.load(Ordering::Relaxed) && !playing.title.is_empty()) {
        Some(playing) => sung(playing),
        None => (None, None),
    };
    let (title, artist, app, is_playing) = playing.map_or_else(Default::default, |playing| (playing.title, playing.artist, playing.app, playing.playing));
    let is_playing = match *ASKED.lock().unwrap() {
        Some((till, asked)) if std::time::Instant::now() < till => asked,
        _ => is_playing,
    };
    keep(Some(Now { title, artist, app, playing: is_playing, volume: sound.map(|(level, _)| level), muted: sound.is_some_and(|(_, muted)| muted), sung }));
    next
}

/// What is sung of `playing`, and how long till its next line is due (while it plays).
#[cfg(windows)]
fn sung(playing: &crate::media::Playing) -> (Option<Sung>, Option<std::time::Duration>) {
    let Some((position, length)) = playing.timeline else {
        return (Some(Sung::Untimed { netease: playing.app_id.eq_ignore_ascii_case("cloudmusic.exe") }), None);
    };
    // Its number on NetEase Cloud Music, where its player says it; else its first artist (InfLink-rs and others
    // join them with " / "), as the collections name a track by.
    let netease = playing.genres.iter().find_map(|genre| genre.strip_prefix("NCM-")?.parse().ok());
    let artist = playing.artist.split(" / ").next().unwrap_or_default().to_string();
    let track = crate::lyrics::Track { netease, title: playing.title.clone(), artist, album: playing.album.clone(), seconds: length.round() as u32 };
    match crate::lyrics::of(&track) {
        crate::lyrics::Known::Asking => (Some(Sung::Asking), None),
        crate::lyrics::Known::Found(None) => (Some(Sung::Unknown), None),
        crate::lyrics::Known::Found(Some(lines)) => {
            let now = crate::lyrics::sung_at(&lines, position);
            let next = now.map_or(0, |now| now + 1);
            let due = lines.get(next).filter(|_| playing.playing).map(|line| std::time::Duration::from_secs_f64((line.at - crate::lyrics::EARLY - position).max(0.0)));
            // The same line as before keeps its time; another is the one from when it became due (from now, if
            // that was longer ago than a glide: come to by a jump in the track, it glides all the same).
            let before = NOW.lock().unwrap().as_ref().and_then(|kept| match &kept.sung {
                Some(Sung::Lines { lines: had, now: was, since }) if std::sync::Arc::ptr_eq(had, &lines) && *was == now => Some(*since),
                _ => None,
            });
            let since = before.unwrap_or_else(|| {
                let late = now.map_or(0.0, |now| (position + crate::lyrics::EARLY - lines[now].at).clamp(0.0, GLIDE.as_secs_f64()));
                std::time::Instant::now() - std::time::Duration::from_secs_f64(late)
            });
            if before.is_none() {
                if let Some(told) = TOLD.get() {
                    told();
                }
            }
            (Some(Sung::Lines { lines, now, since }), due)
        }
    }
}

/// Looks every second while `wanted` says the module is on (and whether lyrics are shown), and as each line
/// of the lyrics is due; forgets what it saw while it is off.
#[cfg(windows)]
pub fn watch(wanted: impl Fn() -> (bool, bool) + Send + 'static, changed: impl Fn() + Send + Sync + 'static) {
    let _ = TOLD.set(Box::new(changed));
    std::thread::Builder::new().name("glance: what plays".into()).spawn(move || loop {
        let (on, lyrics) = wanted();
        LYRICS.store(lyrics, Ordering::Relaxed);
        let next = if on { look() } else { keep(None); None };
        // (Not sooner than a twentieth of a second: a line due already is looked at as the next comes.)
        let wait = next.map_or(std::time::Duration::from_secs(1), |next| next.clamp(std::time::Duration::from_millis(50), std::time::Duration::from_secs(1)));
        std::thread::sleep(wait);
    }).expect("a thread started");
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
            let _ = look();
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

#[cfg(all(test, windows))]
mod tests {
    /// On a desktop: how long one look takes, and what it sees.
    #[test]
    #[ignore]
    fn times_a_look() {
        super::look();
        let from = std::time::Instant::now();
        for _ in 0..20 {
            super::look();
        }
        println!("a look takes {:.2} ms; it sees {:?}", from.elapsed().as_secs_f64() * 1000.0 / 20.0, super::now());
    }
}
