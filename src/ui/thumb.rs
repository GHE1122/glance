//! A slider's thumb as Windows 11 draws its own: a round face with a faint
//! edge, and a dot of the accent in it, small at rest, larger under the
//! pointer and smaller while it is held, easing between (the settings', and
//! the panel's and the widgets' in the Windows 11 look).

use std::time::Instant;

use super::canvas::{Canvas, Color, Point};

/// The face's radius with its edge, and its dot's at rest (DIPs): Windows 11's own, measured pixel by pixel on a
/// 150% screen (its face 28.5 pixels across, its dot 13.5).
pub const FACE: f32 = 9.5;
const DOT: f32 = 4.5 / FACE;
/// The dot under the pointer, and held, of its size at rest (held, a little smaller than at rest, as Windows' is).
pub const OVER: f32 = 1.4;
pub const HELD: f32 = 0.85;
/// How quickly the dot goes toward its size: each moment, this much of the way left a second, as an exponent (fast
/// at first, slowing as it comes; most of the way in about a tenth of a second, all of it in about a sixth).
const EASE: f32 = 25.0;
/// Close enough to its size to be there.
const THERE: f32 = 0.004;

/// Draws a thumb at `centre`, `face` in radius with its edge, its dot `dot` times its size at rest and `accent`, on a
/// face `colour` edged `edge`. (All about one centre: each circle is put on whole pixels by its centre, so they stay
/// one in another.)
pub fn draw(frame: &dyn Canvas, centre: Point, face: f32, dot: f32, colour: Color, edge: Color, accent: Color) {
    frame.fill_circle(edge, centre, face);
    frame.fill_circle(colour, centre, face - 1.0);
    frame.fill_circle(accent, centre, face * DOT * dot);
}

/// The dots of a host's sliders, each known by `K`, easing toward their size (of their size at rest): at rest, under
/// the pointer, or held. Only those not at rest are kept. And what is chosen of several, each known by the `K` of the
/// one chosen (one of a kind: see `choose`), its mark sliding to it from the one chosen before.
#[derive(Clone)]
pub struct Dots<K> {
    now: Vec<(K, f32)>,
    at: Option<Instant>,
    slides: Vec<Slide<K>>,
}

/// A choice's mark (of the choice `of`: see `choose`): on `to`, come from `from` (none: it was first chosen so) since `start`, `done` of the way (eased).
#[derive(Clone)]
struct Slide<K> {
    of: usize,
    from: Option<K>,
    to: K,
    start: Instant,
    done: f32,
}

/// How long a choice's mark takes to slide from one to another, slowing as it comes (macOS's: it stops where it is
/// to be, at the time it is to, with no spring past it).
const SLIDE: f32 = 0.22;

impl<K: Copy + PartialEq> Dots<K> {
    pub const fn new() -> Self {
        Dots { now: Vec::new(), at: None, slides: Vec::new() }
    }

    /// What is chosen now of each choice (`chosen`: each choice's number, and what of it is chosen): each that changed
    /// since its mark slides from the one before, the others' come on. Whether any is still on its way (more frames
    /// wanted).
    pub fn choose(&mut self, now: Instant, chosen: &[(usize, K)]) -> bool {
        for &(of, to) in chosen {
            match self.slides.iter_mut().find(|slide| slide.of == of) {
                Some(slide) if slide.to != to => *slide = Slide { of, from: Some(slide.to), to, start: now, done: 0.0 },
                Some(_) => {}
                None => self.slides.push(Slide { of, from: None, to, start: now, done: 1.0 }),
            }
        }
        let mut moving = false;
        for slide in &mut self.slides {
            let t = (now.saturating_duration_since(slide.start).as_secs_f32() / SLIDE).min(1.0);
            slide.done = if slide.from.is_none() { 1.0 } else { 1.0 - (1.0 - t).powi(3) };
            moving |= slide.done < 1.0;
        }
        moving
    }

    /// Where the mark of what is chosen as `to` is on its way from the one chosen before: that one, and how far it has
    /// come (eased); none once it is there.
    pub fn slid(&self, to: K) -> Option<(K, f32)> {
        self.slides.iter().find(|slide| slide.to == to && slide.done < 1.0).and_then(|slide| Some((slide.from?, slide.done)))
    }

    /// The size of `key`'s dot now, of its size at rest.
    pub fn of(&self, key: K) -> f32 {
        self.now.iter().find(|(k, _)| *k == key).map_or(1.0, |(_, dot)| *dot)
    }

    /// Eased on to `now`, toward each dot's size: `over` the one under the pointer, `held` the one held. Whether any
    /// is still on its way (more frames wanted).
    pub fn step(&mut self, now: Instant, over: Option<K>, held: Option<K>) -> bool {
        // (A first step, or one after a long rest, no longer than a frame's: not all the way at once.)
        let step = self.at.map_or(0.0, |at| now.saturating_duration_since(at).as_secs_f32()).min(1.0 / 30.0);
        let toward = 1.0 - (-EASE * step).exp();
        self.at = Some(now);
        for key in [over, held].into_iter().flatten() {
            if !self.now.iter().any(|(k, _)| *k == key) {
                self.now.push((key, 1.0));
            }
        }
        let to = |key: K| if held == Some(key) { HELD } else if over == Some(key) { OVER } else { 1.0 };
        let mut moving = false;
        for (key, dot) in &mut self.now {
            let to = to(*key);
            *dot += (to - *dot) * toward;
            if (to - *dot).abs() < THERE {
                *dot = to;
            }
            moving |= *dot != to;
        }
        self.now.retain(|(key, dot)| *dot != 1.0 || over == Some(*key) || held == Some(*key));
        moving
    }
}

