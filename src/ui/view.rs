//! The panel's content: each module as a lane of blocks, laid out in
//! columns, and drawn for a sample and the history before it.

use std::hash::{Hash, Hasher};

use super::canvas::{Align, Canvas, Color, Family, Fill, Font, Point};
use super::icons::Icon;
use super::prefs::{Prefs, ProcessSort};
use super::seen::Seen;
use super::text::{self, Lang};
use super::theme::{Ink, Skin, Theme};
use crate::reading::{GpuLimit, ProcessSample, Sample, StaticInfo};

/// A column's width (DIPs), whatever the skin.
pub const COLUMN_WIDTH: f32 = 356.0;
/// The label column of a lane, and the gap after it.
const LABEL: f32 = 96.0;
const LABEL_GAP: f32 = 12.0;
const HEAD: f32 = 17.0;
const HEAD_GAP: f32 = 8.0;
const PLOT: f32 = 40.0;
const RATE_PLOT: f32 = 36.0;
/// Rate charts start this much further right, past the rates beside them.
const RATE_INDENT: f32 = 26.0;
const LINE: f32 = 16.0;
/// The sound and what plays: the room over each, and how tall what is pressed there is.
const MEDIA_GAP: f32 = 8.0;
const MEDIA_ROW: f32 = 24.0;

/// How far the lyrics have glid to the line sung now (0 just come to it, 1 there), and what they glide from: the line
/// sung before.
#[derive(Clone, PartialEq, Debug, Default)]
pub struct Glide {
    pub done: f32,
    pub was: String,
}

impl Hash for Glide {
    fn hash<H: Hasher>(&self, state: &mut H) {
        (self.done.to_bits(), &self.was).hash(state);
    }
}

/// The glide to the line sung now, as far as it has gone (eased: quick, then settling).
fn glide_of(heard: Option<&crate::playing::Now>) -> Glide {
    use crate::playing::Sung;
    let Some(Sung::Lines { lines, now: Some(now), since }) = heard.and_then(|heard| heard.sung.as_ref()) else {
        return Glide { done: 1.0, ..Default::default() };
    };
    let t = (since.elapsed().as_secs_f32() / crate::playing::GLIDE.as_secs_f32()).min(1.0);
    let text = |at: Option<usize>| at.and_then(|at| lines.get(at)).map_or(String::new(), |line| line.text.clone());
    Glide { done: 1.0 - (1.0 - t).powi(3), was: text(now.checked_sub(1)) }
}

/// What is said of what plays (`heard`): its title (else its player's name, else that nothing plays), and under it
/// whether it plays, by whom and in which player.
fn said_of(heard: &crate::playing::Now, lang: Lang) -> (String, String) {
    let nothing = heard.title.is_empty() && heard.app.is_empty();
    let title = match (heard.title.is_empty(), nothing) {
        (_, true) => lang.pick("没有正在播放的内容", "Nothing is playing").to_string(),
        (true, false) => heard.app.clone(),
        (false, _) => heard.title.clone(),
    };
    let state = if heard.playing { lang.pick("正在播放", "Playing") } else { lang.pick("已暂停", "Paused") };
    let about = if nothing { String::new() } else { [state, heard.artist.as_str(), if heard.title.is_empty() { "" } else { heard.app.as_str() }].into_iter().filter(|one| !one.is_empty()).collect::<Vec<_>>().join(" · ") };
    (title, about)
}

/// What the lyrics show for what plays now (`heard`): the line sung now and under it its translation (with
/// `translated`) or the next; else why there are none, and whether that is it (`said`); nothing where none were found.
fn sung_now(heard: Option<&crate::playing::Now>, lang: Lang, translated: bool) -> Option<(String, String, bool)> {
    use crate::playing::Sung;
    let said = |line: &str, under: &str| Some((line.to_string(), under.to_string(), true));
    let Some(sung) = heard.and_then(|heard| heard.sung.as_ref()) else {
        return said(lang.pick("没有正在播放的歌曲", "Nothing is playing"), "");
    };
    match sung {
        Sung::Untimed { netease: true } => said(lang.pick("网易云音乐没有提供播放进度", "NetEase Cloud Music does not say how far in it is"), lang.pick("安装 InfLink-rs 插件后可以显示歌词", "With the InfLink-rs plugin, it does")),
        Sung::Untimed { netease: false } => said(lang.pick("这个播放器没有提供播放进度", "This player does not say how far in it is"), lang.pick("无法跟着播放显示歌词", "so no line can be shown as it is sung")),
        Sung::Asking => said(lang.pick("正在查找歌词…", "Finding the lyrics…"), ""),
        Sung::Unknown => None,
        Sung::Lines { lines, now, .. } => {
            let line = now.map_or(String::new(), |now| sung_line(&lines[now].text));
            let translation = now.and_then(|now| lines[now].translation.clone()).filter(|_| translated);
            let next = lines.get(now.map_or(0, |now| now + 1)).map(|line| line.text.clone()).unwrap_or_default();
            Some((line, translation.unwrap_or(next), false))
        }
    }
}

/// A line as it is shown: one with nothing sung (the music between lines), three dots.
fn sung_line(text: &str) -> String {
    if text.is_empty() { "· · ·".to_string() } else { text.to_string() }
}

const FACT_GAP: f32 = 3.0;
pub const TABLE_ROW: f32 = 22.0;
/// The most processes a lane past `Detail::Full` lists.
const COMPACT_PROCESSES: usize = 3;
/// The settings button in the bar.
const BUTTON: f32 = 32.0;
/// Between two of the bar's buttons, so that two lit side by side stay two.
const BUTTON_GAP: f32 = 4.0;
/// Rates below this full scale are drawn against it, so idle chatter stays low.
const MIN_RATE_SCALE: f64 = 10.0 * 1024.0;
/// The frame time chart's least full scale, in milliseconds.
const MIN_FRAME_SCALE: f64 = 10.0;

/// The system icon font's open folder.
const FOLDER: &str = "\u{E838}";

/// The name `label` of a row that opens something (drawn at `left`, `top`),
/// the pointer on it, marked as its skin marks what opens: chart paper
/// underlines it in ink, as a link on paper is; Windows 11 sets it in the
/// accent (by the caller), as its links are; glass, already lit along its
/// rims, leaves it bright and no more. And after it, a folder: what a press opens. The folder is
/// centred on the name's ink, not its line (a name of capitals and a colon
/// has no descender, and sits high in its line).
fn opens_marked(frame: &dyn Canvas, theme: &Theme, label: &str, left: f32, top: f32) {
    let font = theme.small;
    let (ink_top, ink_bottom) = frame.ink(label, font);
    let wide = frame.measure(label, font);
    match theme.skin {
        Skin::Paper => frame.fill(theme.text, left, top + ink_bottom + 1.5, wide, 1.0),
        Skin::Glass | Skin::Fluent => {}
    }
    let icon = Font::new(Family::Icons, 11.0, 400.0);
    let (icon_top, icon_bottom) = frame.ink(FOLDER, icon);
    let middle = top + (ink_top + ink_bottom) / 2.0;
    let ink = if theme.skin == Skin::Fluent { super::theme::accent(theme.dark) } else { theme.text2 };
    frame.text(FOLDER, icon, ink, left + wide + 5.0, middle - (icon_top + icon_bottom) / 2.0, LABEL, Align::Start);
}

/// What a click or a wheel turn on the panel lands on.
#[derive(Clone, Copy, PartialEq, Hash)]
pub enum Hit {
    Settings,
    Pin,
    Sort(ProcessSort),
    /// The process list, and how far it scrolls.
    Processes(u32),
    /// The overlay's switch.
    Overlay,
    /// The game button: the window in front taken for a game, or games turned off (see `presents::GameMode`).
    Game,
    /// A thread's cell in the CPU's grid: its clock and load shown while the
    /// pointer is on it.
    Thread(u16),
    /// What is followed (see `playing::Follow`) paused, played or skipped, or the sound muted; and the sound's
    /// track, pressed where along it the sound is to be.
    Media(crate::playing::Key, crate::playing::Follow),
    Volume,
    /// The track of what is followed, pressed (and dragged along) where in it it is to play from.
    Seek(crate::playing::Follow),
    /// A drive's row in the storage lane: the drive opened (by its place among the drives seen).
    Drive(u16),
    /// A line of the lyrics turned to by the wheel (by its place among them): played from its start.
    Line(u16),
}

impl Hit {
    /// Whether a press on it does something of its own (and so moves nothing): the process list and the threads'
    /// cells only scroll and show, under the wheel and the pointer.
    pub fn takes_press(self) -> bool {
        !matches!(self, Hit::Processes(_) | Hit::Thread(_))
    }

    /// The track it is, where it is one dragged along.
    pub fn slider(self) -> Option<crate::playing::Slider> {
        match self {
            Hit::Volume => Some(crate::playing::Slider::Volume),
            Hit::Seek(follow) => Some(crate::playing::Slider::Seek(follow)),
            _ => None,
        }
    }

    /// How far the process list scrolls, for a hit on it.
    pub fn max_scroll(self) -> Option<f32> {
        match self {
            Hit::Processes(bits) => Some(f32::from_bits(bits)),
            _ => None,
        }
    }
}

/// A region of the panel and what lands on it, in DIPs from its corner.
pub type HitBox = (f32, f32, f32, f32, Hit);

/// What the readings are drawn with: the moment, the data, and the choices.
pub struct Scene<'a> {
    pub info: &'a StaticInfo,
    pub prefs: &'a Prefs,
    pub theme: &'a Theme,
    pub lang: Lang,
    pub history: &'a [Sample],
    /// What the machine has shown it can read: the rows and lanes there are.
    pub seen: &'a Seen,
    /// The pen's time: the chart runs a little behind the newest sample, so
    /// the next one has always arrived by the time it is drawn to.
    pub pen_ms: f64,
    pub process_scroll: f32,
    /// What the pointer is over.
    pub hover: Option<Hit>,
    /// The panel is pinned open.
    pub pinned: bool,
    /// The overlay is on.
    pub overlay: bool,
    /// The bar has its buttons: the panel's, not a widget's (a widget has
    /// its own, round it).
    pub buttons: bool,
    /// A newer Glance is out: a dot on the settings button, for whoever
    /// did not see it said.
    pub newer: bool,
    /// What plays is drawn apart (a music widget merged into these readings): the music lane is its head, and room
    /// left under it for that (see `music_room`).
    pub music_apart: bool,
}

impl Scene<'_> {
    fn latest(&self) -> &Sample {
        self.history.last().unwrap()
    }
}

/// One line of a chart: its reading in a sample, if the sample has it.
type Series = Box<dyn Fn(&Sample) -> Option<f64>>;

/// A chart: series read from samples, drawn against a full scale.
struct Plot {
    series: Vec<Series>,
    max: f64,
    hot: Option<f64>,
}

impl Plot {
    /// A chart against `max`, or with `None`, against the busiest moment on screen.
    fn new(scene: &Scene, series: Vec<Series>, max: Option<f64>, hot: Option<f64>) -> Self {
        let max = max.unwrap_or_else(|| full_scale(scene, &series));
        Plot { series, max, hot }
    }
}

/// What a pass over the panel draws: everything that holds still between
/// samples, or the charts, which move with every frame.
#[derive(Clone, Copy, PartialEq, Eq)]
pub enum Pass {
    Content,
    Plots,
}

