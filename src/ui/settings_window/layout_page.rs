//! The layout page: the components, to be dragged onto the edges of the screens, one to an edge; the screens as
//! Windows arranges them, every edge with what it opens (all of them small, or one screen large, its desktop and the
//! real panel slid out from the edge last pressed); a component on an edge dragged to another (the two change places)
//! or off it.

use std::time::Instant;

use windows::Win32::Foundation::RECT;
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture};

use super::*;
use crate::settings::{Part, Places, PARTS};

/// The library: a tile for each component, side by side, how tall each is and the room between them; the picture of
/// the component at its top, how tall, and the room round it.
const TILE: f32 = 148.0;
const TILE_GAP: f32 = 8.0;
const TILE_PICTURE: f32 = 88.0;
const TILE_PAD: f32 = 8.0;
/// What plays, drawn as its strip at the least a music widget is wide, made smaller into its picture.
const TILE_MUSIC: (f32, f32) = (240.0, 72.0);
/// The editor's head (the screens to show, every screen alike), and the room round the screens under it.
const EDITOR_HEAD: f32 = 52.0;
const EDITOR_ROOM: f32 = 20.0;
/// The words under the screens, saying how their edges open and what a press does, on two lines.
const EDITOR_FOOT: f32 = 56.0;
/// How far into a screen its edges take a drop: drawn small, and one screen large.
const ZONE_SMALL: f32 = 30.0;
const ZONE_LARGE: f32 = 112.0;
const ZONE_TOP_LARGE: f32 = 48.0;
/// A component on an edge: its pill's height, drawn small (its icon in a disc) and large (its icon and name, and a
/// button that takes it off); how thick the edge's line is.
const CHIP_SMALL: f32 = 22.0;
const CHIP: f32 = 28.0;
const EDGE_LINE: f32 = 3.0;
/// How far the pointer moves before a press on a component carries it.
const CARRY_FROM: f32 = 4.0;

/// A component carried in the editor: from the library, or off an edge (its screen and edge); where it was taken
/// (window DIPs), and whether it has gone far enough from there to be carried.
pub(super) struct Carry {
    pub part: Part,
    pub from: Option<(String, Edge)>,
    pub start: (f32, f32),
    pub moved: bool,
}

/// An edge of a screen in the editor as last drawn: where it takes a drop.
pub(super) struct Zone {
    pub screen: String,
    pub edge: Edge,
    pub rect: Rect,
}

pub(super) fn part_name(part: Part, lang: Lang) -> &'static str {
    match part {
        Part::Monitor => pick(lang, "监控", "Monitor"),
        Part::Workbench => pick(lang, "工作台", "Workbench"),
        Part::Music => pick(lang, "音乐", "Music"),
    }
}

/// What a component shows, in a few words (under its name in its tile).
fn part_word(part: Part, lang: Lang) -> &'static str {
    match part {
        Part::Monitor => pick(lang, "实时硬件数据", "Live readings"),
        Part::Workbench => pick(lang, "时间与工具", "Time and tools"),
        Part::Music => pick(lang, "歌曲与歌词", "Songs and lyrics"),
    }
}

pub(super) fn part_icon(part: Part) -> Icon {
    match part {
        Part::Monitor => Icon::Gauge,
        Part::Workbench => Icon::Briefcase,
        Part::Music => Icon::Music,
    }
}

/// Each component's own colour, its pills drawn in it.
fn part_colour(part: Part) -> Color {
    match part {
        Part::Monitor => Color::hex(0x2F6FE0, 1.0),
        Part::Workbench => Color::hex(0x1F8A7A, 1.0),
        Part::Music => Color::hex(0xC2408F, 1.0),
    }
}

impl Ui {
    /// The library row's height: its words, then the components' tiles side by side.
    pub(super) fn library_height(&self) -> f32 {
        let (_, word) = self.library_words();
        let room = PANE - 2.0 * PAD_SIDE - GUTTER - 2.0 * ROW_SIDE;
        22.0 + self.gfx.wrapped_height(word, Font::new(Family::Segoe, 12.0, 400.0), room) + 14.0 + TILE + ROW_SIDE
    }

