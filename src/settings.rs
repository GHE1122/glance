#[cfg(windows)]
use std::path::{Path, PathBuf};
use std::time::Duration;

use serde::{Deserialize, Serialize};
#[cfg(windows)]
use windows::Win32::System::Com::CoTaskMemFree;
#[cfg(windows)]
use windows::Win32::UI::Shell::{FOLDERID_LocalAppData, FOLDERID_RoamingAppData, SHGetKnownFolderPath, KF_FLAG_DEFAULT};

#[cfg(windows)]
const FILE: &str = "settings.json";

/// Whether Glance has saved settings in `dir` before.
#[cfg(windows)]
pub fn saved(dir: &Path) -> bool {
    dir.join(FILE).is_file()
}
/// Larger than this, a settings file is not one Glance wrote.
#[cfg(windows)]
const MOST: u64 = 1 << 20;

#[derive(Clone, Copy, Default, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Edge {
    Left,
    #[default]
    Right,
    Top,
}

/// The edges a panel can open from, in the order they are gone through.
pub const EDGES: [Edge; 3] = [Edge::Left, Edge::Top, Edge::Right];

/// What an edge opens: a component, shown in the panel it opens with the others the edge holds, in their order.
#[derive(Clone, Copy, PartialEq, Eq, Hash, Debug, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Part {
    /// The readings.
    Monitor,
    /// What plays.
    Music,
}

/// Every component there is, in the order the settings offer them.
pub const PARTS: [Part; 2] = [Part::Monitor, Part::Music];

/// One edge of a screen and what it opens (nothing: it opens no panel).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Hold {
    pub edge: Edge,
    pub parts: Vec<Part>,
}

/// What the edges of one screen open: the screen by the name that stays its own (see `screens`).
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct ScreenHolds {
    pub screen: String,
    pub holds: Vec<Hold>,
}

/// What each screen edge opens: a screen's own where it was set apart, else as every screen not set apart does
/// (every screen alike, while `same`). An edge holding anything opens it where the pointer is pushed into it, and
/// where another screen lies against it (a seam), where the pointer rests there.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct Places {
    pub same: bool,
    pub every: Vec<Hold>,
    pub screens: Vec<ScreenHolds>,
}

impl Default for Places {
    /// The readings at the right edge of every screen.
    fn default() -> Self {
        Places { same: true, every: vec![Hold { edge: Edge::Right, parts: vec![Part::Monitor] }], screens: Vec::new() }
    }
}

impl Places {
    /// What the edges of the screen named `screen` open.
    pub fn holds(&self, screen: &str) -> &[Hold] {
        match self.screens.iter().find(|entry| !self.same && entry.screen == screen) {
            Some(entry) => &entry.holds,
            None => &self.every,
        }
    }

    /// What `edge` of the screen named `screen` opens, in its order (none: nothing).
    pub fn parts(&self, screen: &str, edge: Edge) -> &[Part] {
        self.holds(screen).iter().find(|hold| hold.edge == edge).map_or(&[], |hold| &hold.parts)
    }

    /// The edges of the screen named `screen` that open a panel.
    pub fn lit(&self, screen: &str) -> Lit {
        Lit::of(&EDGES.into_iter().filter(|edge| !self.parts(screen, *edge).is_empty()).collect::<Vec<_>>())
    }

    /// Whether `edge` of any screen opens a panel.
    pub fn anywhere(&self, edge: Edge) -> bool {
        let opens = |holds: &[Hold]| holds.iter().any(|hold| hold.edge == edge && !hold.parts.is_empty());
        opens(&self.every) || (!self.same && self.screens.iter().any(|entry| opens(&entry.holds)))
    }

    /// Whether any edge of any screen opens `part`.
    pub fn shows(&self, part: Part) -> bool {
        let shows = |holds: &[Hold]| holds.iter().any(|hold| hold.parts.contains(&part));
        shows(&self.every) || (!self.same && self.screens.iter().any(|entry| shows(&entry.holds)))
    }

    /// What `edge` of the screen named `screen` opens, to be changed: that screen's own (set apart from the rest as it
    /// was, if it was not), or every screen's while they are alike.
    fn parts_mut(&mut self, screen: &str, edge: Edge) -> &mut Vec<Part> {
        let holds = if self.same {
            &mut self.every
        } else {
            let at = match self.screens.iter().position(|entry| entry.screen == screen) {
                Some(at) => at,
                None => {
                    self.screens.push(ScreenHolds { screen: screen.to_string(), holds: self.every.clone() });
                    self.screens.len() - 1
                }
            };
            &mut self.screens[at].holds
        };
        let at = match holds.iter().position(|hold| hold.edge == edge) {
            Some(at) => at,
            None => {
                holds.push(Hold { edge, parts: Vec::new() });
                holds.len() - 1
            }
        };
        &mut holds[at].parts
    }

