//! The music full screen: over the whole of a screen, the smoke (see `ui::smoke`) flowing in the cover's colours, and
//! the line sung now large at the left, each character coming up out of a blur as it is sung, lighting and swelling
//! with the music, the whole line starting a little at each beat; the next line small under it. The controls come
//! as the pointer moves and go a while after it rests (and the pointer with them): the track short above the keys in
//! the middle, the volume and the way out at the right. Esc, or a double click, leaves it.
//!
//! Its window takes the keyboard (for Esc) and is drawn at every frame while it is up.

use std::rc::Rc;
use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::Win32::Foundation::{HWND, RECT};
use windows::Win32::UI::WindowsAndMessaging::IDC_ARROW;

use crate::lyrics::Line;
use crate::playing::{Follow, Sung};
use crate::ui::canvas::{Align, Canvas, Color, Family, Font, Point};
use crate::ui::gfx::{rect, Frame, Gfx, Surface};
use crate::ui::icons::Icon;
use crate::ui::smoke::{self, Smoke};
use crate::ui::window::Window;

/// How long the controls stay after the pointer rests.
const AWAKE: Duration = Duration::from_millis(1200);
/// How long they take to come or go.
const CONTROLS_FADE: f32 = 0.35;
/// Two presses no further apart than this are a double click.
const DOUBLE: Duration = Duration::from_millis(400);
/// How long the line before takes to leave as the next comes (seconds).
const LEAVING: f32 = 0.7;

/// Each character's size and tilt as it stands in its line, from a few of each (by its place and the line's).
const SIZES: [f32; 8] = [1.15, 0.78, 1.0, 1.32, 0.86, 1.08, 0.72, 1.22];
const TILTS: [f32; 8] = [-0.09, 0.06, -0.03, 0.08, -0.07, 0.04, -0.05, 0.07];

/// What a press is on.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Hit {
    Back,
    Toggle,
    Next,
    Track,
    Mute,
    Volume,
    Leave,
}

/// What it asks of whoever holds it.
#[derive(PartialEq, Debug)]
pub enum Asked {
    Stay,
    Leave,
}

pub struct FullMusic {
    gfx: Rc<Gfx>,
    window: Window,
    surface: Surface,
    smoke: Option<Smoke>,
    /// The screen it covers (physical px) and its scale (physical px to a DIP).
    screen: RECT,
    scale: f32,
    /// The music as it goes: each band followed slowly, the swell of the lows, how far the smoke has flowed.
    smooth: [f32; crate::sound::BANDS],
    swell: f32,
    flow: f32,
    /// The beat, up quickly and down slowly (as it is heard, it comes and goes from one frame to the next).
    beat: f32,
    last: Option<Instant>,
    began: Instant,
    /// The smoke's colours, easing toward the cover's.
    tint: [f32; 3],
    tint2: [f32; 3],
    /// The line sung now, and the one before it with when it began to leave.
    now: Option<usize>,
    leaving: Option<(usize, Instant)>,
    lines: Option<Arc<Vec<Line>>>,
    /// The pointer: where it is, when it last moved; the controls, how far come.
    pointer: Option<(f32, f32)>,
    moved: Option<Instant>,
    controls: f32,
    /// What a press is on, as last drawn; the last press (for a double click); what is dragged.
    hits: Vec<((f32, f32, f32, f32), Hit)>,
    pressed: Option<Instant>,
    dragging: Option<Hit>,
}

