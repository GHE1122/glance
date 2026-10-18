//! A slider's thumb as Windows 11 draws its own: a round face with a faint
//! edge, and a dot of the accent in it, larger under the pointer and smaller
//! while it is held, easing between (the settings', and the panel's and the
//! widgets' in the Windows 11 look).

use std::time::Instant;

use super::canvas::{Canvas, Color, Point};

/// The face's radius, with its edge (DIPs).
pub const FACE: f32 = 10.0;
/// The dot's radius at rest, under the pointer, and held (Windows 11's: 12, 14 and 10 across).
pub const DOT: f32 = 6.0;
pub const DOT_OVER: f32 = 7.0;
pub const DOT_HELD: f32 = 5.0;
/// How fast the dot grows or shrinks (DIPs a second: a pixel's change in a twelfth of a second, as Windows').
const EASE: f32 = 12.0;

/// Draws a thumb at `centre`, its dot `dot` in radius and `accent`, on a face `face` edged `edge`.
pub fn draw(frame: &dyn Canvas, centre: Point, dot: f32, face: Color, edge: Color, accent: Color) {
    frame.fill_circle(edge, centre, FACE);
    frame.fill_circle(face, centre, FACE - 1.0);
    frame.fill_circle(accent, centre, dot);
}

/// The dots of a host's sliders, each known by `K`, easing toward their size: at rest, under the pointer, or held.
/// Only those not at rest are kept.
#[derive(Clone)]
pub struct Dots<K> {
    now: Vec<(K, f32)>,
    at: Option<Instant>,
}

impl<K: Copy + PartialEq> Dots<K> {
    pub const fn new() -> Self {
        Dots { now: Vec::new(), at: None }
    }

    /// The radius of `key`'s dot now.
    pub fn of(&self, key: K) -> f32 {
        self.now.iter().find(|(k, _)| *k == key).map_or(DOT, |(_, dot)| *dot)
    }

    /// Eased on to `now`, toward each dot's size: `over` the one under the pointer, `held` the one held. Whether any
    /// is still on its way (more frames wanted).
    pub fn step(&mut self, now: Instant, over: Option<K>, held: Option<K>) -> bool {
        // (A first step, or one after a long rest, no longer than a frame's: not all the way at once.)
        let step = self.at.map_or(0.0, |at| now.saturating_duration_since(at).as_secs_f32()).min(1.0 / 30.0);
        self.at = Some(now);
        for key in [over, held].into_iter().flatten() {
            if !self.now.iter().any(|(k, _)| *k == key) {
                self.now.push((key, DOT));
            }
        }
        let to = |key: K| if held == Some(key) { DOT_HELD } else if over == Some(key) { DOT_OVER } else { DOT };
        let mut moving = false;
        for (key, dot) in &mut self.now {
            let to = to(*key);
            *dot = if to > *dot { (*dot + EASE * step).min(to) } else { (*dot - EASE * step).max(to) };
            moving |= *dot != to;
        }
        self.now.retain(|(key, dot)| *dot != DOT || over == Some(*key) || held == Some(*key));
        moving
    }
}

impl<K: Copy + PartialEq> Default for Dots<K> {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Under the pointer the dot grows to its size over a few frames, not at once; held, it shrinks; let go and left,
    /// it is back at rest and forgotten, and nothing more is wanted.
    #[test]
    fn the_dot_eases_between_its_sizes() {
        let mut dots: Dots<u8> = Dots::new();
        let start = Instant::now();
        let at = |ms: u64| start + std::time::Duration::from_millis(ms);
        assert!(!dots.step(at(0), None, None));
        assert!(dots.step(at(16), Some(1), None));
        let first = dots.of(1);
        assert!(first > DOT && first < DOT_OVER, "a step of the way: {first}");
        let mut ms = 32;
        while dots.step(at(ms), Some(1), None) {
            ms += 16;
        }
        assert_eq!(dots.of(1), DOT_OVER);
        assert!(ms <= 160, "there in about a tenth of a second: {ms} ms");
        ms += 16;
        while dots.step(at(ms), Some(1), Some(1)) {
            ms += 16;
        }
        assert_eq!(dots.of(1), DOT_HELD);
        ms += 16;
        while dots.step(at(ms), None, None) {
            ms += 16;
        }
        assert_eq!(dots.of(1), DOT);
        assert!(dots.now.is_empty());
        assert_eq!(dots.of(2), DOT);
    }
}