    /// `part` put on `edge` of the screen named `screen`, at `index` among what it holds (taken from where it was
    /// there, if it was).
    pub fn put(&mut self, screen: &str, edge: Edge, index: usize, part: Part) {
        let parts = self.parts_mut(screen, edge);
        let index = match parts.iter().position(|held| *held == part) {
            Some(was) => {
                parts.remove(was);
                if was < index { index - 1 } else { index }
            }
            None => index,
        };
        parts.insert(index.min(parts.len()), part);
        self.tidy();
    }

    /// `part` taken off `edge` of the screen named `screen`.
    pub fn take(&mut self, screen: &str, edge: Edge, part: Part) {
        self.parts_mut(screen, edge).retain(|held| *held != part);
        self.tidy();
    }

    /// Every screen alike (as the screen named `screen` is now), or each set apart as it is now.
    pub fn set_same(&mut self, same: bool, screen: &str) {
        if same && !self.same {
            self.every = self.holds(screen).to_vec();
            self.screens.clear();
        }
        self.same = same;
    }

    /// Where a panel opened by hand (the shortcut, the tray) on a screen none of whose edges opens one opens, and
    /// what it shows: as the first edge of every screen that does, else of any screen; the readings at the right
    /// edge, where no edge opens anything anywhere.
    pub fn by_hand(&self) -> (Edge, Vec<Part>) {
        let first = self.every.iter().chain(self.screens.iter().flat_map(|entry| &entry.holds)).find(|hold| !hold.parts.is_empty());
        first.map_or((Edge::Right, vec![Part::Monitor]), |hold| (hold.edge, hold.parts.clone()))
    }

    /// No edge kept that holds nothing.
    fn tidy(&mut self) {
        self.every.retain(|hold| !hold.parts.is_empty());
        for entry in &mut self.screens {
            entry.holds.retain(|hold| !hold.parts.is_empty());
        }
    }
}

/// The edges of one screen that opened the panel, up to 0.3.11 (see `Places`): the screen by the name that stays
/// its own; those that opened it where they are open, to a pointer pushed into them; and those that opened it where
/// another screen lies against them (a seam), to a pointer resting there.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
pub struct ScreenEdges {
    pub screen: String,
    pub edges: Vec<Edge>,
    #[serde(default)]
    pub seams: Vec<Edge>,
}

/// Which of a screen's edges open a panel.
#[derive(Clone, Copy, Default, PartialEq, Eq, Debug)]
pub struct Lit {
    pub left: bool,
    pub top: bool,
    pub right: bool,
}

impl Lit {
    pub fn of(edges: &[Edge]) -> Lit {
        Lit { left: edges.contains(&Edge::Left), top: edges.contains(&Edge::Top), right: edges.contains(&Edge::Right) }
    }

    pub fn has(self, edge: Edge) -> bool {
        match edge {
            Edge::Left => self.left,
            Edge::Top => self.top,
            Edge::Right => self.right,
        }
    }

    pub fn each(self) -> impl Iterator<Item = Edge> {
        EDGES.into_iter().filter(move |edge| self.has(*edge))
    }

    /// The one of them nearest `point` on a screen at `monitor` (left, top,
    /// right, bottom); none, when none of its edges opens a panel.
    pub fn nearest(self, monitor: (i32, i32, i32, i32), point: (i32, i32)) -> Option<Edge> {
        let (left, top, right, _) = monitor;
        self.each().min_by_key(|edge| match edge {
            Edge::Left => point.0 - left,
            Edge::Top => point.1 - top,
            Edge::Right => right - point.0,
        })
    }
}

/// What a desktop widget was sized to show (see `arrange::Choice`): its
/// form by name ("full-2", "compact-1", "tiles-3x2", "rail", …), its
/// scale, and how much wider and taller than designed it is drawn (DIPs).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct PanelLayout {
    pub form: String,
    pub scale: f32,
    pub extra: (f32, f32),
}

