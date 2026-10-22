//! A note written (see docs/station-and-notes.md, sections 三 and 六): its words in a box the text services write (see
//! `editor`: the keys, the input method, selection, undo and the clipboard theirs), dressed as their Markdown reads,
//! kept in its file as they are typed (a moment after the last key), its to-dos ticked and its links followed where they
//! show. Written on its page beside the panel (see `beside`), or where it is stuck on the desktop, on the widget it is
//! there (see `widget`): each draws it as the other does, its face here.

use std::cell::{Cell, RefCell};
use std::rc::Rc;

use windows::Win32::Foundation::{COLORREF, HWND, LRESULT, POINT, RECT};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    GetKeyState, VIRTUAL_KEY, VK_0, VK_1, VK_2, VK_3, VK_A, VK_B, VK_BACK, VK_C, VK_CONTROL, VK_DOWN, VK_E, VK_APPS, VK_ESCAPE, VK_F10, VK_I, VK_INSERT, VK_K, VK_MENU, VK_RETURN, VK_SHIFT, VK_TAB, VK_UP, VK_V, VK_X, VK_Y, VK_Z,
};
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
/// it taken to the Recycle Bin. Or its writing menu's (see `writing_menu`): something done to its words.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Choice {
    Colour(crate::notes::Colour),
    Stick,
    Unstick,
    TakeAway,
    Write(Write),
}

/// What a note's writing menu does to its words (see `Writing::write`): what is chosen cut or copied, the clipboard's
/// text pasted, a block inserted (see `notes::edit::insert`), a link made, the lines chosen made headings of a level
/// (0: words again).
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Write {
    Cut,
    Copy,
    Paste,
    Insert(crate::notes::edit::Block),
    Link,
    Heading(u8),
}

/// A note's writing menu, shown on its words pressed with the right button (or the menu key, Shift+F10) while it is
/// written in: cut, copy and paste, the blocks inserted and a link, the headings and words again. Its rows, and what
/// each does.
pub fn writing_menu(lang: Lang) -> (Vec<super::menu::Item<'static>>, Vec<Choice>) {
    use super::menu::{Item, Mark};
    use crate::notes::edit::Block;
    let rows = [
        (Write::Cut, lang.pick("剪切", "Cut"), Icon::Scissors, false),
        (Write::Copy, lang.pick("复制", "Copy"), Icon::Copy, false),
        (Write::Paste, lang.pick("粘贴", "Paste"), Icon::ClipboardPaste, false),
        (Write::Insert(Block::Todo), lang.pick("插入待办事项", "Insert a to-do"), Icon::SquareCheck, true),
        (Write::Insert(Block::Table), lang.pick("插入表格", "Insert a table"), Icon::Table, false),
        (Write::Insert(Block::Code), lang.pick("插入代码块", "Insert a code block"), Icon::SquareCode, false),
        (Write::Insert(Block::Rule), lang.pick("插入分隔线", "Insert a divider"), Icon::Minus, false),
        (Write::Link, lang.pick("插入链接", "Insert a link"), Icon::Link, false),
        (Write::Heading(1), lang.pick("一级标题", "Heading 1"), Icon::Heading1, true),
        (Write::Heading(2), lang.pick("二级标题", "Heading 2"), Icon::Heading2, false),
        (Write::Heading(3), lang.pick("三级标题", "Heading 3"), Icon::Heading3, false),
        (Write::Heading(0), lang.pick("正文", "Body text"), Icon::Pilcrow, false),
    ];
    let items = rows.iter().map(|(_, label, icon, rule_before)| Item { label, mark: Some(Mark::Icon(*icon)), checked: false, rule_before: *rule_before }).collect();
    (items, rows.into_iter().map(|(write, ..)| Choice::Write(write)).collect())
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
        rows.push((Choice::TakeAway, lang.pick("删除（移到回收站）", "Delete (to the Recycle Bin)"), Mark::Icon(Icon::Trash), false, false));
    }
    let items = rows.iter().map(|(_, label, mark, checked, rule_before)| Item { label, mark: Some(*mark), checked: *checked, rule_before: *rule_before }).collect();
    (items, rows.into_iter().map(|(choice, ..)| choice).collect())
}

/// How the note written shows where it is not written at, for its words' box's messages (see `Writing::answer`), which
/// come whatever its window is in the middle of: as it was last dressed, where it was written at then, and the folder it
/// is in.
struct Reads {
    shown: crate::notes::markdown::Shown,
    at: crate::notes::markdown::At,
    folder: std::path::PathBuf,
    laid: Laid,
    dressed: Rc<[u16]>,
}

/// A note's tables drawn laid out where it is written at (see `notes::markdown::Shown::tables_drawn`), each laid out in
/// its words' box (pixels: see `Gfx::table_laid`).
type Laid = Vec<(crate::notes::table::Table, super::gfx::TableLaid)>;

/// How what was dressed has moved in a note's words as they are now (the input method's words composed in them, not
/// yet dressed: see `Writing::written`): the first unit that changed, the first after the change (as it was), and how
/// much longer the words grew.
struct Moved {
    before: usize,
    after: usize,
    by: isize,
}

impl Moved {
    /// From `was` (UTF-16 units, as dressed) to `now`.
    fn between(was: &[u16], now: &[u16]) -> Moved {
        let before = was.iter().zip(now).take_while(|(a, b)| a == b).count();
        let after = was[before..].iter().rev().zip(now[before..].iter().rev()).take_while(|(a, b)| a == b).count();
        Moved { before, after: was.len() - after, by: now.len() as isize - was.len() as isize }
    }

    /// Where the unit `at` of the words as dressed is now: before the change where it was, after it moved on with it,
    /// in what changed nowhere.
    fn at(&self, at: usize) -> Option<usize> {
        if at < self.before {
            Some(at)
        } else if at >= self.after {
            Some((at as isize + self.by) as usize)
        } else {
            None
        }
    }
}

/// The face a note's tables' words are written in, as its words are (see `palette_for`, `paint_stuck`).
const TABLE_FAMILY: Family = Family::YaHei;

/// The top of the line the character at `at` (UTF-16 units of `units`, its words) is on in `editor` (in the window),
/// found by its line's break (the line's words may be put away, and nowhere drawn: its break is drawn), where it is
/// drawn or would be (scrolled out of the box).
fn line_top(editor: &Editor, units: &[u16], at: usize) -> Option<i32> {
    let end = (at..units.len()).find(|&unit| units[unit] == u16::from(b'\n'))?;
    editor.character_anywhere(end).map(|drawn| drawn.top)
}

/// The words of the cell of a table drawn laid out in `editor` (`laid`, as its words were dressed: `dressed`) that
/// `point` (in the window) is on: none off them.
fn cell_at(editor: &Editor, laid: &Laid, dressed: &[u16], point: POINT) -> Option<std::ops::Range<usize>> {
    let left = editor.bounds().left;
    let now: Vec<u16> = editor.text().encode_utf16().collect();
    let moved = Moved::between(dressed, &now);
    laid.iter().find_map(|(table, laid)| {
        let top = line_top(editor, &now, moved.at(table.span.start)?)?;
        let laid = &laid.laid;
        let (x, y) = ((point.x - left) as f32, (point.y - top) as f32);
        if x < 0.0 || y < 0.0 || x >= laid.width() || y >= laid.height() {
            return None;
        }
        // (The one whose room it is in, a line counted with the one after it: the last's, past the end.)
        let nth = |lengths: &[f32], along: f32| {
            let mut end = 0.0;
            lengths.iter().position(|length| {
                end += length;
                along < end
            })
            .unwrap_or(lengths.len() - 1)
        };
        let (row, column) = (nth(&laid.rows, y), nth(&laid.columns, x));
        let words = table.rows.get(row)?.get(column)?.words.clone();
        Some(moved.at(words.start)?..moved.at(words.end)?)
    })
}

