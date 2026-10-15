//! What plays, as a music widget shows it: laid out by the room it is given (a strip, a player, a card of lines, a
//! card stood on end), each of what it can show switched on or off (the cover, what plays and by whom, the line under
//! the one sung now, the lines round it, the spectrum, the controls kept showing), on the cover blurred behind it or
//! on nothing (its words edged to be read on whatever is behind). The line sung now is filled as it is sung; the wheel
//! turns through the lines, and the one turned to is played from where it is pressed. The widget's shell (see
//! `widget`) moves, sizes and keeps it; this draws it and says where in it a press does something.

use std::sync::Arc;
use std::time::{Duration, Instant};

use windows::core::Interface;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT, D2D_SIZE_U};
use windows::Win32::Graphics::Direct2D::{ID2D1Bitmap1, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_PROPERTIES1, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_LAYER_PARAMETERS1, D2D1_ROUNDED_RECT};
use windows::Win32::Graphics::DirectWrite::{IDWriteTextLayout, DWRITE_HIT_TEST_METRICS, DWRITE_LINE_METRICS};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;

use super::canvas::{Align, Canvas, Color, Family, Fill, Font, Point};
use super::gfx::{rect, Frame, Layer};
use super::view::{Hit, HitBox};
use crate::cover::Cover;
use crate::lyrics::Line;
use crate::playing::{Key, Sung};

/// How it is laid out, by the room it has (see `Layout::for_room`).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub enum Layout {
    /// One row: the cover small at its start, the line sung now, and at its end the spectrum or the controls. With
    /// no ground and no cover, the line large in its middle and what is under it below (floating lyrics).
    Strip,
    /// The cover at the start as tall as the room; beside it what plays, the line sung now and what is under it; the
    /// spectrum or the controls along the bottom.
    Player,
    /// What plays along the top (the cover small beside it), the lines round the one sung now gliding past it, the
    /// spectrum or the controls along the bottom.
    Card,
    /// The card stood on end: the cover above, what plays under it, then the lines.
    Tall,
}

impl Layout {
    /// The layout for a room `w` × `h` (DIPs) showing `shows`: the one it had (`had`) while the room is within
    /// `HOLD` of where the next begins, so that a room sized about a threshold does not turn back and forth.
    pub fn for_room((w, h): (f32, f32), shows: &Shows, had: Option<Layout>) -> Layout {
        // Controls kept showing over the spectrum take a row more.
        let more = if shows.controls && shows.spectrum { SPECTRUM_ROW + ROW_GAP } else { 0.0 };
        let pick = |slack: f32| {
            if h < PLAYER_FROM + more + slack || w < STRIP_UNDER + slack {
                Layout::Strip
            } else if !shows.lines || h < CARD_FROM + more + slack {
                Layout::Player
            } else if w >= TALL_UNDER + slack {
                Layout::Card
            } else {
                Layout::Tall
            }
        };
        let plain = pick(0.0);
        match had {
            Some(had) if had != plain && (pick(-HOLD) == had || pick(HOLD) == had) => had,
            _ => plain,
        }
    }

    /// As the journal and the studio name it.
    pub fn key(self) -> &'static str {
        match self {
            Layout::Strip => "strip",
            Layout::Player => "player",
            Layout::Card => "card",
            Layout::Tall => "tall",
        }
    }
}

/// Where each layout begins (DIPs): the player from this tall (and the strip below this wide), the card from this
/// tall, stood on end below this wide.
const PLAYER_FROM: f32 = 120.0;
const STRIP_UNDER: f32 = 230.0;
const CARD_FROM: f32 = 250.0;
const TALL_UNDER: f32 = 360.0;
/// How far past where the next layout begins a room keeps the one it has (DIPs).
const HOLD: f32 = 12.0;
/// The least a music widget is (DIPs).
pub const LEAST: (f32, f32) = (240.0, 56.0);

/// What a music widget shows, as the lyrics module's items say (the same for every one).
#[derive(Clone, Copy, PartialEq, Eq, Debug, Hash)]
pub struct Shows {
    pub cover: bool,
    pub title: bool,
    /// Under the line sung now: its translation where it has one (and translations are on), else the next line.
    pub translation: bool,
    pub next: bool,
    /// The lines round the one sung now (the card); off, it is laid out as a player however tall it is.
    pub lines: bool,
    pub spectrum: bool,
    /// The controls kept showing, in a row of their own; off, they come over the spectrum as the pointer does.
    pub controls: bool,
    /// No ground: its words edged, on whatever is behind it.
    pub clear: bool,
}

impl Shows {
    pub fn from(view: &serde_json::Value) -> Shows {
        let on = |name: &str| super::prefs::item_on(view, "lyrics", name);
        Shows {
            cover: on("cover"),
            title: on("title"),
            translation: on("translation"),
            next: on("next"),
            lines: on("lines"),
            spectrum: on("spectrum"),
            controls: on("controls"),
            clear: on("clear"),
        }
    }
}

/// The words: the line sung now in the card's lines, the others, the translation under it, what plays.
const NOW_FONT: Font = Font::new(Family::Segoe, 25.0, 700.0);
const UNDER_FONT: Font = Font::new(Family::Segoe, 14.0, 500.0);
const TITLE: Font = Font::new(Family::Segoe, 15.0, 700.0);
const ABOUT: Font = Font::new(Family::Segoe, 12.0, 500.0);
/// The player's line sung now and what is under it; how long is left.
const PLAYER_LINE: Font = Font::new(Family::Segoe, 18.0, 700.0);
const PLAYER_UNDER: Font = Font::new(Family::Segoe, 12.5, 500.0);
const TIME: Font = Font::new(Family::Segoe, 11.0, 500.0);
/// The strip's line sung now, and the least it is made to fit its room; large, in the middle (floating lyrics), at
/// most, and what is under it there, of its size.
const STRIP_LINE: Font = Font::new(Family::Segoe, 16.0, 700.0);
const STRIP_LEAST: f32 = 12.5;
const FLOAT_LINE: Font = Font::new(Family::Segoe, 36.0, 800.0);
const FLOAT_UNDER: f32 = 0.5;
/// The three that are pressed (what goes back, pauses or plays, and skips): the side of each, and the system icon
/// font's glyphs.
const CONTROL: f32 = 28.0;
const CONTROL_FONT: Font = Font::new(Family::Icons, 14.0, 400.0);
/// The spectrum's row of its own, under kept controls, and the room between rows (DIPs).
const SPECTRUM_ROW: f32 = 18.0;
const ROW_GAP: f32 = 6.0;
/// How fast a band of the spectrum falls, at most (of its whole height, a second): from full, in about 0.6 s.
const SPECTRUM_FALL: f32 = 1.7;
/// How long the controls take to come in over the spectrum (or go), seconds.
const CONTROLS_FADE: f32 = 0.18;
/// How long a line turned to by the wheel is held before the lines go back to the one sung now.
const TURNED_HOLD: Duration = Duration::from_secs(3);
/// How far the wheel turns for a line (its notch is 120).
const NOTCH: f32 = 120.0;
/// The dots of the music between lines: their radius, of the size of the words in their place (lit, a quarter
/// larger), how far apart their centres are, and how wide the three are, of it.
const INTERLUDE_DOT: f32 = 0.08;
const INTERLUDE_STEP: f32 = 0.43;
const INTERLUDE_WIDE: f32 = INTERLUDE_DOT * 2.5 + INTERLUDE_STEP * 2.0;

/// What a music widget keeps from frame to frame.
pub struct Music {
    /// The layout it was last drawn in.
    layout: Option<Layout>,
    /// The rows the card's lines have glided to (the line aimed at, fractional while gliding), and how fast.
    rail: f32,
    rail_speed: f32,
    /// The lyrics it last showed (to tell another track's).
    lyrics: Option<usize>,
    last_frame: Instant,
    /// The cover behind it, blurred, and the one it is fading from (with when that began).
    backdrop: Option<(usize, ID2D1Bitmap1)>,
    fading: Option<(ID2D1Bitmap1, Instant)>,
    /// The colours the lyrics are drawn in, easing toward the cover's.
    tint: [f32; 3],
    accent: [f32; 3],
    /// Its ground (its shadow, the cover blurred and washed, the cover, what plays), kept and drawn again only as it
    /// changes: what moves at every frame is drawn over it.
    ground: Layer,
    /// How far the controls have come in over the spectrum (0 to 1: in while the pointer is over it).
    controls_in: f32,
    /// Its spectrum as shown: up at once with the sound, down no faster than `SPECTRUM_FALL`.
    bands: [f32; crate::sound::BANDS],
    /// Lines turned by the wheel from the one sung now, when last turned, and the turn not yet a line.
    turned: i32,
    turned_at: Instant,
    wheeled: f32,
}

/// What a frame drew: where a press does something (from its corner, DIPs), and whether it is to be drawn again soon
/// (something plays, or it still moves).
pub struct Painted {
    pub hits: Vec<HitBox>,
    pub busy: bool,
}

impl Default for Music {
    fn default() -> Self {
        Music {
            layout: None,
            rail: -1.0,
            rail_speed: 0.0,
            lyrics: None,
            last_frame: Instant::now(),
            backdrop: None,
            fading: None,
            tint: [1.0, 0.9, 0.8],
            accent: [1.0, 0.75, 0.6],
            ground: Layer::default(),
            controls_in: 0.0,
            bands: [0.0; crate::sound::BANDS],
            turned: 0,
            turned_at: Instant::now(),
            wheeled: 0.0,
        }
    }
}

/// What plays at one moment, as it is drawn.
struct Moment {
    lines: Option<Arc<Vec<Line>>>,
    now: Option<usize>,
    since: Option<Instant>,
    position: f64,
    /// How long the track is (seconds), and whether it can be played from elsewhere along it; none where its player
    /// does not say.
    length: Option<(f64, bool)>,
    /// Where its track is being dragged to, while it is (seconds).
    dragged_to: Option<f64>,
    title: String,
    artist: String,
    /// Why there are no lines, where there are none.
    said: Option<String>,
    playing: bool,
    cover: Option<Arc<Cover>>,
    sound: crate::sound::Heard,
}