impl FullMusic {
    /// Over the screen `screen` (physical px) at `scale`, in front with the keyboard; none where its window could
    /// not be made.
    pub fn open(gfx: Rc<Gfx>, screen: RECT, scale: f32) -> Option<Self> {
        let window = Window::activating().ok()?;
        let surface = Surface::new(&gfx, window.hwnd).ok()?;
        let smoke = Smoke::new(&gfx).inspect_err(|error| crate::journal::note(format!("music full screen: the smoke could not be drawn ({error})"))).ok();
        window.place(screen);
        window.show();
        window.focus();
        let now = Instant::now();
        Some(FullMusic {
            gfx,
            window,
            surface,
            smoke,
            screen,
            scale,
            smooth: [0.0; crate::sound::BANDS],
            swell: 0.0,
            flow: 0.0,
            beat: 0.0,
            last: None,
            began: now,
            tint: [0.40, 0.95, 0.88],
            tint2: [0.84, 0.97, 0.92],
            now: None,
            leaving: None,
            lines: None,
            pointer: None,
            moved: Some(now),
            controls: 1.0,
            hits: Vec::new(),
            pressed: None,
            dragging: None,
        })
    }

    pub fn owns(&self, hwnd: HWND) -> bool {
        self.window.hwnd == hwnd
    }

    fn size(&self) -> (f32, f32) {
        (((self.screen.right - self.screen.left) as f32) / self.scale, ((self.screen.bottom - self.screen.top) as f32) / self.scale)
    }

    /// Draws a frame; when the next is due (at once: it moves all the time).
    pub fn tick(&mut self, now: Instant) -> Duration {
        let dt = self.last.map_or(0.0, |last| now.duration_since(last).as_secs_f32().min(0.1));
        self.last = Some(now);
        let heard = crate::sound::heard();
        let k = 1.0 - (-dt / 0.22).exp();
        for (shown, band) in self.smooth.iter_mut().zip(heard.bands) {
            *shown += (band - *shown) * k;
        }
        let part = |from: usize, to: usize, of: &[f32]| of[from..to].iter().sum::<f32>() / (to - from) as f32;
        let bass = part(0, 4, &heard.bands);
        self.swell += (bass - self.swell) * (1.0 - (-dt / 0.45).exp());
        self.flow += dt * (0.55 + self.swell * 0.9);
        let heard_beat = heard.beat.clamp(0.0, 1.0);
        let pace = if heard_beat > self.beat { 0.04 } else { 0.3 };
        self.beat += (heard_beat - self.beat) * (1.0 - (-dt / pace).exp());
        // The controls: in while the pointer moved lately (or something is dragged), out after.
        let awake = self.dragging.is_some() || self.moved.is_some_and(|moved| now.duration_since(moved) < AWAKE);
        let to = if awake { 1.0 } else { 0.0 };
        self.controls += (to - self.controls).signum() * (dt / CONTROLS_FADE).min((to - self.controls).abs());
        if awake {
            self.window.point(IDC_ARROW);
        } else {
            self.window.point_none();
        }
        // What plays, and its cover's colours.
        let heard_now = crate::playing::music();
        if let Some(playing) = heard_now.as_ref().filter(|playing| !playing.title.is_empty()) {
            if let Some(cover) = crate::cover::of(&playing.title, &playing.artist) {
                let top = cover.base.iter().fold(0.01f32, |top, c| top.max(*c));
                let tint = cover.base.map(|c| (c / top * 0.96).min(1.0));
                let tint2 = tint.map(|c| c + (1.0 - c) * 0.55);
                let ease = 1.0 - (-dt / 0.6).exp();
                self.tint = std::array::from_fn(|i| self.tint[i] + (tint[i] - self.tint[i]) * ease);
                self.tint2 = std::array::from_fn(|i| self.tint2[i] + (tint2[i] - self.tint2[i]) * ease);
            }
        }
        let (lines, sung_now) = match heard_now.as_ref().and_then(|playing| playing.sung.clone()) {
            Some(Sung::Lines { lines, now, .. }) => (Some(lines), now),
            _ => (None, None),
        };
        if lines.as_ref().map(Arc::as_ptr) != self.lines.as_ref().map(Arc::as_ptr) {
            self.leaving = None;
        } else if sung_now != self.now {
            self.leaving = self.now.map(|before| (before, now));
        }
        self.lines = lines;
        self.now = sung_now;
        let position = crate::playing::position(Follow::Music).unwrap_or(0.0);
        let playing = heard_now.as_ref().is_some_and(|playing| playing.playing);
        let length = heard_now.as_ref().and_then(|playing| playing.timeline).map(|timeline| timeline.length);
        let (w, h) = self.size();
        let pixels = ((self.screen.right - self.screen.left).max(1) as u32, (self.screen.bottom - self.screen.top).max(1) as u32);
        if let Some(smoke) = self.smoke.as_mut() {
            let made = ((pixels.0 as f32 * smoke::SCALE) as u32, (pixels.1 as f32 * smoke::SCALE) as u32);
            if let Err(error) = smoke.render(made, &smoke::Now { flow: self.flow, swell: self.swell, tint: self.tint, tint2: self.tint2 }) {
                crate::journal::note(format!("music full screen: the smoke could not be drawn ({error})"));
                self.smoke = None;
            }
        }
        let t = now.duration_since(self.began).as_secs_f32();
        let music = Music { smooth: self.smooth, swell: self.swell, beat: self.beat, high: part(14, 20, &self.smooth), bass: part(0, 4, &self.smooth) };
        let (smoke, lines, now_index, leaving) = (&self.smoke, self.lines.clone(), self.now, self.leaving);
        let (pointer, controls) = (self.pointer, self.controls);
        let mut hits = Vec::new();
        let gfx = self.gfx.clone();
        let _ = self.surface.draw(&gfx, pixels, self.scale, |frame| {
            frame.origin(0.0, 0.0);
            frame.fill(Color::hex(0x000000, 1.0), 0.0, 0.0, w, h);
            if let Some(smoke) = smoke {
                smoke.paint(frame, rect(0.0, 0.0, w, h));
            }
            if let Some(lines) = &lines {
                draw_lines(frame, (w, h), lines, now_index, leaving.map(|(index, since)| (index, now.duration_since(since).as_secs_f32())), position, t, &music);
            }
            if controls > 0.0 {
                draw_controls(frame, (w, h), controls, pointer, playing, position, length, &mut hits);
            }
        });
        self.hits = hits;
        Duration::ZERO
    }