/// What a note's words' box answers its window's messages with (see `Writing::answer`).
pub use super::keys::Answer;

/// The keys held as another goes down.
#[derive(Clone, Copy, Default)]
struct Held {
    ctrl: bool,
    shift: bool,
    alt: bool,
}

/// Whether `key` writes a character (a letter, a digit, a mark): with Ctrl held, one of the text services' own keys
/// that dress the words (bold, centred, spaced) where it is not one a note's (see `pressed`).
fn writes(key: VIRTUAL_KEY) -> bool {
    matches!(key.0, 0x30..=0x39 | 0x41..=0x5A | 0x60..=0x6F | 0xBA..=0xC0 | 0xDB..=0xDF | 0xE2)
}

/// What `key` going down does to the words as Markdown is written (see `notes::edit`), `text` written and `chosen`
/// chosen, `held` held with it (`system`: told as a system key, Alt held): an edit made (`Some(Some)`), or nothing done
/// (`Some(None)`: the key and the character it makes eaten), or none: the key the text services' as always.
///
/// Enter carries a list on (Shift+Enter a line break), Ctrl+Enter ticks a to-do, Tab and Shift+Tab move a list's items in
/// and out, Backspace takes an item's mark off, Alt+Up and Alt+Down move lines. Ctrl+B, Ctrl+I, Ctrl+Shift+X and Ctrl+E
/// put on and take off bold, slanted, struck through and code, Ctrl+K makes a link (to the web address on the clipboard,
/// `clipboard` asked only then, for its text), Ctrl+1 to 3 headings and Ctrl+0 none. Ctrl+V and Shift+Insert paste what is
/// on the clipboard as Markdown, words among others or blocks (`clipboard` asked for it so: see `clipboard_pasted`,
/// `edit::paste`: the text services' own paste would bring what a note's file does not keep). Ctrl with any
/// other key that writes is eaten but for choosing all, copying, cutting, undoing and redoing: the text services' own
/// would dress the words in ways a note's file does not keep.
fn pressed(text: &str, chosen: (usize, usize), key: VIRTUAL_KEY, held: Held, system: bool, clipboard: impl FnOnce(Taken) -> Option<crate::notes::edit::Pasted>) -> Option<Option<crate::notes::edit::Edit>> {
    use crate::notes::edit::{self, Marked};
    let caret = (chosen.0 == chosen.1).then_some(chosen.0);
    if system || held.alt {
        return match key {
            VK_UP | VK_DOWN if held.alt && !held.ctrl && !held.shift => Some(edit::move_lines(text, chosen, key == VK_UP)),
            _ => None,
        };
    }
    let marked = |how| Some(Some(edit::toggle(text, chosen, how)));
    match (key, held.ctrl, held.shift) {
        (VK_RETURN, false, false) => caret.and_then(|caret| edit::enter(text, caret)).map(Some),
        (VK_RETURN, true, false) => Some(edit::toggle_task(text, chosen.0)),
        (VK_TAB, false, back) => crate::notes::table::tab(text, chosen, back).or_else(|| edit::indent(text, chosen, back)),
        (VK_BACK, false, false) => caret.and_then(|caret| edit::backspace(text, caret)).map(Some),
        (VK_INSERT, false, true) => Some(clipboard(Taken::Pasted).map(|pasted| edit::paste(text, chosen, &pasted))),
        (_, false, _) => None,
        (VK_B, true, false) => marked(Marked::Bold),
        (VK_I, true, false) => marked(Marked::Italic),
        (VK_E, true, false) => marked(Marked::Code),
        (VK_X, true, true) => marked(Marked::Struck),
        (VK_K, true, false) => Some(edit::link(text, chosen, clipboard(Taken::Text).as_ref().and_then(|taken| edit::web_address(&taken.text)))),
        (VK_0 | VK_1 | VK_2 | VK_3, true, false) => Some(Some(edit::heading(text, chosen, (key.0 - VK_0.0) as u8))),
        (VK_V, true, false) => Some(clipboard(Taken::Pasted).map(|pasted| edit::paste(text, chosen, &pasted))),
        (VK_A | VK_C | VK_X | VK_Z | VK_Y, true, false) | (VK_Z, true, true) => None,
        (key, true, _) if writes(key) => Some(None),
        _ => None,
    }
}

/// What a button of the format bar puts on the words chosen: marks, or a link.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Format {
    Marked(crate::notes::edit::Marked),
    Link,
}

/// The format bar's buttons (see docs/station-and-notes.md, section 九 6): bold, slanted, struck through, code, link.
const BAR: [(Icon, Format); 5] = {
    use crate::notes::edit::Marked;
    [(Icon::Bold, Format::Marked(Marked::Bold)), (Icon::Italic, Format::Marked(Marked::Italic)), (Icon::Strikethrough, Format::Marked(Marked::Struck)), (Icon::Code, Format::Marked(Marked::Code)), (Icon::Link, Format::Link)]
};
/// How large each of its buttons is, how far in from its edge they are, and how far it is from the words chosen (DIPs,
/// for words `WORDS` high: larger as they are).
const BAR_BUTTON: f32 = 28.0;
const BAR_INSET: f32 = 4.0;
const BAR_GAP: f32 = 6.0;

/// Where the format bar shows (window pixels), a note's words written `pixels` high in `editor`: over what is chosen
/// (its first character), under it where the words' box has no room above, within the box. While something is chosen,
/// not while the pointer is choosing it (`pressing`), nor while the input method composes. None: it does not show.
fn bar_at(editor: &Editor, pixels: f32, pressing: bool) -> Option<RECT> {
    let (from, to) = editor.selection();
    if from == to || pressing || editor.composing() {
        return None;
    }
    let per = pixels / WORDS;
    let (wide, tall) = (((BAR.len() as f32 * BAR_BUTTON + 2.0 * BAR_INSET) * per).round() as i32, ((BAR_BUTTON + 2.0 * BAR_INSET) * per).round() as i32);
    let gap = (BAR_GAP * per).round() as i32;
    let bounds = editor.bounds();
    let (from, to) = (from.min(to) as usize, from.max(to) as usize);
    let first = editor.character(from)?;
    let top = match first.top - gap - tall {
        above if above >= bounds.top => above,
        _ => editor.character(to - 1).map_or(first.bottom, |last| last.bottom) + gap,
    };
    let left = first.left.min(bounds.right - wide).max(bounds.left);
    let top = top.min(bounds.bottom - tall).max(bounds.top);
    Some(RECT { left, top, right: left + wide, bottom: top + tall })
}

/// Which of the format bar's buttons (at `bar`, for words `pixels` high) `point` is on: none between them, at its edge.
fn bar_button(bar: &RECT, point: POINT, pixels: f32) -> Option<usize> {
    let per = pixels / WORDS;
    let (x, y) = (point.x as f32 - bar.left as f32 - BAR_INSET * per, point.y as f32 - bar.top as f32 - BAR_INSET * per);
    let nth = (x / (BAR_BUTTON * per)).floor();
    (x >= 0.0 && y >= 0.0 && y < BAR_BUTTON * per && nth < BAR.len() as f32).then_some(nth as usize)
}

/// What is taken off the clipboard: its text as it is (a link made of a web address), or what is on it as Markdown,
/// to be pasted (see `clipboard_pasted`).
#[derive(Clone, Copy, Debug, PartialEq)]
enum Taken {
    Text,
    Pasted,
}

