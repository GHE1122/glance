//! The phone app's first screen, drawn for the settings' phone remote page:
//! this PC as the phone pictures it (its case, every part a block in it,
//! seen from one corner), with what the phone would be told of it now, and
//! the first of what the phone may ask. A picture only: nothing in it is
//! pressed.
//!
//! The case is drawn as the app and the remote's web page draw it (see
//! `remote/src/web.html`): the same blocks, the same corner.

use serde_json::Value;
use windows::core::Interface;
use windows::Win32::Graphics::Direct2D::Common::{D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_CLOSED, D2D1_FIGURE_END_OPEN};
use windows::Win32::Graphics::Direct2D::{
    ID2D1Geometry, ID2D1StrokeStyle, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_CAP_STYLE_ROUND, D2D1_ELLIPSE, D2D1_LAYER_OPTIONS1_NONE, D2D1_LAYER_PARAMETERS1, D2D1_LINE_JOIN_ROUND, D2D1_QUADRATIC_BEZIER_SEGMENT,
    D2D1_ROUNDED_RECT, D2D1_STROKE_STYLE_PROPERTIES1,
};
use windows_numerics::{Matrix3x2, Vector2};

use super::canvas::{Align, Color, Family, Font};
use super::gfx::{rect, Frame};
use super::text::Lang;
use super::view::Rect;

/// A phone's screen, as wide and as tall as the app lays itself out for (dp).
pub const SCREEN: (f32, f32) = (360.0, 760.0);
/// The case: how wide, deep and tall, and how round its corners.
const CASE: (f32, f32, f32) = (56.0, 124.0, 128.0);
const ROUND: f32 = 9.0;
/// The room the case is drawn in, as the app has it: where it starts, and its size.
const ROOM: (f32, f32, f32, f32) = (-190.0, -148.0, 320.0, 254.0);
/// The heat at which a part is drawn amber, and red (°C).
const WARM: i64 = 62;
const HOT: i64 = 75;
/// How many turns of a fan one turn of its picture stands for.
const SLOWED: f32 = 16.0;

const PAGE: Color = Color::hex(0x0E1116, 1.0);
const CARD: Color = Color::hex(0x171B22, 1.0);
const TILE: Color = Color::hex(0x1F242C, 1.0);
const INK: Color = Color::hex(0xF2F4F7, 1.0);
const DIM: Color = Color::hex(0x98A2AE, 1.0);
const FAINT: Color = Color::hex(0x5B6470, 1.0);
const GOOD: Color = Color::hex(0x4CC38A, 1.0);
const ACCENT: Color = Color::hex(0xFF7A45, 1.0);
const LIT: Color = Color::hex(0x2E211D, 1.0);
const STEEL: Color = Color::hex(0x8EA2C0, 1.0);

type Point = [f32; 3];
/// A figure of a path: where it starts, its curves (each by a corner, to
/// an end), and whether it closes.
type Figure = (Vector2, Vec<(Vector2, Vector2)>, bool);

/// A point of the case as it is seen: x to the right and down, y to the
/// left and down, z up.
fn seen(point: Point) -> Vector2 {
    let lean = (std::f32::consts::PI / 6.0).cos();
    Vector2 { X: (point[0] - point[1]) * lean, Y: (point[0] + point[1]) * 0.5 - point[2] }
}

/// What lays a flat drawing on a face of the case: from its corner `o`,
/// across `u` and along `v`.
fn face(o: Point, u: Point, v: Point) -> Matrix3x2 {
    let (across, along, corner) = (seen(u), seen(v), seen(o));
    Matrix3x2 { M11: across.X, M12: across.Y, M21: along.X, M22: along.Y, M31: corner.X, M32: corner.Y }
}

/// `color`, `percent` of it over the card's own ground.
fn shade(color: Color, percent: f32) -> Color {
    let k = percent / 100.0;
    Color { r: CARD.r + (color.r - CARD.r) * k, g: CARD.g + (color.g - CARD.g) * k, b: CARD.b + (color.b - CARD.b) * k, a: 1.0 }
}