fn moment() -> Moment {
    let heard = crate::playing::now();
    let lang = crate::update::language();
    let (title, artist, playing) = heard.as_ref().map_or_else(Default::default, |heard| (heard.title.clone(), heard.artist.clone(), heard.playing));
    let mut moment = Moment {
        lines: None,
        now: None,
        since: None,
        position: crate::playing::position().unwrap_or(0.0),
        length: heard.as_ref().and_then(|heard| heard.timeline).map(|timeline| (timeline.length, timeline.seekable)),
        dragged_to: crate::playing::dragged_to(),
        cover: (!title.is_empty()).then(|| crate::cover::of(&title, &artist)).flatten(),
        sound: if playing { crate::sound::heard() } else { crate::sound::Heard::default() },
        title,
        artist,
        said: None,
        playing,
    };
    match heard.as_ref().and_then(|heard| heard.sung.clone()) {
        Some(Sung::Lines { lines, now, since }) => {
            moment.lines = Some(lines);
            moment.now = now;
            moment.since = Some(since);
        }
        Some(Sung::Untimed { netease: true }) => moment.said = Some(lang.pick("网易云音乐没有提供播放进度，安装 InfLink-rs 插件后可以显示歌词", "NetEase Cloud Music does not say how far in it is. With the InfLink-rs plugin it does").into()),
        Some(Sung::Untimed { netease: false }) => moment.said = Some(lang.pick("此播放器没有提供播放进度", "This player does not say how far into the track it is").into()),
        Some(Sung::Asking) => moment.said = Some(lang.pick("正在查找歌词…", "Finding the lyrics…").into()),
        Some(Sung::Unknown) => moment.said = Some(lang.pick("没有找到这首歌的歌词", "No lyrics were found for this track").into()),
        None => moment.said = Some(lang.pick("当前没有正在播放的歌曲", "Nothing is playing").into()),
    }
    moment
}

/// The line at `index` of what plays now played from its start (pressed where the wheel turned to it).
pub fn play_line(index: u16) {
    let Some(Sung::Lines { lines, .. }) = crate::playing::now().and_then(|now| now.sung) else { return };
    if let Some(line) = lines.get(index as usize) {
        crate::playing::act(crate::playing::Act::Seek(line.at));
    }
}

impl Music {
    /// The wheel turned by `delta` (a notch, 120) over it: the lines turned by as many, from the one sung now (or the
    /// one turned to), down toward the end for a turn toward the user.
    pub fn wheel(&mut self, delta: i16) {
        self.wheeled -= delta as f32;
        let lines = (self.wheeled / NOTCH).trunc();
        if lines == 0.0 {
            return;
        }
        self.wheeled -= lines * NOTCH;
        self.turned += lines as i32;
        self.turned_at = Instant::now();
    }

    /// Back to the line sung now (a line turned to was pressed).
    pub fn back_to_now(&mut self) {
        (self.turned, self.wheeled) = (0, 0.0);
    }

    /// What it drew with, let go of (its window hidden).
    pub fn release(&mut self) {
        self.ground.release();
        self.backdrop = None;
        self.fading = None;
    }

    /// Draws it in `size` (DIPs, from the frame's origin), showing `shows`, at `scale` physical pixels a DIP, the
    /// pointer at `pointer` (DIPs, from its corner) while it is over it.
    pub fn paint(&mut self, frame: &Frame, size: (f32, f32), shows: &Shows, scale: f32, pointer: Option<(f32, f32)>, now: Instant) -> Painted {
        self.paint_moment(frame, size, shows, scale, pointer, now, moment())
    }

    /// As `paint`, what plays as `moment` says.
    #[allow(clippy::too_many_arguments)]
    fn paint_moment(&mut self, frame: &Frame, size: (f32, f32), shows: &Shows, scale: f32, pointer: Option<(f32, f32)>, now: Instant, mut moment: Moment) -> Painted {
        let step = now.duration_since(self.last_frame).as_secs_f32().min(0.1);
        self.last_frame = now;
        // The colours, toward the cover's (a little each frame).
        if let Some(cover) = &moment.cover {
            let ease = (step * 3.0).min(1.0);
            self.tint = toward(self.tint, readable(cover.base, 0.72), ease);
            self.accent = toward(self.accent, readable(cover.accent, 0.62), ease);
        }
        // Turned to a line a while ago and left there: back to the one sung now.
        if self.turned != 0 && self.turned_at.elapsed() >= TURNED_HOLD {
            self.back_to_now();
        }
        // Turned no further than the lines go.
        if let (Some(lines), true) = (&moment.lines, self.turned != 0) {
            let at = moment.now.map_or(-1, |now| now as i32);
            self.turned = self.turned.clamp(-at.max(0), lines.len() as i32 - 1 - at.max(0));
        }
        // The rail, sprung toward the line aimed at (as Apple Music's lyrics move: quick, then settling).
        let target = moment.now.map_or(-1.0, |now| now as f32) + self.turned as f32;
        let key = moment.lines.as_ref().map(|lines| Arc::as_ptr(lines) as usize);
        if key != self.lyrics {
            self.lyrics = key;
            self.rail = target;
            self.rail_speed = 0.0;
            self.back_to_now();
        }
        let (stiffness, damping, mass) = (142.0, 28.0, 0.82);
        let mut left = step;
        while left > 0.0 {
            let dt = left.min(1.0 / 240.0);
            let force = -stiffness * (self.rail - target) - damping * self.rail_speed;
            self.rail_speed += force / mass * dt;
            self.rail += self.rail_speed * dt;
            left -= dt;
        }
        let settling = (self.rail - target).abs() > 0.002 || self.rail_speed.abs() > 0.002;
        // The controls, in toward the pointer over it and out after it (kept showing, always in).
        let controls_to = if pointer.is_some() || shows.controls { 1.0 } else { 0.0 };
        self.controls_in = if controls_to > self.controls_in { (self.controls_in + step / CONTROLS_FADE).min(controls_to) } else { (self.controls_in - step / CONTROLS_FADE).max(controls_to) };
        let coming = self.controls_in != controls_to;
        // The spectrum, falling where the sound has fallen (drawn from what it shows, not what is heard).
        for (shown, heard) in self.bands.iter_mut().zip(moment.sound.bands) {
            *shown = if heard >= *shown { heard } else { (*shown - step * SPECTRUM_FALL).max(heard) };
        }
        moment.sound.bands = self.bands;
        let falling = !moment.playing && self.bands.iter().any(|&band| band > 0.0);
        let layout = Layout::for_room(size, shows, self.layout);
        if self.layout != Some(layout) {
            crate::journal::note(format!("music widget: {}", layout.key()));
            self.layout = Some(layout);
        }
        let mut drawing = Drawing { music: self, frame, moment: &moment, shows, scale, pointer, size, hits: Vec::new() };
        drawing.ground(layout);
        match layout {
            Layout::Strip => drawing.strip(),
            Layout::Player => drawing.player(),
            Layout::Card | Layout::Tall => drawing.card(layout),
        }
        let hits = drawing.hits;
        Painted { hits, busy: moment.playing || settling || coming || falling || self.fading.is_some() || self.turned != 0 || moment.dragged_to.is_some() }
    }
}

/// One frame being drawn.
struct Drawing<'a> {
    music: &'a mut Music,
    frame: &'a Frame<'a>,
    moment: &'a Moment,
    shows: &'a Shows,
    scale: f32,
    pointer: Option<(f32, f32)>,
    size: (f32, f32),
    hits: Vec<HitBox>,
}

/// Each layout's room inside its edge, and the corners of its ground (DIPs).
fn pad(layout: Layout) -> f32 {
    match layout {
        Layout::Strip => 10.0,
        Layout::Player => 18.0,
        Layout::Card | Layout::Tall => 22.0,
    }
}
const RADIUS: f32 = 14.0;
/// The cover in the card's top row, beside what plays (DIPs).
const CARD_COVER: f32 = 64.0;

/// Where its parts go, as `layout` lays them out in `size` showing `shows`: the cover (its corner and side), what
/// plays (its corner, how wide, centred or not), and where the rest begins below them.
struct Placed {
    cover: Option<(f32, f32, f32)>,
    title: Option<(f32, f32, f32, bool)>,
    below: f32,
    /// Where the words beside the cover begin (the player and the strip), and how wide they are.
    left: f32,
    width: f32,
}

fn placed(layout: Layout, (w, h): (f32, f32), shows: &Shows) -> Placed {
    let pad = pad(layout);
    match layout {
        Layout::Strip => {
            let side = (h - 2.0 * pad).max(0.0);
            let cover = shows.cover.then_some((pad, pad, side));
            let left = if shows.cover { pad + side + 12.0 } else { pad + 4.0 };
            Placed { cover, title: None, below: 0.0, left, width: (w - pad - left).max(0.0) }
        }
        Layout::Player => {
            let side = (h - 2.0 * pad).min(w * 0.36).max(0.0);
            let cover = shows.cover.then_some((pad, pad, side));
            let left = if shows.cover { pad + side + 16.0 } else { pad };
            let width = (w - left - pad).max(0.0);
            let title = shows.title.then_some((left, pad - 2.0, width, false));
            Placed { cover, title, below: if shows.title { pad + 46.0 } else { pad + 4.0 }, left, width }
        }
        Layout::Card => {
            let cover = shows.cover.then_some((pad, pad, CARD_COVER));
            let tx = if shows.cover { pad + CARD_COVER + 14.0 } else { pad };
            let title = shows.title.then_some((tx, pad + if shows.cover { 12.0 } else { 0.0 }, (w - tx - pad).max(0.0), false));
            let below = pad + if shows.cover { CARD_COVER + 12.0 } else if shows.title { 44.0 } else { 0.0 };
            Placed { cover, title, below, left: pad, width: (w - 2.0 * pad).max(0.0) }
        }
        Layout::Tall => {
            let wide = (w - 2.0 * pad).max(0.0);
            let side = wide.min(h * 0.3);
            let cover = shows.cover.then_some(((w - side) / 2.0, pad, side));
            let mut y = pad + if shows.cover { side + 12.0 } else { 0.0 };
            let title = shows.title.then_some((pad, y, wide, true));
            if shows.title {
                y += 44.0;
            }
            Placed { cover, title, below: y, left: pad, width: wide }
        }
    }
}

