//! What the panel is drawn with, on any platform: colours, fonts, and the
//! canvas each platform paints them on (Direct2D on Windows).

/// A colour as a CSS-style straight (not premultiplied) RGBA.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Color {
    pub r: f32,
    pub g: f32,
    pub b: f32,
    pub a: f32,
}

impl Color {
    pub const CLEAR: Color = Color { r: 0.0, g: 0.0, b: 0.0, a: 0.0 };

    pub const fn hex(rgb: u32, a: f32) -> Self {
        Color {
            r: ((rgb >> 16) & 0xFF) as f32 / 255.0,
            g: ((rgb >> 8) & 0xFF) as f32 / 255.0,
            b: (rgb & 0xFF) as f32 / 255.0,
            a,
        }
    }

    pub fn alpha(self, a: f32) -> Self {
        Color { a: self.a * a, ..self }
    }

}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum Family {
    Archivo,
    Inter,
    Segoe,
    SegoeDisplay,
    /// The system's icon font: Segoe Fluent Icons on Windows 11, Segoe MDL2
    /// Assets before it; both put the same glyphs at the same code points.
    Icons,
    /// Microsoft YaHei UI (the system's, Chinese with Latin): the music full screen's lines, till the serif face
    /// they are drawn in is had.
    YaHei,
    /// Noto Serif SC Black: the music full screen's lines, once taken from Glance's site (see `serif`). Asked for
    /// only once it is had.
    Serif,
}

/// A text style: face, size in DIPs, weight (100–900), width (% of normal)
/// and letter spacing (em).
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Font {
    pub family: Family,
    pub size: f32,
    pub weight: f32,
    pub width: f32,
    pub tracking: f32,
}

impl Font {
    pub const fn new(family: Family, size: f32, weight: f32) -> Self {
        Font { family, size, weight, width: 100.0, tracking: 0.0 }
    }

    pub const fn width(self, width: f32) -> Self {
        Font { width, ..self }
    }

    pub const fn tracking(self, tracking: f32) -> Self {
        Font { tracking, ..self }
    }

    pub(super) fn key(&self) -> FontKey {
        (self.family, self.size.to_bits(), self.weight.to_bits(), self.width.to_bits(), self.tracking.to_bits())
    }
}

pub(super) type FontKey = (Family, u32, u32, u32, u32);

#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Align {
    Start,
    End,
}

/// A point, in DIPs.
#[derive(Clone, Copy, Debug, PartialEq)]
pub struct Point {
    pub x: f32,
    pub y: f32,
}

/// How a shape is filled: one colour, or a colour at `top` fading to another
/// at `bottom` (both heights in DIPs).
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Fill {
    Solid(Color),
    Down { top: f32, from: Color, bottom: f32, to: Color },
}

/// The part of `whole` (a box already on whole pixels, as `on_pixels` puts it) that `fraction` of it fills, from its
/// bottom (`up`) or its left: as many whole pixels as that comes to (as `on_pixels` makes it), ending where the box
/// does at the side it fills from (a part put on pixels alone ended a pixel off the box's own edge, and the box's
/// colour showed there). None of it for a fraction of nothing (or not a number).
pub fn filled_part(whole: (f32, f32, f32, f32), fraction: f32, up: bool, on_pixels: impl Fn(f32, f32, f32, f32) -> (f32, f32, f32, f32)) -> (f32, f32, f32, f32) {
    let (x, y, w, h) = whole;
    let fraction = if fraction.is_nan() { 0.0 } else { fraction.clamp(0.0, 1.0) };
    if up {
        let filled = if fraction > 0.0 { on_pixels(x, y, w, h * fraction).3.min(h) } else { 0.0 };
        (x, y + h - filled, w, filled)
    } else {
        let filled = if fraction > 0.0 { on_pixels(x, y, w * fraction, h).2.min(w) } else { 0.0 };
        (x, y, filled, h)
    }
}

/// A picture to draw (a track's cover): its pixels, premultiplied BGRA rows, `side` square and opaque; known by
/// `key` (the same pixels, the same key: what is made of them on the card is made once).
#[derive(Clone, Debug)]
pub struct Picture {
    pub key: u64,
    pub side: u32,
    pub pixels: std::sync::Arc<Vec<u8>>,
}

impl PartialEq for Picture {
    fn eq(&self, other: &Self) -> bool {
        self.key == other.key
    }
}