    /// The pointer moved to `at` (physical px, in the window).
    pub fn moved(&mut self, at: (i32, i32)) {
        let at = (at.0 as f32 / self.scale, at.1 as f32 / self.scale);
        if self.pointer.is_some_and(|was| (was.0 - at.0).abs() + (was.1 - at.1).abs() < 0.5) {
            return;
        }
        self.pointer = Some(at);
        self.moved = Some(Instant::now());
        if let Some(dragged) = self.dragging {
            self.drag(dragged, at);
        }
    }

    /// Pressed at `at` (physical px): a control does what it does; twice in a row elsewhere, it is left.
    pub fn press(&mut self, at: (i32, i32)) -> Asked {
        let at = (at.0 as f32 / self.scale, at.1 as f32 / self.scale);
        self.pointer = Some(at);
        self.moved = Some(Instant::now());
        let hit = self.hits.iter().find(|((x, y, w, h), _)| at.0 >= *x && at.0 < x + w && at.1 >= *y && at.1 < y + h).map(|(_, hit)| *hit);
        match hit {
            Some(Hit::Back) => crate::playing::act_on(Follow::Music, crate::playing::Act::Key(crate::playing::Key::Previous)),
            Some(Hit::Toggle) => crate::playing::act_on(Follow::Music, crate::playing::Act::Key(crate::playing::Key::Toggle)),
            Some(Hit::Next) => crate::playing::act_on(Follow::Music, crate::playing::Act::Key(crate::playing::Key::Next)),
            Some(Hit::Mute) => {
                let muted = crate::actions::volume().is_some_and(|(_, muted)| muted);
                let _ = crate::actions::set_volume(None, Some(!muted));
            }
            Some(Hit::Leave) => return Asked::Leave,
            Some(dragged @ (Hit::Track | Hit::Volume)) => {
                self.dragging = Some(dragged);
                self.drag(dragged, at);
            }
            None => {
                let now = Instant::now();
                if self.pressed.is_some_and(|before| now.duration_since(before) <= DOUBLE) {
                    return Asked::Leave;
                }
                self.pressed = Some(now);
            }
        }
        Asked::Stay
    }