impl Drawing<'_> {
    /// Its ground (the cover blurred under a wash of its colour, inside its rounded corners, with a shadow; none,
    /// clear), the cover and what plays: kept, drawn again only as one of them changes.
    fn ground(&mut self, layout: Layout) {
        let (w, h) = self.size;
        let frame = self.frame;
        if let Some(cover) = &self.moment.cover {
            self.music.backdrop(frame, cover);
        }
        let shown = self.music.fading.as_ref().map_or(1.0, |(_, since)| (since.elapsed().as_secs_f32() / 1.2).min(1.0));
        let before = self.music.fading.as_ref().map(|(before, _)| before.clone());
        if shown >= 1.0 {
            self.music.fading = None;
        }
        let backdrop = self.music.backdrop.as_ref().map(|(key, bitmap)| (*key, bitmap.clone()));
        let (accent, tint) = (self.music.accent, self.music.tint);
        let shows = *self.shows;
        let at = placed(layout, self.size, &shows);
        let cover = self.moment.cover.clone();
        let (title, artist) = (self.moment.title.clone(), self.moment.artist.clone());
        let key = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            (w.to_bits(), h.to_bits(), self.scale.to_bits(), layout, shows, backdrop.as_ref().map(|(key, _)| *key), (shown * 64.0).round() as u32).hash(&mut hasher);
            accent.iter().chain(&tint).map(|v| (v * 255.0).round() as u8).collect::<Vec<_>>().hash(&mut hasher);
            (&title, &artist, cover.as_ref().map(|cover| Arc::as_ptr(cover) as usize)).hash(&mut hasher);
            hasher.finish()
        };
        let radius = RADIUS.min(h / 2.0);
        let ground = Ground { size: (w, h), radius, backdrop: backdrop.map(|(_, bitmap)| bitmap), before, shown, accent, tint, clear: shows.clear };
        // (Round it, room for its shadow.)
        let room = 24.0;
        let _ = self.music.ground.draw(frame, key, windows_numerics::Matrix3x2::identity(), (-room, -room, w + 2.0 * room, h + 2.0 * room), self.scale, None, |frame| {
            ground.paint(frame, |frame| {
                if let Some((x, y, side)) = at.cover {
                    let corner = if side < 50.0 { 6.0 } else { 9.0 };
                    frame.shadow(Color::hex(0x000000, 0.35), x, y, side, side, corner, 10.0, 3.0);
                    match &cover {
                        Some(cover) => {
                            let picture = super::canvas::Picture { key: Arc::as_ptr(cover) as usize as u64, side: crate::cover::SIDE, pixels: cover.pixels.clone() };
                            frame.picture(&picture, x, y, side, corner, 1.0);
                        }
                        // None (its player gives none, or it is being read): a plate with a note on it.
                        None => {
                            frame.fill_rounded(Color::hex(0xFFFFFF, if shows.clear { 0.18 } else { 0.08 }), x, y, side, side, corner);
                            let note = Font::new(Family::Icons, side * 0.38, 400.0);
                            let (ink_top, ink_bottom) = frame.ink("\u{E8D6}", note);
                            let wide = frame.measure("\u{E8D6}", note);
                            frame.text("\u{E8D6}", note, Color::hex(0xFFFFFF, 0.5), x + (side - wide) / 2.0, y + side / 2.0 - (ink_top + ink_bottom) / 2.0, wide + 2.0, Align::Start);
                        }
                    }
                }
                if let Some((x, y, wide, centred)) = at.title {
                    for (text, font, alpha, down) in [(&title, TITLE, 0.95, 0.0), (&artist, ABOUT, 0.62, 21.0)] {
                        let text = fitted(frame, text, font, wide);
                        let from = if centred { x + (wide - frame.measure(&text, font)) / 2.0 } else { x };
                        words(frame, &text, font, Color::hex(0xFFFFFF, alpha), from, y + down, wide + 2.0, Align::Start, shows.clear);
                    }
                }
            })
        });
    }

    /// The rows along the bottom, from `x`, `wide`, up from `edge`: the controls kept showing in a row of their own
    /// with the spectrum's row above them; else the spectrum, the controls coming over it as the pointer does. Where
    /// they leave room down to.
    fn rows(&mut self, x: f32, edge: f32, wide: f32, bars: usize) -> f32 {
        let row = edge - CONTROL;
        let (accent, tint) = (self.music.accent, self.music.tint);
        if self.shows.controls {
            self.keys(x - (CONTROL - 16.0) / 2.0, row, 1.0).map(|end| self.progress(end + 10.0, x + wide, row, 1.0));
            if !self.shows.spectrum {
                return row;
            }
            let top = row - ROW_GAP - SPECTRUM_ROW;
            spectrum(self.frame, self.moment, (accent, tint), (x, row - ROW_GAP, wide, SPECTRUM_ROW), bars, 0.85);
            return top;
        }
        let shown_in = self.music.controls_shown();
        if shown_in < 1.0 && self.shows.spectrum {
            spectrum(self.frame, self.moment, (accent, tint), (x, edge - 2.0, wide, CONTROL - 4.0), bars, 1.0 - shown_in);
        }
        if shown_in > 0.0 {
            self.keys(x - (CONTROL - 16.0) / 2.0, row, shown_in).map(|end| self.progress(end + 10.0, x + wide, row, shown_in));
        }
        row
    }

    /// What goes back, pauses or plays, and skips, in a row from `x` at `row`, `fade` of them shown (each lit under
    /// the pointer, pressed only once they are half come); where the row ends.
    fn keys(&mut self, mut x: f32, row: f32, fade: f32) -> Option<f32> {
        let frame = self.frame;
        for (key, glyph) in [(Key::Previous, "\u{E892}"), (Key::Toggle, if self.moment.playing { "\u{E769}" } else { "\u{E768}" }), (Key::Next, "\u{E893}")] {
            let on = self.pointer.is_some_and(|(px, py)| px >= x && px < x + CONTROL && py >= row && py < row + CONTROL);
            if on {
                frame.fill_rounded(Color::hex(0xFFFFFF, 0.14 * fade), x, row, CONTROL, CONTROL, CONTROL / 2.0);
            }
            let font = if key == Key::Toggle { Font { size: 16.0, ..CONTROL_FONT } } else { CONTROL_FONT };
            let (ink_top, ink_bottom) = frame.ink(glyph, font);
            let wide = frame.measure(glyph, font);
            let colour = Color::hex(0xFFFFFF, (if on { 1.0 } else { 0.85 }) * fade);
            if self.shows.clear {
                outlined(frame, glyph, font, colour, x + (CONTROL - wide) / 2.0, row + CONTROL / 2.0 - (ink_top + ink_bottom) / 2.0, wide + 2.0);
            } else {
                frame.text(glyph, font, colour, x + (CONTROL - wide) / 2.0, row + CONTROL / 2.0 - (ink_top + ink_bottom) / 2.0, wide + 2.0, Align::Start);
            }
            if self.music.controls_in >= 0.5 {
                self.hits.push((x, row, CONTROL, CONTROL, Hit::Media(key)));
            }
            x += CONTROL + 2.0;
        }
        Some(x)
    }

    /// What plays's track, from `from` to `to` on `row`, with how long is left at its end, `fade` of it shown
    /// (thicker under the pointer, or dragged; dragged, the time it is dragged to over the place it is at). Pressed and
    /// dragged along it (see `Hit::Seek`), what plays is shown that far in, and played from there once let go.
    fn progress(&mut self, from: f32, to: f32, row: f32, fade: f32) {
        let Some((length, seekable)) = self.moment.length.filter(|(length, _)| *length > 0.0) else { return };
        let frame = self.frame;
        let position = self.moment.dragged_to.unwrap_or(self.moment.position);
        let left_over = format!("-{}", super::text::clock((length - position).max(0.0).round() as u32));
        let time_wide = frame.measure(&left_over, TIME) + 2.0;
        let track = (to - time_wide - 10.0 - from).max(0.0);
        if track < 24.0 {
            return;
        }
        let lit = self.pointer.is_some_and(|(px, py)| px >= from && px < from + track && py >= row && py < row + CONTROL);
        let dragged = self.moment.dragged_to.is_some();
        let middle = row + CONTROL / 2.0;
        let fraction = (position / length).clamp(0.0, 1.0) as f32;
        let thick = if seekable && (lit || dragged) { 5.0 } else { 3.0 };
        frame.fill_rounded(Color::hex(0xFFFFFF, 0.2 * fade), from, middle - thick / 2.0, track, thick, thick / 2.0);
        frame.fill_rounded(colour(self.music.accent, 0.95 * fade), from, middle - thick / 2.0, track * fraction, thick, thick / 2.0);
        if seekable && (lit || dragged) {
            frame.fill_circle(Color::hex(0xFFFFFF, fade), Point { x: from + track * fraction, y: middle }, 6.0);
        }
        words(frame, &left_over, TIME, Color::hex(0xFFFFFF, 0.6 * fade), to - time_wide, middle - TIME.size * 0.7, time_wide, Align::End, self.shows.clear);
        // Dragged: the time it is dragged to, over where it is.
        if dragged {
            let said = super::text::clock(position.max(0.0).round() as u32);
            let wide = frame.measure(&said, TIME) + 12.0;
            let x = (from + track * fraction - wide / 2.0).clamp(from - 4.0, from + track - wide + 4.0);
            let y = row - 18.0;
            frame.fill_rounded(Color::hex(0x000000, 0.7), x, y, wide, 17.0, 5.0);
            frame.text(&said, TIME, Color::hex(0xFFFFFF, 0.95), x + 6.0, y + 1.5, wide, Align::Start);
        }
        if seekable && self.music.controls_in >= 0.5 {
            self.hits.push((from, row, track, CONTROL, Hit::Seek));
        }
    }

    /// The line at `index`, turned to by the wheel: whole, its time beside it to be played from (pressed there, or
    /// on it), in one row from `(x, y)` at most `width` wide, in `font`.
    fn turned_to(&mut self, lines: &[Line], index: usize, x: f32, y: f32, width: f32, font: Font) {
        let frame = self.frame;
        let line = &lines[index];
        let chip = format!("▶ {}", super::text::clock(line.at.max(0.0) as u32));
        let chip_wide = frame.measure(&chip, TIME) + 14.0;
        let text = if line.text.trim().is_empty() { "…".to_string() } else { line.text.clone() };
        let room = (width - chip_wide - 10.0).max(0.0);
        let shown = fitted(frame, &text, font, room);
        let wide = frame.measure(&shown, font);
        words(frame, &shown, font, Color::hex(0xFFFFFF, 0.92), x, y, room + 2.0, Align::Start, self.shows.clear);
        let (cx, cy) = (x + wide + 10.0, y + font.size * 0.66 - 9.0);
        let on = self.pointer.is_some_and(|(px, py)| px >= cx && px < cx + chip_wide && py >= cy && py < cy + 18.0);
        frame.fill_rounded(Color::hex(if on { 0xFFFFFF } else { 0x000000 }, if on { 0.22 } else { 0.28 }), cx, cy, chip_wide, 18.0, 9.0);
        frame.stroke_rounded(Fill::Solid(Color::hex(0xFFFFFF, 0.5)), cx, cy, chip_wide, 18.0, 9.0, 1.0);
        frame.text(&chip, TIME, Color::hex(0xFFFFFF, 0.95), cx + 7.0, cy + 2.0, chip_wide, Align::Start);
        self.hits.push((x, y, wide, font.size * 1.35, Hit::Line(index as u16)));
        self.hits.push((cx, cy, chip_wide, 18.0, Hit::Line(index as u16)));
    }

    /// The strip: the cover small at its start, the line sung now, and at its end the spectrum or the controls (kept
    /// showing: the controls at the end, the spectrum before them). With no ground and no cover, the line large in its
    /// middle and what is under it below, the controls at its start as the pointer comes.
    fn strip(&mut self) {
        let (w, h) = self.size;
        let pad = pad(Layout::Strip);
        let at = placed(Layout::Strip, self.size, self.shows);
        let (accent, tint) = (self.music.accent, self.music.tint);
        let keys_wide = 3.0 * CONTROL + 4.0;
        let row = (h - CONTROL) / 2.0;
        let centred = self.shows.clear && !self.shows.cover;
        let shown_in = self.music.controls_shown();
        let mut end = w - pad;
        if self.shows.controls {
            self.keys(end - keys_wide, row, 1.0);
            end -= keys_wide + 10.0;
            if self.shows.spectrum && !centred {
                spectrum(self.frame, self.moment, (accent, tint), (end - 70.0, h / 2.0 + 10.0, 70.0, 20.0), 12, 0.85);
                end -= 80.0;
            }
        } else if centred {
            if shown_in > 0.0 {
                self.keys(pad, row, shown_in);
            }
        } else {
            let wide = keys_wide.max(84.0);
            if shown_in < 1.0 && self.shows.spectrum {
                spectrum(self.frame, self.moment, (accent, tint), (end - wide, h / 2.0 + 10.0, wide, 20.0), 14, 1.0 - shown_in);
            }
            if shown_in > 0.0 {
                self.keys(end - keys_wide, row, shown_in);
            }
            end -= wide + 12.0;
        }
        if centred {
            // Clear of the controls at either end, the words kept in the middle.
            let side = if self.shows.controls { keys_wide + 14.0 } else { (keys_wide + 14.0) * shown_in };
            self.lines_centred(pad + side, w - pad - side, h);
            return;
        }
        let left = at.left;
        let width = (end - left).max(0.0);
        let frame = self.frame;
        frame.clip(left - 4.0, 0.0, width + 8.0, h);
        let (lines, now) = (self.moment.lines.clone(), self.moment.now);
        match (&lines, now) {
            (Some(lines), _) if self.music.turned != 0 => {
                let index = (now.map_or(0, |now| now as i32) + self.music.turned).clamp(0, lines.len() as i32 - 1) as usize;
                self.turned_to(lines, index, left, h / 2.0 - STRIP_LINE.size * 0.7, width, STRIP_LINE);
            }
            (Some(lines), Some(now)) => {
                let line = &lines[now];
                let next = lines.get(now + 1).map(|line| line.at);
                let (text, parts) = parts(line, next);
                let eased = self.came();
                let (dim, bright) = self.line_colours(eased);
                if text.trim().is_empty() {
                    let y = h / 2.0 - STRIP_LINE.size * 0.7 + (1.0 - eased) * 8.0;
                    interlude(frame, left, y, STRIP_LINE.size, between(line, next, self.moment.position), dim, bright);
                } else {
                    let font = Font { size: (STRIP_LINE.size * (width / frame.measure(&text, STRIP_LINE).max(1.0))).clamp(STRIP_LEAST, STRIP_LINE.size), ..STRIP_LINE };
                    let y = h / 2.0 - font.size * 0.7 + (1.0 - eased) * 8.0;
                    // (One row: where it would wrap, cut where it wraps.)
                    let layout = frame.gfx.wrapped(&text, font, 100_000.0);
                    let sung = sung_to(&parts, self.moment.position);
                    self.filled(&layout, &text, font, left, y, 100_000.0, sung, dim, bright, 12.0);
                }
            }
            (Some(lines), None) => {
                let first = lines.first().map_or(0.0, |line| line.at);
                let done = if first > 0.0 { (self.moment.position / first).clamp(0.0, 1.0) as f32 } else { 0.0 };
                let (dim, bright) = self.line_colours(1.0);
                interlude(frame, left, h / 2.0 - STRIP_LINE.size * 0.7, STRIP_LINE.size, done, dim, bright);
            }
            (None, _) => {
                let said = [self.moment.title.as_str(), self.moment.artist.as_str()].into_iter().filter(|one| !one.is_empty()).collect::<Vec<_>>().join(" · ");
                let said = if said.is_empty() { self.moment.said.clone().unwrap_or_default() } else { said };
                words(frame, &fitted(frame, &said, ABOUT, width), ABOUT, Color::hex(0xFFFFFF, 0.8), left, h / 2.0 - ABOUT.size * 0.7, width + 2.0, Align::Start, self.shows.clear);
            }
        }
        frame.unclip();
    }

    /// The line sung now large in the middle between `from` and `to` (floating lyrics), what is under it below where
    /// there is room: glowing in the cover's colour, a dark haze round it to be read on anything.
    fn lines_centred(&mut self, from: f32, to: f32, h: f32) {
        let frame = self.frame;
        let room = (to - from).max(0.0);
        let under_room = |size: f32| h >= size * 1.4 + size * FLOAT_UNDER * 1.4 + 16.0;
        let size = (h * 0.34).clamp(STRIP_LEAST, FLOAT_LINE.size);
        let font = Font { size, ..FLOAT_LINE };
        let under_font = Font { size: (size * FLOAT_UNDER).max(11.0), weight: 600.0, ..FLOAT_LINE };
        let with_under = under_room(size);
        let top = if with_under { (h - size * 1.4 - under_font.size * 1.4) / 2.0 } else { (h - size * 1.4) / 2.0 };
        let Some(lines) = self.moment.lines.clone() else {
            if let Some(said) = &self.moment.said {
                let wide = frame.measure(said, under_font).min(room);
                outlined(frame, said, under_font, Color::hex(0xFFFFFF, 0.85), from + (room - wide) / 2.0, (h - under_font.size * 1.4) / 2.0, wide + 2.0);
            }
            return;
        };
        if self.music.turned != 0 {
            let index = (self.moment.now.map_or(0, |now| now as i32) + self.music.turned).clamp(0, lines.len() as i32 - 1) as usize;
            let text = lines[index].text.clone();
            let wide = (frame.measure(&text, font) + frame.measure("▶ 0:00", TIME) + 24.0).min(room);
            self.turned_to(&lines, index, from + (room - wide) / 2.0, top, wide, font);
            return;
        }
        let Some(now) = self.moment.now else { return };
        let line = &lines[now];
        let next = lines.get(now + 1).map(|line| line.at);
        let (text, parts) = parts(line, next);
        let eased = self.came();
        let y = top + (1.0 - eased) * 14.0;
        let (accent, tint) = (self.music.accent, self.music.tint);
        let (dim, bright) = (Color::hex(0xFFFFFF, 0.6 * eased), colour(tint, eased));
        if text.trim().is_empty() {
            let x = from + (room - font.size * INTERLUDE_WIDE) / 2.0;
            let done = between(line, next, self.moment.position);
            frame.glow(Color::hex(0x000000, 0.55 * eased), 3.0, |frame| interlude(frame, x, y + 1.0, font.size, 0.0, Color::hex(0xFFFFFF, 1.0), Color::hex(0xFFFFFF, 1.0)));
            frame.glow(colour(accent, 0.75 * eased), 10.0, |frame| interlude(frame, x, y, font.size, done, Color::hex(0, 0.0), Color::hex(0xFFFFFF, 1.0)));
            interlude(frame, x, y, font.size, done, dim, bright);
        } else {
            let wide = frame.measure(&text, font).min(room);
            let x = from + (room - wide) / 2.0;
            let layout = frame.gfx.wrapped(&text, font, wide + 2.0);
            let sung = sung_to(&parts, self.moment.position);
            let width = wide + 2.0;
            // A dark haze for it to be read on anything; a glow in the cover's colour round what is sung.
            frame.glow(Color::hex(0x000000, 0.55 * eased), 3.0, |frame| frame.text_wrapped(&text, font, Color::hex(0xFFFFFF, 1.0), x, y + 1.0, width));
            frame.glow(colour(accent, 0.75 * eased), 10.0, |frame| sung_part(frame, &layout, &text, font, x, y, width, sung, Color::hex(0xFFFFFF, 1.0), 14.0));
            filled(frame, &layout, &text, font, x, y, width, sung, dim, bright, 14.0);
        }
        if with_under {
            let under = self.under(&lines, now);
            if !under.is_empty() {
                let wide = frame.measure(&under, under_font).min(room);
                outlined(frame, &fitted(frame, &under, under_font, room), under_font, Color::hex(0xFFFFFF, 0.78 * eased), from + (room - wide) / 2.0, y + size * 1.45, wide + 2.0);
            }
        }
    }

    /// The player: the cover at the start, what plays beside it, the line sung now (rising in as it comes, filled as
    /// it is sung, wrapped where it is too long) and what is under it, the rows along the bottom.
    fn player(&mut self) {
        let (_, h) = self.size;
        let pad = pad(Layout::Player);
        let at = placed(Layout::Player, self.size, self.shows);
        let (left, width) = (at.left, at.width);
        let free = self.rows(left, h - pad, width, 28);
        let line_top = at.below;
        let bottom = free - 8.0;
        let under_tall = PLAYER_UNDER.size * 1.35;
        if bottom <= line_top {
            return;
        }
        let frame = self.frame;
        frame.clip(left - 4.0, line_top - 4.0, width + 8.0, bottom - line_top + 4.0);
        let (lines, now) = (self.moment.lines.clone(), self.moment.now);
        match (&lines, now) {
            (Some(lines), _) if self.music.turned != 0 => {
                let index = (now.map_or(0, |now| now as i32) + self.music.turned).clamp(0, lines.len() as i32 - 1) as usize;
                self.turned_to(lines, index, left, line_top, width, PLAYER_LINE);
                let under = lines[index].translation.clone().filter(|_| self.shows.translation).unwrap_or_default();
                let under_top = line_top + PLAYER_LINE.size * 1.4 + 2.0;
                if !under.is_empty() && under_top + under_tall <= bottom {
                    words(frame, &fitted(frame, &under, PLAYER_UNDER, width), PLAYER_UNDER, Color::hex(0xFFFFFF, 0.55), left, under_top, width + 2.0, Align::Start, self.shows.clear);
                }
            }
            (Some(lines), Some(now)) => {
                let line = &lines[now];
                let next = lines.get(now + 1).map(|line| line.at);
                let (text, parts) = parts(line, next);
                let eased = self.came();
                let y = line_top + (1.0 - eased) * 10.0;
                let (dim, bright) = self.line_colours(eased);
                let under = self.under(lines, now);
                let tall = if text.trim().is_empty() {
                    interlude(frame, left, y, PLAYER_LINE.size, between(line, next, self.moment.position), dim, bright);
                    PLAYER_LINE.size * 1.4
                } else {
                    // Too long for a row: wrapped, at the largest size (down to that of what is under it) at which
                    // all its rows fit with what is under it; else all its rows alone; else at the least, its rows
                    // as far as they fit.
                    let room = bottom - line_top;
                    let sizes = || (0..).map(|step| PLAYER_LINE.size - step as f32).take_while(|size| *size >= PLAYER_UNDER.size).map(|size| Font { size, ..PLAYER_LINE });
                    let height = |font: Font| frame.gfx.wrapped_height(&text, font, width);
                    let wants_under = if under.is_empty() { 0.0 } else { 2.0 + under_tall };
                    let font = sizes()
                        .find(|font| height(*font) + wants_under <= room)
                        .or_else(|| sizes().find(|font| height(*font) <= room))
                        .unwrap_or(Font { size: PLAYER_UNDER.size, ..PLAYER_LINE });
                    let layout = frame.gfx.wrapped(&text, font, width);
                    let sung = sung_to(&parts, self.moment.position);
                    self.filled(&layout, &text, font, left, y, width, sung, dim, bright, 12.0);
                    height(font)
                };
                // Under it, where there is room, its translation or the next line.
                let under_top = line_top + tall + 2.0;
                if !under.is_empty() && under_top + under_tall <= bottom {
                    words(frame, &fitted(frame, &under, PLAYER_UNDER, width), PLAYER_UNDER, Color::hex(0xFFFFFF, 0.5 * eased), left, under_top, width + 2.0, Align::Start, self.shows.clear);
                }
            }
            // None sung yet: the dots, lit as the music before the first line goes on.
            (Some(lines), None) => {
                let first = lines.first().map_or(0.0, |line| line.at);
                let done = if first > 0.0 { (self.moment.position / first).clamp(0.0, 1.0) as f32 } else { 0.0 };
                let (dim, bright) = self.line_colours(1.0);
                interlude(frame, left, line_top, PLAYER_LINE.size, done, dim, bright);
            }
            // None to be had: why, in their place.
            (None, _) => {
                let said = self.moment.said.clone().unwrap_or_default();
                if self.shows.clear {
                    outlined(frame, &fitted(frame, &said, PLAYER_UNDER, width), PLAYER_UNDER, Color::hex(0xFFFFFF, 0.8), left, line_top + 2.0, width + 2.0);
                } else {
                    frame.text_wrapped(&said, PLAYER_UNDER, Color::hex(0xFFFFFF, 0.62), left, line_top + 2.0, width);
                }
            }
        }
        frame.unclip();
    }

    /// The card (and the card stood on end): what plays at the top, the lines round the one sung now gliding past
    /// it, the rows along the bottom.
    fn card(&mut self, layout: Layout) {
        let (_, h) = self.size;
        let pad = pad(layout);
        let at = placed(layout, self.size, self.shows);
        let (x, width) = (at.left, at.width);
        let free = self.rows(x, h - pad, width, 40);
        let (top, bottom) = (at.below, free - 10.0);
        if bottom <= top + 20.0 {
            return;
        }
        match self.moment.lines.clone() {
            Some(lines) => self.rail_of_lines(&lines, x, top, bottom, width),
            None => {
                let said = self.moment.said.clone().unwrap_or_default();
                let height = self.frame.gfx.wrapped_height(&said, UNDER_FONT, width);
                let y = (top + bottom - height) / 2.0;
                if self.shows.clear {
                    outlined(self.frame, &fitted(self.frame, &said, UNDER_FONT, width), UNDER_FONT, Color::hex(0xFFFFFF, 0.8), x, y, width + 2.0);
                } else {
                    self.frame.text_wrapped(&said, UNDER_FONT, Color::hex(0xFFFFFF, 0.7), x, y, width);
                }
            }
        }
    }

    /// The lines before and after the one aimed at (sung now, or turned to by the wheel), smaller and fainter the
    /// farther they are from it, the rail gliding; the one sung now filled as it is sung, its translation under it;
    /// the one turned to, its time beside it.
    fn rail_of_lines(&mut self, lines: &[Line], x: f32, top: f32, bottom: f32, width: f32) {
        let frame = self.frame;
        let anchor = top + (bottom - top) * 0.38;
        let current = self.moment.now;
        let turned = (self.music.turned != 0).then(|| (current.map_or(0, |now| now as i32) + self.music.turned).clamp(0, lines.len() as i32 - 1) as usize);
        let translated = self.shows.translation;
        // Each line's size and how it shows, by how far it is from the rail.
        let scale_at = |d: f32| {
            let d = d.abs();
            if d <= 1.0 { 1.0 - 0.32 * d } else { (0.68 * 0.9f32.powf(d - 1.0)).clamp(0.5, 0.68) }
        };
        let opacity_at = |d: f32| {
            if d >= 0.0 {
                if d <= 1.0 { 1.0 - 0.28 * d } else { (0.72 - 0.18 * (d - 1.0)).max(0.3) }
            } else {
                let d = -d;
                if d <= 1.0 { 1.0 - 0.48 * d } else { (0.52 - 0.12 * (d - 1.0)).max(0.22) }
            }
        };
        let gap = 10.0;
        let size = (width / 16.0).clamp(18.0, NOW_FONT.size);
        let height_of = |index: usize, scale: f32| {
            let line = &lines[index];
            frame.gfx.wrapped_height(if line.text.trim().is_empty() { "♪" } else { &line.text }, Font { size: size * scale, ..NOW_FONT }, width) + if Some(index) == current && translated && line.translation.is_some() { 20.0 } else { 0.0 }
        };
        let rail = self.music.rail;
        let base = rail.floor().max(-1.0);
        // The lines from a few before the rail to a few after, each placed from the one the rail is nearest.
        let first = (base as isize - 4).max(0) as usize;
        let last = ((base as isize + 7).max(0) as usize).min(lines.len());
        if first >= last {
            return;
        }
        // Their places (the middle of each), laid one after another at their sizes, then shifted so the rail's
        // place is at the anchor.
        let mut middles = Vec::with_capacity(last - first);
        let mut y = 0.0;
        for index in first..last {
            let scale = scale_at(index as f32 - rail);
            let tall = height_of(index, scale);
            middles.push((index, y + tall / 2.0, scale, tall));
            y += tall + gap;
        }
        let at_rail = {
            let below = middles.iter().find(|m| m.0 as f32 >= rail).or(middles.last()).map_or(0.0, |m| m.1);
            let above = middles.iter().rev().find(|m| (m.0 as f32) <= rail).map_or(below, |m| m.1);
            let t = rail - rail.floor();
            if rail < first as f32 { below } else { above + (below - above) * t }
        };
        let (tint, clear) = (self.music.tint, self.shows.clear);
        frame.clip(x - 8.0, top, width + 16.0, bottom - top + 12.0);
        for (index, middle, scale, tall) in middles {
            let d = index as f32 - rail;
            let y = anchor + middle - at_rail - tall / 2.0;
            if y + tall < top - 4.0 || y > bottom + 12.0 {
                continue;
            }
            // Faded toward the rail's edges.
            let edge = ((y - top) / 28.0).clamp(0.0, 1.0).min(((bottom + 12.0 - (y + tall)) / 34.0).clamp(0.0, 1.0));
            let alpha = opacity_at(d) * edge;
            let line = &lines[index];
            let font = Font { size: size * scale, ..NOW_FONT };
            if turned == Some(index) {
                self.turned_to(lines, index, x, y, width, font);
            } else if Some(index) == current {
                let next = lines.get(index + 1).map(|line| line.at);
                let (text, parts) = parts(line, next);
                let bright = colour(toward([1.0, 1.0, 1.0], tint, 0.35), alpha);
                let dim = Color::hex(0xFFFFFF, if clear { 0.55 } else { 0.34 } * alpha);
                if text.trim().is_empty() {
                    let done = between(line, next, self.moment.position);
                    // What has gone glows in the cover's colour.
                    frame.glow(colour(tint, 0.5 * alpha), 6.0, |frame| interlude(frame, x, y, font.size, done, Color::hex(0, 0.0), bright));
                    interlude(frame, x, y, font.size, done, dim, bright);
                } else {
                    let layout = frame.gfx.wrapped(&text, font, width);
                    let sung = sung_to(&parts, self.moment.position);
                    let soft = (font.size * 0.45).clamp(6.0, 16.0);
                    // What is sung glows in the cover's colour.
                    frame.glow(colour(tint, 0.5 * alpha), 6.0, |frame| sung_part(frame, &layout, &text, font, x, y, width, sung, bright, soft));
                    self.filled(&layout, &text, font, x, y, width, sung, dim, bright, soft);
                }
                if let Some(translation) = line.translation.as_ref().filter(|_| translated) {
                    let under = frame.gfx.wrapped_height(&text, font, width);
                    words(frame, &fitted(frame, translation, UNDER_FONT, width), UNDER_FONT, Color::hex(0xFFFFFF, 0.62 * alpha), x, y + under + 2.0, width, Align::Start, clear);
                }
            } else {
                let shown = Color::hex(0xFFFFFF, if clear { 0.7 } else { 0.5 } * alpha);
                if line.text.trim().is_empty() {
                    interlude(frame, x, y, font.size, 0.0, shown, shown);
                } else if clear {
                    outlined(frame, &line.text, font, shown, x, y, width);
                } else {
                    frame.text_wrapped(&line.text, font, shown, x, y, width);
                }
                // Pressed, played from (where the wheel has turned the lines: as the line turned to is).
                if turned.is_some() {
                    self.hits.push((x, y, width, tall, Hit::Line(index as u16)));
                }
            }
        }
        frame.unclip();
    }

    /// How far the line sung now has risen in as it came (0 to 1, eased).
    fn came(&self) -> f32 {
        let came = self.moment.since.map_or(1.0, |since| (since.elapsed().as_secs_f32() / 0.28).min(1.0));
        1.0 - (1.0 - came).powi(3)
    }

    /// The line sung now's colours: what is not sung yet, and what is (near white, the cover's colour in it).
    fn line_colours(&self, eased: f32) -> (Color, Color) {
        let dim = if self.shows.clear { 0.6 } else { 0.36 };
        (Color::hex(0xFFFFFF, dim * eased), colour(toward(self.music.tint, [1.0; 3], 0.65), eased))
    }

    /// What is under the line sung now: its translation (where there is one and they are on), else the next line
    /// (where that is on).
    fn under(&self, lines: &[Line], now: usize) -> String {
        let translation = lines[now].translation.clone().filter(|_| self.shows.translation);
        let next = lines.get(now + 1).filter(|_| self.shows.next).map(|line| line.text.clone());
        translation.or(next).unwrap_or_default()
    }

    /// A line drawn in `dim`, and over it in `bright` as far as it is sung; edged, with no ground.
    #[allow(clippy::too_many_arguments)]
    fn filled(&self, layout: &IDWriteTextLayout, text: &str, font: Font, x: f32, y: f32, width: f32, sung: f32, dim: Color, bright: Color, soft: f32) {
        if self.shows.clear {
            for (dx, dy) in [(0.0, 1.2), (0.8, 0.8), (-0.8, 0.8)] {
                self.frame.text_wrapped(text, font, Color::hex(0x000000, 0.45 * bright.a), x + dx, y + dy, width);
            }
        }
        filled(self.frame, layout, text, font, x, y, width, sung, dim, bright, soft);
    }
}

