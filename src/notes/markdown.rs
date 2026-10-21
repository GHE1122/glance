//! How a note's Markdown shows as it is written (see docs/station-and-notes.md, sections 二 and 六): what each stretch
//! of it looks like, for the box it is written in to dress it, the words themselves left as they are. Parsed with
//! pulldown-cmark, as CommonMark (and its task lists and strike-through) reads it.
//!
//! Its marks (a heading's `#`, emphasis's `*`, a list's `-`, code's `` ` ``) stay where they are, faint; what they
//! mark shows as it will read: a heading larger, emphasis bold or slanted, code in a fixed face, a link in the accent
//! (a web address written out one too), a quote and a done task faint.
//!
//! Where it is not written at (all of it, in a note not being written in), its marks are put away as it reads (see
//! `Shown`): a heading's `#`, emphasis's, code's and a link's marks hidden, a to-do's list mark and box drawn as a box
//! to tick, a link's words pressed to follow it. What the caret touches (the heading, the bold words, the link it is in
//! or at the end of: see `At`) shows as written, marks and all, so the caret is never in what is put away. A list's
//! and a quote's marks stay, faint: they are what shows it is one.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::ops::Range;

/// What of Markdown a note is read for, beyond CommonMark: to-dos and words struck through. One reading, for what is
/// shown and for what the keys do to a note's words (see `edit`).
pub const READ: Options = Options::ENABLE_TASKLISTS.union(Options::ENABLE_STRIKETHROUGH);

/// How a stretch of words looks.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Look {
    /// A heading of this level (1 the largest), its words bold.
    Heading(u8),
    Bold,
    Italic,
    Struck,
    /// In a fixed face: inline code, or a block of it.
    Code,
    /// A link's words, in the accent.
    Link,
    /// Faint: Markdown's marks, a quote, a done task.
    Faint,
    /// As words are, not faint (a mark's words, within what marks them faint).
    Plain,
}

/// How much larger than the words round it a heading of `level` is.
pub fn heading_size(level: u8) -> f32 {
    match level {
        1 => 1.6,
        2 => 1.35,
        3 => 1.2,
        _ => 1.08,
    }
}

/// Which of a note's inks a stretch of its words is in.
#[derive(Clone, Copy, PartialEq, Eq, Debug)]
pub enum Ink {
    Words,
    Faint,
    Accent,
}

/// How a stretch of words is dressed, all the looks over it laid on: how much larger than the words round it, bold,
/// slanted, struck through, in the fixed face, in which ink.
#[derive(Clone, Copy, PartialEq, Debug)]
pub struct Dress {
    pub size: f32,
    pub bold: bool,
    pub italic: bool,
    pub struck: bool,
    pub code: bool,
    pub ink: Ink,
}

impl Dress {
    const PLAIN: Dress = Dress { size: 1.0, bold: false, italic: false, struck: false, code: false, ink: Ink::Words };

    fn with(mut self, look: Look) -> Dress {
        match look {
            Look::Heading(level) => {
                self.bold = true;
                self.size = heading_size(level);
            }
            Look::Bold => self.bold = true,
            Look::Italic => self.italic = true,
            Look::Struck => self.struck = true,
            Look::Code => self.code = true,
            Look::Link => self.ink = Ink::Accent,
            Look::Faint => self.ink = Ink::Faint,
            Look::Plain => self.ink = Ink::Words,
        }
        self
    }
}

/// `text` (`units` UTF-16 units long) in stretches each dressed alike, end to end, as its looks (see `looks`) dress it
/// laid on in their order, a later one over an earlier one where they meet: as the box a note is written in dresses
/// its words (see `ui::editor::Editor::dress`), for its words drawn as they are written there.
pub fn dressing(text: &str) -> Vec<(Range<usize>, Dress)> {
    let units = text.encode_utf16().count();
    let mut each = vec![Dress::PLAIN; units];
    for (range, look) in looks(text) {
        for dress in &mut each[range.start.min(units)..range.end.min(units)] {
            *dress = dress.with(look);
        }
    }
    let mut stretches: Vec<(Range<usize>, Dress)> = Vec::new();
    for (at, dress) in each.into_iter().enumerate() {
        match stretches.last_mut() {
            Some((range, last)) if *last == dress => range.end = at + 1,
            _ => stretches.push((at..at + 1, dress)),
        }
    }
    stretches
}

