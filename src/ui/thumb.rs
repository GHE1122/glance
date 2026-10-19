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
    /// What is kept at hand, as it moves among itself: see `apps`.
    apps: Apps,
}

/// What is kept at hand moving among itself (one dragged along the others, or their order changed): the order they
/// show in (the one dragged where it would go), each one on its way from where it was (`Move`), and the one held, by
/// what opens it, and where the pointer has it (in the host's own DIPs).
#[derive(Clone, Default)]
struct Apps {
    order: Vec<String>,
    moves: Vec<Move>,
    held: Option<(String, (f32, f32))>,
}

/// One kept at hand held (see `Dots::apps`): its place as kept, the place it would go to, and where the pointer has it
/// (none: let go there).
pub type Held = (usize, usize, Option<(f32, f32)>);

/// Where a thing kept at hand is drawn: in its place among them (`Slot`, the places between one and the next as far
/// along as it is from the one to the other), or where the pointer has it.
#[derive(Clone, Copy, PartialEq, Debug)]
pub enum AppPlace {
    Slot(f32),
    Point(f32, f32),
}

/// One thing kept at hand on its way: from where it was to its place, since `start`, `done` of the way (eased).
#[derive(Clone)]
struct Move {
    target: String,
    from: AppPlace,
    to: usize,
    start: Instant,
    done: f32,
}

/// How long a thing kept at hand takes to its new place.
const SHIFT: f32 = 0.2;

impl Move {
    /// Where it is now.
    fn place(&self) -> AppPlace {
        let to = self.to as f32;
        match self.from {
            AppPlace::Slot(from) => AppPlace::Slot(from + (to - from) * self.done),
            // (From the pointer: on its way to its place, which the painter knows; told as how far, from where.)
            point => if self.done >= 1.0 { AppPlace::Slot(to) } else { point },
        }
    }
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
        Dots { now: Vec::new(), at: None, slides: Vec::new(), apps: Apps { order: Vec::new(), moves: Vec::new(), held: None } }
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

    /// What is kept at hand now (`order`: each by what opens it), one held and dragged (`held`: its place in `order`, the
    /// place it would go to among them, and where the pointer has it; no point: let go there, kept so till the order
    /// comes to it): each whose place among them changes goes there from where it is, the one let go from the
    /// pointer. Whether any is still on its way, or held (more frames wanted).
    pub fn apps(&mut self, now: Instant, order: &[String], held: Option<Held>) -> bool {
        // Where they show: as kept, or the one held where it would go.
        let mut shown: Vec<String> = order.to_vec();
        if let Some((from, to, _)) = held.filter(|(from, to, _)| *from < order.len() && *to < order.len()) {
            let moving = shown.remove(from);
            shown.insert(to, moving);
        }
        let held_now = held.and_then(|(from, _, point)| Some((order.get(from)?.clone(), point?)));
        let held_target = held_now.as_ref().map(|(target, _)| target.clone());
        // (Before: where each was drawn, mid-way where it was on its way.)
        let was: Vec<(String, AppPlace)> = self.apps.order.iter().enumerate().map(|(at, target)| {
            let place = self.apps.moves.iter().find(|one| &one.target == target).map_or(AppPlace::Slot(at as f32), Move::place);
            (target.clone(), place)
        }).collect();
        let let_go = self.apps.held.clone().filter(|_| held_now.is_none());
        for (at, target) in shown.iter().enumerate() {
            if Some(target) == held_target.as_ref() {
                self.apps.moves.retain(|one| &one.target != target);
                continue;
            }
            let heading = self.apps.moves.iter().find(|one| &one.target == target).map(|one| one.to);
            let from = match &let_go {
                Some((gone, point)) if gone == target => Some(AppPlace::Point(point.0, point.1)),
                _ => was.iter().find(|(one, _)| one == target).map(|(_, place)| *place).filter(|place| *place != AppPlace::Slot(at as f32) && heading != Some(at)),
            };
            if let Some(from) = from {
                self.apps.moves.retain(|one| &one.target != target);
                self.apps.moves.push(Move { target: target.clone(), from, to: at, start: now, done: 0.0 });
            }
        }
        self.apps.order = shown;
        let mut moving = held_now.is_some();
        self.apps.held = held_now;
        for one in &mut self.apps.moves {
            let t = (now.saturating_duration_since(one.start).as_secs_f32() / SHIFT).min(1.0);
            one.done = 1.0 - (1.0 - t).powi(3);
            moving |= one.done < 1.0;
        }
        self.apps.moves.retain(|one| one.done < 1.0);
        moving
    }