impl Music {
    /// How far the controls have come in over the spectrum (0 to 1), eased.
    fn controls_shown(&self) -> f32 {
        self.controls_in * self.controls_in * (3.0 - 2.0 * self.controls_in)
    }

    /// The cover behind it, blurred: made once for each cover (the one before fading out over a moment).
    fn backdrop(&mut self, frame: &Frame, cover: &Arc<Cover>) {
        let key = Arc::as_ptr(cover) as usize;
        if self.backdrop.as_ref().is_some_and(|(of, _)| *of == key) {
            return;
        }
        let side = crate::cover::SIDE as usize;
        let pixels = blurred(&cover.pixels, side, 9);
        let properties = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
            ..Default::default()
        };
        if let Ok(bitmap) = unsafe { frame.dc.CreateBitmap(D2D_SIZE_U { width: side as u32, height: side as u32 }, Some(pixels.as_ptr().cast()), side as u32 * 4, &properties) } {
            if let Some((_, before)) = self.backdrop.take() {
                self.fading = Some((before, Instant::now()));
            }
            self.backdrop = Some((key, bitmap));
        }
    }
}

/// How a line is sung, part by part: each part's place in the text drawn (UTF-16, from and to) and its time (from
/// and to, seconds). Word by word where that is known; else each character of Chinese or Japanese, each word of
/// other writing (weighed by its length), sharing nine tenths of the line's time.
fn parts(line: &Line, next: Option<f64>) -> (String, Vec<(usize, usize, f64, f64)>) {
    if !line.words.is_empty() {
        let mut text = String::new();
        let mut parts = Vec::new();
        for word in &line.words {
            let from = text.encode_utf16().count();
            text.push_str(&word.text);
            parts.push((from, text.encode_utf16().count(), word.at, word.end.max(word.at + 0.01)));
        }
        return (text, parts);
    }
    let text = line.text.clone();
    let end = line.end.or(next).unwrap_or(line.at + 4.0).max(line.at + 0.3);
    let span = (end - line.at) * 0.9;
    // The pieces: a character of the scripts written without spaces, a run of anything else up to a space.
    let mut pieces: Vec<(usize, usize, f64)> = Vec::new();
    let mut at = 0;
    let mut run: Option<usize> = None;
    let close = |pieces: &mut Vec<(usize, usize, f64)>, run: &mut Option<usize>, to: usize| {
        if let Some(from) = run.take() {
            pieces.push((from, to, 1.0 + 0.15 * (to - from) as f64));
        }
    };
    for c in text.chars() {
        let units = c.len_utf16();
        let wide = matches!(c as u32, 0x3040..=0x30FF | 0x3400..=0x4DBF | 0x4E00..=0x9FFF | 0xAC00..=0xD7AF | 0xF900..=0xFAFF);
        if wide {
            close(&mut pieces, &mut run, at);
            pieces.push((at, at + units, 1.0));
        } else if c.is_whitespace() {
            close(&mut pieces, &mut run, at);
        } else if run.is_none() {
            run = Some(at);
        }
        at += units;
    }
    close(&mut pieces, &mut run, at);
    let weight: f64 = pieces.iter().map(|p| p.2).sum::<f64>().max(1e-6);
    let mut t = line.at;
    let parts = pieces
        .into_iter()
        .map(|(from, to, w)| {
            let took = span * w / weight;
            let part = (from, to, t, t + took);
            t += took;
            part
        })
        .collect();
    (text, parts)
}