/// The looks of `text`'s stretches (in UTF-16 units, as the text services count, each line break one), in the order
/// they are laid on: a container's look first (its marks with it), its words' after.
pub fn looks(text: &str) -> Vec<(Range<usize>, Look)> {
    let units = Units::of(text);
    let mut looks = Vec::new();
    let mut quoted = 0usize;
    for (event, range) in Parser::new_ext(text, READ).into_offset_iter() {
        let at = units.range(&range);
        match event {
            Event::Start(Tag::Heading { level, .. }) => {
                let level = match level {
                    HeadingLevel::H1 => 1,
                    HeadingLevel::H2 => 2,
                    HeadingLevel::H3 => 3,
                    HeadingLevel::H4 => 4,
                    HeadingLevel::H5 => 5,
                    HeadingLevel::H6 => 6,
                };
                looks.push((at.clone(), Look::Heading(level)));
                looks.push((at, Look::Faint));
            }
            Event::Start(Tag::Strong) => {
                looks.push((at.clone(), Look::Bold));
                looks.push((at, Look::Faint));
            }
            Event::Start(Tag::Emphasis) => {
                looks.push((at.clone(), Look::Italic));
                looks.push((at, Look::Faint));
            }
            Event::Start(Tag::Strikethrough) => {
                looks.push((at.clone(), Look::Struck));
                looks.push((at, Look::Faint));
            }
            Event::Start(Tag::Link { .. }) => looks.push((at, Look::Faint)),
            Event::Start(Tag::CodeBlock(_)) => looks.push((at, Look::Code)),
            Event::Start(Tag::Item) => looks.push((at, Look::Faint)),
            Event::Start(Tag::BlockQuote(_)) => {
                looks.push((at, Look::Faint));
                quoted += 1;
            }
            Event::End(TagEnd::BlockQuote(_)) => quoted = quoted.saturating_sub(1),
            Event::Code(_) => {
                looks.push((at.clone(), Look::Code));
                looks.push((at, Look::Faint));
            }
            Event::TaskListMarker(done) => looks.push((at, if done { Look::Faint } else { Look::Plain })),
            // Its words as they read: plain (in a quote, faint as the quote is).
            Event::Text(_) if quoted == 0 => looks.push((at, Look::Plain)),
            _ => {}
        }
    }
    // A web address written out in the accent, as a link's words are.
    for (words, _) in addresses(text) {
        looks.push((units.range(&words), Look::Link));
    }
    // A link's words in the accent, over their plain: found as the words a link holds.
    let mut in_link = Vec::new();
    for (event, range) in Parser::new_ext(text, READ).into_offset_iter() {
        match event {
            Event::Start(Tag::Link { .. }) => in_link.push(()),
            Event::End(TagEnd::Link) => {
                in_link.pop();
            }
            Event::Text(_) if !in_link.is_empty() => looks.push((units.range(&range), Look::Link)),
            _ => {}
        }
    }
    // Code's words plain over its marks (in a fixed face already), after the marks' faintness.
    for (event, range) in Parser::new_ext(text, READ).into_offset_iter() {
        if let Event::Code(code) = event {
            let at = units.range(&range);
            // (The code's words where they are within its backticks.)
            let inner = text[range.clone()].find(code.as_ref()).map(|start| range.start + start..range.start + start + code.len());
            if let Some(inner) = inner {
                looks.push((units.range(&inner), Look::Plain));
            } else {
                looks.push((at, Look::Plain));
            }
        }
    }
    looks
}