/// What the phone is told of a part: how warm it is, and how busy.
fn read(reads: &Value, part: &str) -> (Option<i64>, Option<i64>) {
    let figure = |name: &str| reads.get(part).and_then(|part| part.get(name)).and_then(Value::as_i64);
    (figure("temp"), figure("load"))
}

/// A part's one figure: how warm it is; where that is not known, how busy.
fn figure((temp, load): (Option<i64>, Option<i64>)) -> String {
    temp.map(|temp| format!("{temp}°")).or(load.map(|load| format!("{load}%"))).unwrap_or_else(|| "—".into())
}

/// The picture being drawn: where, and with what.
struct Picture<'a> {
    frame: &'a Frame<'a>,
    /// From the screen's own measure (dp) to the window's.
    screen: Matrix3x2,
    /// From the case's room to the screen's measure.
    room: Matrix3x2,
    round: ID2D1StrokeStyle,
}

impl Picture<'_> {
    /// A path through `figures` (each a start, then curves by a corner and
    /// an end; closed or left open), as the case's room has it.
    fn path(&self, figures: &[Figure], through: Matrix3x2) -> Option<ID2D1Geometry> {
        unsafe {
            let path = self.frame.gfx.factory.CreatePathGeometry().ok()?;
            let sink = path.Open().ok()?;
            for (start, curves, closed) in figures {
                sink.BeginFigure(*start, if *closed { D2D1_FIGURE_BEGIN_FILLED } else { D2D1_FIGURE_BEGIN_HOLLOW });
                for (corner, end) in curves {
                    sink.AddQuadraticBezier(&D2D1_QUADRATIC_BEZIER_SEGMENT { point1: *corner, point2: *end });
                }
                sink.EndFigure(if *closed { D2D1_FIGURE_END_CLOSED } else { D2D1_FIGURE_END_OPEN });
            }
            sink.Close().ok()?;
            self.frame.gfx.factory.CreateTransformedGeometry(&path, &(through * self.room)).ok()?.cast().ok()
        }
    }

    fn draw(&self, shape: Option<ID2D1Geometry>, fill: Option<Color>, stroke: Option<(Color, f32)>) {
        let Some(shape) = shape else { return };
        unsafe {
            if let Some(fill) = fill {
                self.frame.dc.FillGeometry(&shape, self.frame.brush(fill), None);
            }
            if let Some((color, width)) = stroke {
                self.frame.dc.DrawGeometry(&shape, self.frame.brush(color), width, &self.round);
            }
        }
    }

    /// A loop of corners in space, each rounded by `r` in the plane of its
    /// two edges.
    fn ring(&self, corners: &[Point], r: f32) -> Option<ID2D1Geometry> {
        let count = corners.len();
        let toward = |from: Point, to: Point| {
            let along = [to[0] - from[0], to[1] - from[1], to[2] - from[2]];
            let length = (along[0] * along[0] + along[1] * along[1] + along[2] * along[2]).sqrt();
            let k = r.min(length / 2.0) / length;
            seen([from[0] + along[0] * k, from[1] + along[1] * k, from[2] + along[2] * k])
        };
        // Each corner: straight to where its rounding begins, then round it.
        let rounded: Vec<(Vector2, Vector2, Vector2)> = (0..count).map(|at| (toward(corners[at], corners[(at + count - 1) % count]), seen(corners[at]), toward(corners[at], corners[(at + 1) % count]))).collect();
        let mut curves = Vec::new();
        for (at, (begins, corner, ends)) in rounded.iter().enumerate() {
            if at > 0 {
                // (A straight edge, as a curve whose corner is on it.)
                curves.push((*begins, *begins));
            }
            curves.push((*corner, *ends));
        }
        self.path(&[(rounded[0].0, curves, true)], Matrix3x2::identity())
    }

    /// A straight line or several, drawn flat (on a face, through `through`).
    fn lines(&self, lines: &[(Vector2, Vector2)], through: Matrix3x2, color: Color, width: f32) {
        let figures: Vec<_> = lines.iter().map(|(from, to)| (*from, vec![(*to, *to)], false)).collect();
        self.draw(self.path(&figures, through), None, Some((color, width)));
    }

    fn circle(&self, r: f32, through: Matrix3x2) -> Option<ID2D1Geometry> {
        unsafe {
            let circle = self.frame.gfx.factory.CreateEllipseGeometry(&D2D1_ELLIPSE { point: Vector2 { X: 0.0, Y: 0.0 }, radiusX: r, radiusY: r }).ok()?;
            self.frame.gfx.factory.CreateTransformedGeometry(&circle, &(through * self.room)).ok()?.cast().ok()
        }
    }

    fn sheet(&self, size: (f32, f32), r: f32, through: Matrix3x2) -> Option<ID2D1Geometry> {
        unsafe {
            let sheet = self.frame.gfx.factory.CreateRoundedRectangleGeometry(&D2D1_ROUNDED_RECT { rect: rect(0.0, 0.0, size.0, size.1), radiusX: r, radiusY: r }).ok()?;
            self.frame.gfx.factory.CreateTransformedGeometry(&sheet, &(through * self.room)).ok()?.cast().ok()
        }
    }

    /// A block with soft corners: what it hides first, then its side
    /// towards x, its side towards y and its top.
    fn block(&self, from: Point, to: Point, r: f32, color: Color) {
        let ([x0, y0, z0], [x1, y1, z1]) = (from, to);
        let behind: Vec<Vector2> = [[x0, y0, z1], [x1, y0, z1], [x1, y0, z0], [x1, y1, z0], [x0, y1, z0], [x0, y1, z1]].map(seen).to_vec();
        self.draw(self.path(&[(behind[0], behind[1..].iter().map(|corner| (*corner, *corner)).collect(), true)], Matrix3x2::identity()), Some(shade(color, 8.0)), None);
        for (side, percent) in [([[x1, y0, z0], [x1, y1, z0], [x1, y1, z1], [x1, y0, z1]], 10.0), ([[x0, y1, z0], [x1, y1, z0], [x1, y1, z1], [x0, y1, z1]], 5.0), ([[x0, y0, z1], [x1, y0, z1], [x1, y1, z1], [x0, y1, z1]], 17.0)] {
            self.draw(self.ring(&side, r), Some(shade(color, percent)), Some((color, 1.0)));
        }
    }

    /// A fan lying in a face: its ring, its blades (turned by `turned`,
    /// radians) and its hub.
    fn fan(&self, centre: Point, u: Point, v: Point, r: f32, color: Color, turned: f32) {
        let through = face(centre, u, v);
        self.draw(self.circle(r, through), Some(shade(color, 6.0)), Some((color, 1.0)));
        let blades: Vec<_> = (0..7)
            .map(|blade| {
                let (sin, cos) = (turned + blade as f32 * std::f32::consts::TAU / 7.0).sin_cos();
                let at = |x: f32, y: f32| Vector2 { X: x * cos - y * sin, Y: x * sin + y * cos };
                (at(r * 0.2, 0.0), vec![(at(r * 0.55, -r * 0.42), at(r * 0.86, -r * 0.1))], false)
            })
            .collect();
        self.draw(self.path(&blades, through), None, Some((color, 1.0)));
        self.draw(self.circle(r * 0.2, through), Some(CARD), Some((color, 1.0)));
    }

    /// Words with their baseline at `y`, from `x` on or (`end`) up to it.
    fn words(&self, text: &str, font: Font, color: Color, x: f32, y: f32, end: bool) {
        let (width, top) = (400.0, y - self.frame.gfx.baseline(font).0);
        if end {
            self.frame.text(text, font, color, x - width, top, width, Align::End);
        } else {
            self.frame.text(text, font, color, x, top, width, Align::Start);
        }
    }

    /// Words with their middle at `x`.
    fn middled(&self, text: &str, font: Font, color: Color, x: f32, y: f32) {
        let width = self.frame.gfx.measure(text, font);
        self.words(text, font, color, x - width / 2.0, y, false);
    }

    fn plate(&self, at: Rect, r: f32, color: Color) {
        unsafe { self.frame.dc.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(at.x, at.y, at.w, at.h), radiusX: r, radiusY: r }, self.frame.brush(color)) };
    }

    /// The case and what is in it, with what is said of each part out
    /// beside it, on a line to it: in a room `width` wide whose corner is
    /// at `(x, y)`.
    fn rig(&mut self, x: f32, y: f32, width: f32, reads: &Value, lang: Lang, seconds: f32) {
        let k = width / ROOM.2;
        self.room = Matrix3x2::scale(k, k) * Matrix3x2::translation(x - ROOM.0 * k, y - ROOM.1 * k);
        let (w, d, h) = CASE;
        let at = |x: f32, y: f32| Vector2 { X: x, Y: y };
        let turn = |rpm: Option<i64>| seconds * rpm.unwrap_or(0).max(0) as f32 / 60.0 / SLOWED * std::f32::consts::TAU;
        let (cpu_fan, case_fan) = (reads.get("cpu_fan").and_then(Value::as_i64), reads.get("case_fan").and_then(Value::as_i64));
        let fans = Color::hex(0x59636F, 1.0);
        // The walls behind: the floor, the back and the far side.
        for wall in [[[0.0, 0.0, 0.0], [w, 0.0, 0.0], [w, d, 0.0], [0.0, d, 0.0]], [[0.0, 0.0, 0.0], [0.0, d, 0.0], [0.0, d, h], [0.0, 0.0, h]], [[0.0, 0.0, 0.0], [w, 0.0, 0.0], [w, 0.0, h], [0.0, 0.0, h]]] {
            self.draw(self.ring(&wall, ROUND), Some(Color::hex(0x12161B, 1.0)), Some((Color::hex(0x3A424D, 1.0), 1.0)));
        }
        // The board on the far side.
        let board = face([0.5, 10.0, 32.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]);
        let traced = Color::hex(0x46505C, 1.0);
        self.draw(self.sheet((86.0, 86.0), 3.0, board), Some(Color::hex(0x161B21, 1.0)), Some((traced, 1.0)));
        self.lines(&[(at(8.0, 12.0), at(78.0, 12.0)), (at(8.0, 20.0), at(78.0, 20.0))], board, traced.alpha(0.55), 1.0);
        self.fan([w / 2.0, 0.6, 98.0], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 16.0, fans, turn(case_fan));
        // The cover over the power supply, and its vents.
        let cover = Color::hex(0x4A5461, 1.0);
        self.block([2.0, 2.0, 2.0], [w - 2.0, d - 2.0, 24.0], 3.0, cover);
        let vents: Vec<_> = (0..9).map(|vent| (at(vent as f32 * 6.0, 0.0), at(vent as f32 * 6.0, 10.0))).collect();
        self.lines(&vents, face([w - 2.0, 18.0, 8.0], [0.0, 1.0, 0.0], [0.0, 0.0, 1.0]), cover.alpha(0.7), 1.0);
        // A part is the case's own steel till it runs warm: then amber, then red.
        let colour = |part: &str| match read(reads, part).0 {
            Some(temp) if temp >= HOT => Color::hex(0xFF6B5E, 1.0),
            Some(temp) if temp >= WARM => Color::hex(0xFFB057, 1.0),
            _ => STEEL,
        };
        let disk = colour("disk");
        self.block([16.0, 86.0, 24.0], [46.0, 114.0, 28.0], 1.6, disk);
        self.draw(self.sheet((22.0, 20.0), 2.0, face([20.0, 90.0, 28.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])), None, Some((disk.alpha(0.5), 1.0)));
        let gpu = colour("gpu");
        self.block([0.5, 10.0, 58.0], [42.0, 102.0, 72.0], 2.4, gpu);
        self.draw(self.sheet((30.0, 76.0), 3.0, face([6.0, 18.0, 72.0], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0])), None, Some((gpu.alpha(0.5), 1.0)));
        let cpu = colour("cpu");
        self.block([0.5, 28.0, 86.0], [22.0, 52.0, 110.0], 2.4, cpu);
        self.fan([11.5, 40.0, 110.3], [1.0, 0.0, 0.0], [0.0, 1.0, 0.0], 8.5, cpu, turn(cpu_fan));
        // The cooler's little display, and how warm it says the processor is.
        let display = face([22.0, 49.0, 107.0], [0.0, -1.0, 0.0], [0.0, 0.0, -1.0]);
        self.draw(self.sheet((18.0, 18.0), 3.5, display), Some(Color::hex(0x0B0E12, 1.0)), Some((cpu, 1.0)));
        if let Some(temp) = read(reads, "cpu").0 {
            self.frame.place(display * self.room * self.screen);
            self.middled(&temp.to_string(), Font::new(Family::Segoe, 8.5, 600.0), cpu, 9.0, 12.4);
            self.frame.place(self.screen);
        }
        let ram = colour("ram");
        for stick in [60.0, 66.0, 72.0, 78.0] {
            self.block([0.5, stick, 84.0], [15.0, stick + 2.4, 116.0], 0.8, ram);
        }
        for z in [38.0, 70.0, 102.0] {
            self.fan([w / 2.0, d - 0.6, z], [1.0, 0.0, 0.0], [0.0, 0.0, 1.0], 15.0, fans, turn(case_fan));
        }
        // The case's own shell, over all: its top, its near side and its front.
        for panel in [[[0.0, 0.0, h], [w, 0.0, h], [w, d, h], [0.0, d, h]], [[w, 0.0, 0.0], [w, d, 0.0], [w, d, h], [w, 0.0, h]], [[0.0, d, 0.0], [w, d, 0.0], [w, d, h], [0.0, d, h]]] {
            self.draw(self.ring(&panel, ROUND), None, Some((Color::hex(0x8F99A6, 1.0), 1.4)));
        }

        // What is said of each part. (In the room's own measure, as the case is.)
        self.frame.place(self.room * self.screen);
        let (large, small) = (Font::new(Family::Segoe, 15.0, 600.0), Font::new(Family::Segoe, 9.5, 400.0));
        let say = |from: Point, to: (f32, f32), end: bool, lines: [&str; 3]| {
            let (from, elbow) = (seen(from), to.0 + if end { 12.0 } else { -12.0 });
            unsafe {
                self.frame.dc.DrawLine(from, at(elbow, to.1), self.frame.brush(FAINT), 0.8, &self.round);
                self.frame.dc.DrawLine(at(elbow, to.1), at(to.0, to.1), self.frame.brush(FAINT), 0.8, &self.round);
                self.frame.dc.FillEllipse(&D2D1_ELLIPSE { point: from, radiusX: 2.0, radiusY: 2.0 }, self.frame.brush(INK));
            }
            let x = to.0 + if end { -5.0 } else { 5.0 };
            self.words(lines[0], large, INK, x, to.1 + 1.0, end);
            self.words(lines[1], small, DIM, x, to.1 + 13.0, end);
            self.words(lines[2], small, DIM, x, to.1 + 25.0, end);
        };
        let named = |part: &str, name: &str| match read(reads, part) {
            (Some(_), Some(load)) => format!("{name} · {load}%"),
            _ => name.to_string(),
        };
        let fan = cpu_fan.map(|rpm| lang.pick(&format!("风扇 {rpm} 转"), &format!("Fan {rpm} rpm")).to_string()).unwrap_or_default();
        say([22.0, 36.0, 100.0], (66.0, -100.0), false, [&figure(read(reads, "cpu")), &named("cpu", "CPU"), &fan]);
        say([42.0, 36.0, 68.0], (74.0, -30.0), false, [&figure(read(reads, "gpu")), &named("gpu", lang.pick("显卡", "Graphics")), ""]);
        say([40.0, 98.0, 28.0], (74.0, 62.0), false, [&figure(read(reads, "disk")), &named("disk", lang.pick("硬盘", "Disk")), ""]);
        say([15.0, 70.0, 116.0], (-122.0, -124.0), true, [&figure(read(reads, "ram")), &named("ram", lang.pick("内存", "Memory")), ""]);
        if let Some(rpm) = case_fan {
            say([w / 2.0, d, 38.0], (-134.0, 64.0), true, [&rpm.to_string(), lang.pick("机箱风扇", "Case fans"), lang.pick("转/分", "rpm")]);
        }
        self.frame.place(self.screen);
    }
}