/// How much of the text drawn is sung at `t`: its place, in UTF-16 units, fractional within the part being sung.
fn sung_to(parts: &[(usize, usize, f64, f64)], t: f64) -> f32 {
    let mut done = 0.0;
    for (from, to, at, end) in parts {
        if t < *at {
            break;
        }
        done = if t >= *end { *to as f32 } else { *from as f32 + (*to - *from) as f32 * ((t - at) / (end - at)) as f32 };
    }
    done
}

/// How far through it is (0 to 1) at `t`: a line without words (the music between), from its start to its end or
/// the next line's.
fn between(line: &Line, next: Option<f64>, t: f64) -> f32 {
    let end = line.end.or(next).unwrap_or(line.at + 4.0).max(line.at + 0.3);
    ((t - line.at) / (end - line.at)).clamp(0.0, 1.0) as f32
}

/// The music between lines, where a line of words `size` DIPs tall would be (its box's corner at `(x, y)`): three
/// dots in `dim`, each coming to `bright` (and a little larger) as its third of it goes (`done`, 0 to 1): how many
/// are lit, how much of it has gone.
fn interlude(frame: &Frame, x: f32, y: f32, size: f32, done: f32, dim: Color, bright: Color) {
    let radius = size * INTERLUDE_DOT;
    let centre_y = y + size * 0.68;
    for index in 0..3 {
        let lit = (done * 3.0 - index as f32).clamp(0.0, 1.0);
        let lit = lit * lit * (3.0 - 2.0 * lit);
        let color = Color { r: dim.r + (bright.r - dim.r) * lit, g: dim.g + (bright.g - dim.g) * lit, b: dim.b + (bright.b - dim.b) * lit, a: dim.a + (bright.a - dim.a) * lit };
        if color.a > 0.0 {
            frame.fill_circle(color, Point { x: x + radius * 1.25 + index as f32 * size * INTERLUDE_STEP, y: centre_y }, radius * (1.0 + 0.25 * lit));
        }
    }
}