/// How a note shows where it is not written at (see the head of this file): what of it is put away, where (in UTF-16
/// units, as the text services count, each line break one), each with all of what it marks (its span: see `At`).
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shown {
    /// Its marks hidden: a heading's `#`, emphasis's, code's, a link's (all but its words). Each with the heading's,
    /// emphasis's, code's or link's span.
    pub marks: Vec<(Range<usize>, Range<usize>)>,
    pub boxes: Vec<Boxed>,
    pub links: Vec<Linked>,
}

/// A to-do drawn as a box to tick: its list's mark and its box (`- [ ]`), put away, and the space after them, the room
/// the box is drawn in (`BOX_ROOM` wide), done or not. Its span is its marks and its room (not where its words begin).
#[derive(Clone, Debug, PartialEq)]
pub struct Boxed {
    pub marks: Range<usize>,
    pub room: usize,
    pub done: bool,
}

impl Boxed {
    fn span(&self) -> Range<usize> {
        self.marks.start..self.room + 1
    }
}

/// A link: its words, where it goes, all of it (its span). (A web address written out: its words where it goes, and
/// its span.)
#[derive(Clone, Debug, PartialEq)]
pub struct Linked {
    pub words: Range<usize>,
    pub to: String,
    pub span: Range<usize>,
}

/// Where a note is written at: what is chosen in it (from, to: UTF-16 units, the caret where they are one). None: a
/// note shown, not written in. What of its Markdown it touches shows as written, marks and all: what the caret is in or
/// at either end of (a to-do's box not where its words begin), what is chosen over any of.
pub type At = Option<(usize, usize)>;

/// Whether `at` touches `span` (see `At`): the caret at its end too where `closed`.
fn touches(span: &Range<usize>, at: At, closed: bool) -> bool {
    match at {
        None => false,
        Some((from, to)) if from == to => span.start <= from && (from < span.end || closed && from == span.end),
        Some((from, to)) => from.min(to) < span.end && span.start < from.max(to),
    }
}

/// How wide the room a to-do's box is drawn in is, its words' size over (the box in it, `BOX_SIDE` of their size,
/// `BOX_IN` from its start, its middle `BOX_RISE` over its words' baseline, as a Chinese character's is; the rest a gap
/// before its words).
pub const BOX_ROOM: f32 = 1.55;
pub const BOX_SIDE: f32 = 0.9;
pub const BOX_IN: f32 = 0.08;
pub const BOX_RISE: f32 = 0.36;

