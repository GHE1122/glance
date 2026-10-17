//! What plays on this PC and how loud the sound is, for the module that
//! shows them and lets them be changed: the track paused, played and
//! skipped, the sound turned up and down and muted. Read and done as the
//! phone remote reads and does them (the same code: see the remote's
//! `media` and `actions`), by Glance itself, whether the remote is on or
//! not; looked at every second while the module is on, and at once after
//! anything is asked. With them, while lyrics are shown, the line sung now
//! (looked at again as the next is due).
//!
//! Two are followed: whatever plays (the time and media show it, and let it be paused, whatever it is), and music
//! alone (the music component shows it, with its lyrics: not a video playing meanwhile). See `media::Follow`.

use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::Mutex;

#[cfg(windows)]
pub use crate::media::Follow;
/// What is followed (see the media module's).
#[cfg(not(windows))]
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug)]
pub enum Follow {
    Any,
    Music,
}

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
    /// How far into the track it is and how long it is, where its player says.
    pub timeline: Option<Timeline>,
    /// How many players tell Windows what they play (one of them followed: see `Key::Player`).
    pub players: u8,
    /// What is sung, while lyrics are shown and a track plays.
    pub sung: Option<Sung>,
}

/// How far into its track what plays is (whole seconds, as last looked at, or as it is being dragged to), how
/// long the track is (seconds), and whether its player lets it be moved along it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Timeline {
    pub at: f64,
    pub length: f64,
    pub seekable: bool,
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
    NOW[Follow::Music as usize].lock().unwrap().as_ref().and_then(|now| now.sung.as_ref()).is_some_and(|sung| matches!(sung, Sung::Lines { since, .. } if since.elapsed() < GLIDE))
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
    /// The next of the players followed (where several tell Windows what they play).
    Player,
}

/// What is asked for.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum Act {
    Key(Key),
    /// The sound as loud as this (0 to 100).
    Volume(u8),
    /// The track played from this far into it (seconds).
    Seek(f64),
}

/// How it is, for each followed (see `Follow`).
static NOW: [Mutex<Option<Now>>; 2] = [Mutex::new(None), Mutex::new(None)];
/// Counted up each time what `NOW` holds changes: what is drawn of it is drawn again then.
static STAMP: AtomicU64 = AtomicU64::new(0);

/// Music shown by the settings (their music page, and the layout's): it is looked at meanwhile, shown anywhere else
/// or not.
static SHOWN_IN_SETTINGS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// Whether the settings show music now (see `SHOWN_IN_SETTINGS`), and the settings saying so.
pub fn shown_in_settings() -> bool {
    SHOWN_IN_SETTINGS.load(Ordering::Relaxed)
}

pub fn show_in_settings(shown: bool) {
    SHOWN_IN_SETTINGS.store(shown, Ordering::Relaxed);
}

/// The players telling Windows they play music, as last looked at (each as Windows identifies it, and its name).
static MUSIC_PLAYERS: Mutex<Vec<(String, String)>> = Mutex::new(Vec::new());

/// The players telling Windows they play music, as last looked at while music is shown.
pub fn music_players() -> Vec<(String, String)> {
    MUSIC_PLAYERS.lock().unwrap().clone()
}

/// How whatever plays is, as last looked at; none while nothing shows it (or before the first look).
pub fn now() -> Option<Now> {
    NOW[Follow::Any as usize].lock().unwrap().clone()
}

/// How the music is, as last looked at (with its lyrics, while they are shown); none while nothing shows it.
pub fn music() -> Option<Now> {
    NOW[Follow::Music as usize].lock().unwrap().clone()
}

/// A number that changes whenever `now` does.
pub fn stamp() -> u64 {
    STAMP.load(Ordering::Relaxed)
}

