//! How a note's Markdown shows as it is written: what each stretch
//! of it looks like, for the box it is written in to dress it, the words themselves left as they are. Parsed with
//! pulldown-cmark, as CommonMark (and its task lists and strike-through) reads it.
//!
//! Its marks (a heading's `#`, emphasis's `*`, a list's `-`, code's `` ` ``) stay where they are, faint; what they
//! mark shows as it will read: a heading larger, emphasis bold or slanted, code in a fixed face, a link in the accent
//! (a web address written out one too), a quote and a done task faint.
//!
//! As it reads (unless its source is asked for: see `crate::notes::source`), its marks are put away (see `Shown`),
//! wherever the caret is: a heading's `#`, emphasis's, code's and a link's marks hidden, a to-do's list mark and box
//! drawn as a box to tick, a list's mark as a dot, a quote's as a bar beside it, a block of code's fences as the room
//! round the box it is drawn in, a rule as a line across, a table laid out, a link's words pressed to follow it. Only a
//! table the caret is in (see `At`) shows as written, marks and all. A list's numbers stay as written.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::ops::Range;

/// What of Markdown a note is read for, beyond CommonMark: to-dos, words struck through and tables. One reading, for what is
/// shown and for what the keys do to a note's words (see `edit`).
pub const READ: Options = Options::ENABLE_TASKLISTS.union(Options::ENABLE_STRIKETHROUGH).union(Options::ENABLE_TABLES);

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
    // (Whether each list open is one of bullets: a numbered list's numbers are words, not faint.)
    let mut bulleted = Vec::new();
    for (event, range) in Parser::new_ext(text, READ).into_offset_iter() {
        let at = units.range(&range);
        match event {
            Event::Start(Tag::List(first)) => bulleted.push(first.is_none()),
            Event::End(TagEnd::List(_)) => {
                bulleted.pop();
            }
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
            Event::Start(Tag::Item) if bulleted.last() == Some(&true) => looks.push((at, Look::Faint)),
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

/// How a note shows as it reads (see the head of this file): what of it is put away, where (in UTF-16 units, as the
/// text services count, each line break one), each with all of what it marks (its span: see `At`). Its source asked
/// for, none of it is: `Shown::default()`.
#[derive(Clone, Debug, Default, PartialEq)]
pub struct Shown {
    /// Its marks hidden: a heading's `#`, emphasis's, code's, a link's (all but its words), a block of code's fences.
    /// Each with the heading's, emphasis's, code's, link's or block's span (a table's, where it is in one).
    pub marks: Vec<(Range<usize>, Range<usize>)>,
    pub boxes: Vec<Boxed>,
    /// Its lists' bullets and its quotes' marks, each drawn in the room after it.
    pub roomed: Vec<Roomed>,
    pub links: Vec<Linked>,
    /// Its tables, put away whole (each line's words among its marks, with the table's span) and drawn laid out in
    /// their room (see `table`), and its rules, put away and drawn as a line across (each with its line's words).
    pub tables: Vec<super::table::Table>,
    pub rules: Vec<Range<usize>>,
    /// Its blocks of code drawn in a box, their fences put away.
    pub codes: Vec<Coded>,
    /// Where each line's marks put away at its start end (a heading's `#`, a list's mark and its to-do's box, a
    /// quote's `>`, the spaces before them, the rooms after them): the caret is not to rest among them (see `settle`).
    pub leads: Vec<Range<usize>>,
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

/// A list's bullet (`-`, `*`, `+`) or a quote's marks (`>`, as many as it is deep) put away, and the space after them,
/// the room what stands in for them is drawn in (see `Drawn::room`).
#[derive(Clone, Debug, PartialEq)]
pub struct Roomed {
    pub marks: Range<usize>,
    pub room: usize,
    pub drawn: Drawn,
}

/// What is drawn in a room in place of the marks put away: a list's dot, or a quote's bars (as many as it is deep).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Drawn {
    Bullet,
    Bars(usize),
}

impl Drawn {
    /// How wide its room is, its words' size over.
    pub fn room(self) -> f32 {
        match self {
            Drawn::Bullet => BULLET_ROOM,
            Drawn::Bars(deep) => QUOTE_ROOM * deep as f32,
        }
    }
}

/// A fenced block of code drawn in a box: all its lines (`span`, its fences' too), and its fences' lines, put away and
/// made low (`FENCE` of its words' size), the room above and below its words in the box (one not closed, its first
/// alone).
#[derive(Clone, Debug, PartialEq)]
pub struct Coded {
    pub span: Range<usize>,
    pub fences: Vec<Range<usize>>,
}

/// A link: its words, where it goes, all of it (its span). (A web address written out: its words where it goes, and
/// its span.)
#[derive(Clone, Debug, PartialEq)]
pub struct Linked {
    pub words: Range<usize>,
    pub to: String,
    pub span: Range<usize>,
}

/// How wide the room a to-do's box is drawn in is, its words' size over (the box in it, `BOX_SIDE` of their size,
/// `BOX_IN` from its start, its middle `BOX_RISE` over its words' baseline, as a Chinese character's is; the rest a gap
/// before its words).
pub const BOX_ROOM: f32 = 1.55;
pub const BOX_SIDE: f32 = 0.9;
pub const BOX_IN: f32 = 0.08;
pub const BOX_RISE: f32 = 0.36;
/// How wide the room a list's dot is drawn in is, its words' size over (the dot `BULLET_SIDE` across, its middle
/// `BULLET_IN` from the room's start and `BOX_RISE` over the words' baseline); a quote's, for each it is deep (its bar
/// `BAR_WIDE` wide, `BAR_IN` from the start of its share of the room, as tall as the line).
pub const BULLET_ROOM: f32 = 1.0;
pub const BULLET_SIDE: f32 = 0.3;
pub const BULLET_IN: f32 = 0.4;
pub const QUOTE_ROOM: f32 = 0.9;
pub const BAR_WIDE: f32 = 0.16;
pub const BAR_IN: f32 = 0.1;
/// How tall a block of code's fence's line is set, its words' size over (the room above and below its words in its
/// box), and how far its box reaches out past the words' sides.
pub const FENCE: f32 = 0.6;
pub const CODE_OUT: f32 = 0.5;

/// Which way the caret was moved (see `Shown::settle`): by an arrow, or put somewhere (pressed, Home, End, the words
/// changed).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Way {
    Left,
    Right,
    Up,
    Down,
    Near,
}

/// Where the caret settles (see `Shown::settle`): at a unit, or on a line to be begun after the last (put away whole,
/// nothing after it to write on).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Settled {
    At(usize),
    Below,
}

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
        // Each open container whose marks are put away, as far as what it holds reaches (its first and last held), and
        // whether it is a quote or a list's item (what is in it does not stand on its own). A list's item where it
        // begins.
        struct Open {
            hides: bool,
            range: Range<usize>,
            held: Option<Range<usize>>,
            contains: bool,
        }
        let mut open: Vec<Open> = Vec::new();
        let mut item: Option<usize> = None;
        // (Where each table drawn laid out begins. The table being read, all of it: what is marked in it shows as
        // written where it does, its links not gone to, and all the tables' spans.)
        let mut drawn = Vec::new();
        let mut table: Option<Range<usize>> = None;
        let mut table_spans = Vec::new();
        // (Whether each list open is one of bullets, and the bullet put away for the item being read: taken back where
        // it is a to-do's, its box drawn in its place. The block quotes (bytes), their marks found line by line once
        // all is read. The block of code being read, and whether it is on lines of its own. Where the headings' marks
        // put away at their lines' starts are.)
        let mut bulleted: Vec<bool> = Vec::new();
        let mut bullet: Option<usize> = None;
        let mut quotes: Vec<Range<usize>> = Vec::new();
        let mut code: Option<(Range<usize>, bool, bool)> = None;
        let mut leading: Vec<Range<usize>> = Vec::new();
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
                    let contained = open.iter().any(|open| open.contains);
                    match &tag {
                        Tag::List(first) => bulleted.push(first.is_none()),
                        // A bullet followed by a space: put away, a dot drawn in the space's room.
                        Tag::Item => {
                            item = Some(range.start);
                            let rest = &text[range.start..];
                            let marker = range.start + rest.len() - rest.trim_start_matches([' ', '\t']).len();
                            let marked = text[marker..].starts_with(['-', '*', '+']) && text[marker + 1..].starts_with(' ');
                            bullet = (bulleted.last() == Some(&true) && marked).then(|| {
                                shown.roomed.push(Roomed { marks: units.range(&(marker..marker + 1)), room: units.range(&(marker + 1..marker + 2)).start, drawn: Drawn::Bullet });
                                shown.roomed.len() - 1
                            });
                        }
                        Tag::BlockQuote(_) => quotes.push(range.clone()),
                        Tag::CodeBlock(kind) => code = Some((range.clone(), matches!(kind, pulldown_cmark::CodeBlockKind::Fenced(_)), super::table::on_its_own(text, range.start, contained).is_some())),
                        _ => {}
                    }
                    // A table on lines of its own put away, to be drawn laid out (one in a quote or a list's item is
                    // left as written).
                    if let Tag::Table(_) = &tag {
                        let own = super::table::on_its_own(text, range.start, contained);
                        let all = own.unwrap_or(range.start)..range.end;
                        if own.is_some() {
                            hide(&mut shown, all.clone(), &all);
                            drawn.push(span(&all).start);
                        }
                        table_spans.push(span(&all));
                        table = Some(all);
                    }
                    let hides = match &tag {
                        // (A heading as `#` makes it: one underlined is left as written.)
                        Tag::Heading { .. } => text[range.clone()].starts_with('#'),
                        Tag::Strong | Tag::Emphasis | Tag::Strikethrough | Tag::Link { .. } => true,
                        _ => false,
                    };
                    if let Tag::Link { dest_url, .. } = &tag {
                        // (Its words found as it ends.)
                        shown.links.push(Linked { words: 0..0, to: dest_url.to_string(), span: span(table.as_ref().unwrap_or(&range)) });
                    }
                    open.push(Open { hides, range, held: None, contains: matches!(tag, Tag::Item | Tag::BlockQuote(_)) });
                }
                Event::End(end) => {
                    let Some(closed) = open.pop() else { continue };
                    match end {
                        TagEnd::Table => table = None,
                        TagEnd::List(_) => {
                            bulleted.pop();
                        }
                        // A fenced block of code on lines of its own drawn in a box (its lines, from the first's start,
                        // as far as its last's words), its fences put away, where there is a line between them (with
                        // none, it is left as written: its box would hold nothing). One indented is left as written (a
                        // line begun with a tab or spaces is one: those who do not write Markdown do that).
                        TagEnd::CodeBlock => {
                            if let Some((range, fenced, true)) = code.take() {
                                let start = text[..range.start].rfind('\n').map_or(0, |at| at + 1);
                                let all = start..range.start + text[range.clone()].trim_end_matches(['\n', '\r']).len();
                                let opening = start..text[start..all.end].find('\n').map_or(all.end, |at| start + at);
                                let last = text[..all.end].rfind('\n').map_or(0, |at| at + 1)..all.end;
                                let fence = text[last.clone()].trim();
                                let closing = (last.start > opening.end && fence.len() >= 3 && (fence.chars().all(|c| c == '`') || fence.chars().all(|c| c == '~'))).then_some(last);
                                let between = closing.as_ref().map_or(all.end, |closing| closing.start.saturating_sub(1));
                                if fenced && between > opening.end {
                                    let mut fences = vec![opening.clone()];
                                    fences.extend(closing);
                                    for fence in &fences {
                                        hide(&mut shown, fence.clone(), &all);
                                    }
                                    let fences = fences.iter().map(&span).collect();
                                    shown.codes.push(Coded { span: span(&all), fences });
                                }
                            }
                        }
                        _ => {}
                    }
                    // (One holding nothing is left as written: put away, nothing of it would show.)
                    let of = table.clone().unwrap_or_else(|| closed.range.clone());
                    if let (true, Some(held)) = (closed.hides, &closed.held) {
                        hide(&mut shown, closed.range.start..held.start, &of);
                        hide(&mut shown, held.end..closed.range.end, &of);
                        if matches!(end, TagEnd::Heading(_)) {
                            leading.push(units.range(&(closed.range.start..held.start)));
                        }
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
                        let of = table.clone().unwrap_or_else(|| range.clone());
                        hide(&mut shown, range.start..range.start + ticks, &of);
                        hide(&mut shown, range.end - ticks..range.end, &of);
                    }
                }
                Event::Rule => {
                    hold(&mut open, &range);
                    if let Some(start) = super::table::on_its_own(text, range.start, open.iter().any(|open| open.contains)) {
                        let all = start..range.end;
                        hide(&mut shown, all.clone(), &all);
                        shown.rules.push(span(&all));
                    }
                }
                Event::TaskListMarker(done) => {
                    hold(&mut open, &range);
                    // (Its box drawn in the space after it: none, it is left as written.)
                    let from = item.unwrap_or(range.start);
                    if text[range.end..].starts_with(' ') && !text[from..range.end].contains('\n') {
                        shown.boxes.push(Boxed { marks: units.range(&(from..range.end)), room: units.range(&(range.end..range.end + 1)).start, done });
                        if let Some(bullet) = bullet.take() {
                            shown.roomed.remove(bullet);
                        }
                    }
                }
                _ => hold(&mut open, &range),
            }
        }
        Self::quote_marks(&mut shown, text, &units, &quotes);
        // Each line's marks put away at its start, from it (the spaces before them too) as far as they go on.
        leading.extend(shown.boxes.iter().map(Boxed::span));
        leading.extend(shown.roomed.iter().map(|roomed| roomed.marks.start..roomed.room + 1));
        let wide: Vec<u16> = text.encode_utf16().collect();
        for line in lines_of(&wide) {
            let (mut at, mut end) = (line.start, None);
            loop {
                while at < line.end && (wide[at] == u16::from(b' ') || wide[at] == u16::from(b'\t')) {
                    at += 1;
                }
                match leading.iter().find(|piece| piece.start == at && piece.end > at) {
                    Some(piece) => {
                        at = piece.end;
                        end = Some(at);
                    }
                    None => break,
                }
            }
            if let Some(end) = end {
                shown.leads.push(line.start..end);
            }
        }
        shown.links.retain(|link| !link.words.is_empty());
        for (words, to) in addresses(text) {
            let words = units.range(&words);
            let span = table_spans.iter().find(|table| table.start <= words.start && words.start < table.end).cloned().unwrap_or_else(|| words.clone());
            shown.links.push(Linked { words, to, span });
        }
        shown.tables = super::table::tables(text).into_iter().filter(|table| drawn.contains(&table.span.start)).collect();
        shown
    }

    /// Each line's block quotes' marks (`quotes`, bytes: all of each, the lines a quote is in), as many `>` as the
    /// quotes it is in (each after up to three spaces), and the space after the last: put away, bars drawn in the
    /// space's room. A line whose marks have no space after them, or whose words go on without them (a quote's
    /// paragraph carried on), is left as written.
    fn quote_marks(shown: &mut Shown, text: &str, units: &Units, quotes: &[Range<usize>]) {
        let mut start = 0;
        for line in text.split('\n') {
            let end = start + line.len();
            let deep = quotes.iter().filter(|quote| quote.start <= end && start < quote.end).count();
            let (mut at, mut marks) = (start, None::<Range<usize>>);
            for _ in 0..deep {
                let spaces = text[at..end].bytes().take_while(|&c| c == b' ').count();
                if spaces > 3 || !text[at + spaces..end].starts_with('>') {
                    break;
                }
                let mark = at + spaces;
                marks = Some(marks.map_or(mark..mark + 1, |marks| marks.start..mark + 1));
                at = mark + 1;
            }
            if let Some(marks) = marks.filter(|marks| text[marks.end..end].starts_with(' ')) {
                let count = text[marks.clone()].matches('>').count();
                shown.roomed.push(Roomed { marks: units.range(&marks), room: units.range(&(marks.end..marks.end + 1)).start, drawn: Drawn::Bars(count) });
            }
            start = end + 1;
        }
        shown.roomed.sort_by_key(|roomed| roomed.marks.start);
    }

    /// Its marks hidden: all of them (a table's, written in, written in its cell's own box: see `ui::note`).
    pub fn hidden(&self) -> Vec<Range<usize>> {
        self.marks.iter().map(|(range, _)| range.clone()).collect()
    }

    /// Its blocks of code's fences' lines, set low (`FENCE`).
    pub fn fences(&self) -> impl Iterator<Item = &Range<usize>> + '_ {
        self.codes.iter().flat_map(|code| &code.fences)
    }

    /// The lines set otherwise than their words would set them, its words `words` pixels high: its fences', `FENCE` of
    /// that (whole pixels), and each table's drawn laid out (`tables`, its span and how tall it is laid out), for
    /// `line_heights`.
    pub fn drawn_lines(&self, words: f32, tables: impl IntoIterator<Item = (Range<usize>, f32)>) -> Vec<(Range<usize>, f32)> {
        let mut drawn: Vec<_> = tables.into_iter().collect();
        drawn.extend(self.fences().map(|fence| (fence.clone(), (words * FENCE).round())));
        drawn
    }

    /// What is put away: its marks hidden, and its to-dos', bullets' and quotes' marks (each drawn in its room, the
    /// rooms given too, each with how wide it is, its words' size over).
    pub fn put_away(&self) -> (Vec<Range<usize>>, Vec<(usize, f32)>) {
        let mut hidden = self.hidden();
        hidden.extend(self.boxes.iter().map(|boxed| boxed.marks.clone()));
        hidden.extend(self.roomed.iter().map(|roomed| roomed.marks.clone()));
        let mut rooms: Vec<(usize, f32)> = self.boxes.iter().map(|boxed| (boxed.room, BOX_ROOM)).collect();
        rooms.extend(self.roomed.iter().map(|roomed| (roomed.room, roomed.drawn.room())));
        (hidden, rooms)
    }

    /// The link whose words are at `unit` (UTF-16 units), where they show as it reads: not in a table's cell (its words
    /// drawn in the table, not where they are written).
    pub fn link_at(&self, unit: usize) -> Option<&Linked> {
        self.links.iter().find(|link| link.words.contains(&unit) && !self.tables.iter().any(|table| table.span.start == link.span.start))
    }

    /// Where the caret at `caret` in its words (`units`, as it was read) is to rest, moved `way`: not on a line put
    /// away whole (a rule, a fence), past it the way it was moved (left or up, to the end of the line before it; else
    /// to the start of the line after it, or, after the last, on a line to be begun), nor among the marks put away at
    /// a line's start (past them; moved left from past them, to the end of the line before). None where it rests
    /// where it is.
    pub fn settle(&self, units: &[u16], caret: usize, way: Way) -> Option<Settled> {
        let lines = lines_of(units);
        let whole = |line: &Range<usize>| self.rules.iter().chain(self.fences()).any(|hidden| hidden == line);
        let (mut at, mut way) = (caret.min(units.len()), way);
        loop {
            let index = lines.partition_point(|line| line.end < at).min(lines.len() - 1);
            let line = &lines[index];
            let back = matches!(way, Way::Left | Way::Up);
            if whole(line) {
                match (back, index > 0, index + 1 < lines.len()) {
                    (true, true, _) => at = lines[index - 1].end,
                    (_, _, true) => {
                        at = lines[index + 1].start;
                        way = if back { Way::Down } else { way };
                    }
                    (false, _, false) => return Some(Settled::Below),
                    (true, false, false) => break,
                }
                continue;
            }
            match self.leads.iter().find(|lead| lead.start == line.start && at < lead.end) {
                Some(_) if way == Way::Left && index > 0 => at = lines[index - 1].end,
                Some(lead) => {
                    at = lead.end;
                    break;
                }
                None => break,
            }
        }
        (at != caret).then_some(Settled::At(at))
    }
}