impl Shown {
    pub fn of(text: &str) -> Shown {
        let units = Units::of(text);
        let mut shown = Shown::default();
        // (All of what is marked, but the line breaks it ends with.)
        let span = |bytes: &Range<usize>| units.range(&(bytes.start..bytes.start + text[bytes.clone()].trim_end_matches(['\n', '\r']).len()));
        // (Pieces of `bytes` on one line each: no line break put away, two lines never made one.)
        let hide = |shown: &mut Shown, bytes: Range<usize>, of: &Range<usize>| {
            let mut from = bytes.start;
            for (at, c) in text[bytes.clone()].char_indices() {
                if c == '\n' || c == '\r' {
                    if from < bytes.start + at {
                        shown.marks.push((units.range(&(from..bytes.start + at)), span(of)));
                    }
                    from = bytes.start + at + c.len_utf8();
                }
            }
            if from < bytes.end {
                shown.marks.push((units.range(&(from..bytes.end)), span(of)));
            }
        };
        // Each open container whose marks are put away, as far as what it holds reaches (its first and last held); a
        // list's item where it begins.
        struct Open {
            hides: bool,
            range: Range<usize>,
            held: Option<Range<usize>>,
        }
        let mut open: Vec<Open> = Vec::new();
        let mut item: Option<usize> = None;
        let hold = |open: &mut Vec<Open>, range: &Range<usize>| {
            if let Some(parent) = open.last_mut() {
                let held = parent.held.get_or_insert(range.clone());
                held.start = held.start.min(range.start);
                held.end = held.end.max(range.end);
            }
        };
        for (event, range) in Parser::new_ext(text, READ).into_offset_iter() {
            match event {
                Event::Start(tag) => {
                    hold(&mut open, &range);
                    if matches!(tag, Tag::Item) {
                        item = Some(range.start);
                    }
                    let hides = match &tag {
                        // (A heading as `#` makes it: one underlined is left as written.)
                        Tag::Heading { .. } => text[range.clone()].starts_with('#'),
                        Tag::Strong | Tag::Emphasis | Tag::Strikethrough | Tag::Link { .. } => true,
                        _ => false,
                    };
                    if let Tag::Link { dest_url, .. } = &tag {
                        // (Its words found as it ends.)
                        shown.links.push(Linked { words: 0..0, to: dest_url.to_string(), span: span(&range) });
                    }
                    open.push(Open { hides, range, held: None });
                }
                Event::End(end) => {
                    let Some(closed) = open.pop() else { continue };
                    // (One holding nothing is left as written: put away, nothing of it would show.)
                    if let (true, Some(held)) = (closed.hides, &closed.held) {
                        hide(&mut shown, closed.range.start..held.start, &closed.range);
                        hide(&mut shown, held.end..closed.range.end, &closed.range);
                    }
                    if matches!(end, TagEnd::Link) {
                        if let Some(link) = shown.links.last_mut() {
                            link.words = closed.held.as_ref().map_or(0..0, |held| units.range(held));
                        }
                    }
                }
                Event::Code(code) => {
                    hold(&mut open, &range);
                    // (Its backticks, before and after its words.)
                    let written = &text[range.clone()];
                    let ticks = written.len() - written.trim_start_matches('`').len();
                    if !code.is_empty() && written.len() > 2 * ticks {
                        hide(&mut shown, range.start..range.start + ticks, &range);
                        hide(&mut shown, range.end - ticks..range.end, &range);
                    }
                }
                Event::TaskListMarker(done) => {
                    hold(&mut open, &range);
                    // (Its box drawn in the space after it: none, it is left as written.)
                    let from = item.unwrap_or(range.start);
                    if text[range.end..].starts_with(' ') && !text[from..range.end].contains('\n') {
                        shown.boxes.push(Boxed { marks: units.range(&(from..range.end)), room: units.range(&(range.end..range.end + 1)).start, done });
                    }
                }
                _ => hold(&mut open, &range),
            }
        }
        shown.links.retain(|link| !link.words.is_empty());
        for (words, to) in addresses(text) {
            shown.links.push(Linked { words: units.range(&words), to, span: units.range(&words) });
        }
        shown
    }

    /// Its marks hidden where what they mark is not written at (`at`).
    pub fn hidden(&self, at: At) -> Vec<Range<usize>> {
        self.marks.iter().filter(|(_, span)| !touches(span, at, true)).map(|(range, _)| range.clone()).collect()
    }

    /// Its to-dos drawn as boxes: those not written at.
    pub fn boxes(&self, at: At) -> impl Iterator<Item = &Boxed> + '_ {
        self.boxes.iter().filter(move |boxed| !touches(&boxed.span(), at, false))
    }

    /// What is put away where it is written at `at`: its marks hidden, and its to-dos' marks (each drawn as a box in
    /// its room, the rooms given too).
    pub fn put_away(&self, at: At) -> (Vec<Range<usize>>, Vec<usize>) {
        let mut hidden = self.hidden(at);
        hidden.extend(self.boxes(at).map(|boxed| boxed.marks.clone()));
        (hidden, self.boxes(at).map(|boxed| boxed.room).collect())
    }

    /// The link whose words are at `unit` (UTF-16 units), where they show as it reads (it not written at: `at`).
    pub fn link_at(&self, unit: usize, at: At) -> Option<&Linked> {
        self.links.iter().find(|link| link.words.contains(&unit) && !touches(&link.span, at, true))
    }
}