/// The form a widget showing what plays is kept as, in place of the readings' (see `PanelLayout`): an older Glance,
/// which has no such form, leaves it out rather than show something else.
pub const MUSIC_FORM: &str = "music";

impl WidgetAt {
    /// Whether it shows what plays (merged into it or not).
    pub fn shows_music(&self) -> bool {
        self.layout.form == MUSIC_FORM || self.parts.iter().flatten().any(|part| part.form == MUSIC_FORM)
    }
}

/// A desktop widget as it is kept: its surface's corner on the screen
/// (physical px), what it shows, whether it is pinned (not moved or sized),
/// lets clicks through and shows only while a game runs, the side of its
/// screen it is stuck to, if it is, and what it showed before it was.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
pub struct WidgetAt {
    pub at: (i32, i32),
    pub layout: PanelLayout,
    #[serde(default)]
    pub pinned: bool,
    /// Clicks pass through it (Ctrl held, it takes them).
    #[serde(default)]
    pub click_through: bool,
    #[serde(default)]
    pub game_only: bool,
    /// Kept on the desktop, under every other window, rather than above
    /// them all.
    #[serde(default)]
    pub on_desktop: bool,
    #[serde(default)]
    pub stuck: Option<WidgetSide>,
    #[serde(default)]
    pub before: Option<PanelLayout>,
    /// How large it is (DIPs), where its size is its own (what plays) rather than its form's.
    #[serde(default)]
    pub size: Option<(f32, f32)>,
    /// Widgets merged into it: each part's form (its own first), and whether they are beside it rather than below.
    #[serde(default)]
    pub parts: Option<Vec<PanelLayout>>,
    #[serde(default)]
    pub beside: bool,
}

#[derive(Clone, Copy, Debug, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum WidgetSide {
    Left,
    Right,
    Top,
    Bottom,
}

/// Where along the edge the panel opens.
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Anchor {
    /// Centred on the pointer, as far as the screen allows.
    #[default]
    Pointer,
    Center,
}

/// A key combination: modifier keys held, and one other key, by its
/// Windows virtual-key code.
#[derive(Clone, Copy, PartialEq, Eq, Debug, Serialize, Deserialize)]
pub struct Shortcut {
    pub ctrl: bool,
    pub alt: bool,
    pub shift: bool,
    pub win: bool,
    pub key: u16,
}

impl Default for Shortcut {
    /// Ctrl+Alt+G.
    fn default() -> Self {
        Shortcut { ctrl: true, alt: true, shift: false, win: false, key: u16::from(b'G') }
    }
}

impl Shortcut {
    /// Whether it can be a shortcut: a key that is not itself a modifier,
    /// with Ctrl, Alt or Win held (Shift alone would take a key from typing).
    pub fn usable(&self) -> bool {
        // Shift, Ctrl, Alt (either side or neither) and the Windows keys.
        const MODIFIERS: [u16; 9] = [0x10, 0x11, 0x12, 0xA0, 0xA1, 0xA2, 0xA3, 0xA4, 0xA5];
        let key = self.key != 0 && !MODIFIERS.contains(&self.key) && self.key != 0x5B && self.key != 0x5C;
        key && (self.ctrl || self.alt || self.win)
    }
}

/// Where the panel was moved to, away from the screen's edge: it stays
/// there, Glance restarting too, until it is moved back to the edge.
#[derive(Clone, Copy, PartialEq, Debug, Serialize, Deserialize)]
pub struct PanelAt {
    /// How far across and down the room there is for it on its screen's
    /// work area (0 at the left or top, 1 at the right or bottom).
    pub at: (f32, f32),
    /// A point on its screen (physical pixels).
    pub screen: (i32, i32),
    /// Pinned there: not moved or sized by the pointer.
    #[serde(default)]
    pub locked: bool,
}