enum Block {
    Head { title: String, device: String, aside: String, aside_hot: bool },
    /// A figure, and its chart unless the module's chart is switched off.
    Readout { figure: String, unit: &'static str, hot: bool, plot: Option<Plot> },
    Rates { rows: Vec<(String, String)>, plot: Option<Plot> },
    /// A second chart below a readout's, aligned with it, `gap` below it:
    /// what it charts named over its value.
    Trace { label: String, value: String, hot: bool, plot: Plot, gap: f32 },
    /// Each thread's load (0–1), unread where `None`, whether it is hot,
    /// and what the tag over it says while the pointer is on it.
    Threads(Vec<(Option<f32>, bool, String)>),
    /// A labelled bar; `opens`: a click on it opens that drive (its place among those seen: see `Hit::Drive`).
    Meter { label: String, fraction: f32, value: String, hot: bool, gap: f32, opens: Option<u16> },
    Facts { rows: Vec<(String, String, bool)>, gap: f32 },
    /// How loud the sound is, on a track it is dragged along, and what
    /// mutes it.
    Sound { level: u8, muted: bool },
    /// What plays (and, under it, by whom and where), with what pauses,
    /// plays and skips it.
    /// `players`: how many tell Windows what they play (the line under the title switches between them).
    Playing { title: String, about: String, playing: bool, players: u8 },
    /// How far into its track what plays is and how long the track is (whole seconds), on a track it is dragged
    /// along where its player lets it be (`seekable`).
    Progress { at: u32, length: u32, seekable: bool },
    /// The line sung now, and under it its translation (or the next line); `said`, where they say why there are
    /// none instead.
    Lyric { line: String, under: String, said: bool, glide: Glide },
    /// A list in five columns: headings that sort it (the processes'), or
    /// not; scrolled through past `visible` rows, or not.
    Table { headings: Vec<(String, Option<ProcessSort>)>, sort: Option<ProcessSort>, rows: Vec<[String; 5]>, visible: usize, scrolls: bool, widths: [f32; 4] },
    /// Room this tall left empty, for what is drawn there apart (see `Scene::music_apart`).
    Room(f32),
}

impl Block {
    fn height(&self) -> f32 {
        match self {
            Block::Head { .. } => HEAD + HEAD_GAP,
            Block::Readout { .. } => PLOT,
            Block::Rates { .. } => RATE_PLOT,
            Block::Trace { gap, .. } => gap + RATE_PLOT,
            Block::Threads(_) => 10.0 + 14.0,
            Block::Meter { gap, .. } => gap + LINE,
            Block::Facts { rows, gap } if !rows.is_empty() => gap + rows.len() as f32 * LINE + (rows.len() - 1) as f32 * FACT_GAP,
            Block::Facts { .. } => 0.0,
            Block::Sound { .. } => MEDIA_GAP + MEDIA_ROW,
            Block::Playing { .. } => MEDIA_GAP + 2.0 * LINE,
            Block::Progress { .. } => MEDIA_ROW,
            Block::Lyric { .. } => MEDIA_GAP + 2.0 * LINE,
            Block::Table { visible, .. } => *visible as f32 * TABLE_ROW,
            Block::Room(tall) => *tall,
        }
    }
}

impl Hash for Block {
    /// Everything the content pass draws from: two blocks that hash alike
    /// draw alike.
    fn hash<H: Hasher>(&self, state: &mut H) {
        std::mem::discriminant(self).hash(state);
        match self {
            Block::Head { title, device, aside, aside_hot } => (title, device, aside, aside_hot).hash(state),
            Block::Room(tall) => tall.to_bits().hash(state),
            Block::Readout { figure, unit, hot, .. } => (figure, unit, hot).hash(state),
            Block::Rates { rows, plot } => (rows, plot.as_ref().map(|plot| plot.max.to_bits())).hash(state),
            Block::Trace { label, value, hot, plot, gap } => (label, value, hot, plot.max.to_bits(), gap.to_bits()).hash(state),
            Block::Threads(cells) => cells.iter().for_each(|(load, hot, tag)| (load.map(f32::to_bits), hot, tag).hash(state)),
            Block::Meter { label, fraction, value, hot, gap, opens } => (label, fraction.to_bits(), value, hot, gap.to_bits(), opens).hash(state),
            Block::Facts { rows, gap } => (rows, gap.to_bits()).hash(state),
            Block::Sound { level, muted } => (level, muted).hash(state),
            Block::Playing { title, about, playing, players } => (title, about, playing, players).hash(state),
            Block::Progress { at, length, seekable } => (at, length, seekable).hash(state),
            Block::Lyric { line, under, said, glide } => (line, under, said, glide).hash(state),
            Block::Table { headings, sort, rows, visible, scrolls, widths } => {
                headings.iter().for_each(|(label, key)| (label, key.map(|key| key as u8)).hash(state));
                (sort.map(|sort| sort as u8), rows, visible, scrolls, widths.map(f32::to_bits)).hash(state);
            }
        }
    }
}

/// How much of its module a lane shows (see `arrange::Form`).
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Default)]
pub enum Detail {
    /// All of it.
    #[default]
    Full,
    /// Its figure and chart, and what belongs with them (a card's video
    /// memory, its volumes, a few processes); none of the rows below.
    Compact,
}

pub struct Lane {
    /// Its module's (what names its parts, see `morph`).
    id: String,
    blocks: Vec<Block>,
    ink: Ink,
}

impl Lane {
    /// Its module's id.
    pub fn id(&self) -> &str {
        &self.id
    }
}

impl Hash for Lane {
    fn hash<H: Hasher>(&self, state: &mut H) {
        self.blocks.hash(state);
    }
}

/// The game in `s`: one found, or the window in front taken for one by hand.
fn found_game(s: &Sample) -> Option<&crate::reading::GameSample> {
    s.game.as_ref()
}

/// The game in `s`, if it is the process `pid`: a chart of a game's frames
/// is of that game's alone, not of another's before it.
fn same_game(s: &Sample, pid: Option<u32>) -> Option<&crate::reading::GameSample> {
    found_game(s).filter(|game| Some(game.pid) == pid)
}

/// (see `presents::GameMode`).
pub fn front_is_game() -> bool {
    #[cfg(windows)]
    return crate::presents::game_mode() == crate::presents::GameMode::On;
    #[cfg(not(windows))]
    false
}

/// Whether a game shows (the game button lit): one found and not turned off, or one asked for by hand.
pub fn game_shown(history: &[Sample]) -> bool {
    if front_is_game() {
        true
    } else if game_hidden() {
        false
    } else {
        history.last().is_some_and(|s| s.game.is_some())
    }
}

/// Whether games are turned off by hand: none shows, found or not.
pub fn game_hidden() -> bool {
    #[cfg(windows)]
    return crate::presents::game_mode() == crate::presents::GameMode::Off;
    #[cfg(not(windows))]
    false
}

/// Whether a module's lane shows: switched on; or the game's, asked for by hand (the game button pressed: the
/// window in front taken for a game) though it is switched off.
pub fn shows_module(entry: &super::prefs::ModuleEntry) -> bool {
    entry.on || (entry.id == "game" && front_is_game())
}

/// What a game asked for by hand says before its first frames.
pub fn waiting(lang: Lang) -> String {
    if front_is_game() { lang.pick("等待画面", "No frames yet").into() } else { String::new() }
}

/// The lanes of the modules that are on and have something to show, all
/// of each.
pub fn lanes(scene: &Scene) -> Vec<Lane> {
    lanes_at(scene, |_| Detail::Full)
}

impl Lane {
    pub fn height(&self, theme: &Theme) -> f32 {
        theme.pad_top + theme.pad_bottom + self.blocks.iter().map(Block::height).sum::<f32>()
    }
}

/// The lanes of the modules that are on and have something to show, each
/// showing as much as `detail` says for it.
pub fn lanes_at(scene: &Scene, detail: impl Fn(&str) -> Detail) -> Vec<Lane> {
    scene
        .prefs
        .modules
        .iter()
        .filter(|entry| shows_module(entry))
        .filter_map(|entry| Some(Lane { id: entry.id.clone(), blocks: lane(scene, &entry.id, detail(&entry.id))?, ink: scene.theme.ink(&entry.id) }))
        .collect()
}

fn head(title: impl Into<String>, device: impl Into<String>, aside: impl Into<String>, aside_hot: bool) -> Block {
    Block::Head { title: title.into(), device: device.into(), aside: aside.into(), aside_hot }
}

fn celsius(value: f32) -> String {
    format!("{value:.0} °C")
}

/// What a reading shows while it is not there to read.
const UNREAD: &str = "—";

/// A reading as a fact's value: as `show` writes it, or "—".
fn shown<T>(value: Option<T>, show: impl Fn(T) -> String) -> String {
    value.map_or_else(|| UNREAD.to_string(), show)
}

/// Each lane's module and its height at every detail (see
/// `arrange::Heights`), for the modules that are on and have something to
/// show.
pub fn heights(scene: &Scene) -> Vec<(String, [f32; 2])> {
    let height = |id: &str, detail: Detail| lane(scene, id, detail).map(|blocks| scene.theme.pad_top + scene.theme.pad_bottom + blocks.iter().map(Block::height).sum::<f32>());
    scene
        .prefs
        .modules
        .iter()
        .filter(|entry| shows_module(entry))
        .filter_map(|entry| Some((entry.id.clone(), [height(&entry.id, Detail::Full)?, height(&entry.id, Detail::Compact)?])))
        .collect()
}

/// The height of the lane of module `id` showing `detail`, holding what
/// `seen` holds; `None` where there is no such lane.
pub fn lane_height(scene: &Scene, id: &str, seen: &Seen, detail: Detail) -> Option<f32> {
    let scene = Scene { seen, ..*scene };
    let blocks = lane(&scene, id, detail)?;
    Some(scene.theme.pad_top + scene.theme.pad_bottom + blocks.iter().map(Block::height).sum::<f32>())
}

/// What a board's sensor `name` of `group` ("temps" or "fans") is called:
/// by what it is wired to, where the board is known; else by its number.
pub fn board_sensor(name: &str, group: &str, lang: Lang) -> String {
    if name.parse::<u32>().is_ok() {
        let numbered = if group == "fans" { ("风扇", "Fan") } else { ("传感器", "Sensor") };
        format!("{} {name}", lang.pick(numbered.0, numbered.1))
    } else {
        lang.name(name)
    }
}

