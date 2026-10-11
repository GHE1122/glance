//! A game's frames: the moments it handed each to the screen, and what they
//! tell. Frames a second over the last second; the 1% low over the last ten
//! (the slowest hundredth of the frames, as the frames a second they come to
//! together: what FrameView and CapFrameX give by that name; one stutter
//! shows in it, where the frame time at the 99th percentile would pass it
//! over); and the longest frame of the last second, the spike a stutter
//! makes.

use std::collections::VecDeque;
use std::time::Duration;

/// How long frames are kept: the span of the 1% low.
pub const KEPT: Duration = Duration::from_secs(10);
/// The span frames a second, and the longest frame, are taken over; a
/// game with no frame in it is not drawing (paused, minimized, loading).
pub const RECENT: Duration = Duration::from_secs(1);
/// Fewer frames than this over `KEPT` say nothing of the slowest 1%.
const LOW_FRAMES: usize = 20;

/// What a game's frames tell.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct FrameStats {
    pub fps: f32,
    /// None until enough frames have come.
    pub low: Option<f32>,
    /// The longest frame of the last second, in milliseconds.
    pub longest_ms: f32,
}

/// One program's frames, as the moments it presented them (any clock,
/// counted from any origin).
#[derive(Default)]
pub struct Presents {
    times: VecDeque<Duration>,
}

impl Presents {
    /// A frame presented at `at`. Frames can be told out of order (each
    /// processor buffers its own events): it goes in its place among them.
    pub fn push(&mut self, at: Duration) {
        let place = self.times.iter().rposition(|&t| t <= at).map_or(0, |before| before + 1);
        self.times.insert(place, at);
        let newest = *self.times.back().unwrap();
        while self.times.front().is_some_and(|&first| newest - first > KEPT) {
            self.times.pop_front();
        }
    }

    /// The frames as of `now`: none if it has presented no two frames in the
    /// last second. `heard` is the moment of the latest frame heard of from
    /// any program: frames are told of a while after they are presented
    /// (in batches), so only up to there is it known that this program
    /// presented no more.
    pub fn stats(&self, now: Duration, heard: Duration) -> Option<FrameStats> {
        let recent: Vec<Duration> = self.times.iter().copied().filter(|&t| now.saturating_sub(t) <= RECENT).collect();
        let (first, last) = (*recent.first()?, *recent.last()?);
        if recent.len() < 2 || last <= first {
            return None;
        }
        // The frame being waited for is as long as the wait for it already: counted with the rest once that is
        // longer than they are (a few frames in a burst and then none are not that many a second).
        let span = (last - first).as_secs_f64();
        let waited = heard.saturating_sub(last).as_secs_f64();
        let pending = (waited > span / (recent.len() - 1) as f64).then_some(waited);
        let fps = (recent.len() - 1) as f64 / (span + pending.unwrap_or(0.0));
        let longest_ms = recent.windows(2).map(|pair| (pair[1] - pair[0]).as_secs_f64() * 1000.0).fold(0.0, f64::max);
        // Every frame time kept: the slowest 1% of them, as the frames a second they come to together.
        // (With them the frame being waited for, where it counts: the frames a second and their 1% low are of the same frames.)
        let mut times: Vec<f64> = self.times.iter().collect::<Vec<_>>().windows(2).map(|pair| (*pair[1] - *pair[0]).as_secs_f64()).chain(pending).collect();
        let low = (times.len() >= LOW_FRAMES).then(|| {
            times.sort_by(f64::total_cmp);
            let slowest = &times[times.len() - (times.len() as f64 * 0.01).ceil() as usize..];
            (slowest.len() as f64 / slowest.iter().sum::<f64>()) as f32
        });
        Some(FrameStats { fps: fps as f32, low, longest_ms: longest_ms as f32 })
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    fn ms(n: f64) -> Duration {
        Duration::from_secs_f64(n / 1000.0)
    }

    #[test]
    fn takes_frames_told_out_of_order() {
        let mut presents = Presents::default();
        for t in [0.0, 20.0, 10.0, 30.0, 50.0, 40.0] {
            presents.push(ms(t));
        }
        let stats = presents.stats(ms(50.0), ms(50.0)).unwrap();
        assert!((stats.fps - 100.0).abs() < 0.01);
        assert!((stats.longest_ms - 10.0).abs() < 0.01);
    }

    #[test]
    fn counts_frames_a_second_and_the_slowest_one_percent() {
        let mut presents = Presents::default();
        // Ten seconds at 100 frames a second (10 ms each), one frame in a
        // hundred taking 50 ms.
        let mut t = 0.0;
        for i in 0..1000 {
            t += if i % 100 == 99 { 50.0 } else { 10.0 };
            presents.push(ms(t));
        }
        let stats = presents.stats(ms(t), ms(t)).unwrap();
        assert!((stats.fps - 100.0).abs() < 5.0, "{stats:?}");
        // The slowest hundredth are the 50 ms ones: 20 frames a second.
        assert!((stats.low.unwrap() - 20.0).abs() < 0.5, "{stats:?}");
        assert!((stats.longest_ms - 50.0).abs() < 0.5, "{stats:?}");
    }

    #[test]
    fn frames_not_yet_told_of_are_not_taken_for_a_wait() {
        let mut presents = Presents::default();
        // 200 frames a second, told of up to a quarter of a second ago (they come in batches).
        let mut t = 0.0;
        for _ in 0..400 {
            t += 5.0;
            presents.push(ms(t));
        }
        let stats = presents.stats(ms(t + 250.0), ms(t)).unwrap();
        assert!((stats.fps - 200.0).abs() < 0.5, "{stats:?}");
        assert!((stats.low.unwrap() - 200.0).abs() < 0.5, "{stats:?}");
        // Heard of beyond its last frame (another program's frames came since): it is waiting for one.
        let stats = presents.stats(ms(t + 250.0), ms(t + 250.0)).unwrap();
        assert!(stats.fps < 160.0, "{stats:?}");
    }

    #[test]
    fn one_stutter_shows_in_the_1_percent_low_and_never_puts_it_above_the_frames_a_second() {
        let mut presents = Presents::default();
        // Nine and a half seconds at 200 frames a second, one frame of 300 ms among them near the end.
        let mut t = 0.0;
        for i in 0..1900 {
            t += if i == 1800 { 300.0 } else { 5.0 };
            presents.push(ms(t));
        }
        let stats = presents.stats(ms(t), ms(t)).unwrap();
        // The last second holds the stutter: fewer frames a second than the game makes when it runs freely.
        assert!(stats.fps < 160.0, "{stats:?}");
        // Its slowest hundredth (19 frames, the 300 ms one among them) come to about 49 a second: below that.
        let low = stats.low.unwrap();
        assert!((low - 19.0 / 0.39).abs() < 1.0 && low < stats.fps, "{stats:?}");
    }

    #[test]
    fn tells_nothing_of_a_game_not_drawing() {
        let mut presents = Presents::default();
        presents.push(ms(0.0));
        presents.push(ms(16.0));
        // A frame and its follower, then nothing for two seconds.
        assert!(presents.stats(ms(16.0), ms(16.0)).is_some());
        assert_eq!(presents.stats(ms(2016.0), ms(2016.0)), None);
        // Too few frames for a 1% low.
        assert_eq!(presents.stats(ms(16.0), ms(16.0)).unwrap().low, None);
    }

    #[test]
    fn keeps_only_the_last_ten_seconds() {
        let mut presents = Presents::default();
        for i in 0..2000 {
            presents.push(ms(i as f64 * 10.0));
        }
        assert!(presents.times.len() <= 1001);
    }
}