/// A few readings floating over the screen: all the time, or while a game
/// is played (over the game) only.
#[derive(Clone, PartialEq, Debug, Serialize, Deserialize)]
#[serde(default)]
pub struct OverlaySettings {
    /// Shown all the time.
    pub on: bool,
    /// Shown while a game is played, off or not otherwise.
    pub in_game: bool,
    /// What it shows, group by group, in order (see `ui::overlay::GROUPS`).
    pub groups: Vec<crate::ui::prefs::ModuleEntry>,
    /// What it showed up to 0.2.1, by name: read once, to carry the choices
    /// over, and not written again.
    #[serde(skip_serializing)]
    items: Option<Vec<String>>,
    /// Where it is on its screen: how far across and down the room there is
    /// for it (0 at the left or top, 1 at the right or bottom).
    pub at: (f32, f32),
    /// A point on the screen it was put on (physical pixels), where it
    /// shows, a game played or not; never put anywhere, it shows over the
    /// game's screen.
    pub screen: Option<(i32, i32)>,
    /// Locked where it is: not dragged.
    pub locked: bool,
    /// How clear its glass is: 0, all there (what is behind tinted); 1,
    /// gone (the words alone over what is behind).
    pub clear: f32,
    /// Clicks pass through it (Ctrl held, it takes them).
    pub click_through: bool,
    /// A card or a strip.
    pub layout: crate::ui::overlay::Layout,
    /// A strip's rows (see `ui::overlay::ROWS`).
    pub rows: usize,
    /// How large it is drawn, 1 as designed (0.75 to 2).
    pub size: f32,
    /// Offered once already: the first game told of it.
    pub offered: bool,
}

impl OverlaySettings {
    /// What it shows, by item, group by group.
    pub fn chosen(&self) -> Vec<&'static str> {
        crate::ui::overlay::chosen(&self.groups)
    }

    /// How its readings are laid out, on a screen `width` DIPs wide (at
    /// its size).
    pub fn shape(&self, width: f32) -> crate::ui::overlay::Shape {
        crate::ui::overlay::Shape { layout: self.layout, rows: self.rows, width }
    }
}

impl Default for OverlaySettings {
    fn default() -> Self {
        OverlaySettings {
            on: false,
            in_game: false,
            groups: crate::ui::overlay::default_groups(),
            items: None,
            at: (0.0, 0.0),
            screen: None,
            locked: false,
            clear: 0.0,
            click_through: false,
            layout: Default::default(),
            rows: 1,
            size: 1.0,
            offered: false,
        }
    }
}

/// What opens the panel over a fullscreen game, borderless or exclusive.
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum OverFullscreen {
    /// Nothing: the game keeps the screen.
    Never,
    /// The shortcut, which is asked for; a push into the edge may be an
    /// accident mid-game.
    #[default]
    Shortcut,
    /// The shortcut and a push into the edge.
    Both,
}

/// How hard the pointer has to push into the edge.
#[derive(Clone, Copy, Default, PartialEq, Eq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Sensitivity {
    /// Up to 0.2.6, pushing into the edge opened nothing; read as no edge
    /// opening the panel so (see `Settings::pushed`), and not written again.
    Off,
    Light,
    #[default]
    Medium,
    Firm,
}

impl Sensitivity {
    /// Raw mouse counts of outward travel against the edge; none when the
    /// edge opens nothing.
    pub fn pressure(self) -> Option<i32> {
        match self {
            Sensitivity::Off => None,
            Sensitivity::Light => Some(50),
            Sensitivity::Medium => Some(120),
            Sensitivity::Firm => Some(260),
        }
    }
}