/// The clipboard's `format`, if it holds it: its data as far as the zero that ends it (a unit `unit` bytes long, 1 for
/// bytes, 2 for UTF-16).
fn clipboard_data(owner: HWND, format: u32, unit: usize) -> Option<Vec<u8>> {
    use windows::Win32::Foundation::HGLOBAL;
    use windows::Win32::System::DataExchange::{GetClipboardData, IsClipboardFormatAvailable};
    use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
    if unsafe { IsClipboardFormatAvailable(format) }.is_err() {
        return None;
    }
    let mut taken = Vec::new();
    let _opened = crate::clipboard::open(owner)?;
    unsafe {
        if let Ok(held) = GetClipboardData(format) {
            let memory = HGLOBAL(held.0);
            let size = GlobalSize(memory);
            let bytes = GlobalLock(memory) as *const u8;
            if !bytes.is_null() {
                taken = std::slice::from_raw_parts(bytes, size).to_vec();
                let _ = GlobalUnlock(memory);
            }
        }
    }
    // (As far as the zero that ends it: what is held past it is the memory's rounding.)
    let end = taken.chunks(unit).position(|chunk| chunk.iter().all(|byte| *byte == 0)).map_or(taken.len(), |at| at * unit);
    taken.truncate(end);
    (!taken.is_empty()).then_some(taken)
}

/// The text on the clipboard (its line breaks `\n`), if there is any.
fn clipboard_text(owner: HWND) -> Option<String> {
    // (CF_UNICODETEXT.)
    const TEXT: u32 = 13;
    let bytes = clipboard_data(owner, TEXT, 2)?;
    let units: Vec<u16> = bytes.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).collect();
    Some(String::from_utf16_lossy(&units).replace("\r\n", "\n").replace('\r', "\n"))
}

/// What is on the clipboard, to be pasted into a note: its HTML (a web page's, Word's) as Markdown (see `notes::html`),
/// else its text. A web address alone as its text is pasted as written (over words chosen, a link of them: see
/// `edit::paste`), not as its HTML's link to it.
fn clipboard_pasted(owner: HWND) -> Option<crate::notes::edit::Pasted> {
    use crate::notes::edit::{web_address, Pasted};
    use windows::Win32::System::DataExchange::RegisterClipboardFormatW;
    let text = clipboard_text(owner);
    let plain = |text: String| Pasted { text, blocks: false };
    if text.as_deref().is_some_and(|text| web_address(text).is_some()) {
        return text.map(plain);
    }
    let html = unsafe { RegisterClipboardFormatW(windows::core::w!("HTML Format")) };
    let written = clipboard_data(owner, html, 1).and_then(|bytes| crate::notes::html::pasted(&bytes));
    written.filter(|written| !written.text.is_empty()).or_else(|| text.map(plain))
}