/// The blocks of module `id`'s lane: all of it, or (`Compact`) none of the
/// rows below its figure and chart.
fn lane(scene: &Scene, id: &str, detail: Detail) -> Option<Vec<Block>> {
    let (s, prefs, lang, info) = (scene.latest(), scene.prefs, scene.lang, scene.info);
    let hot_load = prefs.hot_load;
    let hot_temp = prefs.hot_temp;
    let full = detail == Detail::Full;
    // Whether the module's item is switched on.
    let on = |item: &str| prefs.shows(id, item);
    let chart = on("chart");
    let readout = |value: Option<f32>, read: Series| Block::Readout {
        figure: shown(value, |value| format!("{value:.0}")),
        unit: "%",
        hot: value.is_some_and(|value| value > hot_load),
        plot: chart.then(|| Plot::new(scene, vec![read], Some(100.0), Some(hot_load as f64))),
    };
    Some(match id {
        "cpu" => {
            let sensors = s.cpu_sensors.as_ref();
            let temp = sensors.and_then(|c| c.temp);
            let had_temp = on("temp") && scene.seen.cpu_temp;
            let clock = s.ghz.map(|ghz| format!("{ghz:.2} GHz"));
            let had_clock = on("clock") && scene.seen.cpu_clock;
            // The temperature takes the corner, as on the GPU lanes; the
            // clock, the power and each chiplet's temperature go below.
            let aside = if had_temp {
                shown(temp, celsius)
            } else if had_clock {
                clock.clone().unwrap_or_else(|| UNREAD.into())
            } else {
                String::new()
            };
            let mut facts = Vec::new();
            // Cores of more than one kind: each kind's clock, the big first.
            let kinds = &s.kinds_ghz;
            if on("clock") && kinds.len() > 1 {
                let names: &[(&str, &str)] = if kinds.len() == 2 { &[("大核频率", "P-core clock"), ("小核频率", "E-core clock")] } else { &[("大核频率", "P-core clock"), ("小核频率", "E-core clock"), ("低功耗核频率", "LP E-core clock")] };
                for (ghz, (zh, en)) in kinds.iter().zip(names) {
                    facts.push((lang.pick(zh, en).into(), shown(*ghz, |ghz| format!("{ghz:.2} GHz")), false));
                }
            } else if had_temp && had_clock {
                facts.push((lang.pick("频率", "Clock").into(), clock.unwrap_or_else(|| UNREAD.into()), false));
            }
            if on("power") && scene.seen.cpu_power {
                facts.push((lang.pick("功耗", "Power").into(), shown(sensors.and_then(|c| c.power), |power| format!("{power:.1} W")), false));
            }
            let chiplets = &scene.seen.ccds;
            if on("ccds") && chiplets.len() > 1 {
                for &ccd in chiplets {
                    let value = sensors.and_then(|c| c.ccds.iter().find(|(read, _)| *read == ccd)).map(|(_, value)| *value);
                    facts.push((format!("CCD {}", ccd + 1), shown(value, celsius), value.is_some_and(|v| v > hot_temp)));
                }
            }
            if !full {
                facts.clear();
            }
            let mut blocks = vec![
                head("CPU", &info.cpu_name, aside, had_temp && temp.is_some_and(|t| t > hot_temp)),
                readout(s.cpu, Box::new(|s| s.cpu.map(f64::from))),
                Block::Facts { rows: facts, gap: 10.0 },
            ];
            let threads = scene.seen.threads;
            if full && on("threads") && threads > 0 {
                let load = |i: usize| s.threads.get(i).copied().flatten();
                // Each one's tag: which thread, its clock and its load.
                let tag = |i: usize| {
                    let ghz = s.threads_ghz.get(i).copied().flatten();
                    let thread = if lang == Lang::Zh { format!("线程 {}", i + 1) } else { format!("Thread {}", i + 1) };
                    match ghz {
                        Some(ghz) => format!("{thread} · {ghz:.2} GHz · {}", shown(load(i), text::percent)),
                        None => format!("{thread} · {}", shown(load(i), text::percent)),
                    }
                };
                blocks.push(Block::Threads((0..threads).map(|i| (load(i).map(|load| load / 100.0), load(i).is_some_and(|load| load > hot_load), tag(i))).collect()));
            }
            blocks
        }
        _ if id.starts_with("gpu:") => {
            let index = info.gpu_of(id)?;
            let gpu = &info.gpus[index];
            let had = scene.seen.gpus.get(index).filter(|seen| seen.present)?;
            let reading = s.gpus.get(index);
            let shows_temp = on("temp") && had.temp;
            let temp = reading.and_then(|g| g.temp).filter(|_| shows_temp);
            let aside = if shows_temp { shown(temp, celsius) } else { String::new() };
            let usage = reading.and_then(|g| g.usage);
            let mut blocks = vec![
                head("GPU", &gpu.name, aside, temp.is_some_and(|t| t > hot_temp)),
                readout(usage, Box::new(move |s| s.gpus.get(index)?.usage.map(f64::from))),
            ];
            if on("vram") {
                let used = reading.and_then(|g| g.mem_used);
                blocks.push(Block::Meter {
                    label: lang.pick("显存", "VRAM").into(),
                    fraction: used.map_or(0.0, |used| used as f32 / gpu.mem_total.max(1) as f32),
                    value: shown(used, |used| text::usage(used, gpu.mem_total)),
                    hot: false,
                    gap: 8.0,
                    opens: None,
                });
            }
            if full && on("engines") {
                // An engine the counters no longer list has nothing running
                // on it; with the engines unread, none is known.
                const SHOWN: [&str; 6] = ["3D", "Copy", "VideoDecode", "VideoEncode", "VideoCodec", "Compute"];
                for kind in had.engines.iter().map(String::as_str).filter(|kind| SHOWN.contains(kind)) {
                    let load = reading.and_then(|g| g.engines.as_ref()).map(|engines| engines.iter().find(|(known, _)| known == kind).map_or(0.0, |(_, load)| *load));
                    blocks.push(Block::Meter {
                        label: lang.name(kind),
                        fraction: load.unwrap_or(0.0) / 100.0,
                        value: shown(load, text::percent),
                        hot: load.is_some_and(|load| load > hot_load),
                        gap: 6.0,
                        opens: None,
                    });
                }
            }
            let mut facts = Vec::new();
            // (The warmest point of the chip, on the cards that say it apart: red as the temperature is.)
            if on("hotspot") && had.hot_spot {
                let hot_spot = reading.and_then(|g| g.hot_spot);
                facts.push((lang.pick("热点温度", "Hot spot").into(), shown(hot_spot, celsius), hot_spot.is_some_and(|t| t > scene.prefs.hot_temp)));
            }
            if on("clock") && had.clock {
                facts.push((lang.pick("频率", "Clock").into(), shown(reading.and_then(|g| g.clock_mhz), |clock| format!("{clock:.0} MHz")), false));
            }
            if on("power") && had.power {
                facts.push((lang.pick("功耗", "Power").into(), shown(reading.and_then(|g| g.power), |power| format!("{power:.1} W")), false));
            }
            if on("fan") && had.fan {
                facts.push((lang.pick("风扇", "Fan").into(), shown(reading.and_then(|g| g.fan_rpm), |rpm| format!("{rpm} RPM")), false));
            }
            // Memory the card borrows from the system's, where it has its own
            // besides (not where all of it is the system's, as on a Mac).
            if on("shared") && gpu.shared_total > 0 {
                facts.push((lang.pick("共享显存", "Shared").into(), shown(reading.and_then(|g| g.shared_used), |used| text::usage(used, gpu.shared_total)), false));
            }
            if full {
                blocks.push(Block::Facts { rows: facts, gap: 10.0 });
            }
            blocks
        }
        "memory" => {
            let total = info.mem_total.max(1) as f64;
            let percent = (s.memory.used as f64 / total * 100.0) as f32;
            let mut facts = Vec::new();
            // What it runs at, and what it is rated for where that is more.
            if let Some((speed, rated)) = info.memory_speed.filter(|_| on("speed")) {
                let value = match rated {
                    Some(rated) if lang == Lang::Zh => format!("{speed} MT/s（额定 {rated}）"),
                    Some(rated) => format!("{speed} MT/s (rated {rated})"),
                    None => format!("{speed} MT/s"),
                };
                facts.push((lang.pick("频率", "Speed").into(), value, false));
            }
            // Each module's own temperature, as each chiplet's on the CPU lane.
            for i in 0..if on("dimms") { scene.seen.dimms } else { 0 } {
                let name = if lang == Lang::Zh { format!("内存条 {}", i + 1) } else { format!("Module {}", i + 1) };
                let value = s.dimm_temps.get(i).copied();
                facts.push((name, shown(value, celsius), value.is_some_and(|v| v > hot_temp)));
            }
            if on("committed") {
                facts.push((lang.pick("已提交", "Committed").into(), text::usage(s.memory.committed, s.memory.commit_limit), false));
            }
            if on("cached") {
                facts.push((lang.pick("缓存", "Cached").into(), text::size(s.memory.cached), false));
            }
            if !full {
                facts.clear();
            }
            vec![
                head(lang.pick("内存", "Memory"), info.memory_modules.clone().unwrap_or_default(), text::usage(s.memory.used, info.mem_total), false),
                Block::Readout {
                    figure: format!("{percent:.0}"),
                    unit: "%",
                    hot: percent > hot_load,
                    plot: chart.then(|| Plot::new(scene, vec![Box::new(move |s| Some(s.memory.used as f64 / total * 100.0))], Some(100.0), Some(hot_load as f64))),
                },
                Block::Facts { rows: facts, gap: 10.0 },
            ]
        }
        "network" => {
            let bits = prefs.network.bits;
            let plot = Plot::new(scene, vec![Box::new(|s| s.net_down), Box::new(|s| s.net_up)], None, None);
            let scale = plot.max;
            let mut facts = Vec::new();
            let adapter = s.network.as_ref();
            if on("adapter") {
                facts.push((lang.pick("网卡", "Adapter").into(), adapter.map_or(lang.pick("未连接", "Not connected").into(), |a| a.name.clone()), false));
            }
            if on("wifi") && scene.seen.wifi {
                facts.push(("Wi-Fi".into(), shown(adapter.and_then(|a| a.wifi.clone()), |name| name), false));
            }
            if on("address") && scene.seen.address {
                facts.push((lang.pick("地址", "Address").into(), shown(adapter.and_then(|a| a.ipv4.clone()), |ip| ip), false));
            }
            let link = |s: &Sample| s.network.as_ref().map(|a| a.link_bps).filter(|bps| *bps > 0);
            if on("link") && scene.seen.link {
                facts.push((lang.pick("链路", "Link").into(), shown(link(s), text::link_speed), false));
            }
            if on("totals") {
                let (down, up) = (text::bytes(s.net_total_down as f64), text::bytes(s.net_total_up as f64));
                let total = if lang == Lang::Zh { format!("下载 {down}，上传 {up}") } else { format!("{down} down, {up} up") };
                facts.push((lang.pick("开机以来", "Since boot").into(), total, false));
            }
            if !full {
                facts.clear();
            }
            vec![
                // The adapter traffic leaves by now, which may have changed.
                // The chart's scale goes with the chart.
                head(lang.pick("网络", "Network"), s.network.as_ref().map(|a| a.model.clone()).unwrap_or_default(), if chart { full_scale_text(lang, scale, bits) } else { String::new() }, false),
                Block::Rates {
                    rows: vec![
                        (lang.pick("下载", "Down").into(), shown(s.net_down, |rate| text::rate(rate, bits))),
                        (lang.pick("上传", "Up").into(), shown(s.net_up, |rate| text::rate(rate, bits))),
                    ],
                    plot: chart.then_some(plot),
                },
                Block::Facts { rows: facts, gap: 10.0 },
            ]
        }
        "disk" => {
            let plot = Plot::new(scene, vec![Box::new(|s| s.disk_read), Box::new(|s| s.disk_write)], None, None);
            let scale = plot.max;
            let drives: &[(u32, String)] = if on("drives") { &scene.seen.drives } else { &[] };
            let temp = |id: u32| s.drive_temps.iter().find(|d| d.id == id).map(|d| d.celsius);
            let hottest = drives.iter().filter_map(|(id, _)| temp(*id)).fold(None, |max: Option<f32>, t| Some(max.map_or(t, |m| m.max(t))));
            // A drive's temperature takes the corner, as a GPU's does, and the
            // chart's scale moves below.
            let mut facts: Vec<(String, String, bool)> = Vec::new();
            // More than one drive: each on a line of its own, by name, with
            // its temperature (unread, for one that gives none).
            // (Those the machine started with, and any that has given a
            // temperature since: one plugged in later.)
            let mut all: Vec<(u32, &String)> = info.drive_ids.iter().copied().zip(&info.drives).collect();
            all.extend(drives.iter().filter(|(id, _)| !info.drive_ids.contains(id)).map(|(id, name)| (*id, name)));
            let every = if on("drives") { all.len() } else { 0 };
            if every > 1 {
                for (id, name) in &all {
                    let value = temp(*id);
                    facts.push(((*name).clone(), shown(value, celsius), value.is_some_and(|t| t > hot_temp)));
                }
            }
            if on("active") {
                facts.push((lang.pick("活动时间", "Active time").into(), shown(s.disk_active, text::percent), false));
            }
            // The chart's scale goes with the chart.
            let aside = if drives.is_empty() {
                if chart { full_scale_text(lang, scale, false) } else { String::new() }
            } else {
                if chart {
                    facts.push((lang.pick("满刻度", "Scale").into(), text::rate(scale, false), false));
                }
                shown(hottest, celsius)
            };
            if !full {
                facts.clear();
            }
            vec![
                // Named below, where there are several.
                head(lang.pick("磁盘", "Disk"), if every > 1 { if lang == Lang::Zh { format!("{every} 块硬盘") } else { format!("{every} drives") } } else { info.drives.join(", ") }, aside, hottest.is_some_and(|t| t > hot_temp)),
                Block::Rates {
                    rows: vec![
                        (lang.pick("读取", "Read").into(), shown(s.disk_read, |rate| text::rate(rate, false))),
                        (lang.pick("写入", "Write").into(), shown(s.disk_write, |rate| text::rate(rate, false))),
                    ],
                    plot: chart.then_some(plot),
                },
                Block::Facts { rows: facts, gap: 10.0 },
            ]
        }
        "processes" => {
            let sort = prefs.processes.sort;
            let mut ranked: Vec<&ProcessSample> = s.processes.iter().collect();
            ranked.sort_by(|a, b| sort_value(b, sort).total_cmp(&sort_value(a, sort)));
            vec![head(lang.pick("进程", "Processes"), "", "", false), Block::Table {
                headings: vec![
                    ("CPU".into(), Some(ProcessSort::Cpu)),
                    (lang.pick("内存", "Memory").into(), Some(ProcessSort::Memory)),
                    (lang.pick("读写", "I/O").into(), Some(ProcessSort::Io)),
                    ("GPU".into(), Some(ProcessSort::Gpu)),
                ],
                sort: Some(sort),
                scrolls: true,
                widths: PROCESS_COLUMNS,
                rows: ranked
                    .iter()
                    .map(|p| [p.name.clone(), text::percent(p.cpu), text::size(p.mem), text::rate(p.io, false), shown(p.gpu, text::percent)])
                    .collect(),
                visible: if full { prefs.processes.count } else { prefs.processes.count.min(COMPACT_PROCESSES) },
            }]
        }
        "storage" => {
            let mut blocks = vec![head(lang.pick("存储", "Storage"), "", "", false)];
            // The drives switched on (see `Prefs::shows`); none, no lane.
            let drives: Vec<&String> = scene.seen.volumes.iter().filter(|name| on(&format!("volumes:{name}"))).collect();
            if drives.is_empty() {
                return None;
            }
            for (i, name) in drives.into_iter().enumerate() {
                let volume = s.volumes.iter().find(|v| v.name == *name);
                let fraction = volume.map_or(0.0, |v| v.used as f32 / v.total.max(1) as f32);
                blocks.push(Block::Meter {
                    label: name.clone(),
                    fraction,
                    value: shown(volume, |v| text::usage(v.used, v.total)),
                    hot: fraction * 100.0 > hot_load,
                    gap: if i == 0 { 0.0 } else { 8.0 },
                    opens: scene.seen.volumes.iter().position(|seen| seen == name).map(|place| place as u16),
                });
            }
            blocks
        }
        "board" => {
            let had = scene.seen.board.as_ref()?;
            let board = s.board.as_ref();
            let named = |name: &str, group: &str| board_sensor(name, group, lang);
            // Each sensor shown as it is switched (see `Prefs::shows`).
            let temps: Vec<&String> = had.temps.iter().filter(|n| on(&format!("temps:{n}"))).collect();
            let fans: Vec<&String> = had.fans.iter().filter(|n| on(&format!("fans:{n}"))).collect();
            let value = |list: Option<&Vec<(String, f32)>>, name: &str| list.and_then(|list| list.iter().find(|(n, _)| n == name)).map(|(_, v)| *v);
            vec![
                head(lang.pick("主板", "Motherboard"), &info.board, "", false),
                Block::Facts {
                    rows: temps
                        .iter()
                        .map(|n| {
                            let t = value(board.map(|b| &b.temps), n);
                            (named(n, "temps"), shown(t, celsius), t.is_some_and(|t| t > hot_temp))
                        })
                        .collect(),
                    gap: 0.0,
                },
                Block::Facts {
                    rows: fans
                        .iter()
                        .filter(|_| full)
                        .map(|n| (named(n, "fans"), shown(value(board.map(|b| &b.fans), n), |rpm| format!("{rpm:.0} RPM")), false))
                        .collect(),
                    gap: 10.0,
                },
            ]
        }
        "game" => {
            // A game played, or asked for by hand (waiting for its frames).
            // (In the panel as in the widgets: the game button is the same everywhere.)
            let by_hand = front_is_game();
            if !(scene.seen.game || by_hand) || game_hidden() {
                return None;
            }
            // In the panel, only a game found (one taken for a game by hand is
            // the widgets' own).
            let game = found_game(s);
            let pid = game.map(|g| g.pid);
            // Its screen's refresh rate in the corner: the most frames it can show.
            let refresh = game.and_then(|g| g.refresh_hz);
            // A stutter: a frame taking as long as three at the screen's rate.
            let stutter = refresh.map(|hz| 3000.0 / hz as f64);
            let longest = game.map(|g| g.longest_ms);
            let frametimes = on("frametimes");
            let mut facts = Vec::new();
            if on("low") {
                facts.push(("1% low".into(), shown(game.and_then(|g| g.low), |low| format!("{low:.0} FPS")), false));
            }
            // Beside its chart, with that on.
            if on("longest") && !frametimes {
                let hot = longest.zip(stutter).is_some_and(|(ms, stutter)| ms as f64 > stutter);
                facts.push((lang.pick("最长一帧", "Longest frame").into(), shown(longest, |ms| format!("{ms:.1} ms")), hot));
            }
            if on("usage") {
                let cpu = shown(game.and_then(|g| g.cpu), text::percent);
                let gpu = shown(game.and_then(|g| g.gpu), text::percent);
                facts.push((lang.pick("占用", "Use").into(), format!("CPU {cpu} · GPU {gpu}"), false));
            }
            if on("memory") {
                let memory = shown(game.and_then(|g| g.mem), text::size);
                let vram = shown(game.and_then(|g| g.vram), text::size);
                let value = if lang == Lang::Zh { format!("{memory} · 显存 {vram}") } else { format!("{memory} · VRAM {vram}") };
                facts.push((lang.pick("内存", "Memory").into(), value, false));
            }
            if on("limit") && scene.seen.game_limit {
                let limit = game.and_then(|g| g.gpu_limit);
                let value = shown(limit, |limit| {
                    match limit {
                        GpuLimit::Free => lang.pick("未受限", "Not limited"),
                        GpuLimit::Power => lang.pick("功耗墙", "Power limit"),
                        GpuLimit::Thermal => lang.pick("温度墙", "Thermal limit"),
                        GpuLimit::Hardware => lang.pick("硬件降频", "Slowed by board"),
                    }
                    .into()
                });
                facts.push((lang.pick("显卡", "GPU").into(), value, limit.is_some_and(|limit| matches!(limit, GpuLimit::Thermal | GpuLimit::Hardware))));
            }
            if on("time") {
                facts.push((lang.pick("已玩", "Played").into(), shown(game.map(|g| g.playing_s), |s| lang.duration(s)), false));
            }
            if on("mic") && scene.seen.mic {
                let muted = s.mic_muted;
                let value = shown(muted, |muted| if muted { lang.pick("已静音", "Muted") } else { lang.pick("开启", "On") }.into());
                facts.push((lang.pick("麦克风", "Microphone").into(), value, muted == Some(true)));
            }
            // Full scale: the screen's rate, or the most frames drawn, if more
            // (a game not held to the screen's rate draws frames it never shows).
            let most = scene.history.iter().filter_map(|s| same_game(s, pid).map(|g| g.fps as f64)).fold(0.0, f64::max);
            let scale = refresh.map(f64::from).map(|hz| hz.max(most));
            let mut blocks = vec![
                head(lang.pick("游戏", "Game"), game.map_or_else(|| if by_hand { waiting(lang) } else { String::new() }, |g| g.name.clone()), shown(refresh, |hz| lang.pick(&format!("屏幕 {hz} Hz"), &format!("Screen {hz} Hz")).to_string()), false),
                Block::Readout {
                    figure: shown(game.map(|g| g.fps), |fps| format!("{fps:.0}")),
                    unit: "FPS",
                    hot: false,
                    plot: chart.then(|| Plot::new(scene, vec![Box::new(move |s| same_game(s, pid).map(|g| g.fps as f64))], scale, None)),
                },
            ];
            if !full {
                facts.clear();
            }
            if full && frametimes {
                // Full scale: two frames at the screen's rate, or the longest
                // frame on screen, if longer, rounded up.
                let series: Series = Box::new(move |s| same_game(s, pid).map(|g| g.longest_ms as f64));
                let floor = stutter.map_or(MIN_FRAME_SCALE, |stutter| (stutter * 2.0 / 3.0).max(MIN_FRAME_SCALE));
                let peak = visible_peak(scene, &series).max(floor);
                blocks.push(Block::Trace {
                    label: lang.pick("帧时间", "Frame time").into(),
                    value: shown(longest, |ms| format!("{ms:.1} ms")),
                    hot: longest.zip(stutter).is_some_and(|(ms, stutter)| ms as f64 > stutter),
                    plot: Plot { series: vec![series], max: round_up(peak), hot: stutter },
                    gap: 10.0,
                });
            }
            blocks.push(Block::Facts { rows: facts, gap: 10.0 });
            blocks
        }
        "battery" => {
            if !scene.seen.battery {
                return None;
            }
            let battery = s.battery.as_ref();
            let state = match battery {
                Some(battery) if battery.charging => lang.pick("正在充电", "Charging").to_string(),
                Some(battery) if battery.plugged => lang.pick("已接通电源", "Plugged in").to_string(),
                Some(battery) => match battery.seconds_left {
                    Some(left) => {
                        let left = lang.duration(left as u64);
                        if lang == Lang::Zh { format!("剩余 {left}") } else { format!("{left} left") }
                    }
                    None => lang.pick("使用电池", "On battery").into(),
                },
                None => UNREAD.into(),
            };
            // In, charging; out, what the whole machine draws.
            let power = |watts: f32| match watts {
                w if w > 0.05 => format!("{} {w:.1} W", lang.pick("充电", "In")),
                w if w < -0.05 => format!("{} {:.1} W", lang.pick("耗电", "Out"), -w),
                _ => "0 W".to_string(),
            };
            let mut facts = Vec::new();
            if scene.seen.battery_power && on("power") {
                facts.push((lang.pick("功率", "Power").to_string(), shown(battery.and_then(|b| b.watts), power), false));
            }
            if scene.seen.battery_health && on("health") {
                facts.push((lang.pick("健康度", "Health").to_string(), shown(battery.and_then(|b| b.health), |h| format!("{h:.0}%")), false));
            }
            if !full {
                facts.clear();
            }
            vec![
                head(lang.pick("电池", "Battery"), "", state, false),
                Block::Readout {
                    figure: shown(battery, |b| b.percent.to_string()),
                    unit: "%",
                    hot: battery.is_some_and(|b| !b.plugged && b.percent <= 20),
                    plot: chart.then(|| Plot::new(scene, vec![Box::new(|s| s.battery.as_ref().map(|b| b.percent as f64))], Some(100.0), None)),
                },
                Block::Facts { rows: facts, gap: 8.0 },
            ]
        }
        "docker" => {
            use crate::reading::DockerSample;
            let (context, state, mut containers) = match &s.docker {
                Some(DockerSample::Stopped { context }) => (context.clone(), lang.pick("未运行", "Not running").to_string(), Vec::new()),
                Some(DockerSample::Unsupported { context, .. }) => (context.clone(), lang.pick("不支持这种连接", "Connection not supported").to_string(), Vec::new()),
                Some(DockerSample::Running { context, containers }) => {
                    let count = containers.len();
                    (context.clone(), if lang == Lang::Zh { format!("{count} 个容器") } else if count == 1 { "1 container".into() } else { format!("{count} containers") }, containers.iter().collect())
                }
                None => (String::new(), UNREAD.to_string(), Vec::new()),
            };
            containers.sort_by(|a, b| b.cpu.unwrap_or(0.0).total_cmp(&a.cpu.unwrap_or(0.0)));
            vec![
                // The head row holds the table's headings: the engine and
                // what it is doing go under the list.
                head("Docker", "", "", false),
                Block::Table {
                    headings: vec![
                        ("CPU".into(), None),
                        (lang.pick("内存", "Memory").into(), None),
                        (lang.pick("网络", "Network").into(), None),
                        (lang.pick("读写", "I/O").into(), None),
                    ],
                    sort: None,
                    rows: containers
                        .iter()
                        .map(|c| [c.name.clone(), shown(c.cpu, text::percent), shown(c.mem, text::size), shown(c.net, |rate| text::rate(rate, false)), shown(c.io, |rate| text::rate(rate, false))])
                        .collect(),
                    // A row for each of the most there have been at once (an
                    // open panel is laid out for those), as many as the
                    // processes' list shows at most.
                    visible: scene.seen.containers.min(if full { prefs.processes.count } else { prefs.processes.count.min(COMPACT_PROCESSES) }),
                    scrolls: false,
                    widths: CONTAINER_COLUMNS,
                },
                Block::Facts { rows: vec![(if context.is_empty() { "Docker".into() } else { context }, state, false)], gap: 6.0 },
            ]
        }
        "wsl" => {
            use crate::reading::WslSample;
            // What it is doing at the top; a machine stopped or not there
            // keeps its rows, unread, so the lane does not change size.
            let (state, distros, cpu, used, total, gpu) = match &s.wsl {
                Some(WslSample::Missing) => (lang.pick("未安装", "Not installed"), None, None, None, None, None),
                Some(WslSample::Stopped) => (lang.pick("已停止", "Stopped"), None, None, None, None, None),
                Some(WslSample::Running { distros, cpu, used, total, gpu }) => (lang.pick("运行中", "Running"), distros.as_ref(), *cpu, *used, *total, *gpu),
                None => (UNREAD, None, None, None, None, None),
            };
            let names = match distros {
                Some(names) if names.is_empty() => lang.pick("没有发行版在运行", "No distribution running").to_string(),
                Some(names) => names.join(", "),
                None => String::new(),
            };
            // Its processors in the figure (named, as the lane is not theirs
            // alone), its memory and its GPU charted under them.
            let mut figure = readout(cpu, Box::new(|s| match &s.wsl {
                Some(crate::reading::WslSample::Running { cpu, .. }) => cpu.map(f64::from),
                _ => None,
            }));
            if let Block::Readout { unit, .. } = &mut figure {
                *unit = "% CPU";
            }
            let mut blocks = vec![head("WSL", if on("distros") { names } else { String::new() }, state, false), figure];
            if on("memory") {
                let both = used.zip(total);
                let value = shown(both, |(used, total)| text::usage(used, total));
                let series: Series = Box::new(|s| match &s.wsl {
                    Some(crate::reading::WslSample::Running { used: Some(used), total: Some(total), .. }) => Some(*used as f64 / (*total).max(1) as f64 * 100.0),
                    _ => None,
                });
                if chart {
                    blocks.push(Block::Trace { label: lang.pick("内存", "Memory").into(), value, hot: false, plot: Plot::new(scene, vec![series], Some(100.0), None), gap: 10.0 });
                } else {
                    blocks.push(Block::Meter {
                        label: lang.pick("内存", "Memory").into(),
                        fraction: both.map_or(0.0, |(used, total)| used as f32 / total.max(1) as f32),
                        value,
                        hot: false,
                        gap: 8.0,
                        opens: None,
                    });
                }
            }
            // The GPU as the memory: charted with the usage chart on.
            if on("gpu") {
                let value = shown(gpu, text::percent);
                let hot = gpu.is_some_and(|gpu| gpu > hot_load);
                if chart {
                    let series: Series = Box::new(|s| match &s.wsl {
                        Some(crate::reading::WslSample::Running { gpu, .. }) => gpu.map(f64::from),
                        _ => None,
                    });
                    blocks.push(Block::Trace { label: "GPU".into(), value, hot, plot: Plot::new(scene, vec![series], Some(100.0), Some(hot_load as f64)), gap: 10.0 });
                } else {
                    blocks.push(Block::Meter { label: "GPU".into(), fraction: gpu.unwrap_or(0.0) / 100.0, value, hot, gap: 8.0, opens: None });
                }
            }
            blocks
        }
        // The time of day, and the day under its name, as the sample has them.
        // With it, what else is the PC's own to set at a glance: how loud the sound is, and what plays (as last
        // looked at: see `playing`).
        "time" => {
            let now = text::Local::at(s.t);
            let (figure, unit) = now.time(on("twelve"), lang);
            let mut blocks = vec![head(lang.pick("时间", "Time"), now.date(lang), "", false), Block::Readout { figure, unit, hot: false, plot: None }];
            if let Some(heard) = crate::playing::now() {
                if let Some(level) = heard.volume.filter(|_| on("sound")) {
                    blocks.push(Block::Sound { level, muted: heard.muted });
                }
                if on("media") {
                    let (title, about) = said_of(&heard, lang);
                    blocks.push(Block::Playing { title, about, playing: heard.playing, players: heard.players });
                    if let Some(timeline) = heard.timeline {
                        let whole = |seconds: f64| seconds.max(0.0).round() as u32;
                        blocks.push(Block::Progress { at: whole(timeline.at).min(whole(timeline.length)), length: whole(timeline.length), seekable: timeline.seekable });
                    }
                }
            }
            // The music's line, while what plays is it (a video playing meanwhile has none); none where none were found.
            let heard = crate::playing::music().filter(|music| crate::playing::now().is_some_and(|now| now.title == music.title && now.app == music.app));
            if let Some((line, under, said)) = sung_now(heard.as_ref(), lang, prefs.shows("lyrics", "translation")).filter(|_| on("lyrics") && heard.is_some()) {
                blocks.push(Block::Lyric { line, under, said, glide: glide_of(heard.as_ref()) });
            }
            blocks
        }
        // What plays, in a widget merged with readings: drawn apart (see `Scene::music_apart`), its lane its head and
        // room under it (a player's, full; a strip's, compact).
        "lyrics" if scene.music_apart => {
            let heard = crate::playing::music();
            vec![
                head(lang.pick("音乐", "Music"), heard.as_ref().map_or(String::new(), |heard| heard.app.clone()), "", false),
                Block::Room(if full { MUSIC_ROOM.0 } else { MUSIC_ROOM.1 }),
            ]
        }
        "system" => {
            let rows = [
                ("uptime", lang.pick("开机时长", "Uptime"), lang.duration(s.system.uptime_s)),
                ("processes", lang.pick("进程", "Processes"), s.system.processes.to_string()),
                ("threads", lang.pick("线程", "Threads"), s.system.threads.to_string()),
                ("handles", lang.pick("句柄", "Handles"), s.system.handles.to_string()),
            ];
            vec![
                head(lang.pick("系统", "System"), "", "", false),
                // Past `Full`, the first of them alone.
                Block::Facts {
                    rows: rows
                        .into_iter()
                        .filter(|(item, ..)| on(item))
                        .map(|(_, label, value)| (label.into(), value, false))
                        .take(if full { usize::MAX } else { 1 })
                        .collect(),
                    gap: 0.0,
                },
            ]
        }
        _ => return None,
    })
}