/// Everything the backend acts on, plus the page's own preferences, which it
/// stores without looking inside.
#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    /// What each screen edge opens; none, in settings from before (see `carry_over`).
    #[serde(default)]
    pub places: Option<Places>,
    /// Up to 0.3.11, the edge every screen `screens` did not name opened the panel from; whether a pointer pushed
    /// into it opened it, and whether one resting where another screen lies against it did; and the screens whose
    /// edges were chosen, each with those that opened it. Read once (see `carry_over`), not written again.
    #[serde(skip_serializing)]
    edge: Edge,
    #[serde(skip_serializing)]
    pushed: bool,
    #[serde(skip_serializing)]
    seam: bool,
    #[serde(skip_serializing)]
    screens: Vec<ScreenEdges>,
    pub skin: String,
    pub anchor: Anchor,
    /// How many columns the panel's lanes are dealt into; none chosen, as
    /// few as fit the screen's height.
    pub columns: Option<usize>,
    /// How large the panel is drawn, 1 as designed (0.75 to 2).
    pub panel_size: f32,
    pub sensitivity: Sensitivity,
    pub close_delay_ms: u64,
    pub interval_ms: u64,
    /// Keep what shows behind the glass up with what changes there (see
    /// `ui::scene`).
    pub live_backdrop: bool,
    /// The panel, pulled away from its edge, is torn off as a widget; off,
    /// pulling it does nothing.
    pub tear_off: bool,
    /// Ask GitHub once a day whether a newer Glance is out.
    pub check_updates: bool,
    /// The shortcut that opens and closes the panel (Ctrl+Alt+G unless
    /// another is chosen); none, when it is cleared.
    pub shortcut: Option<Shortcut>,
    /// Up to 0.1.8, the shortcut's own switch: off, it reads as cleared.
    /// Read once, not written again.
    #[serde(skip_serializing)]
    hotkey: Option<bool>,
    /// What opens the panel over a game holding the screen in exclusive
    /// fullscreen, which the panel showing sends to the background.
    pub over_fullscreen: OverFullscreen,
    /// Tell from the tray when the CPU or a graphics card stays at or above
    /// the temperature alert.
    pub heat_alert: bool,
    /// The version that last ran: a newer one starting says it was updated.
    pub last_version: Option<String>,
    /// The day Glance first ran here (days since 1970), where it has been
    /// known since: asking its site for a newer version, Glance says how
    /// many weeks ago that was, and no more of when.
    pub began: Option<u64>,
    /// The version that last asked Glance's site for a newer one and was
    /// answered: the next to ask says which it came from, where another.
    pub asked_as: Option<String>,
    pub overlay: OverlaySettings,
    /// See `PanelAt`; none while the panel opens from the edge.
    pub panel_at: Option<PanelAt>,
    /// Pinned open at its edge, along it where this point is (physical px;
    /// its screen's): open so again as Glance starts.
    #[serde(default)]
    pub panel_pinned: Option<(i32, i32)>,
    /// The desktop widgets torn off the panel.
    pub widgets: Vec<WidgetAt>,
    pub view: serde_json::Value,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            places: Some(Places::default()),
            edge: Edge::default(),
            pushed: true,
            seam: false,
            screens: Vec::new(),
            skin: "paper".into(),
            anchor: Anchor::default(),
            columns: None,
            panel_size: 1.0,
            sensitivity: Sensitivity::default(),
            close_delay_ms: 200,
            interval_ms: 1000,
            live_backdrop: false,
            tear_off: true,
            check_updates: true,
            shortcut: Some(Shortcut::default()),
            hotkey: None,
            over_fullscreen: OverFullscreen::default(),
            heat_alert: false,
            last_version: None,
            began: None,
            asked_as: None,
            overlay: OverlaySettings::default(),
            panel_at: None,
            panel_pinned: None,
            widgets: Vec::new(),
            view: serde_json::Value::Null,
        }
    }
}

impl Settings {
    /// What each screen edge opens.
    pub fn places(&self) -> &Places {
        self.places.as_ref().expect("places carried over as the settings were read")
    }

    /// What each screen edge opens, to be changed.
    pub fn places_mut(&mut self) -> &mut Places {
        self.places.as_mut().expect("places carried over as the settings were read")
    }

    pub fn close_delay(&self) -> Duration {
        Duration::from_millis(self.close_delay_ms)
    }

    pub fn interval(&self) -> Duration {
        Duration::from_millis(self.interval_ms)
    }
}

/// Where Glance keeps its settings: the user's roaming application data,
/// under Glance's identifier (a debug build's, under its own, so it never
/// rewrites an installed Glance's).
#[cfg(windows)]
pub fn config_dir() -> PathBuf {
    own_folder_in(&FOLDERID_RoamingAppData)
}

/// Where Glance keeps what it can fetch again (what it has fetched, so as not to ask again): the user's local
/// application data, which goes with no roaming profile, under Glance's identifier as `config_dir`.
#[cfg(windows)]
pub fn local_dir() -> PathBuf {
    own_folder_in(&FOLDERID_LocalAppData)
}

#[cfg(windows)]
fn own_folder_in(known: &windows::core::GUID) -> PathBuf {
    let folder = unsafe { SHGetKnownFolderPath(known, KF_FLAG_DEFAULT, None) }.expect("application data folder");
    let path = PathBuf::from(unsafe { folder.to_string() }.expect("folder path"));
    unsafe { CoTaskMemFree(Some(folder.0 as *const _)) };
    path.join(if cfg!(debug_assertions) { "dev.weiyi.glance.debug" } else { "dev.weiyi.glance" })
}