    fn library_words(&self) -> (&'static str, &'static str) {
        (
            pick(self.lang, "组件", "Components"),
            pick(
                self.lang,
                "把组件拖到右边屏幕的边上，这条边就能呼出它。每条边放一个组件，同一个组件可以放在好几条边上。拖到已有组件的边上会替换它，从另一条边拖来则两边互换。没有组件的边不呼出",
                "Drag a component onto an edge of a screen on the right, and that edge opens it. An edge holds one, and a component can be on several edges. Dropped on an edge that holds one, it takes its place; brought from another edge, the two change places. An edge holding none opens nothing",
            ),
        )
    }

    /// The library: a tile for each component, side by side, to be dragged onto an edge: at its top the component
    /// as it looks now, small (the readings' load and its chart, the workbench's time and what plays, what plays as its
    /// strip shows it), its name and what it shows under that.
    #[allow(clippy::too_many_arguments)]
    pub(super) fn paint_library(&mut self, frame: &Frame, palette: &Palette, left: f32, y: f32, width: f32, height: f32, hovered: &Option<Target>) {
        card(frame, palette, left, y, width, height, palette.card);
        let (label, hint) = (Font::new(Family::Segoe, 14.0, 400.0), Font::new(Family::Segoe, 12.0, 400.0));
        let (name, word) = self.library_words();
        let room = width - 2.0 * ROW_SIDE;
        let head = 22.0 + self.gfx.wrapped_height(word, hint, room) + 14.0;
        field_label(frame, palette, name, Some(word), label, hint, left + ROW_SIDE, y + head / 2.0, room);
        let wide = (room - TILE_GAP * (PARTS.len() - 1) as f32) / PARTS.len() as f32;
        for (index, part) in PARTS.into_iter().enumerate() {
            let r = Rect { x: left + ROW_SIDE + index as f32 * (wide + TILE_GAP), y: y + head, w: wide, h: TILE };
            let target = Target::Library(part);
            let carried = self.carry.as_ref().is_some_and(|carry| carry.moved && carry.part == part && carry.from.is_none());
            fill(frame, if hovered.as_ref() == Some(&target) || carried { palette.control_on } else { palette.control }, r.x, r.y, r.w, r.h, 8.0);
            stroke_inside(frame, r, 8.0, palette.card_stroke);
            let picture = Rect { x: r.x + TILE_PAD, y: r.y + TILE_PAD, w: r.w - 2.0 * TILE_PAD, h: TILE_PICTURE };
            fill(frame, palette.window, picture.x, picture.y, picture.w, picture.h, 6.0);
            unsafe { frame.dc.PushAxisAlignedClip(&rect(picture.x, picture.y, picture.w, picture.h), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
            match part {
                Part::Monitor => self.monitor_sample(frame, palette, picture),
                Part::Workbench => self.workbench_sample(frame, palette, picture),
                Part::Music => {
                    let mut shows = super::super::music::Shows::from(&self.settings.view);
                    // As a strip: one line, the cover and what plays beside it, made smaller into the picture.
                    shows.clear = false;
                    let k = ((picture.w - 8.0) / TILE_MUSIC.0).min((picture.h - 8.0) / TILE_MUSIC.1);
                    let (w, h) = (TILE_MUSIC.0 * k, TILE_MUSIC.1 * k);
                    frame.place(Matrix3x2::scale(k, k) * Matrix3x2::translation(picture.x + (picture.w - w) / 2.0, picture.y + (picture.h - h) / 2.0));
                    let (ground, now) = (super::super::music::Ground::Own, Instant::now());
                    let _ = if super::super::music::heard() {
                        self.library_music.paint(frame, TILE_MUSIC, &shows, ground, self.scale * k, None, now)
                    } else {
                        self.library_music.paint_example(frame, TILE_MUSIC, &shows, ground, self.scale * k, None, now, self.example_from.elapsed().as_secs_f64())
                    };
                    frame.origin(0.0, 0.0);
                }
            }
            unsafe { frame.dc.PopAxisAlignedClip() };
            let below = picture.y + picture.h + 8.0;
            text_centred(frame, part_name(part, self.lang), Font::new(Family::Segoe, 13.5, 600.0), palette.text, r.x + TILE_PAD + 2.0, below + 10.0, r.w - 2.0 * TILE_PAD, Align::Start);
            text_centred(frame, part_word(part, self.lang), hint, palette.text2, r.x + TILE_PAD + 2.0, below + 30.0, r.w - 2.0 * TILE_PAD, Align::Start);
            self.targets.push((r, target));
        }
    }

    /// The readings, small: the processor's and the graphics' load now, and the processor's over the last minute as a
    /// line under them.
    fn monitor_sample(&self, frame: &Frame, palette: &Palette, r: Rect) {
        let app = crate::app();
        let history = app.controller.history.lock().unwrap();
        let Some(sample) = history.back() else { return };
        let figures = [("CPU", sample.cpu), ("GPU", sample.gpus.first().and_then(|gpu| gpu.usage))];
        let cell = (r.w - 16.0) / figures.len() as f32;
        for (i, (name, figure)) in figures.into_iter().enumerate() {
            let x = r.x + 8.0 + i as f32 * cell;
            text_centred(frame, name, Font::new(Family::Segoe, 10.0, 400.0), palette.text2, x, r.y + 13.0, cell, Align::Start);
            let shown = figure.map_or("—".to_string(), |f| format!("{f:.0}%"));
            text_centred(frame, &shown, Font::new(Family::Segoe, 17.0, 600.0), palette.text, x, r.y + 31.0, cell, Align::Start);
        }
        // The processor's load, a point a sample, the last minute of it (as much of it as there is).
        let loads: Vec<f32> = history.iter().rev().take(60).filter_map(|sample| sample.cpu).collect();
        if loads.len() < 2 {
            return;
        }
        let (x0, x1, bottom, top) = (r.x + 8.0, r.x + r.w - 8.0, r.y + r.h - 8.0, r.y + 46.0);
        let step = (x1 - x0) / 59.0;
        let point = |i: usize, load: f32| Vector2 { X: x1 - i as f32 * step, Y: bottom - (bottom - top) * (load / 100.0).clamp(0.0, 1.0) };
        let brush = frame.brush(palette.selection);
        for (i, pair) in loads.windows(2).enumerate() {
            unsafe { frame.dc.DrawLine(point(i, pair[0]), point(i + 1, pair[1]), brush, 1.4, None) };
        }
    }

    /// What the workbench shows, small: the time and the day, and what plays.
    fn workbench_sample(&self, frame: &Frame, palette: &Palette, r: Rect) {
        let app = crate::app();
        let Some(t) = app.controller.history.lock().unwrap().back().map(|sample| sample.t) else { return };
        let now = super::super::text::Local::at(t);
        let (figure, unit) = now.time(false, self.lang);
        let (x, w) = (r.x + 10.0, r.w - 20.0);
        text_centred(frame, &format!("{figure}{unit}"), Font::new(Family::Segoe, 22.0, 600.0), palette.text, x, r.y + 24.0, w, Align::Start);
        text_centred(frame, &now.date(self.lang), Font::new(Family::Segoe, 10.5, 400.0), palette.text2, x, r.y + 45.0, w, Align::Start);
        let playing = crate::playing::now().filter(|heard| !heard.title.is_empty());
        let title = match &playing {
            Some(heard) => heard.title.clone(),
            None => pick(self.lang, "没有正在播放的内容", "Nothing is playing").to_string(),
        };
        text_centred(frame, &title, Font::new(Family::Segoe, 10.5, 600.0), palette.text, x, r.y + 68.0, w, Align::Start);
    }

    /// The screen shown large in the editor, by its name: the one chosen, while it is there; every screen small, else
    /// (but the one screen there is, shown large).
    pub(super) fn editor_screen(&self) -> Option<String> {
        if let [only] = self.screens.as_slice() {
            return Some(only.id.clone());
        }
        self.view.clone().filter(|id| self.screens.iter().any(|screen| screen.id == *id))
    }

    /// The editor, in the preview's room: its head (the screens to show, every screen alike), the screens, and the
    /// words under them. Returns how soon to draw again (see `paint`), while the real panel shows.
    pub(super) fn paint_layout(&mut self, frame: &Frame, palette: &Palette, area: Rect, now: Instant) -> Option<f32> {
        let hovered = self.hovered();
        let lang = self.lang;
        self.zones.clear();
        let places = self.settings.places().clone();
        // The head: a button for every screen together and one for each, and every screen alike.
        let cy = area.y + EDITOR_HEAD / 2.0 + 6.0;
        let mut x = area.x + EDITOR_ROOM;
        let shown = self.editor_screen();
        if self.screens.len() > 1 {
            let mut views = vec![(pick(lang, "全部屏幕", "All screens").to_string(), None)];
            views.extend(self.screens.iter().enumerate().map(|(i, screen)| (if lang == Lang::Zh { format!("屏幕 {}", i + 1) } else { format!("Screen {}", i + 1) }, Some(screen.id.clone()))));
            let font = Font::new(Family::Segoe, 14.0, 400.0);
            for (text, view) in views {
                let w = frame.gfx.measure(&text, font) + 32.0;
                let kind = if view == shown { Button::Accent } else { Button::Plain };
                self.button(frame, palette, &text, x + w, cy, Target::View(view), kind, &hovered);
                x += w + 8.0;
            }
            let same = places.same;
            let label = pick(lang, "所有屏幕同样布局", "Every screen alike");
            let toggle_x = area.x + area.w - EDITOR_ROOM - 40.0;
            let label_w = frame.gfx.measure(label, font);
            text_centred(frame, label, font, palette.text, toggle_x - 12.0 - label_w, cy, label_w + 4.0, Align::Start);
            let pressed = self.pressed == Some(Target::Same);
            self.toggle(frame, palette, "layout:same".into(), same, true, toggle_x, cy, now, pressed);
            self.targets.push((Rect { x: toggle_x - 12.0 - label_w, y: cy - 16.0, w: label_w + 52.0, h: 32.0 }, Target::Same));
        }
        let canvas = Rect { x: area.x + EDITOR_ROOM, y: area.y + EDITOR_HEAD + 8.0, w: area.w - 2.0 * EDITOR_ROOM, h: area.h - EDITOR_HEAD - 8.0 - EDITOR_FOOT };
        if canvas.w <= 40.0 || canvas.h <= 40.0 {
            return None;
        }
        let creep = match &shown {
            Some(screen) => self.paint_one_screen(frame, palette, &places, screen, canvas, &hovered),
            None => {
                self.paint_all_screens(frame, palette, &places, canvas, &hovered);
                None
            }
        };
        // The words under the screens: how an edge opens, and under that what a press on one does.
        let foot = area.y + area.h - EDITOR_FOOT + 16.0;
        let small = Font::new(Family::Segoe, 12.0, 400.0);
        let mut x = area.x + EDITOR_ROOM;
        let mut legend = |frame: &Frame, dashed: bool, text: &str| {
            let (w, h) = (18.0, EDGE_LINE);
            if dashed {
                for i in 0..3 {
                    fill(frame, palette.selection, x + i as f32 * 7.0, foot - h / 2.0, 4.0, h, h / 2.0);
                }
            } else {
                fill(frame, palette.selection, x, foot - h / 2.0, w, h, h / 2.0);
            }
            let tw = frame.gfx.measure(text, small);
            text_centred(frame, text, small, palette.text2, x + w + 6.0, foot, tw + 4.0, Align::Start);
            x += w + 6.0 + tw + 20.0;
        };
        legend(frame, false, pick(lang, "外边缘：推进去呼出", "Outer edge: push into it"));
        legend(frame, true, pick(lang, "两块屏幕相接处：停一下呼出", "Where screens meet: rest on it"));
        let rest = if shown.is_some() {
            pick(lang, "点一条边，可以试看它呼出的面板，再点一次收起。底边是任务栏，不放组件", "Press an edge to try the panel it opens, and again to put it away. The bottom is the taskbar's")
        } else {
            pick(lang, "点一块屏幕，放大查看。底边是任务栏，不放组件", "Press a screen to see it large. The bottom is the taskbar's")
        };
        text_centred(frame, rest, small, palette.text3, area.x + EDITOR_ROOM, foot + 22.0, area.w - 2.0 * EDITOR_ROOM, Align::Start);
        // What is carried, at the pointer.
        if let (Some(carry), Some((px, py))) = (self.carry.as_ref().filter(|carry| carry.moved), self.pointer) {
            let part = carry.part;
            self.chip(frame, part, Rect { x: px + 10.0, y: py + 6.0, w: self.chip_width(frame, part) - CHIP + 8.0, h: CHIP }, true, 0.92, None);
        }
        creep
    }

    /// Every screen, small, as Windows arranges them: each its desktop and its number, its edges with what they open.
    fn paint_all_screens(&mut self, frame: &Frame, palette: &Palette, places: &Places, canvas: Rect, hovered: &Option<Target>) {
        let screens = self.screens.clone();
        let bound = |pick: fn(&RECT) -> i32, most: bool| {
            let all = screens.iter().map(|screen| pick(&screen.monitor));
            if most { all.max() } else { all.min() }.unwrap_or(0)
        };
        let (left, top, right, bottom) = (bound(|m| m.left, false), bound(|m| m.top, false), bound(|m| m.right, true), bound(|m| m.bottom, true));
        let (across, down) = ((right - left).max(1) as f32, (bottom - top).max(1) as f32);
        let k = (canvas.w / across).min(canvas.h / down);
        let (ox, oy) = (canvas.x + (canvas.w - across * k) / 2.0, canvas.y + (canvas.h - down * k) / 2.0);
        let number = Font::new(Family::Segoe, 13.0, 600.0);
        let monitors: Vec<RECT> = screens.iter().map(|screen| screen.monitor).collect();
        for (index, screen) in screens.iter().enumerate() {
            let m = screen.monitor;
            let r = Rect { x: ox + (m.left - left) as f32 * k + 2.0, y: oy + (m.top - top) as f32 * k + 2.0, w: (m.right - m.left) as f32 * k - 4.0, h: (m.bottom - m.top) as f32 * k - 4.0 };
            let target = Target::View(Some(screen.id.clone()));
            let pointed = hovered.as_ref() == Some(&target);
            let pixels = ((r.w * self.scale).round().max(1.0) as u32, (r.h * self.scale).round().max(1.0) as u32);
            match self.map_picture(frame, &screen.id, m, pixels) {
                Some(picture) => picture_rounded(frame, &picture, r, MAP_RADIUS * 2.0, if pointed { 1.0 } else { 0.85 }),
                None => fill(frame, palette.switch_off, r.x, r.y, r.w, r.h, MAP_RADIUS * 2.0),
            }
            stroke_inside(frame, r, MAP_RADIUS * 2.0, palette.card_stroke);
            let digit = (index + 1).to_string();
            let digit_w = frame.gfx.measure(&digit, number);
            let (cx, cy) = (r.x + r.w / 2.0, r.y + r.h / 2.0);
            fill(frame, palette.window.alpha(0.92), cx - 13.0, cy - 13.0, 26.0, 26.0, 13.0);
            text_centred(frame, &digit, number, palette.text, cx - digit_w / 2.0, cy, digit_w + 1.0, Align::Start);
            let along = |px: i32, upright: bool| if upright { r.y + (px - m.top) as f32 * r.h / (m.bottom - m.top).max(1) as f32 } else { r.x + (px - m.left) as f32 * r.w / (m.right - m.left).max(1) as f32 };
            self.paint_zones(frame, palette, places, &screen.id, &monitors, index, r, &along, false, hovered);
            // The screen takes a press where its edges do not (they are found first).
            self.targets.push((r, target));
        }
    }

    /// One screen, large: its desktop, the real panel slid out from the edge last pressed (where that edge opens one),
    /// and its edges with what they open.
    fn paint_one_screen(&mut self, frame: &Frame, palette: &Palette, places: &Places, screen: &str, canvas: Rect, hovered: &Option<Target>) -> Option<f32> {
        let index = self.screens.iter().position(|known| known.id == screen)?;
        let held = self.shown_edge.and_then(|edge| Some((edge, places.part(screen, edge)?)));
        let (creep, r) = match held {
            Some((edge, Part::Music)) => self.music_on_stage(frame, canvas, edge),
            Some((edge, part)) => {
                let prefs = self.prefs.clone().holding(part);
                self.panel_on_stage(frame, canvas, Some(edge), Some(&prefs), true)
            }
            None => self.panel_on_stage(frame, canvas, None, None, true),
        };
        let work = self.stage.work;
        let monitors: Vec<RECT> = self.screens.iter().map(|known| known.monitor).collect();
        let along = |px: i32, upright: bool| if upright { r.y + (px - work.top) as f32 * r.h / (work.bottom - work.top).max(1) as f32 } else { r.x + (px - work.left) as f32 * r.w / (work.right - work.left).max(1) as f32 };
        self.paint_zones(frame, palette, places, screen, &monitors, index, r, &along, true, hovered);
        creep
    }

    /// The stage's whole screen in `area`, and what plays slid out from its `edge` as the panel shows it there (as
    /// large as on the screen, the screen made smaller with it). Returns how soon to draw again, and where the screen
    /// was drawn.
    fn music_on_stage(&mut self, frame: &Frame, area: Rect, edge: Edge) -> (Option<f32>, Rect) {
        let (_, r) = self.panel_on_stage(frame, area, None, None, true);
        let k = r.w / self.stage.size.0;
        let shows = super::super::music::Shows::from(&self.settings.view);
        let room = self.settings.music_rooms.at(edge == Edge::Top);
        let zoom = self.settings.panel_size.clamp(SIZES.0, SIZES.1);
        let size = super::super::music::rests_at(room, &shows);
        let (w, h) = (size.0 * zoom, size.1 * zoom);
        let (sw, sh) = self.stage.size;
        let inset = 8.0;
        let (x, y) = match edge {
            Edge::Left => (inset, (sh - h) / 2.0),
            Edge::Right => (sw - inset - w, (sh - h) / 2.0),
            Edge::Top => ((sw - w) / 2.0, inset),
        };
        unsafe { frame.dc.PushAxisAlignedClip(&rect(r.x, r.y, r.w, r.h), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
        frame.place(Matrix3x2::scale(zoom * k, zoom * k) * Matrix3x2::translation(r.x + x * k, r.y + y * k));
        let (ground, scale, now) = (super::super::music::Ground::of(&shows), self.scale * zoom * k, Instant::now());
        let painted = if super::super::music::heard() {
            self.music.paint(frame, size, &shows, ground, scale, None, now)
        } else {
            self.music.paint_example(frame, size, &shows, ground, scale, None, now, self.example_from.elapsed().as_secs_f64())
        };
        frame.origin(0.0, 0.0);
        unsafe { frame.dc.PopAxisAlignedClip() };
        ((painted.busy || !super::super::music::heard()).then_some(MUSIC_PREVIEW_PACE), r)
    }

    /// The edges of the screen `screen` (`monitors[index]`), drawn at `r` (`along` placing a physical pixel along an
    /// edge on it): each edge's line (solid where it is pushed into, dashed where another screen lies against it; in
    /// the accent where it opens anything), and the component it holds in a pill at its middle; each the zone it takes
    /// a drop in.
    #[allow(clippy::too_many_arguments)]
    fn paint_zones(&mut self, frame: &Frame, palette: &Palette, places: &Places, screen: &str, monitors: &[RECT], index: usize, r: Rect, along: &dyn Fn(i32, bool) -> f32, large: bool, hovered: &Option<Target>) {
        let eased = crate::screens::eased();
        let deep = if large { ZONE_LARGE } else { ZONE_SMALL.min(r.w / 4.0) };
        let deep_top = if large { ZONE_TOP_LARGE } else { ZONE_SMALL.min(r.h / 4.0) };
        let carrying = self.carry.as_ref().is_some_and(|carry| carry.moved);
        let pale = Color::hex(0xFFFFFF, 0.55);
        for edge in EDGES {
            let upright = edge != Edge::Top;
            let zone = match edge {
                Edge::Left => Rect { x: r.x, y: r.y, w: deep, h: r.h },
                Edge::Right => Rect { x: r.x + r.w - deep, y: r.y, w: deep, h: r.h },
                Edge::Top => Rect { x: r.x + deep, y: r.y, w: r.w - 2.0 * deep, h: deep_top },
            };
            let part = places.part(screen, edge);
            let over = carrying && self.pointer.is_some_and(|(x, y)| x >= zone.x && x < zone.x + zone.w && y >= zone.y && y < zone.y + zone.h);
            // While a component is carried, every edge shows where it takes it; the one under the pointer, more.
            if carrying {
                fill(frame, palette.selection.alpha(if over { 0.32 } else { 0.12 }), zone.x, zone.y, zone.w, zone.h, 6.0);
            } else if large && hovered.as_ref() == Some(&Target::Zone(screen.to_string(), edge)) {
                fill(frame, Color::hex(0xFFFFFF, 0.14), zone.x, zone.y, zone.w, zone.h, 6.0);
            }
            // The line along the edge, stretch by stretch.
            let colour = if part.is_some() { palette.selection } else { pale };
            let (from, to) = if upright { (r.y, r.y + r.h) } else { (r.x, r.x + r.w) };
            for (a, b, seam) in crate::screens::stretches(monitors, index, edge, eased) {
                let (a, b) = (along(a, upright).max(from + 4.0), along(b, upright).min(to - 4.0));
                if b <= a {
                    continue;
                }
                let bar = |a: f32, b: f32| match edge {
                    Edge::Left => fill(frame, colour, r.x + 2.0, a, EDGE_LINE, b - a, EDGE_LINE / 2.0),
                    Edge::Right => fill(frame, colour, r.x + r.w - 2.0 - EDGE_LINE, a, EDGE_LINE, b - a, EDGE_LINE / 2.0),
                    Edge::Top => fill(frame, colour, a, r.y + 2.0, b - a, EDGE_LINE, EDGE_LINE / 2.0),
                };
                if seam {
                    let count = ((b - a + MAP_GAP) / (MAP_DASH + MAP_GAP)).floor().max(1.0);
                    let dash = ((b - a) - (count - 1.0) * MAP_GAP) / count;
                    for i in 0..count as usize {
                        let at = a + i as f32 * (dash + MAP_GAP);
                        bar(at, at + dash);
                    }
                } else {
                    bar(a, b);
                }
            }
            // What it holds, in a pill at its middle (going, while one carried is over it to take its place); none
            // while its panel shows there, slid out over where it would be (pressed again, the edge puts it away).
            let showing = large && !carrying && self.shown_edge == Some(edge);
            if let Some(part) = part.filter(|_| !showing) {
                let (w, h) = if large { (self.chip_width(frame, part), CHIP) } else { (CHIP_SMALL, CHIP_SMALL) };
                let rect = match edge {
                    Edge::Left => Rect { x: r.x + 10.0, y: zone.y + (zone.h - h) / 2.0, w, h },
                    Edge::Right => Rect { x: r.x + r.w - 10.0 - w, y: zone.y + (zone.h - h) / 2.0, w, h },
                    Edge::Top => Rect { x: zone.x + (zone.w - w) / 2.0, y: r.y + 10.0, w, h },
                };
                let lifted = self.carry.as_ref().is_some_and(|carry| carry.moved && carry.from.as_ref().is_some_and(|(s, e)| s == screen && *e == edge));
                let pointed = hovered.as_ref().is_some_and(|target| matches!(target, Target::Chip(s, e, _) | Target::Unplace(s, e) if s == screen && *e == edge));
                self.chip(frame, part, rect, large, if lifted || over { 0.35 } else { 1.0 }, large.then_some(pointed && !carrying));
                // What takes a press: the pill's button first, then the pill.
                if large {
                    self.targets.push((Rect { x: rect.x + rect.w - CHIP, y: rect.y, w: CHIP, h: rect.h }, Target::Unplace(screen.to_string(), edge)));
                }
                self.targets.push((rect, Target::Chip(screen.to_string(), edge, part)));
            }
            if large {
                self.targets.push((zone, Target::Zone(screen.to_string(), edge)));
            }
            self.zones.push(Zone { screen: screen.to_string(), edge, rect: zone });
        }
    }

    /// How wide a component's pill is drawn large: its icon and name, and the button that takes it off.
    fn chip_width(&self, frame: &Frame, part: Part) -> f32 {
        8.0 + 16.0 + 6.0 + frame.gfx.measure(part_name(part, self.lang), Font::new(Family::Segoe, 13.0, 600.0)) + 8.0 + CHIP - 6.0
    }

    /// A component's pill at `r`, as opaque as `opacity`: in its colour, its icon (and, large, its name, and where
    /// `button` says, the button that takes it off, lit while the pointer is on the pill).
    #[allow(clippy::too_many_arguments)]
    fn chip(&self, frame: &Frame, part: Part, r: Rect, large: bool, opacity: f32, button: Option<bool>) {
        let white = Color::hex(0xFFFFFF, opacity);
        fill(frame, Color::hex(0x000000, 0.18 * opacity), r.x, r.y + 1.0, r.w, r.h, r.h / 2.0);
        fill(frame, part_colour(part).alpha(opacity), r.x, r.y, r.w, r.h, r.h / 2.0);
        if !large {
            frame.icon(part_icon(part), Point { x: r.x + r.w / 2.0, y: r.y + r.h / 2.0 }, 12.0, white);
            return;
        }
        frame.icon(part_icon(part), Point { x: r.x + 8.0 + 8.0, y: r.y + r.h / 2.0 }, 14.0, white);
        let font = Font::new(Family::Segoe, 13.0, 600.0);
        let name = part_name(part, self.lang);
        text_centred(frame, name, font, white, r.x + 30.0, r.y + r.h / 2.0, frame.gfx.measure(name, font) + 4.0, Align::Start);
        let Some(pointed) = button else { return };
        let (cx, cy) = (r.x + r.w - CHIP / 2.0, r.y + r.h / 2.0);
        fill(frame, Color::hex(0x000000, if pointed { 0.32 } else { 0.18 } * opacity), cx - 8.0, cy - 8.0, 16.0, 16.0, 8.0);
        frame.icon(Icon::Close, Point { x: cx, y: cy }, 10.0, white);
    }

    /// A press on a component (in the library, or on an edge): carried from there once the pointer moves.
    pub(super) fn take_up(&mut self, target: &Target, x: f32, y: f32) -> bool {
        let (part, from) = match target {
            Target::Library(part) => (*part, None),
            Target::Chip(screen, edge, part) => (*part, Some((screen.clone(), *edge))),
            _ => return false,
        };
        self.carry = Some(Carry { part, from, start: (x, y), moved: false });
        unsafe { SetCapture(self.hwnd) };
        true
    }

    /// The pointer moved while a component is in hand.
    pub(super) fn carry_to(&mut self, x: f32, y: f32) {
        if let Some(carry) = &mut self.carry {
            if !carry.moved && ((x - carry.start.0).powi(2) + (y - carry.start.1).powi(2)).sqrt() >= CARRY_FROM {
                carry.moved = true;
            }
        }
    }

    /// The component in hand let go: on an edge, put there (taken off the one it came from); off every edge, taken off
    /// the one it came from. Not carried (a press and no more): a pill pressed shows the panel its edge opens.
    pub(super) fn put_down(&mut self) {
        let Some(carry) = self.carry.take() else { return };
        unsafe {
            let _ = ReleaseCapture();
        }
        self.pressed = None;
        if !carry.moved {
            if let Some((screen, edge)) = carry.from {
                self.try_edge(screen, edge);
            }
            return;
        }
        let zone = self.pointer.and_then(|(x, y)| self.zones.iter().find(|zone| x >= zone.rect.x && x < zone.rect.x + zone.rect.w && y >= zone.rect.y && y < zone.rect.y + zone.rect.h));
        let dropped = zone.map(|zone| (zone.screen.clone(), zone.edge));
        let places = self.settings.places_mut();
        match (dropped, &carry.from) {
            (Some((screen, edge)), from) => {
                places.put(&screen, edge, carry.part, from.as_ref().map(|(s, e)| (s.as_str(), *e)));
                self.shown_edge = Some(edge);
            }
            (None, Some((screen, edge))) => places.take(screen, *edge),
            (None, None) => return,
        }
        self.save();
    }

    /// The editor shows screen `screen` large, the real panel slid out from its `edge`.
    fn try_edge(&mut self, screen: String, edge: Edge) {
        self.view = Some(screen);
        self.shown_edge = Some(edge);
        self.next_frame = Instant::now();
    }

    /// Does what a press on the editor does; whether it was one of its.
    pub(super) fn press_editor(&mut self, target: &Target) -> bool {
        match target {
            Target::View(view) => {
                self.view = view.clone();
                self.shown_edge = None;
            }
            Target::Same => {
                let screen = self.editor_screen().or_else(|| self.stage.screen.clone()).unwrap_or_default();
                let same = !self.settings.places().same;
                self.settings.places_mut().set_same(same, &screen);
                self.save();
            }
            // (Pressed again, its panel is put away.)
            Target::Zone(_, edge) if self.shown_edge == Some(*edge) => self.shown_edge = None,
            Target::Zone(screen, edge) => self.try_edge(screen.clone(), *edge),
            Target::Unplace(screen, edge) => {
                self.settings.places_mut().take(screen, *edge);
                self.save();
            }
            _ => return false,
        }
        self.next_frame = Instant::now();
        true
    }

    /// The editor's focus stops: its head's buttons, every screen alike, and each component on an edge (its button).
    pub(super) fn editor_stops(&self) -> Vec<Target> {
        let mut stops = Vec::new();
        if self.screens.len() > 1 {
            stops.push(Target::View(None));
            stops.extend(self.screens.iter().map(|screen| Target::View(Some(screen.id.clone()))));
            stops.push(Target::Same);
        }
        if let Some(screen) = self.editor_screen() {
            let places = self.settings.places();
            for edge in EDGES {
                stops.push(Target::Zone(screen.clone(), edge));
                if places.part(&screen, edge).is_some() {
                    stops.push(Target::Unplace(screen.clone(), edge));
                }
            }
        }
        stops
    }
}