fn sort_value(p: &ProcessSample, sort: ProcessSort) -> f64 {
    match sort {
        ProcessSort::Cpu => p.cpu as f64,
        ProcessSort::Memory => p.mem as f64,
        ProcessSort::Io => p.io,
        // Not read sorts last.
        ProcessSort::Gpu => p.gpu.map_or(-1.0, f64::from),
    }
}

fn full_scale_text(lang: Lang, scale: f64, bits: bool) -> String {
    let value = text::rate(scale, bits);
    if lang == Lang::Zh { format!("满刻度 {value}") } else { format!("Scale {value}") }
}

/// Rounds a byte rate up to 1, 2 or 5 times a power of ten of its unit.
fn round_up_rate(value: f64) -> f64 {
    let unit = 1024f64.powf((value.ln() / 1024f64.ln()).floor());
    let magnitude = 10f64.powf((value / unit).log10().floor());
    let leading = value / unit / magnitude;
    let step = if leading <= 1.0 { 1.0 } else if leading <= 2.0 { 2.0 } else if leading <= 5.0 { 5.0 } else { 10.0 };
    step * magnitude * unit
}

/// The most `series` reads in the samples on screen, or 0.
fn visible_peak(scene: &Scene, series: &Series) -> f64 {
    let oldest = scene.pen_ms - scene.prefs.chart_seconds * 1000.0;
    scene.history.iter().filter(|s| s.t as f64 >= oldest).filter_map(series).fold(0.0, f64::max)
}