#[cfg(windows)]
impl Settings {
    /// A missing or hand-edited file that no longer parses means defaults;
    /// numbers outside what the settings offer are brought within it.
    /// Read and written so that Glance, running elevated, never follows a
    /// link someone put in the user's folder (see `elevation::read_in_place`).
    pub fn load(dir: &Path) -> Self {
        let kept = crate::elevation::read_in_place(dir, FILE, MOST);
        // With none kept, this is where Glance begins here.
        let begins = kept.is_none().then(|| std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map_or(0, |since| since.as_secs() / 86400));
        let mut settings: Settings = kept.and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        if begins.is_some() {
            settings.began = begins;
        }
        // A number too large to be one (it reads as infinite, and is written
        // as none at all): not a file Glance wrote.
        if serde_json::to_value(&settings).and_then(serde_json::from_value::<Settings>).is_err() {
            settings = Settings::default();
        }
        settings.interval_ms = settings.interval_ms.clamp(250, 10_000);
        settings.close_delay_ms = settings.close_delay_ms.min(10_000);
        settings.overlay.clear = settings.overlay.clear.clamp(0.0, 1.0);
        settings.carry_over();
        settings
    }

    /// Settings from before, in today's terms; a shortcut that cannot be one
    /// (a file not written by Glance) is the default.
    fn carry_over(&mut self) {
        if let Some(items) = self.overlay.items.take() {
            self.overlay.groups = crate::ui::overlay::carried_over(&items);
        }
        crate::ui::overlay::complete(&mut self.overlay.groups);
        if self.sensitivity == Sensitivity::Off {
            self.pushed = false;
            self.sensitivity = Sensitivity::default();
        }
        if self.places.is_none() {
            self.places = Some(self.places_before());
        }
        if self.hotkey.take() == Some(false) {
            self.shortcut = None;
        }
        if self.shortcut.is_some_and(|shortcut| !shortcut.usable()) {
            self.shortcut = Some(Shortcut::default());
        }
    }

    /// What each screen edge opens, as settings from before 0.3.12 had the panel open: from the edges that opened
    /// it (pushed into, or rested on where screens meet), the readings with what plays where its lane was on (before
    /// the readings, where it was the first lane shown, else after them).
    fn places_before(&self) -> Places {
        // (As kept: the modules on in their order, a module not listed off.)
        let modules = self.view["modules"].as_array().map_or(&[][..], Vec::as_slice);
        let shown: Vec<&str> = modules.iter().filter(|module| module["on"] == true).filter_map(|module| module["id"].as_str()).collect();
        let parts = match shown.iter().position(|id| *id == "lyrics") {
            Some(0) if shown.len() == 1 => vec![Part::Music],
            Some(0) => vec![Part::Music, Part::Monitor],
            Some(_) => vec![Part::Monitor, Part::Music],
            None => vec![Part::Monitor],
        };
        let holds = |edges: &[Edge]| edges.iter().map(|edge| Hold { edge: *edge, parts: parts.clone() }).collect::<Vec<_>>();
        let opened = |screen: &ScreenEdges| {
            let mut edges: Vec<Edge> = screen.edges.clone();
            edges.extend(screen.seams.iter().filter(|edge| !screen.edges.contains(edge)));
            edges
        };
        Places {
            same: self.screens.is_empty(),
            every: if self.pushed || self.seam { holds(&[self.edge]) } else { Vec::new() },
            screens: self.screens.iter().map(|screen| ScreenHolds { screen: screen.screen.clone(), holds: holds(&opened(screen)) }).collect(),
        }
    }

    pub fn save(&self, dir: &Path) -> std::io::Result<()> {
        crate::elevation::ensure_folder(dir)?;
        crate::elevation::write_in_place(dir, FILE, serde_json::to_string_pretty(self).unwrap().as_bytes())
    }
}