    /// Where the thing kept at hand `target` (shown at `at` among them) is drawn now: none, held (drawn where the
    /// pointer has it: see `app_held`); else in its place, or on its way to it, from a place among them or from a
    /// point (and how far it has come from there).
    pub fn app_place(&self, target: &str, at: usize) -> Option<(AppPlace, f32)> {
        if self.apps.held.as_ref().is_some_and(|(held, _)| held == target) {
            return None;
        }
        Some(match self.apps.moves.iter().find(|one| one.target == target) {
            Some(one) => match one.from {
                AppPlace::Point(..) => (one.from, one.done),
                _ => (one.place(), 1.0),
            },
            None => (AppPlace::Slot(at as f32), 1.0),
        })
    }

    /// The place `target` shows at among what is kept at hand (the one held where it would go): what one dragged over
    /// it goes to.
    pub fn app_slot(&self, target: &str) -> Option<usize> {
        self.apps.order.iter().position(|one| one == target)
    }

    /// The thing kept at hand held, by what opens it, and where the pointer has it.
    pub fn app_held(&self) -> Option<(&str, (f32, f32))> {
        self.apps.held.as_ref().map(|(target, point)| (target.as_str(), *point))
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
        for one in &self.apps.moves {
            one.target.hash(state);
            one.done.to_bits().hash(state);
        }
        if let Some((held, (x, y))) = &self.apps.held {
            (held, x.to_bits(), y.to_bits()).hash(state);
            self.apps.order.hash(state);
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

    /// One kept at hand dragged along the others: those between where it was and where it would go make room for it
    /// (each on its way, then there); let go there, it goes from the pointer to its place, and the others stay.
    #[test]
    fn things_kept_at_hand_make_room_for_one_dragged() {
        let start = Instant::now();
        let at = |ms: u64| start + std::time::Duration::from_millis(ms);
        let order: Vec<String> = ["a", "b", "c"].map(String::from).to_vec();
        let mut motion: Dots<u8> = Dots::new();
        assert!(!motion.apps(start, &order, None));
        // "a" held over where "c" is: "b" and "c" go up a place, "a" drawn at the pointer.
        assert!(motion.apps(start, &order, Some((0, 2, Some((5.0, 50.0))))));
        assert_eq!(motion.app_place("a", 0), None);
        assert_eq!(motion.app_held(), Some(("a", (5.0, 50.0))));
        motion.apps(at(100), &order, Some((0, 2, Some((5.0, 50.0)))));
        let Some((AppPlace::Slot(b), _)) = motion.app_place("b", 0) else { panic!() };
        assert!(b < 1.0 && b > 0.0, "{b}");
        motion.apps(at(400), &order, Some((0, 2, Some((5.0, 50.0)))));
        assert_eq!(motion.app_place("b", 0), Some((AppPlace::Slot(0.0), 1.0)));
        // Let go there, the order not yet come: "a" from the pointer to the last place, "b" and "c" where they are.
        motion.apps(at(400), &order, Some((0, 2, None)));
        assert_eq!(motion.app_place("a", 2), Some((AppPlace::Point(5.0, 50.0), 0.0)));
        assert_eq!(motion.app_place("c", 1), Some((AppPlace::Slot(1.0), 1.0)));
        // The order come so: nothing more moves once "a" is there.
        let kept: Vec<String> = ["b", "c", "a"].map(String::from).to_vec();
        assert!(motion.apps(at(500), &kept, None));
        assert!(!motion.apps(at(700), &kept, None));
        assert_eq!(motion.app_place("a", 2), Some((AppPlace::Slot(2.0), 1.0)));
        assert_eq!(motion.app_place("b", 0), Some((AppPlace::Slot(0.0), 1.0)));
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