/// Rounds `value` (above 0) up to 1, 2 or 5 times a power of ten.
fn round_up(value: f64) -> f64 {
    let magnitude = 10f64.powf(value.log10().floor());
    let leading = value / magnitude;
    let step = if leading <= 1.0 { 1.0 } else if leading <= 2.0 { 2.0 } else if leading <= 5.0 { 5.0 } else { 10.0 };
    step * magnitude
}

fn full_scale(scene: &Scene, series: &[Series]) -> f64 {
    let oldest = scene.pen_ms - scene.prefs.chart_seconds * 1000.0;
    let peak = scene
        .history
        .iter()
        .filter(|s| s.t as f64 >= oldest)
        .flat_map(|s| series.iter().filter_map(move |read| read(s)))
        .fold(MIN_RATE_SCALE, f64::max);
    round_up_rate(peak)
}

/// Where each column starts, and the tallest column's height, when the
/// lanes, in order and `gap` apart, are split into `columns` so the tallest
/// is as short as it can be.
pub fn split(heights: &[f32], columns: usize, gap: f32) -> (Vec<usize>, f32) {
    let n = heights.len();
    // With every module switched off, there is only the bar.
    if n == 0 {
        return (vec![0; columns], 0.0);
    }
    let span = |from: usize, to: usize| heights[from..to].iter().sum::<f32>() + (to - from).saturating_sub(1) as f32 * gap;
    let mut best = vec![vec![f32::INFINITY; n + 1]; columns + 1];
    let mut from = vec![vec![0usize; n + 1]; columns + 1];
    best[0][0] = 0.0;
    for k in 1..=columns {
        for i in k..=n {
            for j in (k - 1)..i {
                let candidate = best[k - 1][j].max(span(j, i));
                if candidate < best[k][i] {
                    best[k][i] = candidate;
                    from[k][i] = j;
                }
            }
        }
    }
    let mut cuts = vec![0; columns];
    let mut i = n;
    for k in (1..=columns).rev() {
        cuts[k - 1] = from[k][i];
        i = from[k][i];
    }
    (cuts, best[columns][n])
}

/// A rectangle in DIPs: left, top, width, height.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Rect {
    pub x: f32,
    pub y: f32,
    pub w: f32,
    pub h: f32,
}

/// The panel laid out: lanes dealt into columns, and the bar below them.
#[derive(Clone)]
pub struct Layout {
    pub columns: usize,
    pub cuts: Vec<usize>,
    pub heights: Vec<f32>,
    /// The tallest column; every column is drawn this tall.
    pub lanes_height: f32,
    outer: (f32, f32, f32),
    lane_gap: f32,
    column_gap: f32,
    bar_gap: f32,
    bar_height: f32,
    /// Each column's width: as designed, or wider (see `stretched`).
    column_width: f32,
    /// How much taller than its lanes need each column was made, for their
    /// charts to grow into (see `stretched`).
    grow: f32,
}

impl Layout {
    /// The fewest columns, up to `max_columns`, whose tallest fits in `room`
    /// DIPs (bar included); failing that, the most.
    pub fn new(heights: Vec<f32>, room: f32, max_columns: usize, theme: &Theme) -> Self {
        let most = max_columns.min(heights.len()).max(1);
        let chrome = theme.outer.0 + theme.outer.2 + theme.bar_gap + theme.bar_height;
        let mut chosen = (1, split(&heights, 1, theme.lane_gap));
        for columns in 1..=most {
            chosen = (columns, split(&heights, columns, theme.lane_gap));
            if chosen.1 .1 + chrome <= room {
                break;
            }
        }
        let (columns, (cuts, tallest)) = chosen;
        Layout::built(heights, columns, cuts, tallest, theme)
    }

    /// The lanes in exactly `columns` columns (at least one, and no more
    /// than there are lanes), however tall that makes them.
    pub fn with_columns(heights: Vec<f32>, columns: usize, theme: &Theme) -> Self {
        let columns = columns.min(heights.len()).max(1);
        let (cuts, tallest) = split(&heights, columns, theme.lane_gap);
        Layout::built(heights, columns, cuts, tallest, theme)
    }

    fn built(heights: Vec<f32>, columns: usize, cuts: Vec<usize>, tallest: f32, theme: &Theme) -> Self {
        Layout {
            columns,
            cuts,
            heights,
            lanes_height: tallest,
            outer: theme.outer,
            lane_gap: theme.lane_gap,
            column_gap: theme.column_gap,
            bar_gap: theme.bar_gap,
            bar_height: theme.bar_height,
            column_width: COLUMN_WIDTH,
            grow: 0.0,
        }
    }

    /// The same, `by` DIPs wider and taller: the width shared out among
    /// the columns, the height given to every column, for its charts (see
    /// `grows`).
    pub fn stretched(mut self, by: (f32, f32)) -> Self {
        self.column_width += by.0.max(0.0) / self.columns as f32;
        self.lanes_height += by.1.max(0.0);
        self.grow = by.1.max(0.0);
        self
    }

    /// How much each lane's charts may grow, in order of the lanes: its
    /// column's share of what the layout was stretched by.
    pub fn grows(&self) -> Vec<f32> {
        (0..self.columns)
            .flat_map(|column| {
                let start = self.cuts[column];
                let end = self.cuts.get(column + 1).copied().unwrap_or(self.heights.len());
                std::iter::repeat_n(self.grow / (end - start).max(1) as f32, end - start)
            })
            .collect()
    }

    pub fn width(&self) -> f32 {
        self.columns as f32 * self.column_width + (self.columns - 1) as f32 * self.column_gap
    }

    pub fn height(&self) -> f32 {
        self.outer.0 + self.lanes_height + self.outer.2 + self.bar_gap + self.bar_height
    }

    /// Each lane's box, in order. A shorter column's lanes share what is
    /// left over, so every column ends at the same line.
    pub fn lanes(&self) -> Vec<Rect> {
        let (top, side, _) = self.outer;
        let column_width = (self.width() - 2.0 * side - (self.columns - 1) as f32 * self.column_gap) / self.columns as f32;
        let mut boxes = Vec::with_capacity(self.heights.len());
        for column in 0..self.columns {
            let start = self.cuts[column];
            let end = self.cuts.get(column + 1).copied().unwrap_or(self.heights.len());
            let count = (end - start) as f32;
            let natural = self.heights[start..end].iter().sum::<f32>() + (count - 1.0).max(0.0) * self.lane_gap;
            let extra = if end > start { (self.lanes_height - natural) / count } else { 0.0 };
            let x = side + column as f32 * (column_width + self.column_gap);
            let mut y = top;
            for height in &self.heights[start..end] {
                boxes.push(Rect { x, y, w: column_width, h: height + extra });
                y += height + extra + self.lane_gap;
            }
        }
        boxes
    }

    pub fn bar(&self) -> Rect {
        let y = self.outer.0 + self.lanes_height + self.outer.2 + self.bar_gap;
        Rect { x: 0.0, y, w: self.width(), h: self.bar_height }
    }

    /// How wide a percentage chart is (rate charts are a little narrower).
    pub fn plot_width(&self, theme: &Theme) -> f32 {
        self.lanes().first().map_or(COLUMN_WIDTH, |lane| lane.w) - 2.0 * theme.pad_x - LABEL - LABEL_GAP
    }
}

/// Draws one pass over the panel with its top-left corner at the origin,
/// and returns where clicks and wheel turns land.
pub fn paint(frame: &dyn Canvas, scene: &Scene, lanes: &[Lane], layout: &Layout, pass: Pass) -> Vec<HitBox> {
    let theme = scene.theme;
    let mut hits = Vec::new();
    let boxes = layout.lanes();
    for ((lane, area), grow) in lanes.iter().zip(&boxes).zip(layout.grows()) {
        paint_lane(frame, scene, lane, *area, grow, pass, &mut hits);
        // Chart paper rules each lane off below; the last rule in a column
        // also sets off the bar.
        if pass == Pass::Content && theme.ruled {
            frame.fill(theme.rule, area.x, area.y + area.h - 1.0, area.w, 1.0);
        }
    }
    if pass == Pass::Content {
        if theme.ruled {
            for column in 1..layout.columns {
                let x = boxes[layout.cuts[column]].x;
                frame.fill(theme.rule, x, 0.0, 1.0, layout.lanes_height);
            }
        }
        paint_bar(frame, scene, layout, &mut hits);
    }
    hits
}

/// Draws `text` so that its baseline sits where a CSS line box `line` DIPs
/// tall, aligned to `bottom`, would put it.
#[allow(clippy::too_many_arguments)]
fn text_on_line(frame: &dyn Canvas, text: &str, font: Font, color: Color, x: f32, bottom: f32, line: f32, width: f32) {
    let (ascent, descent) = frame.baseline(font);
    // The glyphs' ascent and descent centred in the line box.
    let baseline = bottom - line / 2.0 + (ascent - descent) / 2.0;
    frame.text(text, font, color, x, baseline - ascent, width, Align::Start);
}

/// How tall the room left in the music lane for what plays drawn apart is (DIPs): in full, and compact.
const MUSIC_ROOM: (f32, f32) = (176.0, 64.0);

/// Where what plays is drawn apart in the readings `layout` lays out, their lanes `ids` in its order (see
/// `Scene::music_apart`): the music lane's room under its head, as wide as what the lanes show; none without one.
pub fn music_room(layout: &Layout, ids: &[String], theme: &Theme) -> Option<Rect> {
    let index = ids.iter().position(|id| id == "lyrics")?;
    let lane = layout.lanes().get(index).copied()?;
    let top = lane.y + theme.pad_top + HEAD + HEAD_GAP;
    Some(Rect { x: lane.x + theme.pad_x, y: top, w: lane.w - 2.0 * theme.pad_x, h: (lane.y + lane.h - theme.pad_bottom - top).max(0.0) })
}