fn keep(follow: Follow, now: Option<Now>) {
    let mut kept = NOW[follow as usize].lock().unwrap();
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
static ASKED: [Mutex<Option<(std::time::Instant, bool)>>; 2] = [Mutex::new(None), Mutex::new(None)];
/// How long what was asked for stands for what is said.
#[cfg(windows)]
const SETTLES: std::time::Duration = std::time::Duration::from_millis(1500);

/// Where along its track what plays is being dragged to, while it is (shown so, whatever its player says); then,
/// asked to go there, till when what was asked for stands for what is said.
#[cfg(windows)]
static SEEKING: [Mutex<Option<(f64, Option<std::time::Instant>)>>; 2] = [Mutex::new(None), Mutex::new(None)];

/// Whether lyrics are shown (and so asked for), as `watch` was last told.
#[cfg(windows)]
static LYRICS: std::sync::atomic::AtomicBool = std::sync::atomic::AtomicBool::new(false);

/// How far into the track it was as last looked at (seconds), when that was, and whether it was playing.
#[cfg(windows)]
static POSITION: [Mutex<Option<(f64, std::time::Instant, bool, f64)>>; 2] = [Mutex::new(None), Mutex::new(None)];

/// Played from elsewhere along its track: where (seconds), when, and the track (its title).
#[cfg(windows)]
static SOUGHT: [Mutex<Option<(f64, std::time::Instant, String)>>; 2] = [Mutex::new(None), Mutex::new(None)];

/// Paused: where it was held (seconds), and when its player had last said how far in it was as that was taken (see
/// `look`).
#[cfg(windows)]
static HELD: [Mutex<Option<(f64, std::time::Instant)>>; 2] = [Mutex::new(None), Mutex::new(None)];

/// How far into the track of what is followed for `follow` it is now (seconds), from the last look on by the time
/// since while it plays; none where its player does not say.
#[cfg(windows)]
pub fn position(follow: Follow) -> Option<f64> {
    POSITION[follow as usize].lock().unwrap().map(|(at, when, playing, rate)| if playing { at + when.elapsed().as_secs_f64() * rate } else { at })
}

/// Looks at how what is followed for `follow` is; how long till the next line of the lyrics is due, where one is
/// (music's, while lyrics are shown).
#[cfg(windows)]
fn look(follow: Follow) -> Option<std::time::Duration> {
    let f = follow as usize;
    let mut playing = crate::media::playing_of(follow);
    if follow == Follow::Music {
        *MUSIC_PLAYERS.lock().unwrap() = crate::media::music_players();
    }
    // Dragged along: as far as it is dragged. Just asked to go somewhere along it: see `SOUGHT`, below.
    let (dragged, settling) = match *SEEKING[f].lock().unwrap() {
        Some((at, None)) => (Some(at), false),
        Some((_, Some(till))) => (None, std::time::Instant::now() < till),
        None => (None, false),
    };
    let seeking = dragged.is_some() || settling;
    if let (Some(at), Some((position, _))) = (dragged, playing.as_mut().and_then(|playing| playing.timeline.as_mut())) {
        *position = at;
    }
    // Paused, where its player last said how far in it was while it still played (as NetEase's does: it says
    // nothing more as it pauses), it went on from there till it paused: where it was as the pause was first seen,
    // held till the player says again. (Taken as said, the line sung went back as much as a second.)
    let now = std::time::Instant::now();
    // Played from elsewhere along its track: where it was asked to go, on from then while it plays (not held there:
    // held, and reckoned on from each look, the line sung went back and forth till it was let go), whatever its player
    // says while it settles there; after, till its player says again, or another track plays (what it said before
    // stands for where it was then, not where it is: Media Player says nothing more after it is moved along, and was
    // taken back where it had been).
    if let (Some(playing), None) = (playing.as_mut(), dragged) {
        let mut sought = SOUGHT[f].lock().unwrap();
        if let (Some((at, when, title)), Some((position, _))) = (sought.as_ref(), playing.timeline.as_mut()) {
            let said_at = playing.said_ago.map(|ago| now - std::time::Duration::from_secs_f64(ago));
            if !settling && (*title != playing.title || said_at.is_some_and(|said| said > *when)) {
                *sought = None;
            } else {
                *position = at + if playing.playing { now.duration_since(*when).as_secs_f64() * playing.rate } else { 0.0 };
            }
        }
    }
    if let (Some(playing), seeking) = (playing.as_mut(), seeking) {
        let said_at = playing.said_ago.map(|ago| now - std::time::Duration::from_secs_f64(ago));
        let mut held = HELD[f].lock().unwrap();
        match (playing.playing, playing.timeline.as_mut()) {
            (false, Some((position, _))) if !seeking => {
                let was = *POSITION[f].lock().unwrap();
                match *held {
                    // The same saying as when the pause was seen: where it was held.
                    Some((at, by)) if said_at.is_some_and(|said| said <= by + std::time::Duration::from_millis(50)) => *position = at,
                    // Paused just now: on from what was said, to now.
                    _ if was.is_some_and(|(_, _, playing, _)| playing) => {
                        let rate = was.map_or(1.0, |(_, _, _, rate)| rate);
                        let at = *position + playing.said_ago.unwrap_or(0.0) * rate;
                        *position = at;
                        *held = said_at.map(|said| (at, said));
                    }
                    // Said again since (or paused before it was first looked at): as said.
                    _ => *held = None,
                }
            }
            _ => *held = None,
        }
    }
    *POSITION[f].lock().unwrap() = playing.as_ref().and_then(|playing| playing.timeline.map(|(at, _)| (at, now, playing.playing, playing.rate)));
    let timeline = playing.as_ref().and_then(|playing| playing.timeline.map(|(at, length)| Timeline { at: at.floor(), length, seekable: playing.seekable }));
    let sound = crate::actions::volume();
    let (sung, next) = match playing.as_ref().filter(|playing| follow == Follow::Music && LYRICS.load(Ordering::Relaxed) && !playing.title.is_empty()) {
        Some(playing) => sung(playing),
        None => (None, None),
    };
    let (title, artist, app, is_playing) = playing.map_or_else(Default::default, |playing| (playing.title, playing.artist, playing.app, playing.playing));
    let is_playing = match *ASKED[f].lock().unwrap() {
        Some((till, asked)) if std::time::Instant::now() < till => asked,
        _ => is_playing,
    };
    let players = crate::media::players().len().min(u8::MAX as usize) as u8;
    keep(follow, Some(Now { title, artist, app, playing: is_playing, volume: sound.map(|(level, _)| level), muted: sound.is_some_and(|(_, muted)| muted), timeline, players, sung }));
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
            // (Track seconds, at the rate it plays.)
            let due = lines.get(next).filter(|_| playing.playing).map(|line| std::time::Duration::from_secs_f64(((line.at - crate::lyrics::EARLY - position) / playing.rate).max(0.0)));
            // The same line as before keeps its time; another is the one from when it became due (from now, if
            // that was longer ago than a glide: come to by a jump in the track, it glides all the same).
            let before = NOW[Follow::Music as usize].lock().unwrap().as_ref().and_then(|kept| match &kept.sung {
                Some(Sung::Lines { lines: had, now: was, since }) if std::sync::Arc::ptr_eq(had, &lines) && *was == now => Some(*since),
                _ => None,
            });
            let since = before.unwrap_or_else(|| {
                let late = now.map_or(0.0, |now| ((position + crate::lyrics::EARLY - lines[now].at) / playing.rate).clamp(0.0, GLIDE.as_secs_f64()));
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

/// Looks every second at what `wanted` says is shown (whatever plays, and music with its lyrics), and as each line of
/// the lyrics is due; forgets what it saw of one while it is not shown.
#[cfg(windows)]
pub fn watch(wanted: impl Fn() -> (bool, bool) + Send + 'static, changed: impl Fn() + Send + Sync + 'static) {
    let _ = TOLD.set(Box::new(changed));
    std::thread::Builder::new().name("glance: what plays".into()).spawn(move || loop {
        let (any, music) = wanted();
        LYRICS.store(music, Ordering::Relaxed);
        if any {
            look(Follow::Any);
        } else {
            keep(Follow::Any, None);
        }
        let next = if music { look(Follow::Music) } else { keep(Follow::Music, None); None };
        // (Not sooner than a twentieth of a second: a line due already is looked at as the next comes.)
        let wait = next.map_or(std::time::Duration::from_secs(1), |next| next.clamp(std::time::Duration::from_millis(50), std::time::Duration::from_secs(1)));
        std::thread::sleep(wait);
    }).expect("a thread started");
}

/// What is followed for `follow`, shown at once as paused if it played and as playing if it was paused, till its
/// program says how it is (see `ASKED`).
#[cfg(windows)]
fn shown_toggled(follow: Follow) {
    let f = follow as usize;
    // (Taken out before it is kept again: the lock held on would be locked again by `keep`.)
    let now = NOW[f].lock().unwrap().clone();
    if let Some(now) = now {
        *ASKED[f].lock().unwrap() = Some((std::time::Instant::now() + SETTLES, !now.playing));
        keep(follow, Some(Now { playing: !now.playing, ..now }));
    }
}

/// Does `act` on whatever plays (see `act_on`).
#[cfg(windows)]
pub fn act(act: Act) {
    act_on(Follow::Any, act);
}

/// Does `act` on what is followed for `follow`, on a thread of its own, and looks at how it is then: at once, and
/// again a moment later (a program says that it has paused a little after it was asked to).
#[cfg(windows)]
pub fn act_on(follow: Follow, act: Act) {
    let f = follow as usize;
    std::thread::spawn(move || {
        match act {
            Act::Key(Key::Previous) => {
                let _ = crate::media::ask_of(follow, "previous");
            }
            Act::Key(Key::Toggle) => {
                shown_toggled(follow);
                let _ = crate::media::ask_of(follow, "toggle");
            }
            Act::Key(Key::Next) => {
                let _ = crate::media::ask_of(follow, "next");
            }
            Act::Key(Key::Player) => {
                crate::media::follow_next();
            }
            Act::Key(Key::Mute) => {
                let muted = crate::actions::volume().is_some_and(|(_, muted)| muted);
                sound_to(None, Some(!muted));
            }
            Act::Volume(level) => sound_to(Some(level), None),
            Act::Seek(at) => {
                // Shown there at once, and till the player says it is.
                *SEEKING[f].lock().unwrap() = Some((at, Some(std::time::Instant::now() + SETTLES)));
                *SOUGHT[f].lock().unwrap() = NOW[f].lock().unwrap().clone().map(|now| (at, std::time::Instant::now(), now.title));
                if !crate::media::seek_of(follow, at) {
                    *SEEKING[f].lock().unwrap() = None;
                    *SOUGHT[f].lock().unwrap() = None;
                }
            }
        }
        // (The last, once what was asked for no longer stands for what is said.)
        for wait in [0, 150, 500, 900] {
            std::thread::sleep(std::time::Duration::from_millis(wait));
            let _ = look(follow);
        }
    });
}

/// The sound as loud as `level` while its track is dragged along: shown so
/// at once, and set (it is looked at again when the track is let go: see
/// `act`).
#[cfg(windows)]
pub fn slide(level: u8) {
    set_sound(Some(level), None);
}

/// The sound made as loud as `level` and muted or not as `muted` (each, where it is said): shown so at once in what
/// is followed (the sound is the whole system's, the same for both), and set on a thread of its own (it waits on the
/// sound's service, which may take long; this is done on the thread that draws). Nothing done where nothing would
/// change.
#[cfg(windows)]
pub fn set_sound(level: Option<u8>, muted: Option<bool>) {
    if shown_sound(level, muted) {
        sound_to(level, muted);
    }
}

/// The sound set to `level` and `muted` (each, where it is said), on the one thread that sets it, in the order asked;
/// what was asked meanwhile taken together, the latest of each standing. (Set each on a thread of its own, as a
/// slider was dragged, an earlier value could come after a later one and stand: the sound left louder than it was
/// turned down to.)
#[cfg(windows)]
fn sound_to(level: Option<u8>, muted: Option<bool>) {
    static SETTING: Mutex<Option<std::sync::mpsc::Sender<(Option<u8>, Option<bool>)>>> = Mutex::new(None);
    let mut setting = SETTING.lock().unwrap();
    let sender = setting.get_or_insert_with(|| {
        let (asked, asking) = std::sync::mpsc::channel::<(Option<u8>, Option<bool>)>();
        std::thread::Builder::new()
            .name("glance: the sound set".into())
            .spawn(move || {
                while let Ok(mut wanted) = asking.recv() {
                    while let Ok(later) = asking.try_recv() {
                        wanted = (later.0.or(wanted.0), later.1.or(wanted.1));
                    }
                    let _ = crate::actions::set_volume(wanted.0, wanted.1);
                }
            })
            .expect("a thread started");
        asked
    });
    let _ = sender.send((level, muted));
}

/// The sound shown as `set_sound` sets it, at once; whether it is to be set (something shown changed, or nothing is
/// shown to tell).
#[cfg(windows)]
fn shown_sound(level: Option<u8>, muted: Option<bool>) -> bool {
    let mut changed = false;
    let mut seen = false;
    for follow in [Follow::Any, Follow::Music] {
        // (Taken out before it is kept again: the lock held on would be locked again by `keep`.)
        let now = NOW[follow as usize].lock().unwrap().clone();
        if let Some(now) = now {
            seen = true;
            let shown = Now { volume: level.or(now.volume), muted: muted.unwrap_or(now.muted), ..now.clone() };
            if shown != now {
                changed = true;
                keep(follow, Some(shown));
            }
        }
    }
    changed || !seen
}

/// How loud the sound is (0 to 100) and whether it is muted, as last seen; none where it could not be read.
#[cfg(windows)]
pub fn sound() -> Option<(u8, bool)> {
    [Follow::Music, Follow::Any].into_iter().find_map(|follow| NOW[follow as usize].lock().unwrap().as_ref().and_then(|now| now.volume.map(|level| (level, now.muted))))
}

/// A track dragged along: the sound's, or that of what is followed for its `Follow` (see `Follow`).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Slider {
    Volume,
    Seek(Follow),
}

/// `slider` dragged to `fraction` of the way along it: the sound as loud, set at once; what plays shown as that far
/// into its track, and played from there once let go (see `slid`).
#[cfg(windows)]
pub fn slide_along(slider: Slider, fraction: f32) {
    let fraction = fraction.clamp(0.0, 1.0);
    match slider {
        Slider::Volume => slide((fraction * 100.0).round() as u8),
        Slider::Seek(follow) => {
            let Some(length) = NOW[follow as usize].lock().unwrap().as_ref().and_then(|now| now.timeline).map(|timeline| timeline.length) else { return };
            let at = fraction as f64 * length;
            *SEEKING[follow as usize].lock().unwrap() = Some((at, None));
            shown_dragged(follow, at);
        }
    }
}

/// What is followed for `follow` shown `at` that far into its track (seconds) as it is dragged there, from what was
/// last seen of it (as `look` shows it while it is dragged), its lines following. Its player is not asked: this is
/// done at each move of the pointer, on the thread that draws, and a player may take long to answer.
#[cfg(windows)]
fn shown_dragged(follow: Follow, at: f64) {
    let f = follow as usize;
    // (Taken out before it is kept again: the lock held on would be locked again by `keep`.)
    let now = NOW[f].lock().unwrap().clone();
    let Some(now) = now else { return };
    let Some(timeline) = now.timeline else { return };
    let mut position = POSITION[f].lock().unwrap();
    let rate = position.map_or(1.0, |(_, _, _, rate)| rate);
    *position = Some((at, std::time::Instant::now(), now.playing, rate));
    drop(position);
    keep(follow, Some(Now { timeline: Some(Timeline { at: at.floor(), ..timeline }), ..now }));
}

/// Where the track of what is followed for `follow` is being dragged to (seconds), while it is (see `slide_along`).
#[cfg(windows)]
pub fn dragged_to(follow: Follow) -> Option<f64> {
    SEEKING[follow as usize].lock().unwrap().and_then(|(at, settling)| settling.is_none().then_some(at))
}

#[cfg(not(windows))]
pub fn dragged_to(_: Follow) -> Option<f64> {
    None
}

/// `slider` let go (`kept`: where it was left; else, the drag given up, as it was before).
#[cfg(windows)]
pub fn slid(slider: Slider, kept: bool) {
    match slider {
        Slider::Volume => {
            if let Some(level) = now().and_then(|now| now.volume).filter(|_| kept) {
                act(Act::Volume(level));
            }
        }
        Slider::Seek(follow) => {
            let left_at = SEEKING[follow as usize].lock().unwrap().and_then(|(at, settling)| settling.is_none().then_some(at));
            match left_at.filter(|_| kept) {
                Some(at) => act_on(follow, Act::Seek(at)),
                // (Looked at again on a thread of its own: see `shown_dragged`.)
                None => {
                    *SEEKING[follow as usize].lock().unwrap() = None;
                    std::thread::spawn(move || {
                        let _ = look(follow);
                    });
                }
            }
        }
    }
}

#[cfg(not(windows))]
pub fn act(_: Act) {}

#[cfg(not(windows))]
pub fn act_on(_: Follow, _: Act) {}

/// For the studio's pictures: as if this played (and were music).
#[cfg(feature = "studio")]
pub fn pretend(now: Now) {
    keep(Follow::Music, Some(now.clone()));
    keep(Follow::Any, Some(now));
}

#[cfg(all(test, windows))]
mod tests {
    use super::*;

    /// What is followed is kept in statics shared by these tests: one at a time.
    static ONE_AT_A_TIME: std::sync::Mutex<()> = std::sync::Mutex::new(());

    /// The sound turned shows at once as turned, in what is followed for either, without waiting on the sound's
    /// service (it was read and set at every frame and every move on the thread that draws the full screen); turned
    /// to where it is, it is not set again. (Only what is shown: this PC's sound is not touched.)
    #[test]
    fn the_sound_turned_shows_at_once() {
        let _one = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        keep(Follow::Any, Some(Now { title: "Sound".into(), volume: Some(30), ..Default::default() }));
        keep(Follow::Music, Some(Now { title: "Sound".into(), volume: Some(30), ..Default::default() }));
        assert!(shown_sound(Some(45), Some(false)));
        assert_eq!(sound(), Some((45, false)));
        assert_eq!(now().and_then(|now| now.volume), Some(45));
        assert!(!shown_sound(Some(45), None));
        assert!(shown_sound(None, Some(true)));
        assert_eq!(sound(), Some((45, true)));
    }

    /// Dragged along its track, what plays is shown where it is dragged, from what was seen of it, its player not
    /// asked (that was asked at each move of the pointer, on the thread that draws: a player slow to answer held up
    /// the panel, the widgets and the full screen; and what was seen went for what the players said instead).
    #[test]
    fn dragging_along_shows_where_it_is_dragged_without_asking() {
        let _one = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        let length = 200.0;
        keep(Follow::Any, Some(Now { title: "A song".into(), playing: true, timeline: Some(Timeline { at: 10.0, length, seekable: true }), ..Default::default() }));
        slide_along(Slider::Seek(Follow::Any), 0.25);
        let shown = now().expect("still what was seen");
        assert_eq!(shown.title, "A song");
        assert_eq!(shown.timeline.map(|timeline| timeline.at), Some(50.0));
        assert!(position(Follow::Any).is_some_and(|at| (50.0..51.0).contains(&at)));
        *SEEKING[Follow::Any as usize].lock().unwrap() = None;
    }

    /// Pausing shows it paused at once, and returns (it once locked what plays twice, and Glance hung).
    #[test]
    #[cfg(windows)]
    fn pausing_shows_it_paused() {
        let _one = ONE_AT_A_TIME.lock().unwrap_or_else(std::sync::PoisonError::into_inner);
        keep(Follow::Music, Some(Now { title: "A song".into(), playing: true, ..Default::default() }));
        let (done, told) = std::sync::mpsc::channel();
        std::thread::spawn(move || {
            shown_toggled(Follow::Music);
            let _ = done.send(());
        });
        assert!(told.recv_timeout(std::time::Duration::from_secs(2)).is_ok(), "pausing did not return");
        assert_eq!(NOW[Follow::Music as usize].lock().unwrap().as_ref().map(|now| now.playing), Some(false));
    }

    /// On a desktop, something playing: where its player says it is (reckoned on to now from when it said so), ten
    /// times a second for ten seconds, and each step back.
    #[test]
    #[ignore = "needs something playing on this PC"]
    fn follows_what_plays() {
        let start = std::time::Instant::now();
        let mut last: Option<f64> = None;
        while start.elapsed() < std::time::Duration::from_secs(10) {
            if let Some(playing) = crate::media::playing() {
                if let Some((at, _)) = playing.timeline {
                    let back = last.map_or(0.0, |last| last - at);
                    println!("{:6.2} s  {} playing={}  at {at:8.3}  said {:5.2} s ago{}", start.elapsed().as_secs_f64(), playing.app, playing.playing, playing.said_ago.unwrap_or(-1.0), if back > 0.0 { format!("  BACK {back:.3}") } else { String::new() });
                    last = Some(at);
                }
            }
            std::thread::sleep(std::time::Duration::from_millis(100));
        }
    }

    /// On a desktop: how long one look takes, and what it sees.
    #[test]
    #[ignore = "needs something playing on this PC"]
    fn times_a_look() {
        super::look(super::Follow::Any);
        let from = std::time::Instant::now();
        for _ in 0..20 {
            super::look(super::Follow::Any);
        }
        println!("a look takes {:.2} ms; it sees {:?}", from.elapsed().as_secs_f64() * 1000.0 / 20.0, super::now());
    }
}

#[cfg(all(test, windows))]
mod seek_tests {
    /// Moves what plays to a minute in and back to where it was: `cargo test --lib -- --ignored seeks_and_back --nocapture`.
    #[test]
    #[ignore = "moves this PC's player"]
    fn seeks_and_back() {
        let was = crate::media::playing().and_then(|playing| playing.timeline).expect("something playing, with a timeline").0;
        println!("was at {was:.1}; asked 60: {}", crate::media::seek(60.0));
        std::thread::sleep(std::time::Duration::from_millis(800));
        println!("now at {:?}", crate::media::playing().and_then(|playing| playing.timeline).map(|t| t.0));
        println!("asked back: {}", crate::media::seek(was));
        std::thread::sleep(std::time::Duration::from_millis(800));
        println!("back at {:?}", crate::media::playing().and_then(|playing| playing.timeline).map(|t| t.0));
    }
}
