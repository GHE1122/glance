//! A note written (see docs/station-and-notes.md, sections 三 and 六): its words in a box the text services write (see
//! `editor`: the keys, the input method, selection, undo and the clipboard theirs), dressed as their Markdown reads,
//! kept in its file as they are typed (a moment after the last key), its to-dos ticked and its links followed where they
//! show. Written on its page beside the panel (see `beside`), or where it is stuck on the desktop, on the widget it is
//! there (see `widget`): each draws it as the other does, its face here.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use windows::Win32::Foundation::{COLORREF, HWND, LRESULT, POINT, RECT};
use windows::Win32::UI::Input::KeyboardAndMouse::VK_ESCAPE;
use windows::Win32::UI::WindowsAndMessaging::*;

use super::canvas::{Align, Canvas, Color, Family, Font, Inks, Point};
use super::editor::{Editor, Palette};
use super::icons::Icon;
use super::text::Lang;
use super::theme::{Skin, Theme};

/// How high a note's words are written (DIPs).
pub const WORDS: f32 = 15.0;
/// The room a note's page leaves at its sides, round its words, its head's and its foot's, and how tall its head and its foot
/// are (DIPs).
pub const PAD: f32 = 20.0;
pub const HEAD: f32 = 52.0;
pub const FOOT: f32 = 48.0;
/// A button in a note's head (its menu, its page's close): how large, how far down, and how far in from its right edge
/// the first is (the next further in by as much again and a gap).
pub const BUTTON: f32 = 28.0;
const BUTTON_TOP: f32 = 12.0;
const BUTTON_IN: f32 = 16.0;
/// How long after the last key a note is kept.
pub const KEEP_AFTER_MS: u32 = 600;

/// `color` as Windows' GDI writes one.
fn colorref(color: Color) -> COLORREF {
    COLORREF(((color.b * 255.0).round() as u32) << 16 | ((color.g * 255.0).round() as u32) << 8 | (color.r * 255.0).round() as u32)
}

/// What a note's words are written in (`pixels` high), its colour `colour`: as its card's on the panel are (glass's in
/// the ink of its colour), and as opaque colours (the words' box takes none other: a faint ink half see through came out
/// as solid as the words, and Markdown's faint marks no fainter), as they show over its page.
pub(super) fn palette_for(theme: &Theme, colour: crate::notes::Colour, pixels: f32) -> Palette {
    let inks = word_inks(theme, colour);
    Palette { text: colorref(inks.words), faint: colorref(inks.faint), accent: colorref(inks.accent), points: pixels * 0.75, face: "Microsoft YaHei UI".into(), code_face: super::canvas::CODE_FACE.into() }
}

/// The inks a note of colour `colour` is written in on its page (see `palette_for`), opaque.
pub(super) fn word_inks(theme: &Theme, colour: crate::notes::Colour) -> Inks {
    // (The skin's own, as its head and foot are written: a card's own ink, glass's, is for its card's light pane, and
    // on a page's, lighter over a page's breadth, it was dark on dark in the dark. Its colour is the page's tint.)
    let _ = colour;
    let (text, faint) = (theme.text, theme.text3);
    let ground = page_under(theme);
    let opaque = |ink: Color| super::view::over(Color { a: 1.0, ..ground }, ink);
    let accent = match theme.skin {
        Skin::Paper => theme.text,
        _ => super::theme::accent(theme.dark),
    };
    Inks { words: opaque(text), faint: opaque(faint), accent: opaque(accent) }
}

/// What a note's page is reckoned to lie over, its inks made opaque over (the box takes colours with no see-through):
/// the skin's sheet.
pub(super) fn page_under(theme: &Theme) -> Color {
    match theme.skin {
        Skin::Paper => theme.paper,
        _ if theme.dark => Color::hex(0x26282E, 1.0),
        _ => Color::hex(0xF5F6F8, 1.0),
    }
}

/// What is said at the head of a note's page of when it was written (`when`, how long ago, or none: it is new).
fn said(when: Option<&str>, lang: Lang) -> String {
    match (when, lang) {
        (None, Lang::Zh) => "新便签".to_string(),
        (None, Lang::En) => "New note".to_string(),
        (Some(when), Lang::Zh) => format!("{when} 修改"),
        (Some(when), Lang::En) => format!("Written {when}"),
    }
}

/// What is said at the foot of a note's page of its words `text`: how many characters, kept as it is written.
fn counted(text: &str, lang: Lang) -> String {
    let count = text.chars().filter(|c| !c.is_whitespace()).count();
    match lang {
        Lang::Zh => format!("{count} 字 · 自动保存"),
        Lang::En => format!("{count} characters · kept as you write"),
    }
}