/// Draws `lane` in `area`, its charts `grow` DIPs taller between them (each
/// at most twice as tall as designed).
fn paint_lane(frame: &dyn Canvas, scene: &Scene, lane: &Lane, area: Rect, grow: f32, pass: Pass, hits: &mut Vec<HitBox>) {
    let theme = scene.theme;
    let ink = lane.ink;
    let left = area.x + theme.pad_x;
    let width = area.w - 2.0 * theme.pad_x;
    let plot_left = left + LABEL + LABEL_GAP;
    let plot_width = left + width - plot_left;
    let mut y = area.y + theme.pad_top;
    let charts = lane.blocks.iter().filter(|block| matches!(block, Block::Readout { plot: Some(_), .. } | Block::Rates { plot: Some(_), .. })).count();
    let taller = if charts > 0 { (grow / charts as f32).min(RATE_PLOT) } else { 0.0 };
    // Each part named by its lane and what it is, the small layouts naming
    // theirs alike (see `morph`): the CPU's figure is "cpu.value" in both.
    let id = &lane.id;
    let name = |part: &str| frame.key(&format!("{id}.{part}"));
    let mut kinds: Vec<&str> = Vec::new();
    for block in &lane.blocks {
        let kind = match block {
            Block::Head { .. } => "head",
            Block::Room(_) => "room",
            Block::Readout { .. } => "readout",
            Block::Rates { .. } => "rates",
            Block::Trace { .. } => "trace",
            Block::Threads(_) => "threads",
            Block::Meter { .. } => "meter",
            Block::Facts { .. } => "facts",
            Block::Sound { .. } => "sound",
            Block::Playing { .. } => "playing",
            Block::Progress { .. } => "progress",
            Block::Lyric { .. } => "lyric",
            Block::Table { .. } => "table",
        };
        let nth = kinds.iter().filter(|k| **k == kind).count();
        kinds.push(kind);
        let part = |what: &str| name(&format!("{kind}{nth}.{what}"));
        let charted = matches!(block, Block::Readout { plot: Some(_), .. } | Block::Rates { plot: Some(_), .. });
        let height = block.height() + if charted { taller } else { 0.0 };
        match (block, pass) {
            (Block::Room(_), _) => {}
            (Block::Readout { plot: Some(plot), .. }, Pass::Plots) => {
                name("chart");
                paint_plot(frame, scene, ink, plot, plot_left, y, plot_width, height);
            }
            (Block::Rates { plot: Some(plot), .. }, Pass::Plots) => {
                name("chart");
                paint_plot(frame, scene, ink, plot, plot_left + RATE_INDENT, y, plot_width - RATE_INDENT, height);
            }
            (Block::Trace { plot, gap, .. }, Pass::Plots) => {
                part("chart");
                paint_plot(frame, scene, ink, plot, plot_left, y + gap, plot_width, height - gap);
            }
            (_, Pass::Plots) => {}
            (Block::Head { title, device, aside, aside_hot }, Pass::Content) => {
                let title_width = frame.measure(title, theme.title);
                let row = y + (HEAD - theme.title.size) / 2.0 - 1.0;
                name("label");
                frame.text(title, theme.title, theme.text, left, row, width, Align::Start);
                let aside_width = if aside.is_empty() { 0.0 } else { frame.measure(aside, theme.body) };
                let device_left = left + title_width + 8.0;
                let device_width = width - title_width - 8.0 - if aside_width > 0.0 { aside_width + 8.0 } else { 0.0 };
                name("device");
                frame.text(device, theme.small, theme.text3, device_left, row + 1.0, device_width, Align::Start);
                let color = if *aside_hot { theme.signal } else { theme.text2 };
                name("detail");
                frame.text(aside, theme.body, color, left, row, width, Align::End);
            }
            (Block::Readout { figure, unit, hot, plot }, Pass::Content) => {
                let color = if *hot { theme.signal } else { theme.text };
                // The figure and its unit stand on the chart's baseline: in
                // the room before the chart, or, with none, the lane's.
                let room = if plot.is_some() { LABEL } else { width };
                let figure_width = frame.measure(figure, theme.figure);
                let bottom = y + height;
                name("value");
                text_on_line(frame, figure, theme.figure, color, left, bottom, theme.figure.size * theme.figure_line, room);
                name("unit");
                text_on_line(frame, unit, theme.unit, theme.text2, left + figure_width + 3.0, bottom, theme.unit.size, (room - figure_width - 3.0).max(30.0));
            }
            (Block::Rates { rows, .. }, Pass::Content) => {
                for (i, (label, value)) in rows.iter().enumerate() {
                    let row_y = y + height - (rows.len() - i) as f32 * (LINE + 2.0) + 1.0;
                    // A stroke sample in front of each label says which trace it names.
                    let stroke = if i == 0 { (ink.trace, 2.0) } else { (ink.trace2, 1.0) };
                    let which = if i == 0 { "a" } else { "b" };
                    name(&format!("{which}.mark"));
                    frame.fill(stroke.0, left, row_y + LINE / 2.0, 10.0, stroke.1);
                    frame.text(label, theme.small, theme.text2, left + 15.0, row_y + 1.0, 40.0, Align::Start);
                    name(&format!("{which}.value"));
                    frame.text(value, theme.value, theme.text, left, row_y, LABEL + RATE_INDENT, Align::End);
                }
            }
            (Block::Trace { label, value, hot, .. }, Pass::Content) => {
                // The value on the chart's baseline, its name above it.
                let bottom = y + height;
                part("text");
                frame.text(label, theme.small, theme.text2, left, bottom - 2.0 * LINE - 2.0, LABEL, Align::Start);
                frame.text(value, theme.value, if *hot { theme.signal } else { theme.text }, left, bottom - LINE, LABEL, Align::Start);
            }
            (Block::Threads(cells), Pass::Content) => {
                part("cells");
                let gap = 2.0;
                let count = cells.len().max(1) as f32;
                let cell = (width - gap * (count - 1.0)) / count;
                let radius = theme.thread_radius;
                for (i, (load, hot, _)) in cells.iter().enumerate() {
                    // A thread not read leaves its cell empty.
                    let Some(load) = load else { continue };
                    let cx = left + i as f32 * (cell + gap);
                    let cy = y + 10.0;
                    // Its clock and load, in the CPU's facts, while the pointer is on it.
                    hits.push((cx - gap / 2.0, y, cell + gap, 24.0, Hit::Thread(i as u16)));
                    let track = if scene.hover == Some(Hit::Thread(i as u16)) { theme.text3 } else { theme.track };
                    frame.fill_rounded(track, cx, cy, cell, 14.0, radius);
                    let filled = 14.0 * load.clamp(0.0, 1.0);
                    if filled > 0.0 {
                        // The load fills from the bottom, inside the cell's rounding.
                        frame.clip(cx, cy + 14.0 - filled, cell, filled);
                        frame.fill_rounded(if *hot { theme.signal } else { ink.trace }, cx, cy, cell, 14.0, radius);
                        frame.unclip();
                    }
                }
                // The cell pointed at: its tag over it, in the ink's colour
                // and against it, inside the lane.
                if let Some(Hit::Thread(i)) = scene.hover {
                    if let Some((_, hot, tag)) = cells.get(i as usize) {
                        let width_of = frame.measure(tag, theme.small) + 16.0;
                        let centre = left + i as f32 * (cell + gap) + cell / 2.0;
                        let tag_left = (centre - width_of / 2.0).clamp(left, (left + width - width_of).max(left));
                        let (tag_height, tag_top) = (LINE + 6.0, y + 10.0 - LINE - 10.0);
                        // A hot thread's tag in the signal colour, as its cell is.
                        let (ground, ink_on) = match (hot, theme.dark) {
                            (true, _) => (theme.signal, Color::hex(0xFFFFFF, 1.0)),
                            (false, true) => (theme.text, Color::hex(0x000000, 1.0)),
                            (false, false) => (theme.text, Color::hex(0xFFFFFF, 1.0)),
                        };
                        name("tag");
                        frame.fill_rounded(ground, tag_left, tag_top, width_of, tag_height, theme.control_radius.min(tag_height / 2.0));
                        frame.text(tag, theme.small, ink_on, tag_left + 8.0, tag_top + 3.0, width_of - 16.0, Align::Start);
                    }
                }
            }
            (Block::Sound { level, muted }, Pass::Content) => {
                // As a meter is laid out: its name, its track (dragged along, a knob where it is), its figure;
                // and after that what mutes it.
                let (row, middle) = (y + MEDIA_GAP, y + MEDIA_GAP + MEDIA_ROW / 2.0);
                part("label");
                frame.text(scene.lang.pick("音量", "Volume"), theme.small, theme.text2, left, middle - LINE / 2.0, LABEL, Align::Start);
                let said = format!("{level}%");
                let said_width = frame.measure("100%", theme.small);
                let mute = left + width - MEDIA_ROW;
                let track = (mute - 6.0 - said_width - LABEL_GAP - plot_left).max(0.0);
                part("track");
                slider(frame, theme, if *muted { theme.text3 } else { ink.trace }, plot_left, middle, track, *level as f32 / 100.0, true);
                hits.push((plot_left, row, track, MEDIA_ROW, Hit::Volume));
                part("level");
                frame.text(&said, theme.small, if *muted { theme.text3 } else { theme.text2 }, mute - 6.0 - said_width, middle - LINE / 2.0, said_width, Align::End);
                let hit = Hit::Media(crate::playing::Key::Mute, crate::playing::Follow::Any);
                pressed(frame, scene, hits, hit, if *muted { Icon::Muted } else { Icon::Volume }, mute, row, *muted);
            }
            (Block::Progress { at, length, seekable }, Pass::Content) => {
                // How far in it is where a meter's name is; the track, with a knob where it is dragged along; how
                // long it is at the end.
                let middle = y + MEDIA_ROW / 2.0;
                part("at");
                frame.text(&text::clock(*at), theme.small, theme.text2, left, middle - LINE / 2.0, LABEL, Align::Start);
                let long = text::clock(*length);
                let long_width = frame.measure(&long, theme.small);
                let track = (left + width - long_width - LABEL_GAP - plot_left).max(0.0);
                part("track");
                let fraction = if *length > 0 { *at as f32 / *length as f32 } else { 0.0 };
                slider(frame, theme, ink.trace, plot_left, middle, track, fraction, *seekable);
                if *seekable {
                    hits.push((plot_left, y, track, MEDIA_ROW, Hit::Seek(crate::playing::Follow::Any)));
                }
                part("length");
                frame.text(&long, theme.small, theme.text3, left + width - long_width, middle - LINE / 2.0, long_width, Align::End);
            }
            (Block::Playing { title, about, playing, players }, Pass::Content) => {
                use crate::playing::Key;
                // What plays and what is said of it, in the room before the three that are pressed.
                let top = y + MEDIA_GAP;
                let pressed_from = left + width - 3.0 * MEDIA_ROW - 2.0 * 2.0;
                part("title");
                frame.text(title, theme.body, theme.text, left, top, pressed_from - 8.0 - left, Align::Start);
                part("about");
                // Where several players tell what they play: the line is pressed to follow the next (lit under the
                // pointer; what a press does said under it).
                let switches = *players > 1;
                let switch = Hit::Media(crate::playing::Key::Player, crate::playing::Follow::Any);
                let lit = switches && scene.hover == Some(switch);
                frame.text(about, theme.small, if lit { theme.text } else { theme.text3 }, left, top + LINE, pressed_from - 8.0 - left, Align::Start);
                if switches {
                    hits.push((left, top + LINE, pressed_from - 8.0 - left, LINE, switch));
                }
                let row = top + (2.0 * LINE - MEDIA_ROW) / 2.0;
                for (place, (key, icon)) in [(Key::Previous, Icon::SkipBack), (Key::Toggle, if *playing { Icon::Pause } else { Icon::Play }), (Key::Next, Icon::SkipForward)].into_iter().enumerate() {
                    pressed(frame, scene, hits, Hit::Media(key, crate::playing::Follow::Any), icon, pressed_from + place as f32 * (MEDIA_ROW + 2.0), row, false);
                }
            }
            (Block::Lyric { line, under, said, glide }, Pass::Content) => {
                // The line sung now rises into its place as the one before rises away, each fading as it goes.
                let top = y + MEDIA_GAP;
                let rise = (1.0 - glide.done) * LINE;
                frame.clip(left - 2.0, top, width + 4.0, 2.0 * LINE);
                if glide.done < 1.0 && !glide.was.is_empty() {
                    part("was");
                    frame.text(&glide.was, theme.body, theme.text.alpha(1.0 - glide.done), left, top + rise - LINE, width, Align::Start);
                }
                part("line");
                let fade = if glide.was.is_empty() { 1.0 } else { glide.done };
                frame.text(line, if *said { theme.small } else { theme.body }, (if *said { theme.text2 } else { theme.text }).alpha(fade), left, top + rise, width, Align::Start);
                part("under");
                frame.text(under, theme.small, theme.text3.alpha(fade), left, top + LINE + rise, width, Align::Start);
                frame.unclip();
            }
            (Block::Meter { label, fraction, value, hot, gap, opens }, Pass::Content) => {
                let row_y = y + gap;
                part("meter");
                if let Some(drive) = opens {
                    hits.push((left, row_y, width, LINE, Hit::Drive(*drive)));
                }
                // Under the pointer, a row that opens its drive says so, lightly (the rows are close, and a band
                // across one would crowd them): its words in full ink, its name marked as its skin marks what opens,
                // and a folder after it.
                let hovered = opens.is_some_and(|drive| scene.hover == Some(Hit::Drive(drive)));
                let words = if hovered { theme.text } else { theme.text2 };
                let name = if hovered && theme.skin == Skin::Fluent { super::theme::accent(theme.dark) } else { words };
                if hovered {
                    opens_marked(frame, theme, label, left, row_y);
                }
                frame.text(label, theme.small, name, left, row_y, LABEL, Align::Start);
                let value_width = frame.measure(value, theme.small);
                frame.text(value, theme.small, words, left, row_y, width, Align::End);
                let bar_width = left + width - value_width - LABEL_GAP - plot_left;
                let bar_y = row_y + (LINE - theme.meter_height) / 2.0;
                let (height, radius) = (theme.meter_height, theme.meter_radius);
                frame.fill_rounded(theme.track, plot_left, bar_y, bar_width, height, radius);
                let filled = bar_width * fraction.clamp(0.0, 1.0);
                if filled > 0.0 {
                    frame.clip(plot_left, bar_y, filled, height);
                    frame.fill_rounded(if *hot { theme.signal } else { ink.trace }, plot_left, bar_y, bar_width, height, radius);
                    frame.unclip();
                }
            }
            (Block::Facts { rows, gap }, Pass::Content) => {
                for (i, (label, value, hot)) in rows.iter().enumerate() {
                    let row_y = y + gap + i as f32 * (LINE + FACT_GAP);
                    part(&i.to_string());
                    frame.text(label, theme.small, theme.text2, left, row_y, LABEL, Align::Start);
                    let color = if *hot { theme.signal } else { theme.text };
                    frame.text(value, theme.small, color, plot_left, row_y, left + width - plot_left, Align::Start);
                }
            }
            (Block::Table { headings, sort, rows, visible, scrolls, widths }, Pass::Content) => {
                part("rows");
                // The headings share the lane's head row, after the title.
                let head_y = y - HEAD - HEAD_GAP;
                let columns = table_columns(left, width, *widths);
                // Every heading's ink centred on the title's: Chinese, from
                // another face than the Latin, and a heavier weight each sit
                // differently in their line boxes.
                let title_top = head_y + (HEAD - theme.title.size) / 2.0 - 1.0;
                let middle = |text: &str, font: Font| {
                    let (top, bottom) = frame.ink(text, font);
                    (top + bottom) / 2.0
                };
                let centre = title_top + middle(scene.lang.pick("进程", "Processes"), theme.title);
                for ((label, key), (cx, cw)) in headings.iter().zip(&columns[1..]) {
                    let chosen = key.is_some() && key == sort;
                    let font = if chosen { Font { weight: 650.0, ..theme.small } } else { theme.small };
                    let lit = chosen || key.is_some_and(|key| scene.hover == Some(Hit::Sort(key)));
                    frame.text(label, font, if lit { theme.text } else { theme.text2 }, *cx, centre - middle(label, font), *cw, Align::End);
                    if let Some(key) = key {
                        hits.push((*cx, head_y, *cw, HEAD, Hit::Sort(*key)));
                    }
                }
                let list_height = *visible as f32 * TABLE_ROW;
                let max_scroll = if *scrolls { (rows.len().saturating_sub(*visible)) as f32 * TABLE_ROW } else { 0.0 };
                if *scrolls {
                    hits.push((left, y, width, list_height, Hit::Processes(max_scroll.to_bits())));
                }
                let scroll = scene.process_scroll.clamp(0.0, max_scroll);
                frame.clip(left, y, width, list_height);
                for (i, row) in rows.iter().enumerate() {
                    let row_y = y + i as f32 * TABLE_ROW - scroll;
                    if row_y + TABLE_ROW < y || row_y > y + list_height {
                        continue;
                    }
                    let text_y = row_y + (TABLE_ROW - LINE) / 2.0;
                    for (j, (cx, cw)) in columns.iter().enumerate() {
                        let (font, color, align) = if j == 0 { (theme.body, theme.text, Align::Start) } else { (theme.small, theme.text2, Align::End) };
                        frame.text(&row[j], font, color, *cx, text_y, *cw, align);
                    }
                }
                frame.unclip();
                // A thin thumb shows where in the list the view is.
                if max_scroll > 0.0 {
                    let thumb = (list_height * *visible as f32 / rows.len() as f32).max(12.0);
                    let at = y + (list_height - thumb) * scroll / max_scroll;
                    frame.fill_rounded(theme.text3.alpha(0.6), left + width + 6.0, at, 2.0, thumb, 1.0);
                }
            }
        }
        frame.key("");
        y += height;
    }
}