/// A line drawn in `dim`, and over it in `bright` as far as it is sung (see `sung_part`).
#[allow(clippy::too_many_arguments)]
fn filled(frame: &Frame, layout: &IDWriteTextLayout, text: &str, font: Font, x: f32, y: f32, width: f32, sung: f32, dim: Color, bright: Color, soft: f32) {
    frame.text_wrapped(text, font, dim, x, y, width);
    sung_part(frame, layout, text, font, x, y, width, sung, bright, soft);
}

/// As much of a line as is sung (`sung`, UTF-16 units), in `bright`, its edge soft: row by row as `layout` breaks
/// it, the row being sung up to the place sung, the rows before it whole.
#[allow(clippy::too_many_arguments)]
fn sung_part(frame: &Frame, layout: &IDWriteTextLayout, text: &str, font: Font, x: f32, y: f32, width: f32, sung: f32, bright: Color, soft: f32) {
    if sung <= 0.0 {
        return;
    }
    let mut count = 0;
    let _ = unsafe { layout.GetLineMetrics(None, &mut count) };
    let mut rows = vec![DWRITE_LINE_METRICS::default(); count as usize];
    let _ = unsafe { layout.GetLineMetrics(Some(&mut rows), &mut count) };
    // Where the place sung is: its row's top and the x it is at (between the characters about it).
    let place = |unit: usize| {
        let (mut px, mut py, mut metrics) = (0.0, 0.0, DWRITE_HIT_TEST_METRICS::default());
        let _ = unsafe { layout.HitTestTextPosition(unit as u32, false, &mut px, &mut py, &mut metrics) };
        (px, metrics.top)
    };
    let whole = sung.floor() as usize;
    let (x0, top) = place(whole);
    let (x1, top1) = place(whole + 1);
    let at = if (top1 - top).abs() < 0.5 { x0 + (x1 - x0) * sung.fract() } else { x0 };
    let mut row_top = 0.0;
    for row in &rows {
        let height = row.height;
        if row_top + height <= top + 0.5 {
            frame.clip(x - 4.0, y + row_top, width + 8.0, height);
            frame.text_wrapped(text, font, bright, x, y, width);
            frame.unclip();
        } else if row_top <= top + 0.5 {
            // The row being sung: whole to the place, then fading over the soft edge, in steps.
            frame.clip(x - 4.0, y + row_top, at + 4.0, height);
            frame.text_wrapped(text, font, bright, x, y, width);
            frame.unclip();
            for step in 0..4 {
                let from = at + soft * step as f32 / 4.0;
                frame.clip(x + from, y + row_top, soft / 4.0 + 0.5, height);
                frame.text_wrapped(text, font, bright.alpha(1.0 - (step as f32 + 0.5) / 4.0), x, y, width);
                frame.unclip();
            }
            break;
        }
        row_top += height;
    }
}