/// What is drawn of them: each dot not at rest and its size (a layer drawn with them is drawn again as one eases).
impl<K: std::hash::Hash> std::hash::Hash for Dots<K> {
    fn hash<H: std::hash::Hasher>(&self, state: &mut H) {
        for (key, dot) in &self.now {
            key.hash(state);
            dot.to_bits().hash(state);
        }
        for slide in self.slides.iter().filter(|slide| slide.done < 1.0) {
            slide.to.hash(state);
            slide.done.to_bits().hash(state);
        }
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

    /// What is first chosen is marked where it is; another chosen, its mark slides there from the one before, slowing as
    /// it comes, and is there at the slide's end (no further, and nothing more wanted after).
    #[test]
    fn a_choice_slides_to_the_one_chosen() {
        let start = Instant::now();
        let mut marks: Dots<u8> = Dots::new();
        assert!(!marks.choose(start, &[(0, 1)]));
        assert_eq!(marks.slid(1), None);
        assert!(marks.choose(start, &[(0, 2)]));
        let at = |ms: u64| start + std::time::Duration::from_millis(ms);
        assert!(marks.choose(at(55), &[(0, 2)]));
        let quarter = marks.slid(2).unwrap();
        assert_eq!(quarter.0, 1);
        assert!(marks.choose(at(165), &[(0, 2)]));
        let most = marks.slid(2).unwrap().1;
        // (Eased: more than a quarter of the way at a quarter of the time, the last quarter the least of it.)
        assert!(quarter.1 > 0.25 && most > 0.9 && most < 1.0, "{quarter:?} {most}");
        assert!(!marks.choose(at(230), &[(0, 2)]));
        assert_eq!(marks.slid(2), None);
    }

    /// The choices of an enum of no fields (quick launch's kinds), each value of another variant: one chosen after
    /// another slides from it (taken by their variant for the choice they are of, none slid: each was a choice of its
    /// own).
    #[test]
    fn a_choice_of_plain_variants_slides() {
        #[derive(Clone, Copy, PartialEq, Debug)]
        enum Kind {
            A,
            B,
        }
        let start = Instant::now();
        let mut marks: Dots<Kind> = Dots::new();
        marks.choose(start, &[(0, Kind::A)]);
        assert!(marks.choose(start, &[(0, Kind::B)]));
        assert_eq!(marks.slid(Kind::B).map(|(from, _)| from), Some(Kind::A));
    }

    /// At rest the dot is small; under the pointer it grows over several frames, fast then slowing, not at once; held,
    /// it shrinks, a little smaller than at rest; let go and left, it is back at rest and forgotten, and nothing more
    /// is wanted.
    #[test]
    fn the_dot_eases_between_its_sizes() {
        let mut dots: Dots<u8> = Dots::new();
        assert_eq!(dots.of(1), 1.0);
        let start = Instant::now();
        let at = |ms: u64| start + std::time::Duration::from_millis(ms);
        assert!(!dots.step(at(0), None, None));
        assert!(dots.step(at(16), Some(1), None));
        let first = dots.of(1);
        assert!(first > 1.0 && first < 1.0 + (OVER - 1.0) / 2.0, "a step of the way: {first}");
        let mut ms = 32;
        let mut steps = vec![first];
        while dots.step(at(ms), Some(1), None) {
            steps.push(dots.of(1));
            ms += 16;
        }
        assert_eq!(dots.of(1), OVER);
        assert!((96..=260).contains(&ms), "there in about a sixth of a second: {ms} ms");
        // Each step smaller than the one before (slowing as it comes), none a jump.
        let moves: Vec<f32> = steps.windows(2).map(|pair| pair[1] - pair[0]).collect();
        assert!(moves.windows(2).all(|pair| pair[1] <= pair[0] + 1e-6), "{moves:?}");
        ms += 16;
        while dots.step(at(ms), Some(1), Some(1)) {
            ms += 16;
        }
        assert_eq!(dots.of(1), HELD);
        ms += 16;
        while dots.step(at(ms), None, None) {
            ms += 16;
        }
        assert_eq!(dots.of(1), 1.0);
        assert!(dots.now.is_empty());
    }

    /// The face and its dot are one about the other wherever the thumb is, a pixel's fraction off the grid or not.
    #[test]
    #[cfg(feature = "studio")]
    fn the_dot_is_in_the_middle_of_its_face() {
        let gfx = crate::ui::gfx::Gfx::new().expect("graphics");
        for scale in [1.0f32, 1.25, 1.5] {
            for x in [20.0f32, 20.2, 20.33, 20.5, 20.7] {
                let pixels = gfx
                    .draw_offscreen((48, 48), scale, |frame| draw(frame, Point { x, y: 16.4 }, 8.0, 1.0, Color::hex(0xffffff, 1.0), Color::hex(0x000000, 0.1), Color::hex(0x0000ff, 1.0)))
                    .expect("drawn");
                let (w, h) = (48usize, 48usize);
                // The middle of what is blue, and of what is drawn at all, each way: the same.
                let middle = |inside: &dyn Fn(&[u8]) -> bool| {
                    let (mut sx, mut sy, mut n) = (0.0, 0.0, 0.0);
                    for y in 0..h {
                        for x in 0..w {
                            let p = &pixels[(y * w + x) * 4..(y * w + x) * 4 + 4];
                            if inside(p) {
                                sx += x as f32;
                                sy += y as f32;
                                n += 1.0;
                            }
                        }
                    }
                    (sx / n, sy / n)
                };
                let dot = middle(&|p| p[0] > 200 && p[1] < 60);
                let face = middle(&|p| p[3] > 200);
                assert!((dot.0 - face.0).abs() < 0.3 && (dot.1 - face.1).abs() < 0.3, "at {scale}, x {x}: the dot's middle {dot:?}, the face's {face:?}");
            }
        }
    }
}