/// The widths of the process table's four measures: a share, a size, a
/// rate and a share.
const PROCESS_COLUMNS: [f32; 4] = [40.0, 52.0, 62.0, 36.0];
/// The container table's: a share, a size and two rates.
const CONTAINER_COLUMNS: [f32; 4] = [40.0, 52.0, 62.0, 62.0];

/// x and width of a table's name and its four measures, `widths` wide.
fn table_columns(left: f32, width: f32, widths: [f32; 4]) -> [(f32, f32); 5] {
    let gap = 6.0;
    let measures: f32 = widths.iter().sum::<f32>() + gap * widths.len() as f32;
    let mut columns = [(left, width - measures); 5];
    let mut x = left + width - measures + gap;
    for (i, w) in widths.iter().enumerate() {
        columns[i + 1] = (x, *w);
        x += w + gap;
    }
    columns
}

#[allow(clippy::too_many_arguments)]
fn paint_plot(frame: &dyn Canvas, scene: &Scene, ink: Ink, plot: &Plot, left: f32, top: f32, width: f32, height: f32) {
    let theme = scene.theme;
    let span = scene.prefs.chart_seconds * 1000.0;
    let pen = scene.pen_ms;
    let right = left + width;
    let bottom = top + height;
    let max = plot.max;
    let x = |t: f64| right - ((pen - t) / span) as f32 * width;
    let y = |value: f64| bottom - ((value / max).clamp(0.0, 1.0) as f32) * (height - 2.0);

    // Time rules travel with the paper.
    let rule_ms = span / 6.0;
    let mut t = (pen / rule_ms).floor() * rule_ms;
    while x(t) > left {
        frame.fill(theme.rule, x(t).round(), top, 1.0, height);
        t -= rule_ms;
    }
    frame.fill(theme.rule, left, bottom, width, 1.0);

    // One sample beyond each end, so the trace runs off both sides.
    let history = scene.history;
    let Some(first) = history.iter().position(|s| s.t as f64 >= pen - span) else { return };
    let visible = &history[first.saturating_sub(1)..];
    let last = visible.last().unwrap();
    frame.clip(left, top - 2.0, width, height + 2.0);
    for (order, read) in plot.series.iter().enumerate() {
        let mut points: Vec<Option<Point>> = visible.iter().map(|s| read(s).map(|value| Point { x: x(s.t as f64), y: y(value) })).collect();
        // The pen holds its position if the next sample is late.
        if (last.t as f64) < pen {
            points.push(read(last).map(|value| Point { x: right, y: y(value) }));
        }
        // A sample without the reading breaks the line: each stretch of
        // readings is drawn on its own, the last one to the pen.
        let stretches: Vec<&[Option<Point>]> = points.split(Option::is_none).collect();
        let last_stretch = stretches.len() - 1;
        for (i, stretch) in stretches.into_iter().enumerate().filter(|(_, stretch)| !stretch.is_empty()) {
            let stretch: Vec<Point> = stretch.iter().flatten().copied().collect();
            let to_pen = i == last_stretch;
            if order == 0 {
                let mut area = stretch.clone();
                let end = stretch.last().unwrap().x;
                area.push(Point { x: if to_pen { end.max(right) } else { end }, y: bottom });
                area.push(Point { x: stretch[0].x, y: bottom });
                // The wash under the trace, fading where the skin's fades.
                let wash = if ink.wash == ink.wash_end { Fill::Solid(ink.wash) } else { Fill::Down { top, from: ink.wash, bottom, to: ink.wash_end } };
                frame.fill_shape(&area, wash);
            }
            let (color, stroke) = if order == 0 { (ink.trace, 1.5) } else { (ink.trace2, 1.0) };
            frame.stroke(&stretch, color, stroke);
            if let (0, Some(hot)) = (order, plot.hot) {
                frame.clip(left, top - 2.0, width, y(hot) - top + 2.0);
                frame.stroke(&stretch, theme.signal, stroke);
                frame.unclip();
            }
        }
    }
    frame.unclip();

    // The pen tip sits at the moment being drawn.
    let read = &plot.series[0];
    let next = visible.iter().position(|s| s.t as f64 >= pen);
    let (before, after) = match next {
        Some(0) | None => (last, last),
        Some(i) => (&visible[i - 1], &visible[i]),
    };
    let progress = if after.t == before.t { 0.0 } else { (pen - before.t as f64) / (after.t - before.t) as f64 };
    // No tip in a gap: the line is not there to have one.
    let (Some(from), Some(to)) = (read(before), read(after)) else { return };
    let value = from + (to - from) * progress;
    let color = if plot.hot.is_some_and(|hot| value > hot) { theme.signal } else { ink.trace };
    frame.fill_circle(color, Point { x: right, y: y(value) }, 2.5);
}

/// A track `track` wide from `from` along `middle`, filled `fraction` of the way in `colour`; with a knob where
/// the filling ends, where it is dragged along (`knob`). What is filled, its knob and the rest of the track are
/// each beside the next, none over another (their colours are not solid: where two lay over each other, it would
/// show).
#[allow(clippy::too_many_arguments)]
fn slider(frame: &dyn Canvas, theme: &Theme, colour: Color, from: f32, middle: f32, track: f32, fraction: f32, knob: bool) {
    let (tall, radius) = (theme.meter_height, theme.meter_radius);
    let filled = track * fraction.clamp(0.0, 1.0);
    let half = tall / 2.0;
    if !knob {
        frame.fill_rounded(theme.track, from, middle - half, track, tall, radius);
        if filled > 0.0 {
            frame.clip(from, middle - half, filled, tall);
            frame.fill_rounded(colour, from, middle - half, track, tall, radius);
            frame.unclip();
        }
        return;
    }
    let knob = 5.0;
    // What is filled and its knob are one shape, drawn once: from the track's rounded end along it, round the
    // knob, and back.
    let centre = Point { x: from + filled, y: middle };
    // (Where the track's edges meet the knob.)
    let meets = (knob * knob - half * half).sqrt();
    let mut shape: Vec<Point> = Vec::new();
    if filled > meets + half {
        // Its end at the left, rounded: from its lower corner up round to its upper one.
        shape.extend((0..=8).map(|turn| {
            let angle = std::f32::consts::FRAC_PI_2 + std::f32::consts::PI * turn as f32 / 8.0;
            Point { x: from + half + half * angle.cos(), y: middle + half * angle.sin() }
        }));
        // Round the knob, from where the upper edge meets it, over, to where the lower one does.
        let start = (-half).atan2(-meets);
        let round = 2.0 * std::f32::consts::PI - 2.0 * (half / knob).asin();
        shape.extend((0..=32).map(|turn| {
            let angle = start + round * turn as f32 / 32.0;
            Point { x: centre.x + knob * angle.cos(), y: centre.y + knob * angle.sin() }
        }));
        frame.fill_shape(&shape, Fill::Solid(colour));
    } else {
        frame.fill_circle(colour, centre, knob);
    }
    if track - filled > knob {
        frame.clip(from + filled + knob, middle - half, track - filled - knob, tall);
        frame.fill_rounded(theme.track, from, middle - half, track, tall, radius);
        frame.unclip();
    }
}

/// Something pressed, as the bar's are: its glyph in a square from (`left`,
/// `top`), on a plate while the pointer is over it or it is `lit`.
#[allow(clippy::too_many_arguments)]
fn pressed(frame: &dyn Canvas, scene: &Scene, hits: &mut Vec<HitBox>, hit: Hit, icon: Icon, left: f32, top: f32, lit: bool) {
    let theme = scene.theme;
    let hovered = scene.hover == Some(hit);
    if hovered || lit {
        frame.fill_rounded(theme.hover, left, top, MEDIA_ROW, MEDIA_ROW, theme.control_radius.min(MEDIA_ROW / 2.0));
    }
    frame.icon(icon, Point { x: left + MEDIA_ROW / 2.0, y: top + MEDIA_ROW / 2.0 }, 15.0, if hovered || lit { theme.text } else { theme.text2 });
    hits.push((left, top, MEDIA_ROW, MEDIA_ROW, hit));
}