/// The spectrum moving with the music: `bars` of them across `width` from `x`, standing on `floor`, `tall` at most,
/// shaded from the accent to the tint, `alpha` of it shown.
fn spectrum(frame: &Frame, moment: &Moment, (accent, tint): ([f32; 3], [f32; 3]), (x, floor, width, tall): (f32, f32, f32, f32), bars: usize, alpha: f32) {
    let bar = width / bars as f32;
    for index in 0..bars {
        let at = index as f32 / (bars - 1) as f32 * (crate::sound::BANDS - 1) as f32;
        let (low, frac) = (at.floor() as usize, at.fract());
        let value = moment.sound.bands[low] * (1.0 - frac) + moment.sound.bands[(low + 1).min(crate::sound::BANDS - 1)] * frac;
        let height = 2.0 + value.powf(1.6) * (tall - 2.0);
        let shade = toward(accent, tint, index as f32 / bars as f32);
        frame.fill_rounded(colour(shade, (0.3 + 0.45 * value) * alpha), x + index as f32 * bar + bar * 0.2, floor - height, bar * 0.6, height, bar * 0.3);
    }
}

/// The ground: its shadow, the cover blurred behind (the one before fading out) and darkened, the cover's colour
/// washed from a corner; all inside its rounded corners, with what `on` draws. With no ground (`clear`), only what
/// `on` draws.
struct Ground {
    size: (f32, f32),
    radius: f32,
    backdrop: Option<ID2D1Bitmap1>,
    before: Option<ID2D1Bitmap1>,
    shown: f32,
    accent: [f32; 3],
    tint: [f32; 3],
    clear: bool,
}

impl Ground {
    fn paint(&self, frame: &Frame, on: impl FnOnce(&Frame)) -> windows::core::Result<()> {
        if self.clear {
            on(frame);
            return Ok(());
        }
        let ((w, h), radius) = (self.size, self.radius);
        frame.shadow(Color::hex(0x000000, 0.4), 0.0, 0.0, w, h, radius, 8.0, 4.0);
        let mask = unsafe { frame.gfx.factory.CreateRoundedRectangleGeometry(&D2D1_ROUNDED_RECT { rect: rect(0.0, 0.0, w, h), radiusX: radius, radiusY: radius }) }?;
        let layer = D2D1_LAYER_PARAMETERS1 {
            contentBounds: rect(0.0, 0.0, w, h),
            geometricMask: std::mem::ManuallyDrop::new(mask.cast().ok()),
            maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
            maskTransform: windows_numerics::Matrix3x2::identity(),
            opacity: 1.0,
            ..Default::default()
        };
        unsafe { frame.dc.PushLayer(&layer, None) };
        frame.fill(Color::hex(0x101014, 1.0), 0.0, 0.0, w, h);
        let grown = 1.35;
        let (bw, bh) = (w.max(h) * grown, w.max(h) * grown);
        let place = rect((w - bw) / 2.0, (h - bh) / 2.0, bw, bh);
        if let Some(bitmap) = &self.backdrop {
            if let Some(before) = &self.before {
                unsafe { frame.dc.DrawBitmap(before, Some(&place), 1.0 - self.shown, D2D1_INTERPOLATION_MODE_LINEAR, None, None) };
            }
            unsafe { frame.dc.DrawBitmap(bitmap, Some(&place), self.shown, D2D1_INTERPOLATION_MODE_LINEAR, None, None) };
        }
        // Darkened for the words to be read on, the cover's colour washed from a corner.
        frame.fill(Color::hex(0x000000, 0.42), 0.0, 0.0, w, h);
        frame.fill_shape(&[Point { x: 0.0, y: 0.0 }, Point { x: w, y: 0.0 }, Point { x: w, y: h }, Point { x: 0.0, y: h }], Fill::Down { top: 0.0, from: colour(self.accent, 0.12), bottom: h, to: colour(self.tint, 0.0) });
        on(frame);
        unsafe { frame.dc.PopLayer() };
        drop(std::mem::ManuallyDrop::into_inner(layer.geometricMask));
        Ok(())
    }
}

/// `colour` as a canvas colour, as opaque as `alpha`.
fn colour([r, g, b]: [f32; 3], alpha: f32) -> Color {
    Color { r, g, b, a: alpha }
}

/// `a` eased toward `b` by `t`.
fn toward(a: [f32; 3], b: [f32; 3], t: f32) -> [f32; 3] {
    [a[0] + (b[0] - a[0]) * t, a[1] + (b[1] - a[1]) * t, a[2] + (b[2] - a[2]) * t]
}

/// A colour light enough to read on the dark (its hue kept; its lightness raised to at least `least`).
fn readable(rgb: [f32; 3], least: f32) -> [f32; 3] {
    let (h, s, l) = crate::cover::hsl(rgb);
    crate::cover::rgb(h, s.min(0.85), l.max(least))
}

/// A cover's pixels blurred (three box blurs, of `radius` pixels, across then down), for behind it.
fn blurred(pixels: &[u8], side: usize, radius: usize) -> Vec<u8> {
    let mut image: Vec<f32> = pixels.iter().map(|&b| b as f32).collect();
    let mut other = image.clone();
    for _ in 0..3 {
        box_pass(&image, &mut other, side, radius, true);
        box_pass(&other, &mut image, side, radius, false);
    }
    image.into_iter().map(|v| v.round().clamp(0.0, 255.0) as u8).collect()
}

/// One box blur of a square BGRA image, across its rows or down its columns (the edges held).
fn box_pass(from: &[f32], to: &mut [f32], side: usize, radius: usize, across: bool) {
    let at = |line: usize, i: usize| if across { (line * side + i) * 4 } else { (i * side + line) * 4 };
    let span = (2 * radius + 1) as f32;
    for line in 0..side {
        for channel in 0..4 {
            let mut sum: f32 = (0..=2 * radius).map(|k| from[at(line, (k as isize - radius as isize).clamp(0, side as isize - 1) as usize) + channel]).sum();
            for i in 0..side {
                to[at(line, i) + channel] = sum / span;
                let out = (i as isize - radius as isize).clamp(0, side as isize - 1) as usize;
                let into = (i + radius + 1).min(side - 1);
                sum += from[at(line, into) + channel] - from[at(line, out) + channel];
            }
        }
    }
}

/// `text`, or as much of its start as is no wider than `room` with an ellipsis after it.
fn fitted(frame: &Frame, text: &str, font: Font, room: f32) -> String {
    if frame.measure(text, font) <= room {
        return text.to_string();
    }
    let mut kept: Vec<char> = text.chars().collect();
    loop {
        kept.pop();
        let cut = format!("{}…", kept.iter().collect::<String>().trim_end());
        if kept.is_empty() || frame.measure(&cut, font) <= room {
            return cut;
        }
    }
}

/// Words, edged where there is no ground (`clear`) to be read on whatever is behind.
#[allow(clippy::too_many_arguments)]
fn words(frame: &Frame, text: &str, font: Font, colour: Color, x: f32, y: f32, width: f32, align: Align, clear: bool) {
    if clear {
        for (dx, dy) in [(0.0, 1.2), (0.8, 0.8), (-0.8, 0.8)] {
            frame.text(text, font, Color::hex(0x000000, 0.45 * colour.a), x + dx, y + dy, width, align);
        }
    }
    frame.text(text, font, colour, x, y, width, align);
}

/// Words with a dark edge under them, to be read on whatever is behind.
fn outlined(frame: &Frame, text: &str, font: Font, colour: Color, x: f32, y: f32, width: f32) {
    words(frame, text, font, colour, x, y, width, Align::Start, true);
}

#[cfg(test)]
mod tests {
    use super::*;
    use crate::lyrics::Word;

    fn line(text: &str, words: Vec<Word>) -> Line {
        Line { at: 10.0, end: None, text: text.into(), words, backing: None, translation: None }
    }

    #[test]
    fn a_line_sung_part_by_part() {
        // Word by word: as the words say.
        let worded = line("ab cd", vec![Word { at: 10.0, end: 11.0, text: "ab ".into() }, Word { at: 11.0, end: 12.0, text: "cd".into() }]);
        let (text, worded) = parts(&worded, None);
        assert_eq!(text, "ab cd");
        assert_eq!(sung_to(&worded, 10.5), 1.5);
        assert_eq!(sung_to(&worded, 11.5), 4.0);
        assert_eq!(sung_to(&worded, 20.0), 5.0);
        assert_eq!(sung_to(&worded, 9.0), 0.0);
        // By line: each Chinese character a part, the time shared.
        let (_, shared) = parts(&line("你好 hi", Vec::new()), Some(14.0));
        assert_eq!(shared.iter().map(|p| (p.0, p.1)).collect::<Vec<_>>(), [(0, 1), (1, 2), (3, 5)]);
        assert!((shared.last().unwrap().3 - (10.0 + 4.0 * 0.9)).abs() < 1e-9);
    }