/// Draws the app's first screen in `at` (a phone's screen: `SCREEN`, to
/// whatever size), for the PC called `name` and what the phone would be
/// told of it now (`reads`: see `remote::figures`), `seconds` into the
/// fans' turning. Whether anything in it turns: it is then to be drawn
/// again soon.
pub fn paint(frame: &Frame, at: Rect, lang: Lang, name: &str, reads: &Value, seconds: f32) -> bool {
    let k = at.w / SCREEN.0;
    let screen = Matrix3x2::scale(k, k) * Matrix3x2::translation(at.x, at.y);
    let Ok(round) = (unsafe { frame.gfx.factory.CreateStrokeStyle(&D2D1_STROKE_STYLE_PROPERTIES1 { startCap: D2D1_CAP_STYLE_ROUND, endCap: D2D1_CAP_STYLE_ROUND, lineJoin: D2D1_LINE_JOIN_ROUND, ..Default::default() }, None) }) else { return false };
    let mut picture = Picture { frame, screen, room: Matrix3x2::identity(), round: round.into() };
    // Whatever runs past the screen's end is not drawn: a page that goes on below.
    let Ok(glass) = (unsafe { frame.gfx.factory.CreateRoundedRectangleGeometry(&D2D1_ROUNDED_RECT { rect: rect(0.0, 0.0, SCREEN.0, at.h / k), radiusX: 26.0, radiusY: 26.0 }) }) else { return false };
    frame.place(screen);
    let layer = D2D1_LAYER_PARAMETERS1 {
        contentBounds: rect(0.0, 0.0, SCREEN.0, at.h / k),
        geometricMask: std::mem::ManuallyDrop::new(glass.cast().ok()),
        maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
        maskTransform: Matrix3x2::identity(),
        opacity: 1.0,
        opacityBrush: std::mem::ManuallyDrop::new(None),
        layerOptions: D2D1_LAYER_OPTIONS1_NONE,
    };
    unsafe { frame.dc.PushLayer(&layer, None) };
    picture.plate(Rect { x: 0.0, y: 0.0, w: SCREEN.0, h: at.h / k }, 0.0, PAGE);

    let (side, wide) = (16.0, SCREEN.0 - 32.0);
    // Whose it is, and that it answers.
    picture.words(name, Font::new(Family::Segoe, 21.0, 600.0), INK, side, 50.0, false);
    let connected = lang.pick("已连接", "Connected");
    let small = Font::new(Family::Segoe, 12.5, 400.0);
    picture.words(connected, small, DIM, SCREEN.0 - side, 48.0, true);
    unsafe { frame.dc.FillEllipse(&D2D1_ELLIPSE { point: Vector2 { X: SCREEN.0 - side - frame.gfx.measure(connected, small) - 11.0, Y: 43.5 }, radiusX: 4.0, radiusY: 4.0 }, frame.brush(GOOD)) };

    // The PC drawn, and its four parts under it.
    let top = 68.0;
    let drawing = (wide - 16.0) * ROOM.3 / ROOM.2;
    picture.plate(Rect { x: side, y: top, w: wide, h: 8.0 + drawing + 8.0 + 52.0 + 12.0 }, 20.0, CARD);
    picture.rig(side + 8.0, top + 8.0, wide - 16.0, reads, lang, seconds);
    let parts = [("cpu", "CPU"), ("gpu", lang.pick("显卡", "Graphics")), ("ram", lang.pick("内存", "Memory")), ("disk", lang.pick("硬盘", "Disk"))];
    let each = (wide - 24.0 - 3.0 * 8.0) / 4.0;
    for (place, (part, called)) in parts.into_iter().enumerate() {
        let (x, y) = (side + 12.0 + place as f32 * (each + 8.0), top + 8.0 + drawing + 8.0);
        picture.plate(Rect { x, y, w: each, h: 52.0 }, 12.0, TILE);
        picture.words(&figure(read(reads, part)), Font::new(Family::Segoe, 15.0, 600.0), INK, x + 10.0, y + 23.0, false);
        let under = match read(reads, part) {
            (Some(_), Some(load)) => format!("{called} {load}%"),
            _ => called.to_string(),
        };
        picture.words(&under, Font::new(Family::Segoe, 10.0, 400.0), DIM, x + 10.0, y + 40.0, false);
    }

    // The first of what the phone may ask: night, the screens, the lights.
    let mut y = top + 8.0 + drawing + 8.0 + 52.0 + 12.0 + 12.0;
    let (title, under) = (Font::new(Family::Segoe, 16.0, 600.0), Font::new(Family::Segoe, 12.0, 400.0));
    picture.plate(Rect { x: side, y, w: wide, h: 72.0 }, 20.0, CARD);
    picture.words(lang.pick("夜间模式", "Night mode"), title, INK, side + 16.0, y + 31.0, false);
    picture.words(lang.pick("电脑继续运行：关屏、关灯、切到节能", "The PC runs on: screens and lights off, saving power"), under, DIM, side + 16.0, y + 53.0, false);
    y += 72.0 + 12.0;
    let half = (wide - 12.0) / 2.0;
    let tiles = [(lang.pick("屏幕", "Screens"), lang.pick("亮着，点一下全部关闭", "On: press to turn all off")), (lang.pick("灯光", "Lights"), lang.pick("开着", "On"))];
    for (place, (called, how)) in tiles.into_iter().enumerate() {
        let x = side + place as f32 * (half + 12.0);
        picture.plate(Rect { x, y, w: half, h: 84.0 }, 20.0, LIT);
        picture.words(called, title, ACCENT, x + 16.0, y + 36.0, false);
        picture.words(how, under, DIM, x + 16.0, y + 58.0, false);
    }
    // The power modes, the middle one chosen; and the page goes on below.
    y += 84.0 + 12.0;
    let label = Font::new(Family::Segoe, 12.5, 400.0);
    picture.plate(Rect { x: side, y, w: wide, h: 92.0 }, 20.0, CARD);
    picture.words(lang.pick("电源模式", "Power mode"), label, DIM, side + 16.0, y + 27.0, false);
    let chip = Font::new(Family::Segoe, 12.5, 600.0);
    let mut x = side + 16.0;
    for (place, mode) in [lang.pick("最佳能效", "Efficiency"), lang.pick("平衡", "Balanced"), lang.pick("最佳性能", "Performance")].into_iter().enumerate() {
        let across = frame.gfx.measure(mode, chip) + 28.0;
        let chosen = place == 1;
        picture.plate(Rect { x, y: y + 42.0, w: across, h: 34.0 }, 17.0, if chosen { INK } else { TILE });
        picture.words(mode, chip, if chosen { PAGE } else { INK }, x + 14.0, y + 63.5, false);
        x += across + 8.0;
    }
    y += 92.0 + 12.0;
    picture.plate(Rect { x: side, y, w: wide, h: 160.0 }, 20.0, CARD);
    picture.words(lang.pick("显示器", "Monitors"), label, DIM, side + 16.0, y + 27.0, false);

    unsafe { frame.dc.PopLayer() };
    drop(std::mem::ManuallyDrop::into_inner(layer.geometricMask));
    frame.origin(0.0, 0.0);
    [reads.get("cpu_fan"), reads.get("case_fan")].into_iter().flatten().any(|rpm| rpm.as_i64().is_some_and(|rpm| rpm > 0))
}