    /// Let go: what was dragged is where it was let go.
    pub fn release(&mut self) {
        if let Some(Hit::Track) = self.dragging.take() {
            crate::playing::slid(crate::playing::Slider::Seek(Follow::Music), true);
        }
    }

    /// What is dragged, to `at`: the track (shown that far in, played from there once let go) or the volume.
    fn drag(&mut self, dragged: Hit, at: (f32, f32)) {
        let Some(((x, _, w, _), _)) = self.hits.iter().find(|(_, hit)| *hit == dragged) else { return };
        let fraction = ((at.0 - x) / w.max(1.0)).clamp(0.0, 1.0);
        match dragged {
            Hit::Track => crate::playing::slide_along(crate::playing::Slider::Seek(Follow::Music), fraction),
            Hit::Volume => {
                let _ = crate::actions::set_volume(Some((fraction * 100.0).round() as u8), Some(false));
            }
            _ => {}
        }
    }

    /// The wheel turned by `delta` (120 a notch): the sound louder or softer.
    pub fn wheel(&mut self, delta: i16) {
        self.moved = Some(Instant::now());
        if let Some((level, _)) = crate::actions::volume() {
            let level = (level as i32 + delta as i32 / 120 * 5).clamp(0, 100) as u8;
            let _ = crate::actions::set_volume(Some(level), Some(false));
        }
    }

    /// A key pressed: Esc leaves it, the space bar pauses or plays.
    pub fn key(&mut self, key: u16) -> Asked {
        match key {
            0x1B => Asked::Leave,
            0x20 => {
                crate::playing::act_on(Follow::Music, crate::playing::Act::Key(crate::playing::Key::Toggle));
                Asked::Stay
            }
            _ => Asked::Stay,
        }
    }
}

/// The music as the lines move with it: each band followed slowly, the lows' swell, the beat, the lows and the highs.
struct Music {
    #[allow(dead_code)]
    smooth: [f32; crate::sound::BANDS],
    swell: f32,
    beat: f32,
    high: f32,
    bass: f32,
}

fn ease(x: f32) -> f32 {
    1.0 - (1.0 - x.clamp(0.0, 1.0)).powi(3)
}

/// Each character of `line` with when it is sung and for how long: by its words' times where it has them (each
/// word's characters sharing it evenly), else its time shared evenly between them all (to the next line, or four
/// seconds on).
fn timed(line: &Line, next: Option<f64>) -> Vec<(char, f64, f64)> {
    if !line.words.is_empty() {
        return line
            .words
            .iter()
            .flat_map(|word| {
                let sung = word.text.trim_end().chars().count().max(1) as f64;
                let each = (word.end - word.at).max(0.0) / sung;
                word.text.chars().enumerate().map(move |(k, ch)| (ch, word.at + each * (k as f64).min(sung - 1.0), each))
            })
            .collect();
    }
    let chars: Vec<char> = line.text.chars().collect();
    let end = line.end.or(next).unwrap_or(line.at + 4.0);
    let each = (end - line.at).max(0.0) * 0.85 / chars.len().max(1) as f64;
    chars.into_iter().enumerate().map(|(k, ch)| (ch, line.at + each * k as f64, each)).collect()
}