/// A note's page's face, `width` × `height` from (`x`, `y`), but for what is pressed on it and its words: its ground of
/// its colour (see `view::note_page`), at its head its colour's dot and what `said` says of when it was written, at its
/// foot, ruled off, what `counted` says of its words and its file's `name` in the room left (none: the foot left for
/// what was said of keeping it). Its words are written between (see `words_in`).
#[allow(clippy::too_many_arguments)]
fn face(frame: &dyn Canvas, theme: &Theme, colour: crate::notes::Colour, said: &str, counted: Option<&str>, name: Option<&str>, (x, y, width, height): (f32, f32, f32, f32)) {
    super::view::note_page(frame, theme, colour, (x, y, width, height));
    frame.fill_circle(super::view::note_colour(colour), Point { x: x + PAD + 5.0, y: y + 26.0 }, 5.0);
    frame.text(said, theme.small, theme.text2, x + PAD + 18.0, y + 18.0, width - 2.0 * PAD - 80.0, Align::Start);
    let bottom = y + height - FOOT;
    frame.fill(theme.rule, x + PAD, bottom, width - 2.0 * PAD, 1.0);
    let Some(counted) = counted else { return };
    frame.text(counted, theme.small, theme.text3, x + PAD, bottom + (FOOT - 16.0) / 2.0, width - 2.0 * PAD, Align::Start);
    let room = width - 2.0 * PAD - frame.measure(counted, theme.small) - 16.0;
    // (Where it fits: shortened past a few letters, a name says nothing.)
    if let Some(name) = name.filter(|_| room >= 96.0) {
        frame.text(name, theme.small, theme.text3, x + width - PAD - room, bottom + (FOOT - 16.0) / 2.0, room, Align::End);
    }
}

/// Where a note's words are written on its page `width` × `height` (DIPs, from its corner): under its head, over its
/// foot (as `words_box` places its box).
pub fn words_in(width: f32, height: f32) -> (f32, f32, f32, f32) {
    (PAD, HEAD + 4.0, width - 2.0 * PAD, height - HEAD - 4.0 - FOOT)
}

/// Where a note's words are written in a window (physical px), its page `wide` × `tall` from (`left`, `top`) in it: under
/// its head, over its foot.
pub fn words_box(left: i32, top: i32, wide: i32, tall: i32, scale: f32) -> RECT {
    RECT { left: left + (PAD * scale) as i32, top: top + ((HEAD + 4.0) * scale) as i32, right: left + wide - (PAD * scale) as i32, bottom: top + tall - (FOOT * scale) as i32 }
}

/// Where the `nth` button of a note's head is (its right edge's first) on its page `width` wide from (`x`, `y`): its left,
/// its top, its width and its height.
pub fn button_at(nth: usize, (x, y, width): (f32, f32, f32)) -> (f32, f32, f32, f32) {
    (x + width - BUTTON_IN - BUTTON - nth as f32 * (BUTTON + 4.0), y + BUTTON_TOP, BUTTON, BUTTON)
}

/// A button of a note's head at `place` (see `button_at`): its mark, on a plate where the pointer is on it (`lit`).
pub fn paint_button(frame: &dyn Canvas, theme: &Theme, place: (f32, f32, f32, f32), icon: Icon, lit: bool) {
    if lit {
        frame.fill_rounded(theme.hover, place.0, place.1, place.2, place.3, theme.control_radius.min(14.0));
    }
    frame.icon(icon, Point { x: place.0 + place.2 / 2.0, y: place.1 + place.3 / 2.0 }, 16.0, theme.text2);
}

/// A note stuck on the desktop, `width` × `height` from (`x`, `y`), not written in: its page as it is written in (see
/// `Writing::paint`) but for what is pressed there and the caret, its words as the page writes them: its words `text`, its
/// colour `colour`, written `when` (how long ago), kept in the file `name`.
#[allow(clippy::too_many_arguments)]
pub(crate) fn paint_stuck(frame: &dyn Canvas, theme: &Theme, lang: Lang, text: &str, colour: crate::notes::Colour, when: &str, name: &str, (x, y, width, height): (f32, f32, f32, f32)) {
    face(frame, theme, colour, &said(Some(when), lang), Some(&counted(text, lang)), Some(name), (x, y, width, height));
    let (left, top, wide, tall) = words_in(width, height);
    frame.markdown(text, Font::new(Family::YaHei, WORDS, 400.0), word_inks(theme, colour), x + left, y + top, wide, tall);
}

/// Where a to-do's box is drawn, in the room drawn for it from `left`, by its line's `baseline` (in the window: see
/// `Editor::character`, `Editor::baseline`), its words `pixels` high: its left, its top and its side.
pub(super) fn box_in(left: i32, baseline: i32, pixels: f32) -> (f32, f32, f32) {
    use crate::notes::markdown::{BOX_IN, BOX_RISE, BOX_SIDE};
    let side = BOX_SIDE * pixels;
    (left as f32 + BOX_IN * pixels, baseline as f32 - BOX_RISE * pixels - side / 2.0, side)
}

/// What a note's menu does: its colour chosen, it stuck on the desktop, or its card there taken off it (the note kept),
/// it taken to the Recycle Bin.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    Colour(crate::notes::Colour),
    Stick,
    Unstick,
    TakeAway,
}