/// What the panel's readings are painted with: each platform paints these
/// with its own graphics (Direct2D and DirectWrite on Windows). Coordinates
/// are in DIPs; text is placed by the top of its line box.
pub trait Canvas {
    /// `text` on one line from (`x`, `y`), at the start or end of `width`,
    /// cut short with an ellipsis if it does not fit.
    #[allow(clippy::too_many_arguments)]
    fn text(&self, text: &str, font: Font, color: Color, x: f32, y: f32, width: f32, align: Align);
    /// `text` laid out in `font` and drawn `scale` times as large about
    /// (`x`, `y`), in `width` as drawn.
    #[allow(clippy::too_many_arguments)]
    fn text_scaled(&self, text: &str, font: Font, color: Color, x: f32, y: f32, width: f32, align: Align, scale: f32) {
        self.text(text, Font { size: font.size * scale, ..font }, color, x, y, width, align);
    }
    /// How wide `text` is in `font`.
    fn measure(&self, text: &str, font: Font) -> f32;
    /// The ascent and descent of `font`'s line box.
    fn baseline(&self, font: Font) -> (f32, f32);
    /// How far below the top of its line box `text`'s ink starts and ends:
    /// where its glyphs, from whichever face draws them, actually are.
    fn ink(&self, text: &str, font: Font) -> (f32, f32);
    /// Draws only inside the rectangle until `unclip`; clips nest.
    fn clip(&self, x: f32, y: f32, width: f32, height: f32);
    fn unclip(&self);
    /// The rectangle as a fill of it comes out on whole pixels (see `Frame::on_pixels`): for a control drawn in parts
    /// (a track and what fills it, a bar standing on a floor) to cut its parts out of one box, not each put on pixels
    /// alone, which left them a pixel apart. As it is, where nothing is put on pixels.
    fn on_pixels(&self, x: f32, y: f32, width: f32, height: f32) -> (f32, f32, f32, f32) {
        (x, y, width, height)
    }
    /// Clips to the part of the box `(x, y, width, height)` that `fraction` of it fills, from its bottom (`up`) or its
    /// left: the box put on whole pixels once, the part as many whole pixels as that comes to, ending where the box
    /// does (see `filled_part`). (One call, not the box worked out by the caller: a recording of it, played back as a
    /// layout turns into another, works it out on the pixels it is played on.)
    fn clip_part(&self, x: f32, y: f32, width: f32, height: f32, fraction: f32, up: bool) {
        let whole = self.on_pixels(x, y, width, height);
        let part = filled_part(whole, fraction, up, |x, y, w, h| self.on_pixels(x, y, w, h));
        self.clip(part.0, part.1, part.2, part.3);
    }
    fn fill(&self, color: Color, x: f32, y: f32, width: f32, height: f32);
    fn fill_rounded(&self, color: Color, x: f32, y: f32, width: f32, height: f32, radius: f32);
    fn fill_circle(&self, color: Color, centre: Point, radius: f32);
    /// The line through `points`, `width` DIPs wide.
    fn stroke(&self, points: &[Point], color: Color, width: f32);
    /// The same, dashed.
    fn stroke_dashed(&self, points: &[Point], color: Color, width: f32);
    /// The shape `points` outline, closed.
    fn fill_shape(&self, points: &[Point], fill: Fill);
    /// The outline of the rounded rectangle, a line `line` DIPs wide
    /// centred on it.
    #[allow(clippy::too_many_arguments)]
    fn stroke_rounded(&self, fill: Fill, x: f32, y: f32, width: f32, height: f32, radius: f32, line: f32);
    /// The shadow the rounded rectangle casts, `drop` DIPs below it and
    /// blurred by a deviation of `blur` DIPs: outside the rectangle only, so
    /// that what is drawn in it shows through clear.
    #[allow(clippy::too_many_arguments)]
    fn shadow(&self, color: Color, x: f32, y: f32, width: f32, height: f32, radius: f32, blur: f32, drop: f32);
    /// An arc of the circle `radius` round `centre`, from `from` (radians
    /// clockwise from twelve o'clock) through `sweep` more, `width` wide,
    /// its ends round. A sweep of a whole turn or more is the circle.
    fn arc(&self, centre: Point, radius: f32, from: f32, sweep: f32, color: Color, width: f32);
    /// `icon` stroked in `color`, `size` DIPs square, centred on `centre`.
    fn icon(&self, icon: super::icons::Icon, centre: Point, size: f32, color: Color);

    /// `picture` drawn into the square `size` wide from (`x`, `y`), its corners rounded by `radius`, `opacity` of it.
    #[allow(clippy::too_many_arguments)]
    fn picture(&self, picture: &Picture, x: f32, y: f32, size: f32, radius: f32, opacity: f32);
    /// Names what is drawn from here on (`""`: nothing), for a canvas that
    /// tells one drawing's parts from another's (see `morph`).
    fn key(&self, _key: &str) {}
}