/// The line sung now (and the one before it as it leaves, `leaving` seconds since) large at the left of `(w, h)`,
/// and the next line small under it.
#[allow(clippy::too_many_arguments)]
fn draw_lines(frame: &Frame, (w, h): (f32, f32), lines: &[Line], now: Option<usize>, leaving: Option<(usize, f32)>, position: f64, t: f32, music: &Music) {
    let left = w * 0.081;
    let baseline = h * 0.62;
    let base = h * 0.133;
    if let Some((before, since)) = leaving.filter(|(_, since)| *since < LEAVING) {
        if let Some(line) = lines.get(before) {
            draw_line(frame, (w, h), line, lines.get(before + 1).map(|line| line.at), before, position, t, music, ease(since / LEAVING), (left, baseline, base));
        }
    }
    let Some(index) = now else { return };
    let line = &lines[index];
    let next = lines.get(index + 1);
    draw_line(frame, (w, h), line, next.map(|line| line.at), index, position, t, music, 0.0, (left, baseline, base));
    // The next line, small under it, coming in a moment after.
    if let Some(next) = next.filter(|next| !next.text.trim().is_empty()) {
        let font = Font::new(Family::YaHei, (h * 0.029).max(14.0), 700.0);
        let shown = ((position - line.at) as f32 - 0.3).clamp(0.0, 1.0) * 0.5;
        frame.text_moving(&next.text, font, Color::hex(0xFFFFFF, shown), left + 4.0, baseline + h * 0.09, w - 2.0 * left, Align::Start);
    }
}

/// One line, each character its size and tilt along a slowly drifting wave, coming up out of a blur as it is sung,
/// lit and swelling with the music while it is; `out` of the way left (rising and fading).
#[allow(clippy::too_many_arguments)]
fn draw_line(frame: &Frame, (w, _h): (f32, f32), line: &Line, next: Option<f64>, which: usize, position: f64, t: f32, music: &Music, out: f32, (left, baseline, base): (f32, f32, f32)) {
    let chars = timed(line, next);
    if chars.iter().all(|(ch, ..)| ch.is_whitespace()) {
        return;
    }
    // Each character's size and room, the whole made smaller where it would not fit.
    let mut sizes: Vec<f32> = (0..chars.len()).map(|k| base * SIZES[(k + which * 3) % 8]).collect();
    let mut rooms: Vec<f32> = chars.iter().zip(&sizes).map(|((ch, ..), size)| if ch.is_whitespace() { size * 0.4 } else { frame.measure(&ch.to_string(), Font::new(Family::YaHei, *size, 700.0)) + base * 0.08 }).collect();
    let total: f32 = rooms.iter().sum();
    let fit = ((w - 2.0 * left) / total.max(1.0)).min(1.0);
    sizes.iter_mut().for_each(|size| *size *= fit);
    rooms.iter_mut().for_each(|room| *room *= fit);
    let sway = (t * 0.4).sin() * 0.04;
    let mut x = left;
    for (k, ((ch, at, each), (size, room))) in chars.iter().zip(sizes.iter().zip(&rooms)).enumerate() {
        let middle = x + room / 2.0;
        x += room;
        if ch.is_whitespace() {
            continue;
        }
        let age = (position - at) as f32;
        if age < -0.25 {
            continue;
        }
        let font = Font::new(Family::YaHei, *size, 700.0);
        let y = baseline + (k as f32 * 0.9 + t * 0.8 + which as f32).sin() * base * 0.18 + (middle - w / 2.0) * sway;
        let tilt = TILTS[(k + which) % 8] + (t * 0.6 + k as f32).sin() * 0.025;
        let came = ease((age + 0.25) / 0.55);
        // How lit it is: up as it is sung, down after.
        let lit = ease((age + 0.08) / 0.18) * (1.0 - ease((age - *each as f32 - 0.1) / 0.5)) * (1.0 - out);
        let breathe = 1.0 + 0.02 * (t * 1.7 + k as f32 * 1.3).sin() * (0.5 + music.swell);
        let swell = breathe * (1.0 + lit * (0.14 * music.bass + 0.04 * music.beat));
        let glow = 10.0 + 8.0 * music.swell + lit * (14.0 + 30.0 * music.high);
        let rise = (1.0 - came) * 26.0 - out * 60.0 - music.beat * 3.0;
        let alpha = (came * 1.4).min(1.0) * (1.0 - out);
        let (ascent, _) = frame.gfx.baseline(font);
        let said = ch.to_string();
        let top = y - ascent + rise;
        let ink = 0.92 + 0.08 * lit;
        frame.turned(middle, y - size * 0.35 + rise, tilt * (0.4 + 0.6 * came), (swell, swell), |frame| {
            if came < 1.0 {
                // Up out of a blur.
                frame.glow(Color::hex(0xFFFFFF, alpha), (1.0 - came) * 8.0 + 0.5, |frame| frame.text_moving(&said, font, Color::hex(0xFFFFFF, 1.0), middle - room / 2.0, top, room * 2.0, Align::Start));
            }
            frame.glow(Color::hex(0xFFFFFF, 0.85 * alpha), glow / 2.0, |frame| frame.text_moving(&said, font, Color::hex(0xFFFFFF, 1.0), middle - room / 2.0, top, room * 2.0, Align::Start));
            frame.text_moving(&said, font, Color { r: ink, g: ink * 0.985, b: ink * 0.965, a: alpha * came * came }, middle - room / 2.0, top, room * 2.0, Align::Start);
        });
    }
}