    #[test]
    fn the_room_chooses_the_layout() {
        let all = Shows { cover: true, title: true, translation: true, next: true, lines: true, spectrum: true, controls: false, clear: false };
        assert_eq!(Layout::for_room((420.0, 76.0), &all, None), Layout::Strip);
        assert_eq!(Layout::for_room((470.0, 178.0), &all, None), Layout::Player);
        assert_eq!(Layout::for_room((470.0, 400.0), &all, None), Layout::Card);
        assert_eq!(Layout::for_room((290.0, 470.0), &all, None), Layout::Tall);
        // Without the lines, a player however tall.
        assert_eq!(Layout::for_room((470.0, 400.0), &Shows { lines: false, ..all }, None), Layout::Player);
        // Controls kept over the spectrum: a row more before the player.
        assert_eq!(Layout::for_room((470.0, 130.0), &Shows { controls: true, ..all }, None), Layout::Strip);
        // About a threshold, the layout it has is kept.
        assert_eq!(Layout::for_room((470.0, 245.0), &all, Some(Layout::Card)), Layout::Card);
        assert_eq!(Layout::for_room((470.0, 255.0), &all, Some(Layout::Player)), Layout::Player);
        assert_eq!(Layout::for_room((470.0, 270.0), &all, Some(Layout::Player)), Layout::Card);
    }
}

#[cfg(test)]
#[cfg(feature = "studio")]
mod pictures {
    use super::*;
    use crate::lyrics::Word;

    /// A track playing, its lines sung word by word (the second sung now, its third word being sung; or, `between`,
    /// the music between two lines), its cover a warm sky over a dark sea.
    fn playing(between: bool) -> Moment {
        let worded = |at: f64, text: &str| {
            let pieces: Vec<&str> = text.split_inclusive(' ').collect();
            let words = pieces.iter().enumerate().map(|(i, piece)| Word { at: at + i as f64 * 0.4, end: at + (i + 1) as f64 * 0.4, text: piece.to_string() }).collect();
            Line { at, end: Some(at + pieces.len() as f64 * 0.4), text: text.into(), words, backing: None, translation: None }
        };
        let mut lines = vec![
            worded(10.0, "Somewhere far beyond the city lights "),
            worded(14.0, "我们 在 黑夜里 奔跑 "),
            Line { at: 16.0, end: Some(20.0), text: String::new(), words: Vec::new(), backing: None, translation: None },
            worded(20.0, "Every heartbeat sounds like thunder "),
            worded(24.0, "直到 晨光 照亮 归途 "),
            worded(28.0, "And the morning finds us singing "),
        ];
        lines[1].translation = Some("We run through the night".into());
        let now = if between { 2 } else { 1 };
        let position = if between { 17.6 } else { lines[1].words[2].at + 0.2 };
        let mut bands = [0.0; crate::sound::BANDS];
        for (i, band) in bands.iter_mut().enumerate() {
            *band = (0.85 - i as f32 * 0.025 + 0.15 * (i as f32 * 1.7).sin()).clamp(0.0, 1.0);
        }
        let side = crate::cover::SIDE as usize;
        let mut pixels = vec![0u8; side * side * 4];
        for y in 0..side {
            for x in 0..side {
                let (fx, fy) = (x as f32 / side as f32, y as f32 / side as f32);
                let sun = (((fx - 0.62).powi(2) + (fy - 0.4).powi(2)).sqrt() < 0.12) as u8 as f32;
                let [r, g, b] = if fy < 0.6 { [0.95 - 0.3 * fy, 0.45 + 0.2 * fy, 0.35 + 0.1 * fx] } else { [0.08, 0.18 + 0.1 * fx, 0.32] };
                let at = (y * side + x) * 4;
                pixels[at] = ((b * (1.0 - sun) + 0.85 * sun) * 255.0) as u8;
                pixels[at + 1] = ((g * (1.0 - sun) + 0.95 * sun) * 255.0) as u8;
                pixels[at + 2] = ((r * (1.0 - sun) + 1.0 * sun) * 255.0) as u8;
                pixels[at + 3] = 255;
            }
        }
        Moment {
            lines: Some(Arc::new(lines)),
            now: Some(now),
            since: Some(Instant::now() - Duration::from_secs(2)),
            position,
            length: Some((214.0, true)),
            dragged_to: None,
            title: "Night Runners".into(),
            artist: "Some Artist".into(),
            said: None,
            playing: true,
            cover: Some(Arc::new(Cover { pixels: Arc::new(pixels), base: [0.85, 0.5, 0.4], accent: [0.95, 0.6, 0.35] })),
            sound: crate::sound::Heard { level: 0.8, bands, beat: 0.3 },
        }
    }

    /// What plays drawn as a widget draws it (in a layer of its own: see `widget::MusicShown`) at a window's corner as
    /// the compositor's atlas might put it (1, 451, 902 and 1350 pixels down, a third of a DIP apart each at 1.5): drawn the
    /// same, pixel for pixel, at each
    /// (drawn straight onto the surface, its text came out a little differently in some of them, and shook).
    #[test]
    #[ignore]
    fn what_plays_is_drawn_the_same_wherever_in_the_atlas() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let all = Shows { cover: true, title: true, translation: true, next: true, lines: true, spectrum: true, controls: false, clear: false };
        let gfx = crate::ui::gfx::current().unwrap();
        let places = [1u32, 451, 902, 1350];
        let (w, h) = (470.0, 178.0);
        let margin = 34.0;
        let (width, tall) = (((w + 2.0 * margin) * 1.5) as u32, ((h + 2.0 * margin) * 1.5) as u32);
        // (One moment for all, nothing easing from one to the next.)
        let now = Instant::now();
        let pixels = gfx
            .draw_offscreen((width, 1350 + tall + 2), 1.5, |frame| {
                for at in places {
                    let mut music = Music { last_frame: now, ..Music::default() };
                    let moment = playing(false);
                    music.bands = moment.sound.bands;
                    music.rail = 1.0;
                    music.lyrics = moment.lines.as_ref().map(|lines| Arc::as_ptr(lines) as usize);
                    let mut layer = Layer::default();
                    frame.origin(0.0, at as f32 / 1.5);
                    let local = windows_numerics::Matrix3x2::translation(margin, margin);
                    let area = (-24.0, -24.0, w + 48.0, h + 48.0);
                    layer
                        .draw(frame, 1, local, area, 1.5, None, |frame| {
                            music.paint_moment(frame, (w, h), &all, 1.5, None, now, moment);
                            Ok(())
                        })
                        .unwrap();
                }
            })
            .unwrap();
        let region = |at: u32| -> Vec<u8> { (at..at + tall).flat_map(|y| pixels[(y * width * 4) as usize..((y + 1) * width * 4) as usize].to_vec()).collect() };
        let first = region(places[0]);
        for at in places {
            let differing = first.chunks(4).zip(region(at).chunks(4)).filter(|(a, b)| a != b).count();
            println!("at {at}: {differing} pixels differ from at {}", places[0]);
            assert_eq!(differing, 0, "drawn at {at}, it differs from drawn at {}", places[0]);
        }
    }

    /// What plays in each layout, at each size, with its switches as each case says, drawn to PNGs in the target
    /// folder: `cargo test --features studio -- --ignored draws_what_plays --nocapture`.
    #[test]
    #[ignore]
    fn draws_what_plays() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let all = Shows { cover: true, title: true, translation: true, next: true, lines: true, spectrum: true, controls: false, clear: false };
        let floating = Shows { cover: false, title: false, spectrum: false, clear: true, ..all };
        // Name, size, switches, the pointer over it (the controls come), lines turned by the wheel, the track dragged,
        // the music between lines.
        let cases: [(&str, (f32, f32), Shows, bool, i32, bool, bool); 16] = [
            ("strip", (420.0, 76.0), all, false, 0, false, false),
            ("strip-over", (420.0, 76.0), all, true, 0, false, false),
            ("strip-kept", (480.0, 76.0), Shows { controls: true, ..all }, false, 0, false, false),
            ("player", (470.0, 178.0), all, false, 0, false, false),
            ("player-over", (470.0, 178.0), all, true, 0, false, false),
            ("player-dragged", (470.0, 178.0), all, true, 0, true, false),
            ("player-kept", (470.0, 210.0), Shows { controls: true, ..all }, false, 0, false, false),
            ("player-nocover", (470.0, 178.0), Shows { cover: false, ..all }, false, 0, false, false),
            ("player-turned", (470.0, 178.0), all, false, 2, false, false),
            ("player-between", (470.0, 178.0), all, false, 0, false, true),
            ("card", (470.0, 400.0), all, false, 0, false, false),
            ("card-turned", (470.0, 400.0), all, false, 2, false, false),
            ("tall", (290.0, 480.0), all, false, 0, false, false),
            ("clear-player", (470.0, 178.0), Shows { clear: true, ..all }, false, 0, false, false),
            ("floating", (760.0, 110.0), floating, false, 0, false, false),
            ("floating-over", (760.0, 110.0), floating, true, 0, false, false),
        ];
        let out = format!("{}/target/music", env!("CARGO_MANIFEST_DIR"));
        std::fs::create_dir_all(&out).unwrap();
        let gfx = crate::ui::gfx::current().unwrap();
        for (name, (w, h), shows, over, turned, dragged, between) in cases {
            let mut music = Music { controls_in: if over || shows.controls { 1.0 } else { 0.0 }, turned, turned_at: Instant::now(), ..Music::default() };
            let mut moment = playing(between);
            moment.dragged_to = dragged.then_some(150.0);
            music.bands = moment.sound.bands;
            // (The rail at the line aimed at, as it would be once glided there.)
            music.rail = moment.now.map_or(-1.0, |now| now as f32) + turned as f32;
            music.lyrics = moment.lines.as_ref().map(|lines| Arc::as_ptr(lines) as usize);
            let margin = 30.0;
            let size = (((w + 2.0 * margin) * 1.5) as u32, ((h + 2.0 * margin) * 1.5) as u32);
            let pointer = over.then_some((w * 0.5, h * 0.5));
            let pixels = gfx
                .draw_offscreen(size, 1.5, |frame| {
                    // (On a desktop of a mid grey.)
                    frame.fill(Color::hex(0x5A6470, 1.0), 0.0, 0.0, w + 2.0 * margin, h + 2.0 * margin);
                    frame.origin(margin, margin);
                    music.paint_moment(frame, (w, h), &shows, 1.5, pointer, Instant::now(), moment);
                })
                .unwrap();
            let path = std::path::PathBuf::from(format!("{out}/{name}.png"));
            crate::studio::save_png(&path, size.0, size.1, &pixels).unwrap();
            println!("{}", path.display());
        }
    }
}