/// How far apart a note's lines are set, over the height of their words; a heading's, over the height of its own
/// (larger: less room round it), where that sets it further apart than its words' would.
pub const LEADING: f32 = 1.6;
pub const HEADING_LEADING: f32 = 1.4;

/// Each of `text`'s lines (UTF-16 units, its line break with it) and how tall it is set (whole pixels), its words
/// `words` pixels high: the same however it is drawn (the text services as it is written, DirectWrite on its face),
/// so its lines are where they were as one gives way to the other.
pub fn line_heights(text: &str, words: f32) -> Vec<(Range<usize>, f32)> {
    let headings: Vec<(usize, f32)> = looks(text)
        .into_iter()
        .filter_map(|(range, look)| match look {
            Look::Heading(level) => Some((range.start, heading_size(level))),
            _ => None,
        })
        .collect();
    let mut lines = Vec::new();
    let mut start = 0;
    let units: Vec<u16> = text.encode_utf16().collect();
    for (at, unit) in units.iter().enumerate().chain(std::iter::once((units.len(), &0))) {
        if at < units.len() && *unit != u16::from(b'\n') {
            continue;
        }
        let end = (at + 1).min(units.len());
        let size = headings.iter().filter(|(heading, _)| (start..end.max(start + 1)).contains(heading)).map(|(_, size)| *size).fold(1.0, f32::max);
        let height = (words * LEADING).max(words * size * HEADING_LEADING).round();
        lines.push((start..end, height));
        start = end;
    }
    lines
}

/// The web addresses written out in `text`, not as a link's (`http://`, `https://`), each where it is (bytes) and
/// where it goes: as far as a space, a `<`, or what is not ASCII (words written on after it, as Chinese is); less what
/// ends a sentence after it (`.` `,` `:` `;` `!` `?` `*` `_` `~` quotes), and a closing bracket it did not open.
fn addresses(text: &str) -> Vec<(Range<usize>, String)> {
    // The stretches of words, each as far as words go on one after another (the parser gives them in pieces, parted
    // at what may mark something: a `_`, a bracket).
    let mut runs: Vec<Range<usize>> = Vec::new();
    let mut in_link = 0usize;
    for (event, range) in Parser::new_ext(text, READ).into_offset_iter() {
        match event {
            Event::Start(Tag::Link { .. } | Tag::Image { .. }) => in_link += 1,
            Event::End(TagEnd::Link | TagEnd::Image) => in_link = in_link.saturating_sub(1),
            Event::Text(_) if in_link == 0 => match runs.last_mut() {
                Some(run) if run.end == range.start => run.end = range.end,
                _ => runs.push(range),
            },
            _ => {}
        }
    }
    let mut found = Vec::new();
    for range in runs {
        let words = &text[range.clone()];
        let mut from = 0;
        while let Some(found_at) = ["https://", "http://"].iter().filter_map(|scheme| words[from..].find(scheme)).min() {
            let start = from + found_at;
            let mut end = start + words[start..].find(|c: char| c.is_whitespace() || c == '<' || !c.is_ascii()).unwrap_or(words.len() - start);
            loop {
                let address = &words[start..end];
                match address.chars().last() {
                    Some('.' | ',' | ':' | ';' | '!' | '?' | '*' | '_' | '~' | '\'' | '"') => end -= 1,
                    Some(')') if address.matches(')').count() > address.matches('(').count() => end -= 1,
                    _ => break,
                }
            }
            // (A scheme alone is no address.)
            if words[start..end].split("://").nth(1).is_some_and(|rest| !rest.is_empty()) {
                found.push((range.start + start..range.start + end, words[start..end].to_string()));
            }
            from = end.max(start + 1);
        }
    }
    found
}

/// The to-dos' boxes in `text` (`[ ]`, `[x]`: where each is, in UTF-16 units, and whether it is done).
pub fn tasks(text: &str) -> Vec<(Range<usize>, bool)> {
    let units = Units::of(text);
    Parser::new_ext(text, READ)
        .into_offset_iter()
        .filter_map(|(event, range)| match event {
            Event::TaskListMarker(done) => Some((units.range(&range), done)),
            _ => None,
        })
        .collect()
}