/// The controls, `shown` of them, along the bottom of `(w, h)`: the track short in the middle with its times at its
/// ends, the keys under it, the volume and the way out at the right; each lit under the pointer. What they are is
/// put in `hits`.
#[allow(clippy::too_many_arguments)]
fn draw_controls(frame: &Frame, (w, h): (f32, f32), shown: f32, pointer: Option<(f32, f32)>, playing: bool, position: f64, length: Option<f64>, hits: &mut Vec<((f32, f32, f32, f32), Hit)>) {
    let under = |x: f32, y: f32, wide: f32, tall: f32| pointer.is_some_and(|(px, py)| px >= x && px < x + wide && py >= y && py < y + tall);
    // A darkening along the bottom for them to be read on.
    frame.fill_shape(
        &[Point { x: 0.0, y: h - 140.0 }, Point { x: w, y: h - 140.0 }, Point { x: w, y: h }, Point { x: 0.0, y: h }],
        crate::ui::canvas::Fill::Down { top: h - 140.0, from: Color::hex(0x000000, 0.0), bottom: h, to: Color::hex(0x000000, 0.42 * shown) },
    );
    let ink = |lit: bool| Color::hex(0xFFFFFF, if lit { shown } else { 0.82 * shown });
    // The keys.
    let cell = 44.0;
    let row = h - 22.0 - cell;
    let mut x = w / 2.0 - cell * 1.5 - 18.0;
    for (hit, icon, side) in [(Hit::Back, Icon::SkipBack, 20.0), (Hit::Toggle, if playing { Icon::Pause } else { Icon::Play }, 30.0), (Hit::Next, Icon::SkipForward, 20.0)] {
        let lit = under(x, row, cell, cell);
        frame.icon(icon, Point { x: x + cell / 2.0, y: row + cell / 2.0 }, side, ink(lit));
        hits.push(((x, row, cell, cell), hit));
        x += cell + 18.0;
    }
    // The track, short, above them; its times at its ends.
    if let Some(length) = length.filter(|length| *length > 0.0) {
        let time = Font::new(Family::Segoe, 12.0, 500.0);
        let wide = (w * 0.42).min(420.0);
        let (from, y) = (w / 2.0 - wide / 2.0, row - 22.0);
        let lit = under(from - 8.0, y - 10.0, wide + 16.0, 20.0);
        let thick = if lit { 5.0 } else { 3.0 };
        let fraction = (position / length).clamp(0.0, 1.0) as f32;
        frame.fill_rounded(Color::hex(0xFFFFFF, 0.22 * shown), from, y - thick / 2.0, wide, thick, thick / 2.0);
        frame.fill_rounded(Color::hex(0xFFFFFF, shown), from, y - thick / 2.0, wide * fraction, thick, thick / 2.0);
        let clock = |seconds: f64| crate::ui::text::clock(seconds.max(0.0).round() as u32);
        frame.text(&clock(position), time, Color::hex(0xFFFFFF, 0.6 * shown), from - 70.0, y - 9.0, 60.0, Align::End);
        frame.text(&clock(length), time, Color::hex(0xFFFFFF, 0.6 * shown), from + wide + 10.0, y - 9.0, 60.0, Align::Start);
        hits.push(((from - 8.0, y - 10.0, wide + 16.0, 20.0), Hit::Track));
    }
    // The volume and the way out, at the right.
    let (level, muted) = crate::actions::volume().unwrap_or((0, false));
    let leave_x = w - 30.0 - cell;
    let lit = under(leave_x, row, cell, cell);
    frame.icon(Icon::Minimize, Point { x: leave_x + cell / 2.0, y: row + cell / 2.0 }, 20.0, ink(lit));
    hits.push(((leave_x, row, cell, cell), Hit::Leave));
    let bar = 96.0;
    let bar_x = leave_x - 12.0 - bar;
    let middle = row + cell / 2.0;
    let fraction = if muted { 0.0 } else { level as f32 / 100.0 };
    frame.fill_rounded(Color::hex(0xFFFFFF, 0.22 * shown), bar_x, middle - 1.5, bar, 3.0, 1.5);
    frame.fill_rounded(Color::hex(0xFFFFFF, shown), bar_x, middle - 1.5, bar * fraction, 3.0, 1.5);
    if under(bar_x - 8.0, row, bar + 16.0, cell) {
        frame.fill_circle(Color::hex(0xFFFFFF, shown), Point { x: bar_x + bar * fraction, y: middle }, 6.0);
    }
    hits.push(((bar_x - 8.0, row, bar + 16.0, cell), Hit::Volume));
    let mute_x = bar_x - 8.0 - cell;
    let lit = under(mute_x, row, cell, cell);
    frame.icon(if muted || level == 0 { Icon::Muted } else { Icon::Volume }, Point { x: mute_x + cell / 2.0, y: middle }, 20.0, ink(lit));
    hits.push(((mute_x, row, cell, cell), Hit::Mute));
}