/// A note's menu, of colour `colour`: its colours, each a dot of it, and kept in a file (`named`), it stuck on the desktop or
/// (`stuck` there) taken off it, and it taken away. Its rows, and what each does.
pub fn menu(lang: Lang, colour: crate::notes::Colour, named: bool, stuck: bool) -> (Vec<super::menu::Item<'static>>, Vec<Choice>) {
    use crate::notes::Colour;
    use super::menu::{Item, Mark};
    let names = [(Colour::Blue, lang.pick("蓝色", "Blue")), (Colour::Yellow, lang.pick("黄色", "Yellow")), (Colour::Green, lang.pick("绿色", "Green")), (Colour::Pink, lang.pick("粉色", "Pink")), (Colour::Purple, lang.pick("紫色", "Purple")), (Colour::Grey, lang.pick("灰色", "Grey"))];
    let mut rows: Vec<(Choice, &'static str, Mark, bool, bool)> = names.iter().map(|(each, name)| (Choice::Colour(*each), *name, Mark::Colour(super::view::note_colour(*each)), colour == *each, false)).collect();
    if named {
        match stuck {
            true => rows.push((Choice::Unstick, lang.pick("从桌面取下", "Take off the desktop"), Mark::Icon(Icon::PinOff), false, true)),
            false => rows.push((Choice::Stick, lang.pick("贴到桌面", "Stick on the desktop"), Mark::Icon(Icon::Pin), false, true)),
        }
        rows.push((Choice::TakeAway, lang.pick("删除（移到回收站）", "Delete (to the Recycle Bin)"), Mark::Icon(Icon::Close), false, false));
    }
    let items = rows.iter().map(|(_, label, mark, checked, rule_before)| Item { label, mark: Some(*mark), checked: *checked, rule_before: *rule_before }).collect();
    (items, rows.into_iter().map(|(choice, ..)| choice).collect())
}

/// How the note written shows away from the lines written on, for its words' box's messages (see `Writing::answer`),
/// which come whatever its window is in the middle of: as it was last dressed, and the folder it is in.
struct Reads {
    shown: crate::notes::markdown::Shown,
    written: std::ops::RangeInclusive<usize>,
    folder: std::path::PathBuf,
}

/// What a note's words' box answers its window's messages with (see `Writing::answer`).
pub use super::keys::Answer;

/// A note as it is written: its file's name (none till it is first kept), its file as last read or written here, its
/// colour, its words' box, whether what is written is not yet kept, and what was last said of keeping it.
pub struct Writing {
    /// The folder it is in (kept to: the folder chosen in the settings changed meanwhile, it is kept where it was read).
    pub folder: std::path::PathBuf,
    pub name: Option<String>,
    stamp: Option<crate::notes::store::Stamp>,
    pub colour: crate::notes::Colour,
    written_ms: u64,
    pub editor: Rc<Editor>,
    /// What its Markdown is dressed in (see `Editor::dress`).
    palette: Palette,
    pub unkept: bool,
    pub said: Option<String>,
    /// How its Markdown shows away from the lines written on, as it was last dressed, and those lines (see
    /// `notes::markdown::Shown`), the same for its box's messages (see `Reads`).
    shown: crate::notes::markdown::Shown,
    written: std::ops::RangeInclusive<usize>,
    reads: Rc<RefCell<Reads>>,
    /// What was chosen in it as it opened (the caret), and whether it has been written in or the caret moved since: till
    /// then no line is written on, and it shows as the note did where it was pressed (its marks put away).
    opened_at: (i32, i32),
    pub touched: bool,
    /// Its box was given a key, or the input method's words, since this was last asked (see `touched`), and how deep its
    /// box's messages are in it now (one comes inside another: see `answering`).
    keyed: Rc<Cell<bool>>,
    depth: Rc<Cell<u32>>,
}

impl Writing {
    /// The note `name` in `folder` (none: a new one, taking the next colour round), its words in a box at `rect` in the
    /// window `hwnd`, `pixels` high, in `theme`'s inks, the caret at its start. None where the text services cannot be had.
    pub fn open(hwnd: HWND, rect: RECT, folder: &crate::notes::store::Folder, name: Option<String>, theme: &Theme, pixels: f32) -> Option<Writing> {
        let read = name.as_ref().and_then(|name| folder.read(name));
        let shown = crate::notes::shown();
        let colour = match &name {
            Some(name) => shown.iter().find(|note| &note.name == name).map(|note| note.colour).unwrap_or_default(),
            // (A new note takes the next colour round.)
            None => crate::notes::Colour::ALL[shown.len() % crate::notes::Colour::ALL.len()],
        };
        let palette = palette_for(theme, colour, pixels);
        let editor = Editor::new(hwnd, rect, &palette.face, pixels, palette.text)?;
        if let Some((text, _)) = &read {
            editor.set_text(text);
        }
        let editor = Rc::new(editor);
        let opened_at = editor.selection();
        let mut writing = Writing {
            folder: folder.path.clone(),
            name,
            written_ms: read.as_ref().map_or_else(crate::station::now_ms, |(_, stamp)| stamp.written_ms),
            stamp: read.map(|(_, stamp)| stamp),
            colour,
            editor,
            palette,
            unkept: false,
            said: None,
            shown: Default::default(),
            written: crate::notes::markdown::NONE_WRITTEN,
            reads: Rc::new(RefCell::new(Reads { shown: Default::default(), written: crate::notes::markdown::NONE_WRITTEN, folder: folder.path.clone() })),
            opened_at,
            touched: false,
            keyed: Rc::new(Cell::new(false)),
            depth: Rc::new(Cell::new(0)),
        };
        writing.dress();
        Some(writing)
    }

    /// Its words dressed as their Markdown reads, its marks put away off the lines written on (those of the caret, or
    /// of what is chosen, once it has been written in or the caret moved: see `touched`).
    pub fn dress(&mut self) {
        use crate::notes::markdown::{written_lines, Shown, NONE_WRITTEN};
        let text = self.editor.text();
        let shown = Shown::of(&text);
        let (from, to) = self.editor.selection();
        let written = if self.touched { written_lines(&text, (from.max(0) as usize, to.max(0) as usize)) } else { NONE_WRITTEN };
        let mut hidden = shown.hidden(&written);
        hidden.extend(shown.boxes(&written).map(|boxed| boxed.marks.clone()));
        let rooms: Vec<usize> = shown.boxes(&written).map(|boxed| boxed.room).collect();
        self.editor.dress(&crate::notes::markdown::looks(&text), &hidden, &rooms, &self.palette);
        if let Ok(mut reads) = self.reads.try_borrow_mut() {
            *reads = Reads { shown: shown.clone(), written: written.clone(), folder: self.folder.clone() };
        }
        self.shown = shown;
        self.written = written;
    }

    /// The lines written on now (see `dress`).
    fn written_now(&self) -> std::ops::RangeInclusive<usize> {
        let (from, to) = self.editor.selection();
        match self.touched {
            true => crate::notes::markdown::written_lines(&self.editor.text(), (from.max(0) as usize, to.max(0) as usize)),
            false => crate::notes::markdown::NONE_WRITTEN,
        }
    }

    /// Its words written `pixels` high in `theme`'s inks (its window's scale changed, or its skin), dressed again so.
    pub fn restyle(&mut self, theme: &Theme, pixels: f32) {
        self.palette = palette_for(theme, self.colour, pixels);
        self.dress();
    }

    /// Of colour `colour` (chosen in its menu: its words in the ink of its colour, where its card's are), kept so in
    /// its folder where it is in a file.
    pub fn recolour(&mut self, theme: &Theme, colour: crate::notes::Colour) {
        self.colour = colour;
        self.palette = palette_for(theme, colour, self.palette.points / 0.75);
        self.dress();
        if let Some(name) = &self.name {
            let _ = crate::notes::folder_at(self.folder.clone()).colour(name, colour);
        }
        crate::notes::read_again();
    }

    /// How high its words are written (pixels).
    pub fn pixels(&self) -> f32 {
        self.palette.points / 0.75
    }

    /// What its words' box answers its window's messages with, given it whatever the window is in the middle of: an input
    /// method answers inside the message that asked it (the box placing its windows as it is drawn, say), and a
    /// composition the box ends (a press on its words) comes back inside that press. (Given only while the window was
    /// free, those went past the box: a composition it ended never ended in it, its words left in it as they were shown,
    /// the syllables' marks with them.) See `Answer`: Esc is the window's.
    ///
    /// A to-do's box pressed is ticked, or no longer (as if typed: undo takes it back): drawn as a box (off the lines
    /// written on), pressed anywhere in its room, written out, on its `[ ]`. A link's words, where they show as it reads,
    /// pressed, are followed (a web address, or a file of the PC's). Over them the pointer is the hand.
    pub fn answer(&self) -> Answer {
        let (editor, reads, keyed, depth) = (self.editor.clone(), self.reads.clone(), self.keyed.clone(), self.depth.clone());
        Rc::new(move |hwnd, message, wparam, lparam| {
            let point = POINT { x: (lparam.0 & 0xFFFF) as i16 as i32, y: ((lparam.0 >> 16) & 0xFFFF) as i16 as i32 };
            let pointer = || {
                let mut at = POINT::default();
                unsafe {
                    let _ = GetCursorPos(&mut at);
                    let _ = windows::Win32::Graphics::Gdi::ScreenToClient(hwnd, &mut at);
                }
                at
            };
            let its = match message {
                // (Esc is the window's: it leaves the words.)
                WM_KEYDOWN => wparam.0 != usize::from(VK_ESCAPE.0),
                WM_KEYUP | WM_CHAR | WM_SYSCHAR | WM_UNICHAR | WM_SETFOCUS | WM_KILLFOCUS => true,
                WM_IME_STARTCOMPOSITION | WM_IME_COMPOSITION | WM_IME_ENDCOMPOSITION | WM_IME_NOTIFY | WM_IME_SETCONTEXT | WM_IME_CHAR | WM_IME_REQUEST | WM_IME_COMPOSITIONFULL | WM_IME_SELECT => true,
                WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_RBUTTONDOWN => editor.holds(point),
                // (A press begun in it is followed till let go, wherever the pointer goes.)
                WM_LBUTTONUP | WM_MOUSEMOVE => editor.holds(point) || unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetCapture() } == hwnd,
                WM_SETCURSOR => (lparam.0 & 0xFFFF) as u32 == HTCLIENT && editor.holds(pointer()),
                WM_MOUSEWHEEL | WM_TIMER => true,
                _ => false,
            };
            if !its {
                return None;
            }
            if matches!(message, WM_KEYDOWN | WM_CHAR | WM_IME_COMPOSITION) {
                keyed.set(true);
            }
            depth.set(depth.get() + 1);
            let ticked = (message == WM_LBUTTONDOWN)
                .then(|| {
                    let drawn = reads.try_borrow().ok().and_then(|reads| {
                        reads.shown.boxes(&reads.written).find_map(|boxed| {
                            let room = editor.character(boxed.room)?;
                            (point.x >= room.left && point.x < room.right && point.y >= room.top && point.y < room.bottom).then(|| (boxed.marks.end - 3..boxed.marks.end, boxed.done))
                        })
                    });
                    drawn.or_else(|| {
                        let at = editor.at(point);
                        crate::notes::markdown::tasks(&editor.text()).into_iter().find(|(range, _)| range.start <= at && at < range.end)
                    })
                })
                .flatten();
            let link_under = |point: POINT| {
                let at = editor.at(point);
                let on = editor.character(at).is_some_and(|drawn| point.x >= drawn.left && point.x < drawn.right && point.y >= drawn.top && point.y < drawn.bottom);
                reads.try_borrow().ok().filter(|_| on).and_then(|reads| reads.shown.link_at(at, &reads.written).map(|link| (link.to.clone(), reads.folder.clone())))
            };
            let followed = match (ticked.is_none(), message) {
                (true, WM_LBUTTONDOWN) => link_under(point),
                _ => None,
            };
            let answered = match (ticked, followed, message) {
                (Some((range, done)), _, _) => {
                    editor.replace(range.start, range.end, if done { "[ ]" } else { "[x]" });
                    Some(LRESULT(0))
                }
                (None, Some((to, folder)), _) => {
                    crate::notes::follow_link(&to, &folder);
                    Some(LRESULT(0))
                }
                // (Its shape asked of the text services as their host asks it: see `Editor::set_cursor`. Over a link that
                // would be followed, the hand.)
                (None, None, WM_SETCURSOR) => {
                    match link_under(pointer()) {
                        Some(_) => unsafe {
                            SetCursor(LoadCursorW(None, IDC_HAND).ok());
                        },
                        None => editor.set_cursor(pointer()),
                    }
                    Some(LRESULT(1))
                }
                (None, None, _) => editor.send(message, wparam, lparam),
            };
            depth.set(depth.get() - 1);
            // (The input method's, unanswered by the box, are the system's as well.)
            Some(match (answered, message) {
                (None, WM_IME_STARTCOMPOSITION | WM_IME_COMPOSITION | WM_IME_ENDCOMPOSITION | WM_IME_NOTIFY | WM_IME_SETCONTEXT | WM_IME_CHAR | WM_IME_REQUEST) => None,
                (answered, _) => Some(answered.unwrap_or(LRESULT(0))),
            })
        })
    }

    /// Whether its box is in the middle of a message (see `answer`): it is not to go now.
    pub fn answering(&self) -> bool {
        self.depth.get() > 0
    }

    /// Its own part of what was written in its box: its words dressed as their Markdown reads (not while the input
    /// method composes: its words are its till they are written), what is written marked as not kept. Whether it changed
    /// (to be kept a moment after: see `KEEP_AFTER_MS`).
    pub fn written(&mut self) -> bool {
        let composed = self.editor.take_composed();
        let changed = self.editor.take_changed();
        // (Written in, or the caret moved: the lines it is on written on from now.)
        let keyed = self.keyed.replace(false);
        let touched = !self.touched && (keyed || composed || self.editor.selection() != self.opened_at);
        self.touched |= touched;
        let composing = self.editor.composing();
        if composed || (!composing && (changed || touched || self.written_now() != self.written)) {
            self.dress();
        }
        if changed {
            self.unkept = true;
            self.said = None;
        }
        changed
    }

    /// Kept in its file, where what is written is not kept yet (see `notes::Folder::write`): a new one named by its title
    /// as it is first kept (an empty one never), one changed outside meanwhile kept apart (said so). Whether all that was
    /// written is kept now (a failure leaves it not kept, saying so).
    pub fn keep(&mut self, lang: Lang) -> bool {
        if !self.unkept {
            return true;
        }
        let text = self.editor.text();
        let folder = crate::notes::folder_at(self.folder.clone());
        let name = match &self.name {
            Some(name) => name.clone(),
            None if text.trim().is_empty() => {
                self.unkept = false;
                return true;
            }
            None => {
                let title = crate::notes::Note { name: String::new(), text: text.clone(), colour: self.colour, stamp: Default::default() }.title();
                let today = super::text::Local::at(crate::station::now_ms());
                let named = folder.name_for(&title, &match lang {
                    Lang::Zh => format!("便签 {}月{}日", today.month, today.day),
                    Lang::En => format!("Note {}-{}", today.month, today.day),
                });
                let _ = folder.colour(&named, self.colour);
                named
            }
        };
        match folder.write(&name, &text, self.stamp) {
            Ok(crate::notes::Written::Done(stamp)) => {
                self.name = Some(name);
                self.stamp = Some(stamp);
                self.written_ms = stamp.written_ms;
            }
            Ok(crate::notes::Written::Apart(apart, stamp)) => {
                self.said = Some(match lang {
                    Lang::Zh => format!("这条便签在别处被改过或删除，你写的已另存为“{apart}”"),
                    Lang::En => format!("Changed or deleted elsewhere meanwhile: what you wrote is kept as \"{apart}\""),
                });
                // (Its stamp of the bytes written here: read again, another's change meanwhile would be taken for them.)
                self.stamp = Some(stamp);
                self.name = Some(apart);
            }
            Err(_) => {
                self.said = Some(lang.pick("未能保存，请检查便签文件夹，稍后将再次尝试", "Could not be saved. Check the notes folder, it will be tried again").to_string());
                return false;
            }
        }
        self.unkept = false;
        crate::notes::read_again();
        true
    }

    /// Glance going (or the note's window), with what is written not kept and not to be: put beside Glance's settings,
    /// in "unsaved notes", not lost with it (and said so in the journal).
    pub fn put_aside(&self) {
        let stem = self.name.as_deref().and_then(|name| std::path::Path::new(name).file_stem()).map_or_else(|| "note".to_string(), |stem| stem.to_string_lossy().into_owned());
        let dir = crate::settings::config_dir().join("unsaved notes");
        let path = dir.join(format!("{stem} {}.md", crate::station::now_ms()));
        let put = std::fs::create_dir_all(&dir).and_then(|()| std::fs::write(&path, self.editor.text()));
        crate::journal::note(match put {
            Ok(()) => format!("note: what was written could not be kept; it is in {}", path.display()),
            Err(error) => format!("note: what was written could not be kept, nor put in {} ({error})", path.display()),
        });
    }

    /// As its file is now, where that was changed outside and nothing written here waits to be kept (or is being
    /// composed): read again, what was chosen kept as far as the words go. (With something waiting, it is kept apart as it
    /// is kept: see `keep`. Deleted outside: left as it is, kept apart likewise.)
    pub fn read_outside(&mut self) {
        if self.unkept || self.editor.composing() {
            return;
        }
        let Some(name) = self.name.clone() else { return };
        let Some((text, stamp)) = crate::notes::folder_at(self.folder.clone()).read(&name) else { return };
        if self.stamp.is_some_and(|was| was.same(&stamp)) {
            return;
        }
        let chosen = self.editor.selection();
        self.editor.set_text(&text);
        self.editor.select(chosen);
        self.stamp = Some(stamp);
        self.written_ms = stamp.written_ms;
        self.dress();
    }

    /// Whether the notes as last read show it as it is written here (kept, and read again since): drawn from them, it
    /// shows as it does here.
    pub fn read_as_written(&self) -> bool {
        let Some(name) = &self.name else { return !self.unkept };
        !self.unkept && crate::notes::shown().iter().find(|note| &note.name == name).is_some_and(|note| note.text.replace("\r\n", "\n").trim_end() == self.editor.text().trim_end())
    }

    /// Its page `width` × `height` from (`x`, `y`) on `frame` (DIPs), its window's corner at `window` on it and `scale`
    /// pixels a DIP: its face (as a note stuck on the desktop is, see `paint_stuck`: of its colour as its card on the
    /// panel is, its dot by when it was written, at its foot how many characters, kept as it is written, and where, or in
    /// place of them what was said of keeping it, in the foot's whole width, on two lines where it needs them), its words
    /// as the text services draw them, its to-dos' boxes in the rooms made for them (off the lines written on), and the
    /// caret where they say it is (`caret_on`: in its blink, shown).
    #[allow(clippy::too_many_arguments)]
    pub fn paint(&self, frame: &super::gfx::Frame, theme: &Theme, lang: Lang, (x, y, width, height): (f32, f32, f32, f32), window: (f32, f32), scale: f32, caret_on: bool) {
        let when = super::text::since(self.written_ms, crate::station::now_ms(), lang);
        let said_now = said(self.name.as_ref().map(|_| when.as_str()), lang);
        let counted = counted(&self.editor.text(), lang);
        face(frame, theme, self.colour, &said_now, self.said.is_none().then_some(counted.as_str()), self.name.as_deref(), (x, y, width, height));
        if let Some(said) = &self.said {
            let bottom = y + height - FOOT;
            let tall = frame.gfx.wrapped_height(said, theme.small, width - 2.0 * PAD).min(FOOT - 8.0);
            frame.text_wrapped(said, theme.small, theme.signal, x + PAD, bottom + (FOOT - tall) / 2.0, width - 2.0 * PAD);
        }
        // (Its box placed in its window: drawn from the window's corner.)
        let in_window = |px: f32, py: f32| (window.0 + px / scale, window.1 + py / scale);
        let mut was = windows_numerics::Matrix3x2::default();
        unsafe {
            frame.dc.GetTransform(&mut was);
            frame.dc.SetTransform(&(windows_numerics::Matrix3x2::translation(window.0, window.1) * was));
        }
        frame.in_pixels(|target| self.editor.draw(target));
        unsafe { frame.dc.SetTransform(&was) };
        let pixels = self.pixels();
        let inks = word_inks(theme, self.colour);
        let bounds = self.editor.bounds();
        let (left, top) = in_window(bounds.left as f32, bounds.top as f32);
        let (right, bottom) = in_window(bounds.right as f32, bounds.bottom as f32);
        frame.clip(left, top, right - left, bottom - top);
        for boxed in self.shown.boxes(&self.written) {
            let (Some(room), Some(baseline)) = (self.editor.character(boxed.room), self.editor.baseline(boxed.room + 1)) else { continue };
            let (bx, by, side) = box_in(room.left, baseline, pixels);
            let (bx, by) = in_window(bx, by);
            super::canvas::task_box(frame, inks, bx, by, side / scale, boxed.done);
        }
        frame.unclip();
        if let (Some((cx, cy, cw, ch)), true) = (self.editor.caret(), caret_on) {
            let (cx, cy) = in_window(cx as f32, cy as f32);
            frame.fill(theme.text, cx, cy, (cw as f32 / scale).max(1.2), ch as f32 / scale);
        }
    }
}