/// Where each byte of a text is in UTF-16 units.
struct Units {
    at: Vec<usize>,
}

impl Units {
    fn of(text: &str) -> Units {
        let mut at = vec![0; text.len() + 1];
        let mut units = 0;
        for (byte, c) in text.char_indices() {
            for offset in 0..c.len_utf8() {
                at[byte + offset] = units;
            }
            units += c.len_utf16();
        }
        at[text.len()] = units;
        Units { at }
    }

    fn range(&self, bytes: &Range<usize>) -> Range<usize> {
        self.at[bytes.start.min(self.at.len() - 1)]..self.at[bytes.end.min(self.at.len() - 1)]
    }
}

/// `text`'s lines with words, as they read: each line's words as CommonMark reads the whole of `text` (a heading's,
/// a list's, a to-do's, a quote's marks, emphasis's and code's taken off, a link's words without where it goes, a
/// block of code's lines as written), in their order.
pub fn plain_lines(text: &str) -> Vec<String> {
    let starts: Vec<usize> = std::iter::once(0).chain(text.match_indices('\n').map(|(at, _)| at + 1)).collect();
    let line_of = |offset: usize| starts.partition_point(|start| *start <= offset) - 1;
    let mut lines: Vec<(usize, String)> = Vec::new();
    for (event, range) in Parser::new_ext(text, READ).into_offset_iter() {
        let words = match event {
            Event::Text(words) | Event::Code(words) | Event::Html(words) | Event::InlineHtml(words) => words,
            _ => continue,
        };
        // (Words over several lines, a block of code's, each on its own line, from the one they begin on.)
        for (more, piece) in words.split('\n').enumerate() {
            let line = line_of(range.start) + more;
            match lines.last_mut() {
                Some((last, kept)) if *last == line => kept.push_str(piece),
                _ => lines.push((line, piece.to_string())),
            }
        }
    }
    lines.into_iter().map(|(_, words)| words.trim().to_string()).filter(|words| !words.is_empty()).collect()
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Each line set as far apart as its words are tall; a heading's as its own size asks, where that is more; the
    /// empty line after a last line break too; in whole pixels.
    #[test]
    fn sets_each_line_as_tall_as_its_words_ask() {
        let lines = line_heights("# 标题\n正文\n#### 小\n", 20.0);
        assert_eq!(lines, vec![(0..5, (20.0 * 1.6 * HEADING_LEADING).round()), (5..8, 32.0), (8..15, 32.0), (15..15, 32.0)]);
        assert_eq!(line_heights("", 15.0), vec![(0..0, 24.0)]);
    }

    /// In a note not written in, a heading's `#`, emphasis's, code's and a link's marks are put away, a list's and a
    /// quote's not; a to-do is a box in its list's mark's place; a link, and a web address written out, followed from
    /// its words.
    #[test]
    fn puts_away_marks_in_a_note_not_written_in() {
        let text = "## 本周 ##\n**粗**与*斜*、`code`、~~删~~\n- [ ] 买牛奶\n- [x] 完成\n> 引\n[链接](https://a.b/c) 见 https://x.y/z?q=1。\n标题\n===";
        let units: Vec<u16> = text.encode_utf16().collect();
        let of = |range: &Range<usize>| String::from_utf16(&units[range.clone()]).unwrap();
        let shown = Shown::of(text);
        let hidden: Vec<String> = shown.hidden(None).iter().map(of).collect();
        assert_eq!(hidden, ["## ", " ##", "**", "**", "*", "*", "`", "`", "~~", "~~", "[", "](https://a.b/c)"]);
        let boxes: Vec<(String, String, bool)> = shown.boxes(None).map(|boxed| (of(&boxed.marks), of(&(boxed.room..boxed.room + 1)), boxed.done)).collect();
        assert_eq!(boxes, [("- [ ]".to_string(), " ".to_string(), false), ("- [x]".to_string(), " ".to_string(), true)]);
        let links: Vec<(String, &str)> = shown.links.iter().map(|link| (of(&link.words), link.to.as_str())).collect();
        assert_eq!(links, [("链接".to_string(), "https://a.b/c"), ("https://x.y/z?q=1".to_string(), "https://x.y/z?q=1")]);
    }

    /// Written in, what the caret is in or at either end of shows as written, marks and all, the rest put away still:
    /// the bold the caret is in (not the slanted words beside it), the heading the caret is on, a link the caret is in
    /// (not followed then). What is chosen over any of a stretch shows it. A to-do's box stays drawn with the caret
    /// where its words begin, shows as written with the caret before that.
    #[test]
    fn shows_as_written_what_the_caret_touches() {
        let text = "## 本周 ##\n**粗**与*斜*\n- [ ] 买\n[链接](https://a.b/c)";
        let units: Vec<u16> = text.encode_utf16().collect();
        let of = |range: &Range<usize>| String::from_utf16(&units[range.clone()]).unwrap();
        let at = |what: &str| text[..text.find(what).unwrap()].encode_utf16().count();
        let shown = Shown::of(text);
        let hidden = |at: At| shown.hidden(at).iter().map(of).collect::<Vec<_>>();
        let caret = |unit: usize| Some((unit, unit));
        // In the bold (粗), at its end (after its `**`): its marks as written.
        assert_eq!(hidden(caret(at("粗"))), ["## ", " ##", "*", "*", "[", "](https://a.b/c)"]);
        assert_eq!(hidden(caret(at("与"))), ["## ", " ##", "*", "*", "[", "](https://a.b/c)"]);
        // At the slanted words' start, not the bold's end: theirs.
        assert_eq!(hidden(caret(at("*斜"))), ["## ", " ##", "**", "**", "[", "](https://a.b/c)"]);
        // On the heading.
        assert_eq!(hidden(caret(at("周"))), ["**", "**", "*", "*", "[", "](https://a.b/c)"]);
        // What is chosen from the bold's words into the slanted's.
        assert_eq!(hidden(Some((at("粗"), at("斜")))), ["## ", " ##", "[", "](https://a.b/c)"]);
        // Chosen up to the bold's start, not over it.
        assert_eq!(hidden(Some((at("周"), at("**粗")))), ["**", "**", "*", "*", "[", "](https://a.b/c)"]);
        let link = shown.links[0].words.start;
        assert!(shown.link_at(link, caret(at("粗"))).is_some() && shown.link_at(link, caret(at("接"))).is_none());
        assert_eq!(shown.boxes(caret(at("买"))).count(), 1);
        assert_eq!(shown.boxes(caret(at(" 买"))).count(), 0);
        assert_eq!(shown.boxes(caret(at("- ["))).count(), 0);
    }

    /// A web address ends where a sentence does, or where words in another script go on after it.
    #[test]
    fn a_web_address_ends_where_the_sentence_does() {
        let found = |text: &str| addresses(text).into_iter().map(|(_, to)| to).collect::<Vec<_>>();
        assert_eq!(found("见 https://a.b/c."), ["https://a.b/c"]);
        assert_eq!(found("(see https://a.b/wiki/X_(y))"), ["https://a.b/wiki/X_(y)"]);
        assert_eq!(found("https://a.b/c看这个"), ["https://a.b/c"]);
        assert_eq!(found("http:// 与 https:// 不是"), Vec::<String>::new());
        assert_eq!(found("`https://in.code` 与 [w](https://link.only)"), Vec::<String>::new());
    }

    /// A note's lines read as their words, marks off: what its title and its card show.
    #[test]
    fn lines_read_without_their_marks() {
        let text = "## 本周\n\n- [ ] nihao\n- [x] **做完** 了\n> 引的 `code`\n1. 看 [链接](https://a.b/c)\n\n```\nlet a = 1;\n\nb\n```\n";
        assert_eq!(plain_lines(text), ["本周", "nihao", "做完 了", "引的 code", "看 链接", "let a = 1;", "b"]);
        assert_eq!(plain_lines("标题\n===\n下一行"), ["标题", "下一行"]);
        assert!(plain_lines("- [ ]\n#\n").is_empty());
    }

    fn look_of(looks: &[(Range<usize>, Look)], at: usize) -> Vec<Look> {
        looks.iter().filter(|(range, _)| range.contains(&at)).map(|(_, look)| *look).collect()
    }

    /// Each to-do's box, where it is and whether it is done.
    #[test]
    fn finds_the_to_dos() {
        let text = "清单
- [ ] 一
- [x] 二";
        let found = tasks(text);
        assert_eq!(found.len(), 2);
        assert_eq!((&text.encode_utf16().collect::<Vec<_>>()[found[0].0.clone()], found[0].1), (&"[ ]".encode_utf16().collect::<Vec<_>>()[..], false));
        assert!(found[1].1);
    }

    /// A heading: larger and bold, its `#` faint, its words plain; emphasis bold, its marks faint; counted in UTF-16
    /// (a Chinese character one unit, an emoji two).
    #[test]
    fn dresses_what_is_written() {
        let text = "# 本周\n**粗** 😀 `code`\n- [x] 完成\n[链接](https://a.b)";
        let looks = looks(text);
        // "#" (0) faint within the heading; "本" (2) plain within it.
        assert_eq!(look_of(&looks, 0), [Look::Heading(1), Look::Faint]);
        assert_eq!(look_of(&looks, 2), [Look::Heading(1), Look::Faint, Look::Plain]);
        // "**" at 5 faint, "粗" at 7 bold and plain.
        assert_eq!(look_of(&looks, 5), [Look::Bold, Look::Faint]);
        assert_eq!(look_of(&looks, 7), [Look::Bold, Look::Faint, Look::Plain]);
        // The emoji two units (11, 12), then a space; "`" at 14 faint code, "c" at 15 code and plain.
        assert_eq!(look_of(&looks, 14), [Look::Code, Look::Faint]);
        assert_eq!(look_of(&looks, 15), [Look::Code, Look::Faint, Look::Plain]);
        // The done task faint; the link's words in the accent.
        let task = text.encode_utf16().count() - "完成\n[链接](https://a.b)".encode_utf16().count();
        assert!(look_of(&looks, task).contains(&Look::Faint));
        let link = text.encode_utf16().count() - "链接](https://a.b)".encode_utf16().count();
        assert!(look_of(&looks, link).contains(&Look::Link));
    }

    /// Dressed as the looks laid on in their order make it: a heading's words larger, bold and in the words' ink,
    /// its `#` faint; bold's marks faint and its words not; every unit in one stretch, end to end.
    #[test]
    fn dresses_each_stretch_as_its_looks_laid_on_make_it() {
        let text = "# 本周\n**粗** 字";
        let stretches = dressing(text);
        let dress_at = |at: usize| stretches.iter().find(|(range, _)| range.contains(&at)).map(|(_, dress)| *dress).unwrap();
        assert_eq!(dress_at(0), Dress { size: 1.6, bold: true, ink: Ink::Faint, ..Dress::PLAIN });
        assert_eq!(dress_at(2), Dress { size: 1.6, bold: true, ..Dress::PLAIN });
        assert_eq!(dress_at(5), Dress { bold: true, ink: Ink::Faint, ..Dress::PLAIN });
        assert_eq!(dress_at(7), Dress { bold: true, ..Dress::PLAIN });
        assert_eq!(dress_at(11), Dress::PLAIN);
        assert_eq!(stretches.first().unwrap().0.start, 0);
        assert_eq!(stretches.last().unwrap().0.end, text.encode_utf16().count());
        assert!(stretches.windows(2).all(|pair| pair[0].0.end == pair[1].0.start && pair[0].1 != pair[1].1));
    }
}