#[cfg(all(test, feature = "studio"))]
mod pictures {
    use super::*;

    /// A frame of it, a line half sung with the next under it and the controls up, to a PNG in the target folder:
    /// `cargo test --features studio -- --ignored draws_the_full_screen`.
    #[test]
    #[ignore]
    fn draws_the_full_screen() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let gfx = crate::ui::gfx::current().unwrap();
        let (w, h) = (1280.0f32, 720.0f32);
        let mut smoke = Smoke::new(&gfx).unwrap();
        smoke.render(((w * smoke::SCALE) as u32, (h * smoke::SCALE) as u32), &smoke::Now { flow: 14.0, swell: 0.6, tint: [0.95, 0.66, 0.58], tint2: [0.98, 0.85, 0.81] }).unwrap();
        let line = |at: f64, text: &str| Line { at, end: None, text: text.into(), words: Vec::new(), backing: None, translation: None };
        let lines = vec![line(10.0, "如果你因为我而诚实"), line(14.5, "如果你看我的电影")];
        let music = Music { smooth: [0.5; crate::sound::BANDS], swell: 0.6, beat: 0.4, high: 0.5, bass: 0.6 };
        let pixels = gfx
            .draw_offscreen((w as u32, h as u32), 1.0, |frame| {
                frame.fill(Color::hex(0x000000, 1.0), 0.0, 0.0, w, h);
                smoke.paint(frame, rect(0.0, 0.0, w, h));
                draw_lines(frame, (w, h), &lines, Some(0), None, 12.6, 3.0, &music);
                let mut hits = Vec::new();
                draw_controls(frame, (w, h), 1.0, Some((w / 2.0, h - 40.0)), true, 80.0, Some(226.0), &mut hits);
            })
            .unwrap();
        let out = format!("{}/target/music", env!("CARGO_MANIFEST_DIR"));
        std::fs::create_dir_all(&out).unwrap();
        crate::studio::save_png(&std::path::PathBuf::from(format!("{out}/full.png")), w as u32, h as u32, &pixels).unwrap();
    }
}