fn paint_bar(frame: &dyn Canvas, scene: &Scene, layout: &Layout, hits: &mut Vec<HitBox>) {
    let theme = scene.theme;
    let bar = layout.bar();
    frame.key("bar");
    // Windows 11 sets the bar off as a slightly darker footer strip.
    if theme.skin == Skin::Fluent {
        frame.fill(theme.footer, bar.x, bar.y, bar.w, bar.h);
        frame.fill(theme.rule, bar.x, bar.y, bar.w, 1.0);
    }
    // Settings at the end, and the pin before it: glyphs of the system's
    // icon font, each centred in its button. A pinned pin stays lit.
    let top = bar.y + (bar.h - BUTTON) / 2.0;
    let settings = bar.x + bar.w - theme.bar_pad.1 - BUTTON;
    let pin = settings - BUTTON_GAP - BUTTON;
    let overlay = pin - BUTTON_GAP - BUTTON;
    let game = overlay - BUTTON_GAP - BUTTON;
    let uptime = scene.lang.duration(scene.latest().system.uptime_s);
    if !scene.buttons {
        let label = if scene.lang == Lang::Zh { format!("已开机 {uptime}") } else { format!("Up {uptime}") };
        let start = bar.x + theme.bar_pad.0;
        frame.text(&label, theme.small, theme.text2, start, bar.y + (bar.h - LINE) / 2.0, bar.w - 2.0 * start, Align::Start);
        frame.key("");
        return;
    }
    let label = if scene.lang == Lang::Zh { format!("已开机 {uptime}") } else { format!("Up {uptime}") };
    let start = bar.x + theme.bar_pad.0;
    frame.text(&label, theme.small, theme.text2, start, bar.y + (bar.h - LINE) / 2.0, game - 8.0 - start, Align::Start);
    let pin_glyph = if scene.pinned { "\u{E840}" } else { "\u{E718}" };
    // The overlay's: a window with a trace in it, lit while it is on.
    // And the game button before it, lit while a game shows (found, or asked for by hand), as a widget's is.
    let game_lit = game_shown(scene.history.last().map(std::slice::from_ref).unwrap_or_default());
    let buttons = [(game, "", Hit::Game, game_lit), (overlay, "\u{E9D9}", Hit::Overlay, scene.overlay), (pin, pin_glyph, Hit::Pin, scene.pinned), (settings, "\u{E713}", Hit::Settings, false)];
    for (left, glyph, hit, lit) in buttons {
        let hovered = scene.hover == Some(hit);
        if hovered || lit {
            frame.fill_rounded(theme.hover, left, top, BUTTON, BUTTON, theme.control_radius.min(BUTTON / 2.0));
        }
        let colour = if hovered || lit { theme.text } else { theme.text2 };
        if hit == Hit::Game {
            // (Glance's own game icon, as on a widget's button.)
            frame.icon(super::icons::Icon::Game, Point { x: left + BUTTON / 2.0, y: top + BUTTON / 2.0 }, 16.0, colour);
            hits.push((left, top, BUTTON, BUTTON, hit));
            continue;
        }
        let icon = Font::new(Family::Icons, 16.0, 400.0);
        let glyph_left = left + (BUTTON - frame.measure(glyph, icon)) / 2.0;
        frame.text(glyph, icon, colour, glyph_left, top + 8.0, BUTTON, Align::Start);
        if hit == Hit::Settings && scene.newer {
            frame.fill_circle(super::theme::accent(theme.dark), Point { x: left + BUTTON - 7.0, y: top + 7.0 }, 3.0);
        }
        hits.push((left, top, BUTTON, BUTTON, hit));
    }
    frame.key("");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn splits_lanes_into_the_shortest_columns() {
        let heights = [100.0, 200.0, 50.0, 150.0];
        assert_eq!(split(&heights, 1, 0.0), (vec![0], 500.0));
        // [100, 200] | [50, 150] beats [100] | [200, 50, 150].
        assert_eq!(split(&heights, 2, 0.0), (vec![0, 2], 300.0));
        assert_eq!(split(&heights, 3, 0.0).1, 200.0);
        // Gaps count between the lanes of a column, not after the last.
        assert_eq!(split(&heights, 2, 8.0), (vec![0, 2], 308.0));
        assert_eq!(split(&[], 1, 8.0), (vec![0], 0.0));
    }

    #[test]
    fn lays_out_a_panel_with_no_lanes() {
        let theme = Theme::new(Skin::Paper, false);
        let layout = Layout::new(Vec::new(), 1000.0, 3, &theme);
        assert_eq!(layout.columns, 1);
        assert!(layout.lanes().is_empty());
        assert_eq!(layout.height(), theme.bar_height);
    }

    #[test]
    fn takes_the_fewest_columns_that_fit() {
        let theme = Theme::new(Skin::Paper, false);
        let bar = theme.bar_height;
        let heights = vec![300.0, 300.0, 300.0];
        assert_eq!(Layout::new(heights.clone(), 900.0 + bar, 3, &theme).columns, 1);
        assert_eq!(Layout::new(heights.clone(), 700.0, 3, &theme).columns, 2);
        // Too short for even the most columns: the most, to be zoomed out.
        assert_eq!(Layout::new(heights, 200.0, 2, &theme).columns, 2);
    }

    #[test]
    fn deals_lanes_into_level_columns() {
        let theme = Theme::new(Skin::Glass, false);
        let layout = Layout::new(vec![100.0, 200.0, 50.0, 150.0], 400.0, 2, &theme);
        let boxes = layout.lanes();
        assert_eq!(layout.columns, 2);
        assert_eq!(boxes[2].x, COLUMN_WIDTH + theme.column_gap);
        // Both columns end at the same line.
        let end = |r: &Rect| r.y + r.h;
        assert_eq!(end(&boxes[1]), end(&boxes[3]));
        assert_eq!(layout.width(), 2.0 * COLUMN_WIDTH + theme.column_gap);
    }

    fn sample(clock: Option<f32>, battery: Option<u8>, fan: Option<f32>) -> Sample {
        use crate::reading::*;
        Sample {
            t: 0,
            cpu: Some(10.0),
            threads: vec![Some(10.0); 4],
            ghz: Some(3.0),
            kinds_ghz: Vec::new(),
            threads_ghz: Vec::new(),
            memory: MemorySample { used: 1 << 30, committed: 1 << 30, commit_limit: 1 << 31, cached: 0 },
            gpus: vec![GpuSample { usage: Some(5.0), engines: Some(Vec::new()), mem_used: Some(0), shared_used: Some(0), temp: Some(50.0), hot_spot: None, clock_mhz: clock, fan_rpm: None, power: None }],
            net_down: Some(0.0),
            net_up: Some(0.0),
            net_total_down: 0,
            net_total_up: 0,
            network: None,
            disk_read: Some(0.0),
            disk_write: Some(0.0),
            disk_active: Some(0.0),
            volumes: Vec::new(),
            processes: Vec::new(),
            system: SystemSample { uptime_s: 60, processes: 1, threads: 1, handles: 1 },
            battery: battery.map(|percent| BatterySample { percent, charging: false, plugged: false, seconds_left: None, watts: None, health: None }),
            cpu_sensors: None,
            board: fan.map(|rpm| BoardSensors { temps: Vec::new(), fans: vec![("1".into(), rpm)] }),
            drive_temps: Vec::new(),
            dimm_temps: Vec::new(),
            mic_muted: None,
            game: None,
            wsl: None,
            docker: None,
        }
    }

    fn info() -> StaticInfo {
        use crate::reading::GpuInfo;
        StaticInfo {
            cpu_name: "CPU".into(),
            memory_modules: None,
            memory_speed: None,
            drives: Vec::new(),
            drive_ids: Vec::new(),
            network_adapter: None,
            board: "Board".into(),
            threads: 4,
            mem_total: 1 << 32,
            gpus: vec![GpuInfo { slot: 0, name: "GPU".into(), mem_total: 1 << 30, shared_total: 0 }],
            found: Vec::new(),
        }
    }

    /// The facts of a lane: label and value of each row.
    fn facts(blocks: &[Block]) -> Vec<(String, String)> {
        blocks
            .iter()
            .filter_map(|block| match block {
                Block::Facts { rows, .. } => Some(rows.iter().map(|(label, value, _)| (label.clone(), value.clone()))),
                _ => None,
            })
            .flatten()
            .collect()
    }

    #[test]
    fn tags_the_thread_pointed_at() {
        let (info, prefs, theme) = (info(), Prefs::default(), Theme::new(Skin::Paper, false));
        let leak = |history: Vec<Sample>| -> &'static [Sample] { Box::leak(history.into_boxed_slice()) };
        let mut read = sample(None, None, None);
        read.threads_ghz = vec![Some(5.0), Some(4.5), Some(5.4), Some(4.2)];
        let history = leak(vec![read]);
        let seen = Box::leak(Box::new(Seen::of(history)));
        let scene = Scene { music_apart: false, info: &info, prefs: &prefs, theme: &theme, lang: Lang::En, history, seen, pen_ms: 0.0, process_scroll: 0.0, hover: None, pinned: false, overlay: false, buttons: true, newer: false };
        let cpu = lane(&scene, "cpu", Detail::Full).unwrap();
        let tags: Vec<&String> = cpu.iter().filter_map(|block| if let Block::Threads(cells) = block { Some(cells.iter().map(|(_, _, tag)| tag)) } else { None }).flatten().collect();
        assert_eq!(tags[2], "Thread 3 · 5.40 GHz · 10%");
    }

    #[test]
    fn shows_each_kind_of_cores_clock() {
        let (info, prefs, theme) = (info(), Prefs::default(), Theme::new(Skin::Paper, false));
        let leak = |history: Vec<Sample>| -> &'static [Sample] { Box::leak(history.into_boxed_slice()) };
        let scene = |history: &'static [Sample]| Scene { music_apart: false, info: &info, prefs: &prefs, theme: &theme, lang: Lang::En, history, seen: Box::leak(Box::new(Seen::of(history))), pen_ms: 0.0, process_scroll: 0.0, hover: None, pinned: false, overlay: false, buttons: true, newer: false };
        // Big and little cores: each kind's clock, in place of the one clock.
        let mut hybrid = sample(None, None, None);
        hybrid.kinds_ghz = vec![Some(5.2), None];
        let cpu = lane(&scene(leak(vec![hybrid])), "cpu", Detail::Full).unwrap();
        let shown = facts(&cpu);
        assert!(shown.contains(&("P-core clock".into(), "5.20 GHz".into())) && shown.contains(&("E-core clock".into(), UNREAD.into())));
        assert!(!shown.iter().any(|(label, _)| label == "Clock"));
    }

    #[test]
    fn keeps_a_reading_in_place_while_it_is_missing() {
        let (info, prefs, theme) = (info(), Prefs::default(), Theme::new(Skin::Paper, false));
        let scene = |history: &'static [Sample]| Scene { music_apart: false, info: &info, prefs: &prefs, theme: &theme, lang: Lang::En, history, seen: Box::leak(Box::new(Seen::of(history))), pen_ms: 0.0, process_scroll: 0.0, hover: None, pinned: false, overlay: false, buttons: true, newer: false };
        let leak = |history: Vec<Sample>| -> &'static [Sample] { Box::leak(history.into_boxed_slice()) };
        // The GPU's clock, the battery and a fan read a moment ago, and not now.
        let history = leak(vec![sample(Some(1350.0), Some(80), Some(900.0)), sample(None, None, None)]);
        let gpu = lane(&scene(history), "gpu:0", Detail::Full).unwrap();
        assert!(facts(&gpu).contains(&("Clock".into(), UNREAD.into())));
        let battery = lane(&scene(history), "battery", Detail::Full).expect("the battery lane stays");
        assert!(matches!(&battery[1], Block::Readout { figure, .. } if figure == UNREAD));
        let board = lane(&scene(history), "board", Detail::Full).expect("the board lane stays");
        assert_eq!(facts(&board), vec![("Fan 1".to_string(), UNREAD.to_string())]);
        // Read again: shown.
        let history = leak(vec![sample(None, None, None), sample(Some(1350.0), Some(80), Some(900.0))]);
        assert!(facts(&lane(&scene(history), "gpu:0", Detail::Full).unwrap()).contains(&("Clock".into(), "1350 MHz".into())));
        // Never read in the span: no row, no lane.
        let history = leak(vec![sample(None, None, None), sample(None, None, None)]);
        assert!(!facts(&lane(&scene(history), "gpu:0", Detail::Full).unwrap()).iter().any(|(label, _)| label == "Clock"));
        assert!(lane(&scene(history), "battery", Detail::Full).is_none());
        assert!(lane(&scene(history), "board", Detail::Full).is_none());
    }

    #[test]
    fn tells_readings_apart_by_what_they_are_of() {
        use crate::reading::{CpuSensors, DriveTemperature};
        let (info, prefs, theme) = (info(), Prefs::default(), Theme::new(Skin::Paper, false));
        let scene = |history: &'static [Sample]| Scene { music_apart: false, info: &info, prefs: &prefs, theme: &theme, lang: Lang::En, history, seen: Box::leak(Box::new(Seen::of(history))), pen_ms: 0.0, process_scroll: 0.0, hover: None, pinned: false, overlay: false, buttons: true, newer: false };
        let leak = |history: Vec<Sample>| -> &'static [Sample] { Box::leak(history.into_boxed_slice()) };
        let with = |ccds: Vec<(usize, f32)>, drives: Vec<(u32, f32)>| {
            let mut s = sample(None, None, None);
            s.cpu_sensors = Some(CpuSensors { temp: Some(50.0), ccds, power: None });
            s.drive_temps = drives.into_iter().map(|(id, celsius)| DriveTemperature { id, name: "SSD".into(), celsius }).collect();
            s
        };
        // Two drives of one model: a row each.
        let history = leak(vec![with(Vec::new(), vec![(0, 35.0), (1, 95.0)])]);
        let disk = facts(&lane(&scene(history), "disk", Detail::Full).unwrap());
        assert_eq!(&disk[..2], [("SSD".to_string(), "35 °C".to_string()), ("SSD".to_string(), "95 °C".to_string())]);
        // A drive that gives no temperature (a SATA one unread) has its row
        // all the same, by its name.
        let mut two = self::info();
        two.drives = vec!["NVMe".into(), "SATA".into()];
        two.drive_ids = vec![0, 1];
        let history = leak(vec![with(Vec::new(), vec![(0, 35.0)])]);
        let scene_two = Scene { music_apart: false, info: &two, prefs: &prefs, theme: &theme, lang: Lang::En, history, seen: Box::leak(Box::new(Seen::of(history))), pen_ms: 0.0, process_scroll: 0.0, hover: None, pinned: false, overlay: false, buttons: true, newer: false };
        let disk = facts(&lane(&scene_two, "disk", Detail::Full).unwrap());
        assert_eq!(&disk[..2], [("NVMe".to_string(), "35 °C".to_string()), ("SATA".to_string(), UNREAD.to_string())]);
        // The first chiplet missed this time: the second's reading stays its own.
        let history = leak(vec![with(vec![(0, 40.0), (1, 95.0)], Vec::new()), with(vec![(1, 95.0)], Vec::new())]);
        let cpu = facts(&lane(&scene(history), "cpu", Detail::Full).unwrap());
        assert!(cpu.contains(&("CCD 1".into(), UNREAD.into())));
        assert!(cpu.contains(&("CCD 2".into(), "95 °C".into())));
    }

    #[test]
    fn keeps_the_order_of_what_it_has_seen() {
        let fans = |names: &[&str]| {
            let mut s = sample(None, None, None);
            s.board = Some(crate::reading::BoardSensors { temps: Vec::new(), fans: names.iter().map(|n| (n.to_string(), 900.0)).collect() });
            s
        };
        let order = |history: Vec<Sample>| Seen::of(&history).board.unwrap().fans;
        // Fan 2 alone at first, then both: 1 goes before 2, as read.
        assert_eq!(order(vec![fans(&["2"]), fans(&["1", "2"])]), ["1", "2"]);
        // One missing later keeps its place, beside what came before it.
        assert_eq!(order(vec![fans(&["1", "2", "3"]), fans(&["1", "3"])]), ["1", "2", "3"]);
        assert_eq!(order(vec![fans(&["1", "2", "3"]), fans(&["2", "3"])]), ["1", "2", "3"]);
    }

    #[test]
    fn rounds_rates_up_to_a_clean_scale() {
        assert_eq!(round_up_rate(1500.0 * 1024.0), 2.0 * 1024.0 * 1024.0);
        assert_eq!(round_up_rate(30.0 * 1024.0), 50.0 * 1024.0);
        assert_eq!(round_up_rate(10.0 * 1024.0), 10.0 * 1024.0);
    }
}