/// `held` (the settings as they are now) with what `mine` changed from
/// `base` (as they were when `mine` was taken from them): a change made
/// elsewhere meanwhile stands unless `mine` changed the same setting.
/// Compared setting by setting, down to each preference in `view`.
pub fn merged(held: &Settings, base: &Settings, mine: &Settings) -> Settings {
    fn merge(held: &mut serde_json::Value, base: &serde_json::Value, mine: &serde_json::Value) {
        use serde_json::Value;
        match (held, base, mine) {
            (Value::Object(held), Value::Object(base), Value::Object(mine)) => {
                for (key, value) in mine {
                    match (held.get_mut(key), base.get(key)) {
                        (Some(held), Some(base)) => merge(held, base, value),
                        (_, Some(base)) if base == value => {}
                        _ => {
                            held.insert(key.clone(), value.clone());
                        }
                    }
                }
                // Taken out by `mine`.
                for key in base.keys().filter(|key| !mine.contains_key(*key)) {
                    held.remove(key);
                }
            }
            (held, base, mine) => {
                if base != mine {
                    *held = mine.clone();
                }
            }
        }
    }
    let mut value = serde_json::to_value(held).unwrap();
    merge(&mut value, &serde_json::to_value(base).unwrap(), &serde_json::to_value(mine).unwrap());
    serde_json::from_value(value).expect("settings merged from settings")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_change_made_elsewhere_stands() {
        let base = Settings::default();
        // The overlay moved from the overlay, a preference changed from the
        // panel, while the settings window, from `base`, changed the size.
        let mut held = base.clone();
        held.overlay.at = (0.25, 0.75);
        held.view = serde_json::json!({ "language": "zh" });
        let mut mine = base.clone();
        mine.panel_size = 1.5;
        let merged = merged(&held, &base, &mine);
        assert_eq!((merged.overlay.at, merged.panel_size), ((0.25, 0.75), 1.5));
        assert_eq!(merged.view["language"], "zh");
        // The same setting changed in both: the window's, made last, stands.
        mine.overlay.at = (1.0, 0.0);
        assert_eq!(super::merged(&held, &base, &mine).overlay.at, (1.0, 0.0));
    }

    #[test]
    fn takes_only_shortcuts_that_leave_typing_alone() {
        let key = |ctrl, alt, shift, win, key| Shortcut { ctrl, alt, shift, win, key }.usable();
        assert!(Shortcut::default().usable());
        assert!(key(true, false, true, false, u16::from(b'K')));
        assert!(key(false, false, false, true, 0x70));
        // A key alone, or with Shift alone, is typing.
        assert!(!key(false, false, false, false, u16::from(b'K')));
        assert!(!key(false, false, true, false, u16::from(b'K')));
        // Modifiers alone are no shortcut.
        assert!(!key(true, true, false, false, 0x12));
        assert!(!key(true, false, false, false, 0x5B));
    }

    #[test]
    fn each_edge_opens_what_it_holds() {
        use Part::{Monitor, Music};
        // At first, the readings at the right edge of every screen.
        let mut places = Places::default();
        assert_eq!(places.parts("a", Edge::Right), [Monitor]);
        assert_eq!(places.lit("b"), Lit::of(&[Edge::Right]));
        // Every screen alike: what one is given, all are.
        places.put("a", Edge::Left, 0, Music);
        assert_eq!(places.parts("b", Edge::Left), [Music]);
        // Put before the readings on the same edge, then moved after them: in that order, once.
        places.put("a", Edge::Right, 0, Music);
        assert_eq!(places.parts("a", Edge::Right), [Music, Monitor]);
        places.put("a", Edge::Right, 2, Music);
        assert_eq!(places.parts("a", Edge::Right), [Monitor, Music]);
        // Each set apart: one screen changed, the others as they were.
        places.set_same(false, "a");
        places.take("a", Edge::Left, Music);
        assert_eq!(places.parts("a", Edge::Left), [] as [Part; 0]);
        assert_eq!(places.parts("b", Edge::Left), [Music]);
        assert!(!places.anywhere(Edge::Top));
        // The last taken off an edge: it opens nothing, and is not kept; a screen not set apart opens it still.
        places.take("b", Edge::Left, Music);
        assert!(!places.lit("b").has(Edge::Left));
        assert!(places.lit("c").has(Edge::Left));
        assert!(places.screens.iter().all(|entry| entry.holds.iter().all(|hold| !hold.parts.is_empty())));
        // Alike again, as the screen it was made so from.
        places.put("b", Edge::Top, 0, Music);
        places.set_same(true, "a");
        assert_eq!(places.parts("b", Edge::Top), [] as [Part; 0]);
        assert_eq!(places.lit("b"), Lit::of(&[Edge::Right]));
        // Opened by hand where no edge opens anything: as the first edge that does.
        assert_eq!(places.by_hand(), (Edge::Right, vec![Monitor, Music]));
        assert_eq!(Places { same: true, every: Vec::new(), screens: Vec::new() }.by_hand(), (Edge::Right, vec![Monitor]));
        // Opened by hand: from the nearest edge.
        let both = Lit::of(&[Edge::Left, Edge::Right]);
        assert_eq!(both.nearest((0, 0, 1000, 500), (700, 20)), Some(Edge::Right));
        assert_eq!(both.nearest((0, 0, 1000, 500), (300, 20)), Some(Edge::Left));
        assert_eq!(Lit::default().nearest((0, 0, 1000, 500), (300, 20)), None);
    }

    #[test]
    fn edges_chosen_before_hold_what_the_panel_showed() {
        use Part::{Monitor, Music};
        let read = |json: &str| {
            let mut settings: Settings = serde_json::from_str(json).unwrap();
            settings.carry_over();
            settings
        };
        // The one edge every screen had: the readings.
        let settings = read(r#"{"edge":"left"}"#);
        assert_eq!(settings.places(), &Places { same: true, every: vec![Hold { edge: Edge::Left, parts: vec![Monitor] }], screens: Vec::new() });
        // Pushing turned off, and screens' seams not: nothing.
        let settings = read(r#"{"edge":"top","sensitivity":"off"}"#);
        assert!(settings.places().every.is_empty());
        assert!(settings.sensitivity == Sensitivity::default());
        // Where screens meet, alone: it opens there.
        let settings = read(r#"{"edge":"top","sensitivity":"off","seam":true}"#);
        assert_eq!(settings.places().parts("a", Edge::Top), [Monitor]);
        // Screens chosen apart, their edges and seams together; what plays where its lane was on, before the
        // readings where it was the first lane shown.
        let settings = read(r#"{"screens":[{"screen":"a","edges":["left"],"seams":["right"]}],"view":{"modules":[{"id":"cpu","on":true},{"id":"lyrics","on":true}]}}"#);
        assert!(!settings.places().same);
        assert_eq!(settings.places().parts("a", Edge::Left), [Monitor, Music]);
        assert_eq!(settings.places().parts("a", Edge::Right), [Monitor, Music]);
        assert_eq!(settings.places().parts("b", Edge::Right), [Monitor, Music]);
        let settings = read(r#"{"view":{"modules":[{"id":"cpu","on":false},{"id":"lyrics","on":true},{"id":"gpu:0","on":true}]}}"#);
        assert_eq!(settings.places().parts("a", Edge::Right), [Music, Monitor]);
        let settings = read(r#"{"view":{"modules":[{"id":"lyrics","on":true},{"id":"cpu","on":false}]}}"#);
        assert_eq!(settings.places().parts("a", Edge::Right), [Music]);
        // Written as places, the old fields not again; read back as written.
        let written = serde_json::to_string(&settings).unwrap();
        assert!(written.contains("\"places\"") && !written.contains("\"pushed\""));
        assert_eq!(read(&written).places(), settings.places());
        assert!(Settings::default().places().shows(Monitor));
    }

    #[test]
    fn a_number_too_large_is_none_glance_wrote() {
        // Read as infinite, it would be written as nothing, and what was
        // written not read again (see `merged`).
        let settings: Settings = serde_json::from_str(r#"{"overlay":{"clear":1e39}}"#).unwrap();
        assert!(serde_json::to_value(&settings).and_then(serde_json::from_value::<Settings>).is_err());
        let sound = Settings::default();
        assert!(serde_json::to_value(&sound).and_then(serde_json::from_value::<Settings>).is_ok());
    }

    #[test]
    fn settings_from_before_take_the_defaults() {
        let mut settings: Settings = serde_json::from_str(r#"{"edge":"right","hotkey":true}"#).unwrap();
        settings.carry_over();
        assert_eq!(settings.shortcut, Some(Shortcut::default()));
        assert!(settings.over_fullscreen == OverFullscreen::Shortcut);
        assert_eq!(settings.columns, None);
        // The shortcut switched off before is cleared; one cleared stays so,
        // and its switch is not written again.
        let mut settings: Settings = serde_json::from_str(r#"{"hotkey":false}"#).unwrap();
        settings.carry_over();
        assert_eq!(settings.shortcut, None);
        let mut settings: Settings = serde_json::from_str(r#"{"shortcut":null}"#).unwrap();
        settings.carry_over();
        assert_eq!(settings.shortcut, None);
        assert!(!serde_json::to_string(&settings).unwrap().contains("hotkey"));
        // The overlay's readings chosen by name, in groups now; the names
        // not written again.
        let mut settings: Settings = serde_json::from_str(r#"{"overlay":{"items":["gpu","network"],"style":"plate"}}"#).unwrap();
        settings.carry_over();
        assert_eq!(settings.overlay.chosen(), ["gpu", "down", "up"]);
        assert!(!serde_json::to_string(&settings).unwrap().contains("\"items\":[\""));
    }
}