/// What is taken off the clipboard as `taken` says.
fn clipboard(owner: HWND, taken: Taken) -> Option<crate::notes::edit::Pasted> {
    match taken {
        Taken::Text => clipboard_text(owner).map(|text| crate::notes::edit::Pasted { text, blocks: false }),
        Taken::Pasted => clipboard_pasted(owner),
    }
}

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
    /// How its Markdown shows where it is not written at, as it was last dressed, and where it was written at then (see
    /// `notes::markdown::Shown`), the same for its box's messages (see `Reads`).
    shown: crate::notes::markdown::Shown,
    at: crate::notes::markdown::At,
    /// Its tables drawn laid out, as it was last dressed, and its words then (UTF-16 units).
    laid: Laid,
    dressed: Rc<[u16]>,
    reads: Rc<RefCell<Reads>>,
    /// What was chosen in it as it opened (the caret), and whether it has been written in or the caret moved since: till
    /// then it is written at nowhere, and it shows as the note did where it was pressed (its marks put away).
    opened_at: (i32, i32),
    pub touched: bool,
    /// Its box was given a key, or the input method's words, since this was last asked (see `touched`), and how deep its
    /// box's messages are in it now (one comes inside another: see `answering`).
    keyed: Rc<Cell<bool>>,
    depth: Rc<Cell<u32>>,
    /// The character the key last down makes is to be eaten: the key did what it does to the words as Markdown is
    /// written (see `pressed`), or nothing.
    eaten: Rc<Cell<bool>>,
    /// How high its words are written (pixels), whether the pointer is pressed on them (choosing), and which of the
    /// format bar's buttons it is over (see `bar_at`).
    pixels: Rc<Cell<f32>>,
    pressing: Rc<Cell<bool>>,
    over: Rc<Cell<Option<usize>>>,
    /// Where its writing menu was asked for (on the screen), not yet shown: see `take_menu_asked`.
    menu_asked: Rc<Cell<Option<POINT>>>,
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
            at: None,
            laid: Vec::new(),
            dressed: Rc::from([]),
            reads: Rc::new(RefCell::new(Reads { shown: Default::default(), at: None, folder: folder.path.clone(), laid: Vec::new(), dressed: Rc::from([]) })),
            opened_at,
            touched: false,
            keyed: Rc::new(Cell::new(false)),
            depth: Rc::new(Cell::new(0)),
            eaten: Rc::new(Cell::new(false)),
            pixels: Rc::new(Cell::new(pixels)),
            pressing: Rc::new(Cell::new(false)),
            over: Rc::new(Cell::new(None)),
            menu_asked: Rc::new(Cell::new(None)),
        };
        writing.dress();
        Some(writing)
    }

    /// Its words dressed as their Markdown reads, its marks put away where it is not written at (see `at_now`).
    pub fn dress(&mut self) {
        let text = self.editor.text();
        let shown = crate::notes::markdown::Shown::of(&text);
        self.dress_as(shown, self.at_now(), &text);
    }

    /// Its words, `text`, dressed as `shown` reads them, written at `at`: its tables not written at laid out in its words'
    /// box (as a note shown lays them out, see `Frame::markdown`), their lines as tall as they take.
    fn dress_as(&mut self, shown: crate::notes::markdown::Shown, at: crate::notes::markdown::At, text: &str) {
        let (hidden, rooms) = shown.put_away(at);
        // (Its words whole pixels high, as a note shown makes them, at its window's scale.)
        let (pixels, per) = (self.pixels().round(), self.pixels() / WORDS);
        let bounds = self.editor.bounds();
        // (The device asked for only where there is a table to lay out: made on a thread, it is that thread's to let go.)
        let tables: Vec<_> = shown.tables_drawn(at).collect();
        self.laid = match (!tables.is_empty()).then(super::gfx::current) {
            Some(Ok(gfx)) => tables.into_iter().map(|table| (table.clone(), gfx.table_laid(table, TABLE_FAMILY, pixels, per, (bounds.right - bounds.left) as f32))).collect(),
            _ => Vec::new(),
        };
        let drawn: Vec<_> = self.laid.iter().map(|(table, laid)| (table.span.clone(), laid.laid.height())).collect();
        self.editor.dress(&crate::notes::markdown::looks(text), &hidden, &rooms, &drawn, &self.palette);
        self.dressed = text.encode_utf16().collect();
        if let Ok(mut reads) = self.reads.try_borrow_mut() {
            *reads = Reads { shown: shown.clone(), at, folder: self.folder.clone(), laid: self.laid.clone(), dressed: self.dressed.clone() };
        }
        self.shown = shown;
        self.at = at;
    }

    /// Its words' box put at `rect` (in its window): as wide as it was, as it is, else dressed again (its tables laid out
    /// in the room it has now).
    pub fn place(&mut self, rect: RECT) {
        let was = self.editor.bounds();
        if was == rect {
            return;
        }
        self.editor.place(rect);
        if rect.right - rect.left != was.right - was.left {
            self.dress();
        }
    }

    /// Where it is written at now: what is chosen, once it has been written in or the caret moved (see `touched`).
    fn at_now(&self) -> crate::notes::markdown::At {
        let (from, to) = self.editor.selection();
        self.touched.then_some((from.max(0) as usize, to.max(0) as usize))
    }

    /// Its words written `pixels` high in `theme`'s inks (its window's scale changed, or its skin), dressed again so.
    pub fn restyle(&mut self, theme: &Theme, pixels: f32) {
        self.palette = palette_for(theme, self.colour, pixels);
        self.pixels.set(pixels);
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

    /// Where its writing menu was asked for (see `writing_menu`), on the screen, taken: its words pressed with the
    /// right button there, or the menu key or Shift+F10 pressed (at the caret). Its window's to show.
    pub fn take_menu_asked(&self) -> Option<POINT> {
        self.menu_asked.take()
    }

    /// `how` done to its words, chosen in its writing menu (see `Write`): each change one undo.
    pub fn write(&self, how: Write) {
        use crate::notes::edit;
        let (from, to) = self.editor.selection();
        let (text, chosen) = (self.editor.words(), (from.max(0) as usize, to.max(0) as usize));
        let owner = self.editor.window();
        let made = match how {
            Write::Cut | Write::Copy => {
                let _ = self.editor.send(if how == Write::Cut { WM_CUT } else { WM_COPY }, Default::default(), Default::default());
                None
            }
            Write::Paste => clipboard_pasted(owner).map(|pasted| edit::paste(&text, chosen, &pasted)),
            Write::Insert(block) => Some(edit::insert(&text, chosen, block)),
            Write::Link => edit::link(&text, chosen, clipboard_text(owner).as_deref().and_then(edit::web_address)),
            Write::Heading(level) => Some(edit::heading(&text, chosen, level)),
        };
        if let Some(edit) = made {
            self.editor.apply(&edit);
        }
        self.keyed.set(true);
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
        let (editor, reads, keyed, depth, eaten) = (self.editor.clone(), self.reads.clone(), self.keyed.clone(), self.depth.clone(), self.eaten.clone());
        let (pixels, pressing, over, menu_asked) = (self.pixels.clone(), self.pressing.clone(), self.over.clone(), self.menu_asked.clone());
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
                // (Of the system's keys, Alt+Up and Alt+Down: lines moved. Shift+F10: the writing menu.)
                WM_SYSKEYDOWN => match VIRTUAL_KEY(wparam.0 as u16) {
                    VK_UP | VK_DOWN => true,
                    VK_F10 => (unsafe { GetKeyState(i32::from(VK_SHIFT.0)) }) < 0,
                    _ => false,
                },
                WM_KEYUP | WM_CHAR | WM_SYSCHAR | WM_UNICHAR | WM_SETFOCUS | WM_KILLFOCUS => true,
                WM_IME_STARTCOMPOSITION | WM_IME_COMPOSITION | WM_IME_ENDCOMPOSITION | WM_IME_NOTIFY | WM_IME_SETCONTEXT | WM_IME_CHAR | WM_IME_REQUEST | WM_IME_COMPOSITIONFULL | WM_IME_SELECT => true,
                WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_RBUTTONDOWN | WM_RBUTTONUP => editor.holds(point),
                // (A press begun in it is followed till let go, wherever the pointer goes.)
                WM_LBUTTONUP | WM_MOUSEMOVE => editor.holds(point) || unsafe { windows::Win32::UI::Input::KeyboardAndMouse::GetCapture() } == hwnd,
                WM_SETCURSOR => (lparam.0 & 0xFFFF) as u32 == HTCLIENT && editor.holds(pointer()),
                WM_MOUSEWHEEL | WM_TIMER => true,
                _ => false,
            };
            if !its {
                return None;
            }
            if matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN | WM_CHAR | WM_IME_COMPOSITION) {
                keyed.set(true);
            }
            // A key that writes Markdown (see `pressed`), decided as it goes down (with what is held with it), the
            // character it makes eaten after it: not while the input method composes (its keys are its).
            if message == WM_CHAR && eaten.replace(false) {
                return Some(Some(LRESULT(0)));
            }
            if matches!(message, WM_KEYDOWN | WM_SYSKEYDOWN) {
                eaten.set(false);
                let held = |key: VIRTUAL_KEY| unsafe { GetKeyState(i32::from(key.0)) } < 0;
                let held = Held { ctrl: held(VK_CONTROL), shift: held(VK_SHIFT), alt: held(VK_MENU) };
                // The writing menu asked for by its key, or Shift+F10: at the caret (its window's to show, see
                // `take_menu_asked`). Not while the input method composes.
                let key = VIRTUAL_KEY(wparam.0 as u16);
                if (key == VK_APPS || key == VK_F10 && held.shift && !held.ctrl && !held.alt) && !editor.composing() {
                    let bounds = editor.bounds();
                    let mut at = editor.caret().map_or(POINT { x: bounds.left, y: bounds.top }, |(x, y, _, tall)| POINT { x, y: y + tall });
                    let _ = unsafe { windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut at) };
                    menu_asked.set(Some(at));
                    eaten.set(true);
                    return Some(Some(LRESULT(0)));
                }
                let (from, to) = editor.selection();
                let made = (!editor.composing()).then(|| pressed(&editor.words(), (from.max(0) as usize, to.max(0) as usize), VIRTUAL_KEY(wparam.0 as u16), held, message == WM_SYSKEYDOWN, |taken| clipboard(hwnd, taken))).flatten();
                if let Some(made) = made {
                    eaten.set(true);
                    depth.set(depth.get() + 1);
                    if let Some(edit) = made {
                        editor.apply(&edit);
                    }
                    depth.set(depth.get() - 1);
                    return Some(Some(LRESULT(0)));
                }
            }
            // The format bar's, where it shows and the pointer is on it: a button pressed puts what it puts on the words
            // chosen (what is chosen chosen still), the pointer over it lights it, an arrow. Else what is pressed on the
            // words chooses (the bar not shown till it is let go).
            let at_bar = |point: POINT| bar_at(&editor, pixels.get(), pressing.get()).filter(|bar| point.x >= bar.left && point.x < bar.right && point.y >= bar.top && point.y < bar.bottom);
            let on_bar = match message {
                WM_LBUTTONDOWN | WM_LBUTTONDBLCLK | WM_LBUTTONUP | WM_RBUTTONDOWN | WM_MOUSEMOVE => at_bar(point).map(|bar| bar_button(&bar, point, pixels.get())),
                WM_SETCURSOR => at_bar(pointer()).map(|_| None),
                _ => None,
            };
            if message == WM_MOUSEMOVE {
                over.set(on_bar.flatten());
            }
            if let Some(button) = on_bar {
                match (message, button) {
                    (WM_LBUTTONDOWN | WM_LBUTTONDBLCLK, Some(button)) => {
                        let (from, to) = editor.selection();
                        let (text, chosen) = (editor.words(), (from.max(0) as usize, to.max(0) as usize));
                        let made = match BAR[button].1 {
                            Format::Marked(how) => Some(crate::notes::edit::toggle(&text, chosen, how)),
                            Format::Link => crate::notes::edit::link(&text, chosen, clipboard_text(hwnd).as_deref().and_then(crate::notes::edit::web_address)),
                        };
                        depth.set(depth.get() + 1);
                        if let Some(edit) = made {
                            editor.apply(&edit);
                        }
                        depth.set(depth.get() - 1);
                    }
                    (WM_SETCURSOR, _) => unsafe {
                        SetCursor(LoadCursorW(None, IDC_ARROW).ok());
                        return Some(Some(LRESULT(1)));
                    },
                    _ => {}
                }
                return Some(Some(LRESULT(0)));
            }
            // The right button on the words: pressed off what is chosen, the caret there (as pressed on, it is what the
            // menu does its work on), let go, the writing menu asked for there.
            match message {
                WM_RBUTTONDOWN => {
                    let (from, to) = editor.selection();
                    let at = editor.at(point) as i32;
                    if !editor.composing() && (at < from.min(to) || at > from.max(to)) {
                        editor.select((at, at));
                    }
                    return Some(Some(LRESULT(0)));
                }
                WM_RBUTTONUP => {
                    let mut at = point;
                    let _ = unsafe { windows::Win32::Graphics::Gdi::ClientToScreen(hwnd, &mut at) };
                    menu_asked.set(Some(at));
                    return Some(Some(LRESULT(0)));
                }
                _ => {}
            }
            match message {
                WM_LBUTTONDOWN | WM_LBUTTONDBLCLK => pressing.set(true),
                // (Let go, or moved with the button up: a press whose letting go went elsewhere is over too.)
                WM_LBUTTONUP => pressing.set(false),
                WM_MOUSEMOVE => pressing.set(pressing.get() && wparam.0 & 0x0001 != 0),
                _ => {}
            }
            // A table drawn laid out pressed on a cell: the caret at the end of its words (the table shown as written).
            if message == WM_LBUTTONDOWN && !editor.composing() {
                let cell = reads.try_borrow().ok().and_then(|reads| cell_at(&editor, &reads.laid, &reads.dressed, point));
                if let Some(words) = cell {
                    editor.select((words.end as i32, words.end as i32));
                    return Some(Some(LRESULT(0)));
                }
            }
            depth.set(depth.get() + 1);
            let ticked = (message == WM_LBUTTONDOWN && !editor.composing())
                .then(|| {
                    let drawn = reads.try_borrow().ok().and_then(|reads| {
                        reads.shown.boxes(reads.at).find_map(|boxed| {
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
                reads.try_borrow().ok().filter(|_| on).and_then(|reads| reads.shown.link_at(at, reads.at).map(|link| (link.to.clone(), reads.folder.clone())))
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
        // (Written in, or the caret moved: written at from now.)
        let keyed = self.keyed.replace(false);
        let touched = !self.touched && (keyed || composed || self.editor.selection() != self.opened_at);
        self.touched |= touched;
        let composing = self.editor.composing();
        if composed || (!composing && (changed || touched)) {
            self.dress();
        } else if !composing {
            let at = self.at_now();
            if self.shown.put_away(at) != self.shown.put_away(self.at) {
                // (The caret moved onto what was put away, or off it: shown as written, or put away, as the words were
                // read.)
                let text = self.editor.text();
                self.dress_as(self.shown.clone(), at, &text);
            } else {
                // (Nothing put away or shown anew: where it is written at kept, for what its links do.)
                self.at = at;
                if let Ok(mut reads) = self.reads.try_borrow_mut() {
                    reads.at = at;
                }
            }
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
        // (Where what was dressed is now: the input method's words may be in them, not yet dressed.)
        let now: Vec<u16> = self.editor.text().encode_utf16().collect();
        let moved = Moved::between(&self.dressed, &now);
        for (table, laid) in &self.laid {
            let Some(line) = moved.at(table.span.start).and_then(|start| line_top(&self.editor, &now, start)) else { continue };
            let (x, y) = in_window(bounds.left as f32, line as f32);
            frame.table(laid, inks, x, y, (top, bottom));
        }
        for rule in self.shown.rules_drawn(self.at) {
            let Some(line) = moved.at(rule.end).and_then(|end| self.editor.character(end)) else { continue };
            let (x, y) = in_window(bounds.left as f32, line.top as f32);
            frame.rule(inks, x, y, (bounds.right - bounds.left) as f32 / scale, (line.bottom - line.top) as f32);
        }
        for boxed in self.shown.boxes(self.at) {
            let Some(room) = moved.at(boxed.room) else { continue };
            let (Some(room), Some(baseline)) = (self.editor.character(room), self.editor.baseline(room + 1)) else { continue };
            let (bx, by, side) = box_in(room.left, baseline, pixels);
            let (bx, by) = in_window(bx, by);
            super::canvas::task_box(frame, inks, bx, by, side / scale, boxed.done);
        }
        frame.unclip();
        if let Some(bar) = bar_at(&self.editor, self.pixels.get(), self.pressing.get()) {
            let per = self.pixels.get() / WORDS / scale;
            let (left, top) = in_window(bar.left as f32, bar.top as f32);
            let (right, bottom) = in_window(bar.right as f32, bar.bottom as f32);
            let plate = super::menu::plate(frame, super::menu::Look::of(theme.skin, theme.dark), (left, top, right - left, bottom - top));
            for (nth, (icon, _)) in BAR.iter().enumerate() {
                let (x, y, side) = (left + (BAR_INSET + nth as f32 * BAR_BUTTON) * per, top + BAR_INSET * per, BAR_BUTTON * per);
                if self.over.get() == Some(nth) {
                    frame.fill_rounded(plate.highlight, x, y, side, side, (plate.radius - BAR_INSET * per).max(2.0));
                }
                frame.icon(*icon, Point { x: x + side / 2.0, y: y + side / 2.0 }, 16.0 * per, plate.icon);
            }
        }
        if let (Some((cx, cy, cw, ch)), true) = (self.editor.caret(), caret_on) {
            let (cx, cy) = in_window(cx as f32, cy as f32);
            frame.fill(theme.text, cx, cy, (cw as f32 / scale).max(1.2), ch as f32 / scale);
        }
    }
}

#[cfg(all(test, feature = "studio"))]
mod pictures {
    use super::*;

    /// How many pixels of a note's words' room, written `text`, differ drawn written in and not (stuck on the desktop), at
    /// each of the screen's usual scales. Each drawing is written to `target/beside/alike-<name>-<scale>-<page|face>.png`.
    fn drawn_apart(text: &str, name: &str) -> Vec<(f32, usize)> {
        use windows::core::w;
        use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_POPUP};
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let gfx = super::super::gfx::current().unwrap();
        let out = std::path::PathBuf::from(format!("{}/target/beside", env!("CARGO_MANIFEST_DIR")));
        std::fs::create_dir_all(&out).unwrap();
        let folder = std::env::temp_dir().join(format!("glance-note-alike-{name}-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("a.md"), text).unwrap();
        let (w, h) = super::super::view::STUCK_NOTE;
        let hwnd = unsafe { CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"), w!(""), WS_POPUP, 0, 0, (w * 2.0) as i32, (h * 2.0) as i32, None, None, None, None) }.unwrap();
        let mut apart = Vec::new();
        for scale in [1.0f32, 1.25, 1.5, 2.0] {
            let size = ((w * scale) as u32, (h * scale) as u32);
            let theme = Theme::new(Skin::Fluent, true);
            let writing = Writing::open(hwnd, words_box(0, 0, size.0 as i32, size.1 as i32, scale), &crate::notes::store::Folder { path: folder.clone(), apart: " (conflicted copy)" }, Some("a.md".into()), &theme, WORDS * scale).unwrap();
            let written = gfx.draw_offscreen(size, scale, |frame| writing.paint(frame, &theme, Lang::Zh, (0.0, 0.0, w, h), (0.0, 0.0), scale, false)).unwrap();
            let not = gfx.draw_offscreen(size, scale, |frame| paint_stuck(frame, &theme, Lang::Zh, text, writing.colour, "x", "a.md", (0.0, 0.0, w, h))).unwrap();
            crate::studio::save_png(&out.join(format!("alike-{name}-{scale}-page.png")), size.0, size.1, &written).unwrap();
            crate::studio::save_png(&out.join(format!("alike-{name}-{scale}-face.png")), size.0, size.1, &not).unwrap();
            // Over its words' room: how many of its pixels differ.
            let (left, top, wide, tall) = words_in(w, h);
            let differ = ((top * scale) as u32..((top + tall) * scale) as u32)
                .flat_map(|y| ((left * scale) as u32..((left + wide) * scale) as u32).map(move |x| ((y * size.0 + x) * 4) as usize))
                .filter(|&at| written[at..at + 4] != not[at..at + 4])
                .count();
            apart.push((scale, differ));
        }
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        let _ = std::fs::remove_dir_all(&folder);
        // (Its device let go of before the thread ends: else it hung there.)
        drop(gfx);
        super::super::gfx::let_go();
        apart
    }

    /// A note's words drawn not written in (stuck on the desktop) as they are drawn written in, pixel for pixel, at each
    /// of the screen's usual scales: as the text services draw them (else, written in, they came out sharper).
    #[test]
    #[ignore = "makes a window of this session's"]
    fn its_words_are_drawn_alike_written_in_or_not() {
        let apart = drawn_apart("天下\n你是我的我是你的谁哦\nThe quick brown fox, 1% low", "words");
        assert!(apart.iter().all(|(_, differ)| *differ == 0), "pixels apart at each scale: {apart:?}");
    }

    /// A note's tables and rules drawn not written in as they are drawn written in, pixel for pixel, at each of the
    /// screen's usual scales: a table laid out in the room its lines leave, the words under it where they are, a rule
    /// through its line's middle. A table in a quote is left as written.
    #[test]
    #[ignore = "makes a window of this session's"]
    fn tables_and_rules_are_drawn_alike_written_in_or_not() {
        let text = "上面一行\n\n| 名称 | 数量 | 说明 |\n| :-- | :-: | --: |\n| 苹果 | 3 | 红的，很甜，放在厨房的篮子里面 |\n| pear | 12 |  |\n\n---\n下面一行\n> | 引 | 用 |\n> | - | - |";
        let apart = drawn_apart(text, "table");
        assert!(apart.iter().all(|(_, differ)| *differ == 0), "pixels apart at each scale: {apart:?}");
    }

    /// A table below its box's room is found where it would be (below the box, not at its top: the text services'
    /// `S_FALSE`, its point unset, taken for none), what is not drawn is not found drawn, and the box made narrower lays
    /// the table out again in the room it has.
    #[test]
    #[ignore = "makes a window of this session's"]
    fn a_table_out_of_the_box_and_the_box_made_narrower() {
        use windows::core::w;
        use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_POPUP};
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let text = format!("{}| 名称 | 说明 |\n| - | - |\n| 苹果 | 红的，很甜，放在厨房的篮子里面，还有很多别的水果 |\n", "行\n".repeat(60));
        let folder = std::env::temp_dir().join(format!("glance-note-out-of-box-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("a.md"), &text).unwrap();
        let (w, h) = super::super::view::STUCK_NOTE;
        let hwnd = unsafe { CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"), w!(""), WS_POPUP, 0, 0, w as i32, h as i32, None, None, None, None) }.unwrap();
        let theme = Theme::new(Skin::Fluent, true);
        let mut writing = Writing::open(hwnd, words_box(0, 0, w as i32, h as i32, 1.0), &crate::notes::store::Folder { path: folder.clone(), apart: " (conflicted copy)" }, Some("a.md".into()), &theme, WORDS).unwrap();
        let units: Vec<u16> = text.encode_utf16().collect();
        let start = writing.laid[0].0.span.start;
        let bounds = writing.editor.bounds();
        assert!(line_top(&writing.editor, &units, start).unwrap() >= bounds.bottom);
        assert!(writing.editor.character(start).is_none());
        let wide = writing.laid[0].1.laid.width();
        let half = (bounds.right - bounds.left) / 2;
        writing.place(RECT { right: bounds.left + half, ..bounds });
        let narrower = writing.laid[0].1.laid.width();
        assert!(narrower < wide && narrower <= half as f32, "{wide} to {narrower} in {half}");
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        let _ = std::fs::remove_dir_all(&folder);
        super::super::gfx::let_go();
    }

    /// What was dressed found where it is now, the input method's words composed before it, in it or after it.
    #[test]
    fn what_was_dressed_where_it_is_now() {
        let units = |text: &str| text.encode_utf16().collect::<Vec<u16>>();
        let moved = super::Moved::between(&units("a\n\n| h |"), &units("你好a\n\n| h |"));
        assert_eq!((moved.at(0), moved.at(3)), (Some(2), Some(5)));
        let moved = super::Moved::between(&units("ab\ncd"), &units("aXYb\ncd"));
        assert_eq!((moved.at(0), moved.at(1), moved.at(3)), (Some(0), Some(3), Some(5)));
        let moved = super::Moved::between(&units("abcd"), &units("aXd"));
        assert_eq!((moved.at(0), moved.at(1), moved.at(2), moved.at(3)), (Some(0), None, None, Some(2)));
    }

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
            // Written at: the caret at its to-do's box, the box as it is written.
            writing.touched = true;
            let at = text.encode_utf16().position(|c| c == u16::from(b'-')).unwrap() as i32 + 3;
            writing.editor.select((at, at));
            writing.dress();
            let written = gfx.draw_offscreen(size, scale, |frame| writing.paint(frame, &theme, Lang::Zh, (0.0, 0.0, w, h), (0.0, 0.0), scale, false)).unwrap();
            crate::studio::save_png(&out.join(format!("note-{name}-written.png")), size.0, size.1, &written).unwrap();
            // Words chosen (the quote's): the format bar over them.
            let quote = text.encode_utf16().position(|c| c == u16::from(b'>')).unwrap() as i32 + 2;
            writing.editor.select((quote, quote + 3));
            writing.dress();
            let chosen = gfx.draw_offscreen(size, scale, |frame| writing.paint(frame, &theme, Lang::Zh, (0.0, 0.0, w, h), (0.0, 0.0), scale, false)).unwrap();
            crate::studio::save_png(&out.join(format!("note-{name}-bar.png")), size.0, size.1, &chosen).unwrap();
        }
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        let _ = std::fs::remove_dir_all(&folder);
        drop(gfx);
        super::super::gfx::let_go();
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

    /// Which keys write Markdown: Enter in a list (not Shift+Enter, not with words chosen), Tab, Backspace at an item's
    /// words, the format keys. Ctrl with the text services' own keys (centring, line spacing) eaten, choosing all,
    /// copying, pasting, undoing theirs still, as are the keys that write nothing (Ctrl+arrows). The clipboard asked only
    /// for a link.
    #[test]
    fn which_keys_write_markdown() {
        use super::{pressed, Held, VIRTUAL_KEY};
        use windows::Win32::UI::Input::KeyboardAndMouse::*;
        let none = Held::default();
        let ctrl = Held { ctrl: true, ..none };
        let shift = Held { shift: true, ..none };
        let alt = Held { alt: true, ..none };
        let asked = std::cell::Cell::new(None);
        let plain = |text: &str| crate::notes::edit::Pasted { text: text.into(), blocks: false };
        let clipboard = |taken| {
            asked.set(Some(taken));
            Some(plain("https://a.b"))
        };
        let made = |text: &str, chosen: (usize, usize), key: VIRTUAL_KEY, held: Held| pressed(text, chosen, key, held, held.alt, |_| None).map(|edit| edit.map(|edit| edit.with));
        assert_eq!(made("- a", (3, 3), VK_RETURN, none), Some(Some("\n- ".into())));
        assert_eq!(made("- a", (3, 3), VK_RETURN, shift), None);
        assert_eq!(made("- a", (2, 3), VK_RETURN, none), None);
        assert_eq!(made("words", (5, 5), VK_RETURN, none), None);
        assert_eq!(made("- [ ] a", (7, 7), VK_RETURN, ctrl), Some(Some("x".into())));
        assert_eq!(made("- a\n- b", (7, 7), VK_TAB, none), Some(Some("  ".into())));
        assert_eq!(made("- a", (3, 3), VK_TAB, none), Some(None));
        assert_eq!(made("words", (5, 5), VK_TAB, none), None);
        // In a table, Tab and Shift+Tab go from cell to cell (choosing its words, nothing written), past the last a row more.
        let chosen = |text: &str, at: usize, held: Held| pressed(text, (at, at), VK_TAB, held, false, |_| None).flatten().map(|edit| (edit.with, edit.chosen));
        assert_eq!(chosen("| a | b |\n| - | - |", 2, none), Some((String::new(), (6, 7))));
        assert_eq!(chosen("| a | b |\n| - | - |", 6, shift), Some((String::new(), (2, 3))));
        assert_eq!(chosen("| a | b |\n| - | - |\n| c | d |", 26, none), Some(("\n|  |  |".into(), (32, 32))));
        assert_eq!(made("- a", (2, 2), VK_BACK, none), Some(Some(String::new())));
        assert_eq!(made("- a", (3, 3), VK_BACK, none), None);
        assert_eq!(made("a\nb", (2, 2), VK_UP, alt), Some(Some("b\na".into())));
        assert_eq!(made("a\nb", (2, 2), VK_F4, alt), None);
        assert_eq!(made("ab", (0, 2), VK_B, ctrl), Some(Some("**ab**".into())));
        assert_eq!(made("ab", (0, 2), VK_X, Held { ctrl: true, shift: true, alt: false }), Some(Some("~~ab~~".into())));
        assert_eq!(made("ab", (0, 2), VK_2, ctrl), Some(Some("## ".into())));
        for key in [VK_L, VK_R, VK_J, VK_5, VK_OEM_PLUS, VK_H] {
            assert_eq!(made("ab", (0, 2), key, ctrl), Some(None), "Ctrl+{:#x}", key.0);
        }
        for key in [VK_A, VK_C, VK_X, VK_Z, VK_Y, VK_LEFT, VK_BACK, VK_DELETE, VK_HOME] {
            assert_eq!(made("ab", (0, 2), key, ctrl), None, "Ctrl+{:#x}", key.0);
        }
        assert_eq!(pressed("ab", (0, 2), VK_B, ctrl, false, clipboard).map(|edit| edit.map(|edit| edit.with)), Some(Some("**ab**".into())));
        assert_eq!(asked.get(), None);
        assert_eq!(pressed("ab", (0, 2), VK_K, ctrl, false, clipboard).map(|edit| edit.map(|edit| edit.with)), Some(Some("[ab](https://a.b)".into())));
        assert_eq!(asked.get(), Some(super::Taken::Text));
        let _ = pressed("ab", (1, 1), VK_V, ctrl, false, clipboard);
        assert_eq!(asked.get(), Some(super::Taken::Pasted));
        // Pasting: the clipboard's text, its line breaks `\n`, a web address over words a link, nothing with no text.
        let pasted = |chosen: (usize, usize), key: VIRTUAL_KEY, held: Held, on: Option<&str>| pressed("ab", chosen, key, held, false, |_| on.map(plain)).map(|edit| edit.map(|edit| edit.with));
        assert_eq!(pasted((1, 1), VK_V, ctrl, Some("x\r\ny")), Some(Some("x\ny".into())));
        assert_eq!(pasted((1, 1), VK_INSERT, shift, Some("x")), Some(Some("x".into())));
        assert_eq!(pasted((0, 2), VK_V, ctrl, Some("https://a.b")), Some(Some("[ab](https://a.b)".into())));
        assert_eq!(pasted((1, 1), VK_V, ctrl, None), Some(None));
        assert_eq!(pasted((1, 1), VK_INSERT, none, Some("x")), None);
    }

    /// The writing menu: cut, copy, paste, the blocks and a link, the headings and words again, each row's icon its own.
    #[test]
    fn the_writing_menu_s_rows() {
        use super::{writing_menu, Choice, Write};
        use crate::notes::edit::Block;
        let (items, choices) = writing_menu(super::Lang::Zh);
        assert_eq!(items.len(), choices.len());
        assert_eq!(items.iter().map(|item| item.label).collect::<Vec<_>>(), ["剪切", "复制", "粘贴", "插入待办事项", "插入表格", "插入代码块", "插入分隔线", "插入链接", "一级标题", "二级标题", "三级标题", "正文"]);
        assert_eq!(choices[4], Choice::Write(Write::Insert(Block::Table)));
        assert_eq!(choices[11], Choice::Write(Write::Heading(0)));
        assert_eq!(items.iter().filter(|item| item.rule_before).count(), 2);
    }

    /// The format bar's buttons side by side in it, none at its edge or past its last, larger as the words are.
    #[test]
    fn the_format_bar_s_buttons() {
        use super::{bar_button, BAR_BUTTON, BAR_INSET, WORDS};
        use windows::Win32::Foundation::{POINT, RECT};
        let bar = RECT { left: 100, top: 50, right: 100 + (5.0 * BAR_BUTTON + 2.0 * BAR_INSET) as i32, bottom: 50 + (BAR_BUTTON + 2.0 * BAR_INSET) as i32 };
        let at = |x: f32, y: f32| bar_button(&bar, POINT { x: 100 + x as i32, y: 50 + y as i32 }, WORDS);
        assert_eq!(at(BAR_INSET + 1.0, BAR_INSET + 1.0), Some(0));
        assert_eq!(at(BAR_INSET + BAR_BUTTON * 4.5, 20.0), Some(4));
        assert_eq!(at(1.0, 20.0), None);
        assert_eq!(at(BAR_INSET + BAR_BUTTON * 5.0 + 1.0, 20.0), None);
        assert_eq!(bar_button(&bar, POINT { x: 100 + (BAR_INSET * 2.0 + BAR_BUTTON * 1.5 * 2.0) as i32, y: 70 }, WORDS * 2.0), Some(1));
    }

    /// Words chosen in a note's box: the format bar shows over them, its bold pressed makes them bold (chosen still),
    /// pressed again plain. Pressed on the words (choosing), it shows no more till the pointer is let go.
    #[test]
    fn the_format_bar_marks_the_words_chosen() {
        use windows::core::w;
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WS_POPUP};
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let folder = std::env::temp_dir().join(format!("glance-note-bar-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("b.md"), "第一行\n第二行\n第三行\n一 粗体 字").unwrap();
        let hwnd = unsafe { CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"), w!(""), WS_POPUP, 0, 0, 400, 300, None, None, None, None) }.unwrap();
        let theme = super::Theme::new(super::Skin::Paper, false);
        let mut writing = super::Writing::open(hwnd, super::words_box(0, 0, 400, 300, 1.0), &crate::notes::store::Folder { path: folder.clone(), apart: " (conflicted copy)" }, Some("b.md".into()), &theme, super::WORDS).unwrap();
        let answer = writing.answer();
        let at = |x: i32, y: i32| LPARAM(((y as isize) << 16) | (x as isize & 0xFFFF));
        let written = |writing: &super::Writing| writing.editor.text().strip_suffix('\n').unwrap().to_string();
        writing.editor.select((14, 16));
        writing.touched = true;
        writing.written();
        let bar = super::bar_at(&writing.editor, super::WORDS, false).expect("the bar over the words chosen");
        let chosen = writing.editor.character(14).unwrap();
        assert!(bar.bottom <= chosen.top, "{bar:?} over {chosen:?}");
        let bold = (bar.left + (super::BAR_INSET + super::BAR_BUTTON / 2.0) as i32, (bar.top + bar.bottom) / 2);
        answer(hwnd, WM_LBUTTONDOWN, WPARAM(1), at(bold.0, bold.1));
        answer(hwnd, WM_LBUTTONUP, WPARAM(0), at(bold.0, bold.1));
        writing.written();
        assert_eq!(written(&writing), "第一行\n第二行\n第三行\n一 **粗体** 字");
        assert_eq!(writing.editor.selection(), (16, 18));
        let bar = super::bar_at(&writing.editor, super::WORDS, false).unwrap();
        let bold = (bar.left + (super::BAR_INSET + super::BAR_BUTTON / 2.0) as i32, (bar.top + bar.bottom) / 2);
        answer(hwnd, WM_LBUTTONDOWN, WPARAM(1), at(bold.0, bold.1));
        writing.written();
        assert_eq!(written(&writing), "第一行\n第二行\n第三行\n一 粗体 字");
        assert_eq!(writing.editor.selection(), (14, 16));
        // Pressed on the words: choosing, no bar till let go.
        let words = writing.editor.character(15).unwrap();
        answer(hwnd, WM_LBUTTONDOWN, WPARAM(1), at(words.left + 1, words.top + 2));
        answer(hwnd, WM_MOUSEMOVE, WPARAM(1), at(words.right + 4, words.top + 2));
        assert!(writing.pressing.get());
        answer(hwnd, WM_LBUTTONUP, WPARAM(0), at(words.right + 4, words.top + 2));
        assert!(!writing.pressing.get());
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// The writing menu asked for on a note's words: the right button pressed off what is chosen puts the caret there,
    /// let go asks for it there (on the screen), as the menu key does at the caret. What it does is done as one undo.
    #[test]
    fn the_writing_menu_is_asked_for_and_does_its_work() {
        use windows::core::w;
        use windows::Win32::Foundation::{LPARAM, POINT, WPARAM};
        use windows::Win32::UI::Input::KeyboardAndMouse::VK_APPS;
        use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WM_KEYDOWN, WM_KEYUP, WM_RBUTTONDOWN, WM_RBUTTONUP, WS_POPUP};
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let folder = std::env::temp_dir().join(format!("glance-note-menu-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("m.md"), "一行字\n二行字").unwrap();
        let hwnd = unsafe { CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"), w!(""), WS_POPUP, 50, 60, 400, 300, None, None, None, None) }.unwrap();
        let theme = super::Theme::new(super::Skin::Paper, false);
        let mut writing = super::Writing::open(hwnd, super::words_box(0, 0, 400, 300, 1.0), &crate::notes::store::Folder { path: folder.clone(), apart: " (conflicted copy)" }, Some("m.md".into()), &theme, super::WORDS).unwrap();
        let answer = writing.answer();
        let at = |x: i32, y: i32| LPARAM(((y as isize) << 16) | (x as isize & 0xFFFF));
        let written = |writing: &super::Writing| writing.editor.text().strip_suffix('\n').unwrap().to_string();
        writing.editor.select((0, 2));
        let second = writing.editor.character(5).unwrap();
        let (x, y) = (second.left + 1, second.top + 2);
        answer(hwnd, WM_RBUTTONDOWN, WPARAM(2), at(x, y));
        assert_eq!(writing.editor.selection(), (5, 5));
        assert_eq!(writing.take_menu_asked(), None);
        answer(hwnd, WM_RBUTTONUP, WPARAM(0), at(x, y));
        assert_eq!(writing.take_menu_asked(), Some(POINT { x: 50 + x, y: 60 + y }));
        // Pressed on what is chosen, it stays chosen.
        writing.editor.select((4, 7));
        answer(hwnd, WM_RBUTTONDOWN, WPARAM(2), at(x, y));
        assert_eq!(writing.editor.selection(), (4, 7));
        answer(hwnd, WM_KEYDOWN, WPARAM(usize::from(VK_APPS.0)), LPARAM(1));
        answer(hwnd, WM_KEYUP, WPARAM(usize::from(VK_APPS.0)), LPARAM(1));
        assert!(writing.take_menu_asked().is_some());
        writing.editor.select((5, 5));
        writing.write(super::Write::Heading(2));
        writing.written();
        assert_eq!(written(&writing), "一行字\n## 二行字");
        writing.write(super::Write::Insert(crate::notes::edit::Block::Rule));
        writing.written();
        // (After the last line, before the mark the text services end the words with: a line after it to go on, the
        // caret there.)
        assert_eq!(written(&writing), "一行字\n## 二行字\n\n---\n");
        assert_eq!(writing.editor.selection(), (16, 16));
        // (EM_UNDO.)
        writing.editor.send(0x00C7, WPARAM(0), LPARAM(0));
        assert_eq!(written(&writing), "一行字\n## 二行字");
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        let _ = std::fs::remove_dir_all(&folder);
    }

    /// Keys given to a note's words' box as Windows gives them (the key down, the character it makes, the key up): Enter
    /// in a list carries it on, the caret on the new item, Tab moves that item in, its character eaten (no tab written
    /// after it), and one undo takes back each.
    #[test]
    fn keys_given_to_the_box_write_markdown() {
        use windows::core::w;
        use windows::Win32::Foundation::{LPARAM, WPARAM};
        use windows::Win32::UI::Input::KeyboardAndMouse::{VIRTUAL_KEY, VK_RETURN, VK_TAB};
        use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WM_CHAR, WM_KEYDOWN, WM_KEYUP, WS_POPUP};
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let folder = std::env::temp_dir().join(format!("glance-note-keys-{}", std::process::id()));
        std::fs::create_dir_all(&folder).unwrap();
        std::fs::write(folder.join("k.md"), "1. a\n2. b").unwrap();
        let hwnd = unsafe { CreateWindowExW(WINDOW_EX_STYLE(0), w!("STATIC"), w!(""), WS_POPUP, 0, 0, 400, 300, None, None, None, None) }.unwrap();
        let theme = super::Theme::new(super::Skin::Paper, false);
        let writing = super::Writing::open(hwnd, super::words_box(0, 0, 400, 300, 1.0), &crate::notes::store::Folder { path: folder.clone(), apart: " (conflicted copy)" }, Some("k.md".into()), &theme, super::WORDS).unwrap();
        let answer = writing.answer();
        let key = |key: VIRTUAL_KEY, character: usize| {
            answer(hwnd, WM_KEYDOWN, WPARAM(usize::from(key.0)), LPARAM(1));
            answer(hwnd, WM_CHAR, WPARAM(character), LPARAM(1));
            answer(hwnd, WM_KEYUP, WPARAM(usize::from(key.0)), LPARAM(1));
        };
        let written = || writing.editor.text().strip_suffix('\n').unwrap().to_string();
        let undo = || {
            // (EM_UNDO.)
            writing.editor.send(0x00C7, WPARAM(0), LPARAM(0));
        };
        writing.editor.select((4, 4));
        key(VK_RETURN, 0x0D);
        assert_eq!(written(), "1. a\n2. \n3. b");
        assert_eq!(writing.editor.selection(), (8, 8));
        key(VK_TAB, 0x09);
        assert_eq!(written(), "1. a\n   1. \n2. b");
        assert_eq!(writing.editor.selection(), (11, 11));
        undo();
        assert_eq!(written(), "1. a\n2. \n3. b");
        undo();
        assert_eq!(written(), "1. a\n2. b");
        unsafe {
            let _ = DestroyWindow(hwnd);
        }
        let _ = std::fs::remove_dir_all(&folder);
    }
}