#[cfg(all(test, feature = "studio"))]
mod pictures {
    use super::*;

    /// A note written, and as it is drawn not written in (stuck on the desktop), in each skin: each line's words
    /// beginning where they do on the other (written in where it is, its words do not move along their lines), its marks
    /// put away (a heading's `#`, emphasis's, code's, a link's), a to-do a box, and once its to-do's line is written on,
    /// that line as written. (Down the page, the text services set lines a little further apart than DirectWrite, as
    /// they always have: within a few pixels after a few lines.) (`cargo test --features studio --lib -- --ignored
    /// written_and_not` writes `target/beside/note-<skin>-<face|page|written>.png`, the note in a folder of the test's own.)
    #[test]
    #[ignore = "writes pictures to look at"]
    fn written_and_not() {
        use windows::core::w;
        use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_POPUP};
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let gfx = super::super::gfx::current().unwrap();
        let out = std::path::PathBuf::from(format!("{}/target/beside", env!("CARGO_MANIFEST_DIR")));
        std::fs::create_dir_all(&out).unwrap();
        let folder = std::env::temp_dir().join(format!("glance-note-pictures-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        let text = "# 本周要做\n- [ ] 买**牛奶**和面包\n- [x] 交 `周报`\n看 [文档](https://a.b) 或 https://x.y/z\n> 引一句话\n- 列表一项";
        std::fs::write(folder.join("a.md"), text).unwrap();
        let (w, h) = super::super::view::STUCK_NOTE;
        let scale = 1.5;
        let size = ((w * scale) as u32, (h * scale) as u32);
        let hwnd = unsafe { CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"), w!(""), WS_POPUP, 0, 0, size.0 as i32, size.1 as i32, None, None, None, None) }.unwrap();
        let mut unlike = Vec::new();
        for (skin, name) in [(Skin::Glass, "glass"), (Skin::Fluent, "fluent"), (Skin::Paper, "paper")] {
            let theme = Theme::new(skin, false);
            let mut writing = Writing::open(hwnd, words_box(0, 0, size.0 as i32, size.1 as i32, scale), &crate::notes::store::Folder { path: folder.clone(), apart: " (conflicted copy)" }, Some("a.md".into()), &theme, WORDS * scale).unwrap();
            let now = crate::station::now_ms();
            let when = super::super::text::since(writing.written_ms, now, Lang::Zh);
            let face = gfx.draw_offscreen(size, scale, |frame| paint_stuck(frame, &theme, Lang::Zh, text, writing.colour, &when, "a.md", (0.0, 0.0, w, h))).unwrap();
            crate::studio::save_png(&out.join(format!("note-{name}-face.png")), size.0, size.1, &face).unwrap();
            let page = gfx.draw_offscreen(size, scale, |frame| writing.paint(frame, &theme, Lang::Zh, (0.0, 0.0, w, h), (0.0, 0.0), scale, false)).unwrap();
            crate::studio::save_png(&out.join(format!("note-{name}-page.png")), size.0, size.1, &page).unwrap();
            // Each line's words (rows with ink, over white) beginning where they do on the other.
            let (left, top, wide, tall) = words_in(w, h);
            let lines = |pixels: &[u8]| {
                let inked = |x: u32, y: u32| {
                    let at = ((y * size.0 + x) * 4) as usize;
                    u32::from(pixels[at + 1]) + 255 - u32::from(pixels[at + 3]) < 140
                };
                let (xs, ys) = ((left * scale) as u32..((left + wide) * scale) as u32, (top * scale) as u32..((top + tall) * scale) as u32);
                let mut found: Vec<u32> = Vec::new();
                let mut in_line = false;
                for y in ys {
                    let first = xs.clone().find(|&x| inked(x, y));
                    match (first, in_line) {
                        (Some(x), false) => {
                            found.push(x);
                            in_line = true;
                        }
                        (Some(x), true) => {
                            let last = found.last_mut().unwrap();
                            *last = (*last).min(x);
                        }
                        (None, _) => in_line = false,
                    }
                }
                found
            };
            let (on_face, on_page) = (lines(&face), lines(&page));
            let apart = on_face.len() != on_page.len() || on_face.iter().zip(&on_page).any(|(a, b)| a.abs_diff(*b) > 2);
            unlike.push((name, apart, on_face, on_page));
            // Written on: the caret on its to-do's line, that line as it is written.
            writing.touched = true;
            let at = text.encode_utf16().position(|c| c == u16::from(b'-')).unwrap() as i32 + 3;
            writing.editor.select((at, at));
            writing.dress();
            let written = gfx.draw_offscreen(size, scale, |frame| writing.paint(frame, &theme, Lang::Zh, (0.0, 0.0, w, h), (0.0, 0.0), scale, false)).unwrap();
            crate::studio::save_png(&out.join(format!("note-{name}-written.png")), size.0, size.1, &written).unwrap();
        }
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        let _ = std::fs::remove_dir_all(&folder);
        println!("where each line's words begin, not written in and written: {unlike:?}");
        assert!(unlike.iter().all(|(_, apart, ..)| !apart), "{unlike:?}");
    }
}

#[cfg(test)]
mod tests {
    /// A note's words, in every skin, light and dark: its faint marks (Markdown's) fainter than its words, not as solid
    /// (the box takes colours with no see-through: a faint ink given as one came out as the words' own).
    #[test]
    fn a_note_s_faint_marks_are_fainter() {
        use super::super::theme::{Skin, Theme};
        for skin in [Skin::Glass, Skin::Fluent, Skin::Paper] {
            for dark in [false, true] {
                let palette = super::palette_for(&Theme::new(skin, dark), crate::notes::Colour::Blue, 15.0);
                assert_ne!(palette.text.0, palette.faint.0, "{skin:?} {dark}");
            }
        }
    }

    /// A note's words on its page, in every skin, light and dark, every colour: as easily read as text is asked to be
    /// (4.5 to 1 against what is under them, the page drawn over its sheet). On glass in the dark the card's own dark
    /// ink, on a page's lighter pane, read at about 2 to 1.
    #[test]
    #[cfg(feature = "studio")]
    fn a_note_s_words_read_on_its_page() {
        use super::super::canvas::{Canvas, Color};
        use super::super::theme::{Skin, Theme};
        let luminance = |c: Color| {
            let lin = |v: f32| if v <= 0.04045 { v / 12.92 } else { ((v + 0.055) / 1.055).powf(2.4) };
            0.2126 * lin(c.r) + 0.7152 * lin(c.g) + 0.0722 * lin(c.b)
        };
        let gfx = super::super::gfx::Gfx::new().expect("graphics");
        for skin in [Skin::Glass, Skin::Fluent, Skin::Paper] {
            for dark in [false, true] {
                let theme = Theme::new(skin, dark);
                for colour in crate::notes::Colour::ALL {
                    let pixels = gfx
                        .draw_offscreen((40, 40), 1.0, |frame| {
                            frame.fill(super::page_under(&theme), 0.0, 0.0, 40.0, 40.0);
                            super::super::view::note_page(frame, &theme, colour, (0.0, 0.0, 40.0, 40.0));
                        })
                        .unwrap();
                    let at = (20 * 40 + 20) * 4;
                    let ground = Color { r: pixels[at + 2] as f32 / 255.0, g: pixels[at + 1] as f32 / 255.0, b: pixels[at] as f32 / 255.0, a: 1.0 };
                    let words = super::word_inks(&theme, colour).words;
                    let (a, b) = (luminance(ground), luminance(words));
                    let contrast = (a.max(b) + 0.05) / (a.min(b) + 0.05);
                    assert!(contrast >= 4.5, "{skin:?} dark {dark} {colour:?}: words {words:?} on {ground:?}, {contrast:.2} to 1");
                }
            }
        }
    }
}