/// The lines of words `units` (UTF-16 units), each without its line break (`\n`, or `\r\n`): the empty one after a
/// last line break too.
pub fn lines_of(units: &[u16]) -> Vec<Range<usize>> {
    let mut lines = Vec::new();
    let mut start = 0;
    for (at, &unit) in units.iter().enumerate() {
        if unit == u16::from(b'\n') {
            let end = if at > start && units[at - 1] == u16::from(b'\r') { at - 1 } else { at };
            lines.push(start..end);
            start = at + 1;
        }
    }
    lines.push(start..units.len());
    lines
}

/// How far apart a note's lines are set, over the height of their words; a heading's, over the height of its own
/// (larger: less room round it), where that sets it further apart than its words' would.
pub const LEADING: f32 = 1.6;
pub const HEADING_LEADING: f32 = 1.4;

/// Each of `text`'s lines (UTF-16 units, its line break with it) and how tall it is set (whole pixels), its words
/// `words` pixels high: the same however it is drawn (the text services as it is written, DirectWrite on its face),
/// so its lines are where they were as one gives way to the other. The lines of each table drawn laid out (`drawn`: its
/// span, and how tall it is laid out, whole) share its height between them, as its room.
pub fn line_heights(text: &str, words: f32, drawn: &[(Range<usize>, f32)]) -> Vec<(Range<usize>, f32)> {
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
    // (A table's lines, the `nth` of `count`, each as far down its room as its share of it, in whole pixels.)
    for (span, tall) in drawn {
        let within: Vec<usize> = (0..lines.len()).filter(|&line| span.start <= lines[line].0.start && lines[line].0.start <= span.end).collect();
        let count = within.len() as f32;
        for (nth, line) in within.into_iter().enumerate() {
            lines[line].1 = (tall * (nth + 1) as f32 / count).round() - (tall * nth as f32 / count).round();
        }
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
pub(crate) struct Units {
    at: Vec<usize>,
}

impl Units {
    pub(crate) fn of(text: &str) -> Units {
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

    pub(crate) fn range(&self, bytes: &Range<usize>) -> Range<usize> {
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
        let lines = line_heights("# 标题\n正文\n#### 小\n", 20.0, &[]);
        assert_eq!(lines, vec![(0..5, (20.0 * 1.6 * HEADING_LEADING).round()), (5..8, 32.0), (8..15, 32.0), (15..15, 32.0)]);
        assert_eq!(line_heights("", 15.0, &[]), vec![(0..0, 24.0)]);
        // A table's three lines share the 100 pixels it is laid out in, in whole pixels, the lines round it as they were.
        let lines = line_heights("a\n| x |\n| - |\n| y |\nb", 20.0, &[(2..19, 100.0)]);
        assert_eq!(lines.iter().map(|(_, tall)| *tall).collect::<Vec<_>>(), [32.0, 33.0, 34.0, 33.0, 32.0]);
    }

    /// A table on lines of its own, and a rule, put away, to be drawn (a table from its line's start, spaces before it
    /// with it), one in a quote left as written.
    #[test]
    fn puts_tables_and_rules_away() {
        let text = "a\n\n  | x | y |\n| - | - |\n| 1 | 2 |\n\n---\n> | q |\n> | - |";
        let units: Vec<u16> = text.encode_utf16().collect();
        let of = |range: &Range<usize>| String::from_utf16(&units[range.clone()]).unwrap();
        let shown = Shown::of(text);
        assert_eq!(shown.tables.len(), 1);
        assert_eq!(of(&shown.tables[0].span), "  | x | y |\n| - | - |\n| 1 | 2 |");
        assert_eq!(shown.rules.iter().map(of).collect::<Vec<_>>(), ["---"]);
        let hidden: Vec<String> = shown.hidden().iter().map(of).collect();
        for line in ["  | x | y |", "| - | - |", "| 1 | 2 |", "---"] {
            assert!(hidden.contains(&line.to_string()), "{line} in {hidden:?}");
        }
        assert!(!hidden.iter().any(|hidden| hidden.contains('q')));
    }

    /// A table's links are its cells' (drawn in the table, not where they are written): not followed from where they
    /// are written. One in a list's item (only spaces before it on its line) is the item's: left as written.
    #[test]
    fn a_table_s_links_are_its_cells() {
        let text = "| **粗** | [链](https://a.b) |\n| --- | --- |\n| x | https://c.d |\n\n[外](https://e.f)";
        let shown = Shown::of(text);
        assert_eq!(shown.links.len(), 3);
        let to = |to: &str| shown.links.iter().find(|link| link.to == to).unwrap();
        for inside in ["https://a.b", "https://c.d"] {
            assert_eq!(to(inside).span.start, shown.tables[0].span.start);
            assert!(shown.link_at(to(inside).words.start).is_none());
        }
        assert!(shown.link_at(to("https://e.f").words.start).is_some());
        let listed = Shown::of("- 项\n\n  | a |\n  | - |\n  | b |\n\n  ---");
        assert!(listed.tables.is_empty() && listed.rules.is_empty());
    }

    /// A heading's `#`, emphasis's, code's and a link's marks are put away; a to-do is a box in its list's mark's
    /// place, a list's bullet a dot, a quote's `>` a bar; a link, and a web address written out, followed from its
    /// words. A heading underlined and a numbered list's numbers are left as written.
    #[test]
    fn puts_away_marks_as_the_note_reads() {
        let text = "## 本周 ##\n**粗**与*斜*、`code`、~~删~~\n- [ ] 买牛奶\n- [x] 完成\n> 引\n[链接](https://a.b/c) 见 https://x.y/z?q=1。\n标题\n===\n\n- 点\n  * 里\n1. 一";
        let units: Vec<u16> = text.encode_utf16().collect();
        let of = |range: &Range<usize>| String::from_utf16(&units[range.clone()]).unwrap();
        let shown = Shown::of(text);
        let hidden: Vec<String> = shown.hidden().iter().map(of).collect();
        assert_eq!(hidden, ["## ", " ##", "**", "**", "*", "*", "`", "`", "~~", "~~", "[", "](https://a.b/c)"]);
        let boxes: Vec<(String, String, bool)> = shown.boxes.iter().map(|boxed| (of(&boxed.marks), of(&(boxed.room..boxed.room + 1)), boxed.done)).collect();
        assert_eq!(boxes, [("- [ ]".to_string(), " ".to_string(), false), ("- [x]".to_string(), " ".to_string(), true)]);
        let roomed: Vec<(String, String, Drawn)> = shown.roomed.iter().map(|roomed| (of(&roomed.marks), of(&(roomed.room..roomed.room + 1)), roomed.drawn)).collect();
        assert_eq!(roomed, [(">".to_string(), " ".to_string(), Drawn::Bars(1)), ("-".to_string(), " ".to_string(), Drawn::Bullet), ("*".to_string(), " ".to_string(), Drawn::Bullet)]);
        let links: Vec<(String, &str)> = shown.links.iter().map(|link| (of(&link.words), link.to.as_str())).collect();
        assert_eq!(links, [("链接".to_string(), "https://a.b/c"), ("https://x.y/z?q=1".to_string(), "https://x.y/z?q=1")]);
        // What is put away of it, its rooms with how wide each is.
        let (put, rooms) = shown.put_away();
        assert!(put.iter().map(of).any(|put| put == "- [ ]") && put.iter().map(of).any(|put| put == ">"));
        assert_eq!(rooms.iter().map(|(_, wide)| *wide).collect::<Vec<_>>(), [BOX_ROOM, BOX_ROOM, QUOTE_ROOM, BULLET_ROOM, BULLET_ROOM]);
        // A numbered list's number as words, a bullet faint.
        let looks = looks(text);
        assert!(!look_of(&looks, text.encode_utf16().count() - "1. 一".encode_utf16().count()).contains(&Look::Faint));
        assert!(look_of(&looks, text.encode_utf16().count() - "- 点\n  * 里\n1. 一".encode_utf16().count()).contains(&Look::Faint));
    }

    /// A fenced block of code on lines of its own: its fences put away and set low, its box all its lines; with no line
    /// between its fences, or in a quote, left as written, and an indented one.
    #[test]
    fn puts_a_block_of_code_s_fences_away() {
        let text = "a\n\n```rust\nlet x = 1;\n\n```\n\n~~~\n~~~\n\n> ```\n> q\n> ```\n\n    indented\n\n```\n";
        let units: Vec<u16> = text.encode_utf16().collect();
        let of = |range: &Range<usize>| String::from_utf16(&units[range.clone()]).unwrap();
        let shown = Shown::of(text);
        let codes: Vec<(String, Vec<String>)> = shown.codes.iter().map(|code| (of(&code.span), code.fences.iter().map(of).collect())).collect();
        assert_eq!(codes, [("```rust\nlet x = 1;\n\n```".to_string(), vec!["```rust".to_string(), "```".to_string()])]);
        let hidden: Vec<String> = shown.hidden().iter().map(of).collect();
        assert_eq!(hidden.iter().filter(|hidden| hidden.contains('`') || hidden.contains('~')).collect::<Vec<_>>(), ["```rust", "```"]);
        // Its fences' lines set low, the rest as their words set them.
        let drawn = shown.drawn_lines(20.0, []);
        let lines = line_heights(text, 20.0, &drawn);
        let tall: Vec<f32> = lines[2..6].iter().map(|(_, tall)| *tall).collect();
        assert_eq!(tall, [12.0, 32.0, 32.0, 12.0]);
        // One not closed, with a line after its fence: its fence put away, the box as far as its words go.
        let open = Shown::of("```\nx\ny");
        assert_eq!(open.codes.len(), 1);
        assert_eq!((open.codes[0].fences.len(), open.codes[0].fences[0].clone()), (1, 0..3));
        assert_eq!(open.codes[0].span, 0..7);
        assert!(Shown::of("```").codes.is_empty() && Shown::of("```\n").codes.is_empty());
    }

    /// Quotes' marks as deep as each line's quotes, the space after them the room; one with none after it, or a
    /// quote's paragraph carried on without them, left as written.
    #[test]
    fn puts_a_quote_s_marks_away() {
        let text = "> a\n> > b\n>c\nd\n\n  > - e";
        let units: Vec<u16> = text.encode_utf16().collect();
        let of = |range: &Range<usize>| String::from_utf16(&units[range.clone()]).unwrap();
        let shown = Shown::of(text);
        let roomed: Vec<(String, Drawn)> = shown.roomed.iter().map(|roomed| (of(&roomed.marks), roomed.drawn)).collect();
        assert_eq!(roomed, [(">".to_string(), Drawn::Bars(1)), ("> >".to_string(), Drawn::Bars(2)), (">".to_string(), Drawn::Bars(1)), ("-".to_string(), Drawn::Bullet)]);
        // Each line's marks put away at its start, as far as they go on: the spaces before them, a quote's and a
        // list's together.
        let leads: Vec<String> = shown.leads.iter().map(of).collect();
        assert_eq!(leads, ["> ", "> > ", "  > - "]);
    }

    /// The caret is not to rest among a line's marks put away at its start (past them, or moved left, to the line
    /// before), nor on a line put away whole (past it the way it was moved, after the last on a line to be begun).
    #[test]
    fn the_caret_rests_where_words_are() {
        let text = "# 标题\n- [ ] 事\n---\n```\nx\n```";
        let units: Vec<u16> = text.encode_utf16().collect();
        let at = |what: &str| text[..text.find(what).unwrap()].encode_utf16().count();
        let shown = Shown::of(text);
        let settle = |caret: usize, way: Way| shown.settle(&units, caret, way);
        // Among a heading's marks: past them; moved left, from the first line, past them still.
        assert_eq!(settle(0, Way::Near), Some(Settled::At(2)));
        assert_eq!(settle(1, Way::Left), Some(Settled::At(2)));
        assert_eq!(settle(2, Way::Near), None);
        // Among a to-do's: past them and its room; moved left, to the end of the line before.
        assert_eq!(settle(at("- ["), Way::Near), Some(Settled::At(at("事"))));
        assert_eq!(settle(at("- [") + 2, Way::Up), Some(Settled::At(at("事"))));
        assert_eq!(settle(at("- ["), Way::Left), Some(Settled::At(at("\n- ["))));
        // On a rule: down to the line after it (a fence, past that too), or up to the end of the line before.
        assert_eq!(settle(at("---"), Way::Down), Some(Settled::At(at("x"))));
        assert_eq!(settle(at("---") + 3, Way::Right), Some(Settled::At(at("x"))));
        assert_eq!(settle(at("---"), Way::Left), Some(Settled::At(at("\n---"))));
        assert_eq!(settle(at("```\nx") + 1, Way::Up), Some(Settled::At(at("\n---"))));
        // On the last line, a fence: up, to the end of the line before; down or pressed, on a line to be begun.
        assert_eq!(settle(units.len(), Way::Left), Some(Settled::At(at("x") + 1)));
        assert_eq!(settle(units.len(), Way::Near), Some(Settled::Below));
        assert_eq!(settle(units.len() - 1, Way::Down), Some(Settled::Below));
        assert_eq!(settle(at("x"), Way::Near), None);
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
