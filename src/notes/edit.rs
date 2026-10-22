//! What the keys that write Markdown for the writer do to a note's words:
//! a list carried on to the next line, moved in and out, its mark taken off, a to-do ticked, lines moved, emphasis,
//! code and headings put on and taken off, a link made. Each is an `Edit` (one replacement, one step for undo to take
//! back), or none where the key is to do what it always does. Lists are read as the note is shown (`markdown::READ`),
//! so what is in a block of code, or a list's mark written as words, is left alone. Positions are in UTF-16 units, as
//! the text services count them, each line break one (`\n`).

use super::markdown::READ;
use pulldown_cmark::{CodeBlockKind, Event, Parser, Tag, TagEnd};
use std::collections::{BTreeMap, HashMap, HashSet};
use std::ops::{Range, RangeInclusive};

/// An edit of a note's words: `range` (UTF-16 units) written over with `with`, and what is chosen after it (from, to).
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Edit {
    pub range: Range<usize>,
    pub with: String,
    pub chosen: (usize, usize),
}

/// The UTF-16 unit the byte `byte` of `text` begins.
fn unit(text: &str, byte: usize) -> usize {
    text[..byte].encode_utf16().count()
}

/// The byte the UTF-16 unit `unit` of `text` begins (within a character, the next one's, past the end, the end).
fn byte(text: &str, unit: usize) -> usize {
    let mut at = 0;
    for (byte, c) in text.char_indices() {
        if at >= unit {
            return byte;
        }
        at += c.len_utf16();
    }
    text.len()
}

/// The edit that makes `before` into `after` (what differs between them, whole characters), `chosen` (bytes of
/// `after`) chosen after it.
fn edit(before: &str, after: &str, chosen: (usize, usize)) -> Edit {
    let prefix = before.char_indices().zip(after.chars()).find(|((_, a), b)| a != b).map_or(before.len().min(after.len()), |((at, _), _)| at);
    let mut suffix = 0;
    for (a, b) in before[prefix..].chars().rev().zip(after[prefix..].chars().rev()) {
        if a != b {
            break;
        }
        suffix += a.len_utf8();
    }
    Edit { range: unit(before, prefix)..unit(before, before.len() - suffix), with: after[prefix..after.len() - suffix].to_string(), chosen: (unit(after, chosen.0), unit(after, chosen.1)) }
}

/// How far into its line the byte `at` of `text` is, in columns (a tab to the next multiple of four).
fn column(text: &str, at: usize) -> usize {
    let start = text[..at].rfind('\n').map_or(0, |line| line + 1);
    text[start..at].chars().fold(0, |column, c| if c == '\t' { column + 4 - column % 4 } else { column + 1 })
}

/// How many bytes of `line` the marks of `quotes` block quotes it is in take (each `>`, the spaces before it and one
/// after): where what is in them begins.
fn quoted(line: &str, quotes: usize) -> usize {
    let mut at = 0;
    for _ in 0..quotes {
        let rest = &line[at..];
        let spaces = rest.len() - rest.trim_start_matches([' ', '\t']).len();
        if !rest[spaces..].starts_with('>') {
            break;
        }
        at += spaces + 1;
        if line[at..].starts_with(' ') {
            at += 1;
        }
    }
    at
}

/// A list's mark: a bullet (`-`, `*`, `+`), or a number and what follows it (`.` or `)`).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
enum Mark {
    Bullet(char),
    Number(u64, char),
}

/// The greatest number a list's item can be written with (nine digits).
const LARGEST: u64 = 999_999_999;

/// A list's item, as the note is read.
struct Item {
    /// Where its mark is, and the byte after it.
    marker: usize,
    mark: Mark,
    mark_end: usize,
    /// Where what is in it begins: past its mark and the spaces after it (one where there are five or more, the rest
    /// code), as far in as an item under it begins.
    column: usize,
    /// A to-do's box written where what is in it begins (`[ ]`, `[x]`, a space or the line's end after it): where its
    /// `[` is, and whether ticked. And where its words begin, past the box.
    boxed: Option<(usize, bool)>,
    words: usize,
    /// The byte after all of it (what is under it, the blank lines after it).
    end: usize,
    /// The item it is in, its list (`Tree::lists`) and how many block quotes it is in.
    parent: Option<usize>,
    list: usize,
    quotes: usize,
    /// Whether its mark begins its line, past its block quotes' marks (not after another item's mark on the line).
    first: bool,
}

/// A list: all of it (bytes) and its items in order.
struct List {
    range: Range<usize>,
    items: Vec<usize>,
}

/// A note's words as these edits read them: its lines (each without its line break), its lists' items, its lists,
/// its blocks of code (and those of them fenced), its block quotes, its tables and its headings underlined (all of
/// each, the line under it too).
struct Tree {
    lines: Vec<Range<usize>>,
    items: Vec<Item>,
    lists: Vec<List>,
    code: Vec<Range<usize>>,
    fenced: Vec<Range<usize>>,
    quotes: Vec<Range<usize>>,
    tables: Vec<Range<usize>>,
    underlined: Vec<Range<usize>>,
}

/// Whether `line` is a list's marks alone, as written before its words are (`- `, `1.`, `- [ ] `, `> - - `): no rule.
fn marks_alone(line: &str) -> bool {
    let mut rest = line.trim_start_matches([' ', '\t']);
    while let Some(after) = rest.strip_prefix('>') {
        rest = after.trim_start_matches([' ', '\t']);
    }
    let rule = rest.chars().next().is_some_and(|first| matches!(first, '-' | '*' | '_') && rest.chars().all(|c| c == first || c == ' ' || c == '\t') && rest.chars().filter(|&c| c == first).count() >= 3);
    if rule {
        return false;
    }
    let mut marks = 0;
    loop {
        let digits = rest.bytes().take_while(u8::is_ascii_digit).count();
        let mark = match digits {
            0 if rest.starts_with(['-', '*', '+']) => 1,
            1..=9 if rest[digits..].starts_with(['.', ')']) => digits + 1,
            _ => break,
        };
        let after = &rest[mark..];
        if !(after.is_empty() || after.starts_with([' ', '\t'])) {
            break;
        }
        rest = after.trim_start_matches([' ', '\t']);
        marks += 1;
    }
    marks > 0 && (rest.is_empty() || ["[ ]", "[x]", "[X]"].contains(&rest))
}

/// `text` as these edits read it: each line that is a list's marks alone given a word, so it is read as the item it is
/// to be once written in (CommonMark reads an item with nothing in it, under a paragraph, as the paragraph's words), and
/// where each word was put (the byte of `text` it was put before, and how many bytes it is).
fn to_be_written(text: &str) -> (String, Vec<(usize, usize)>) {
    let mut written = String::with_capacity(text.len());
    let mut put = Vec::new();
    let mut start = 0;
    for line in text.split('\n') {
        written.push_str(line);
        if marks_alone(line) {
            let word = if line.ends_with([' ', '\t']) { "x" } else { " x" };
            put.push((start + line.len(), word.len()));
            written.push_str(word);
        }
        start += line.len() + 1;
        if start <= text.len() {
            written.push('\n');
        }
    }
    (written, put)
}

impl Tree {
    fn of(text: &str) -> Tree {
        let mut lines = Vec::new();
        let mut start = 0;
        for (at, _) in text.match_indices('\n') {
            lines.push(start..at);
            start = at + 1;
        }
        lines.push(start..text.len());
        let mut tree = Tree { lines, items: Vec::new(), lists: Vec::new(), code: Vec::new(), fenced: Vec::new(), quotes: Vec::new(), tables: Vec::new(), underlined: Vec::new() };
        let (mut items, mut lists, mut quotes) = (Vec::new(), Vec::new(), 0);
        let (written, put) = to_be_written(text);
        // (A byte of what was read where it is in `text`: within a word put, where it was put.)
        let back = |at: usize| {
            let mut by = 0;
            for &(before, length) in &put {
                if at < before + by {
                    break;
                }
                if at < before + by + length {
                    return before;
                }
                by += length;
            }
            at - by
        };
        for (event, range) in Parser::new_ext(&written, READ).into_offset_iter() {
            let range = back(range.start)..back(range.end);
            match event {
                Event::Start(Tag::BlockQuote(_)) => {
                    quotes += 1;
                    tree.quotes.push(range);
                }
                Event::End(TagEnd::BlockQuote(_)) => quotes -= 1,
                Event::Start(Tag::List(_)) => {
                    lists.push(tree.lists.len());
                    tree.lists.push(List { range, items: Vec::new() });
                }
                Event::End(TagEnd::List(_)) => {
                    lists.pop();
                }
                Event::Start(Tag::Item) => {
                    let list = *lists.last().expect("an item is in a list");
                    let item = tree.item(text, range, items.last().copied(), list, quotes);
                    tree.lists[list].items.push(tree.items.len());
                    items.push(tree.items.len());
                    tree.items.push(item);
                }
                Event::End(TagEnd::Item) => {
                    items.pop();
                }
                Event::Start(Tag::CodeBlock(kind)) => {
                    if matches!(kind, CodeBlockKind::Fenced(_)) {
                        tree.fenced.push(range.clone());
                    }
                    tree.code.push(range);
                }
                Event::Start(Tag::Table(_)) => tree.tables.push(range),
                Event::Start(Tag::Heading { .. }) if !text[range.clone()].starts_with('#') => tree.underlined.push(range),
                _ => {}
            }
        }
        tree
    }

    /// The item all of which is `range`, as `Tree::of` finds one.
    fn item(&self, text: &str, range: Range<usize>, parent: Option<usize>, list: usize, quotes: usize) -> Item {
        let rest = &text[range.start..];
        let marker = range.start + rest.len() - rest.trim_start_matches([' ', '\t']).len();
        let digits = text[marker..].bytes().take_while(u8::is_ascii_digit).count();
        let (mark, mark_end) = match digits {
            0 => (Mark::Bullet(text[marker..].chars().next().expect("a mark")), marker + 1),
            _ => (Mark::Number(text[marker..marker + digits].parse().expect("digits"), char::from(text.as_bytes()[marker + digits])), marker + digits + 1),
        };
        let line = &self.lines[self.line_of(marker)];
        let after = &text[mark_end..line.end];
        let spaces = after.len() - after.trim_start_matches([' ', '\t']).len();
        let column = if spaces == after.len() {
            (mark_end + 1).min(line.end)
        } else if column(text, mark_end + spaces) - column(text, mark_end) > 4 {
            mark_end + 1
        } else {
            mark_end + spaces
        };
        let written = &text[column..line.end];
        let boxed = [("[ ]", false), ("[x]", true), ("[X]", true)].into_iter().find(|(box_, _)| written.starts_with(box_) && (written.len() == 3 || written[3..].starts_with([' ', '\t']))).map(|(_, done)| (column, done));
        let words = match boxed {
            Some(_) => (column + 4).min(line.end),
            None if spaces == after.len() => line.end,
            None => column,
        };
        let opened = &text[line.start..marker];
        let first = opened[quoted(opened, quotes)..].trim().is_empty();
        Item { marker, mark, mark_end, column, boxed, words, end: range.end, parent, list, quotes, first }
    }

    /// Which line the byte `byte` is on (a line break, the line it ends).
    fn line_of(&self, byte: usize) -> usize {
        self.lines.partition_point(|line| line.end < byte)
    }

    /// The items whose marks are on line `index`, the outermost first.
    fn on_line(&self, index: usize) -> impl DoubleEndedIterator<Item = usize> + '_ {
        (0..self.items.len()).filter(move |&item| self.line_of(self.items[item].marker) == index)
    }

    /// The lines of item `item`: its mark's to the last of what is under it.
    fn lines_of(&self, item: usize) -> RangeInclusive<usize> {
        let item = &self.items[item];
        self.line_of(item.marker)..=self.line_of(item.end - 1)
    }

    /// Whether the line `index` is in a block of code (its fences' lines too, wherever on them a fence begins).
    fn in_code(&self, index: usize) -> bool {
        let line = &self.lines[index];
        self.code.iter().any(|code| code.start <= line.end && line.start < code.end)
    }

    /// The innermost item whose lines line `index` is among.
    fn item_holding(&self, index: usize) -> Option<usize> {
        (0..self.items.len()).filter(|&item| self.lines_of(item).contains(&index)).last()
    }

    /// Where line `index`'s words begin: past a list's item's mark and its box, or past block quotes' marks and the
    /// spaces before the words.
    fn words_of(&self, text: &str, index: usize) -> usize {
        if let Some(item) = self.on_line(index).last() {
            return self.items[item].words;
        }
        let line = self.lines[index].clone();
        let quotes = self.quotes.iter().filter(|quote| quote.start <= line.end && line.start < quote.end).count();
        let at = line.start + quoted(&text[line.clone()], quotes);
        line.end - text[at..line.end].trim_start_matches([' ', '\t']).len()
    }
}

/// Changes to a note's words, made together: each a stretch of it (bytes) written over, none over another.
#[derive(Default)]
struct Rewrite {
    edits: Vec<(Range<usize>, String)>,
}

impl Rewrite {
    fn put(&mut self, range: Range<usize>, with: impl Into<String>) {
        self.edits.push((range, with.into()));
    }

    fn sorted(&self) -> Vec<&(Range<usize>, String)> {
        let mut edits: Vec<_> = self.edits.iter().collect();
        edits.sort_by_key(|(range, _)| (range.start, range.end));
        edits
    }

    fn apply(&self, text: &str) -> String {
        let mut out = String::with_capacity(text.len());
        let mut at = 0;
        for (range, with) in self.sorted() {
            out.push_str(&text[at..range.start]);
            out.push_str(with);
            at = range.end;
        }
        out.push_str(&text[at..]);
        out
    }

    /// Where the byte `byte` is once these changes are made: past a change before it as far as the change moved what
    /// follows, within one, as far into what it writes as it was into what it wrote over (at its end, at its end, at
    /// words put in where nothing was, after them).
    fn map(&self, byte: usize) -> usize {
        let mut by = 0isize;
        for (range, with) in self.sorted() {
            if byte < range.start {
                break;
            }
            if byte >= range.end {
                by += with.len() as isize - range.len() as isize;
                continue;
            }
            return (range.start as isize + by) as usize + (byte - range.start).min(with.len());
        }
        (byte as isize + by) as usize
    }

    /// Where the changes are in the text they make.
    fn made(&self) -> Vec<Range<usize>> {
        let mut by = 0isize;
        let mut made = Vec::new();
        for (range, with) in self.sorted() {
            let start = (range.start as isize + by) as usize;
            made.push(start..start + with.len());
            by += with.len() as isize - range.len() as isize;
        }
        made
    }

    /// The lines `which` of `text` (in `quotes` block quotes) moved `by` columns further in (or out, less than none
    /// out not), what is before their words written as spaces. Lines of no words are left.
    fn shift(&mut self, text: &str, tree: &Tree, which: RangeInclusive<usize>, quotes: usize, by: isize) {
        for index in which {
            let line = tree.lines[index].clone();
            if text[line.clone()].trim().is_empty() {
                continue;
            }
            let from = line.start + quoted(&text[line.clone()], quotes);
            let to = line.end - text[from..line.end].trim_start_matches([' ', '\t']).len();
            let width = (column(text, to) - column(text, from)) as isize;
            let wanted = (width + by).max(0);
            if wanted != width {
                self.put(from..to, " ".repeat(wanted as usize));
            }
        }
    }
}

/// `rewrite` made to `text`, and the numbered lists it touches numbered on, as one edit: each list from the number the
/// list its first item was in began with before (as written, for an item new), one more for each item after it, what
/// is under an item whose number grows or shrinks in width moved with its words. `chosen` (bytes of the text as
/// `rewrite` leaves it) moved along.
///
/// The items of the lists touched are read as items still while they are numbered: a numbered list begun right under
/// a paragraph is one only if it begins at one, so each is read with its number written as one (as many digits). A
/// list begun by items moved in under another item is numbered from one.
fn numbered(text: &str, rewrite: &Rewrite, chosen: (usize, usize)) -> Edit {
    numbered_carrying(text, rewrite, chosen, |_| None)
}

/// As `numbered`, `carried` saying where an item whose mark `rewrite` writes over is now (the byte of the text it
/// leaves), where it is written anew: the same item still (a line moved past the lines moved).
fn numbered_carrying(text: &str, rewrite: &Rewrite, chosen: (usize, usize), carried: impl Fn(usize) -> Option<usize>) -> Edit {
    let once = rewrite.apply(text);
    let before = Tree::of(text);
    let touches = |a: &Range<usize>, b: &Range<usize>| a.start <= b.end && b.start <= a.end;
    let removed = |byte: usize| rewrite.edits.iter().any(|(range, _)| range.contains(&byte));
    let touched: HashSet<usize> = (0..before.lists.len()).filter(|&list| rewrite.edits.iter().any(|(range, _)| touches(range, &before.lists[list].range))).collect();
    // Each item of before where it now is: the list it was in, where that list began, and where the item it was in now
    // is.
    let now = |marker: usize| if removed(marker) { carried(marker) } else { Some(rewrite.map(marker)) };
    let mut was: HashMap<usize, (usize, u64, Option<usize>)> = HashMap::new();
    let mut read = once.clone();
    for (index, list) in before.lists.iter().enumerate() {
        let began = match before.items[list.items[0]].mark {
            Mark::Number(n, _) => n,
            Mark::Bullet(_) => continue,
        };
        for &item in &list.items {
            let item = &before.items[item];
            let Some(at) = now(item.marker) else { continue };
            was.insert(at, (index, began, item.parent.and_then(|parent| now(before.items[parent].marker))));
            if touched.contains(&index) {
                let digits = item.mark_end - 1 - item.marker;
                read.replace_range(at..at + digits, &format!("{}1", "0".repeat(digits - 1)));
            }
        }
    }
    let after = Tree::of(&read);
    let written = |item: &Item| once[item.marker..item.mark_end - 1].parse::<u64>().expect("digits");
    let made = rewrite.made();
    let mut numbers = Rewrite::default();
    let mut moved: BTreeMap<(usize, usize), isize> = BTreeMap::new();
    for list in &after.lists {
        let first = &after.items[list.items[0]];
        if matches!(first.mark, Mark::Bullet(_)) {
            continue;
        }
        let reached = made.iter().any(|range| touches(range, &list.range)) || list.items.iter().any(|item| was.get(&after.items[*item].marker).is_some_and(|(list, ..)| touched.contains(list)));
        if !reached {
            continue;
        }
        let parent = first.parent.map(|parent| after.items[parent].marker);
        let mut number = match was.get(&first.marker) {
            // (Moved in under another item: a list begun.)
            Some((.., under)) if parent.is_some() && parent != *under => 1,
            Some((_, began, _)) => *began,
            None => written(first),
        };
        for &index in &list.items {
            let item = &after.items[index];
            if matches!(item.mark, Mark::Bullet(_)) {
                break;
            }
            let written = written(item);
            if number > LARGEST {
                break;
            }
            if written != number {
                let (old, new) = (item.mark_end - 1 - item.marker, number.to_string());
                let by = new.len() as isize - old as isize;
                numbers.put(item.marker..item.mark_end - 1, new);
                if by != 0 {
                    for line in after.lines_of(index).skip(1) {
                        *moved.entry((line, item.quotes)).or_default() += by;
                    }
                }
            }
            number += 1;
        }
    }
    for ((line, quotes), by) in moved {
        numbers.shift(&once, &after, line..=line, quotes, by);
    }
    let twice = numbers.apply(&once);
    edit(text, &twice, (numbers.map(chosen.0), numbers.map(chosen.1)))
}

/// What is before the byte `at` on its line, as a line under it begins: block quotes' marks as written, the marks of
/// items (on the same line) as spaces.
fn opening(text: &str, at: usize) -> String {
    let start = text[..at].rfind('\n').map_or(0, |line| line + 1);
    text[start..at].chars().map(|c| if c == '>' || c.is_whitespace() { c } else { ' ' }).collect()
}

/// Enter pressed with the caret at `caret` (nothing chosen): in a list's item, past its mark, the next line begun as
/// another item of the list (a number one more, a to-do not done, the items after it numbered on), what was under the
/// item moved under the new one, in one with no words, the item ended (see `ended`). None elsewhere: a line break.
pub fn enter(text: &str, caret: usize) -> Option<Edit> {
    let tree = Tree::of(text);
    let at = byte(text, caret);
    let index = tree.line_of(at);
    let found = tree.on_line(index).last()?;
    let item = &tree.items[found];
    if at < item.words {
        return None;
    }
    if text[item.words..item.end].trim().is_empty() {
        return Some(ended(text, &tree, found));
    }
    let next = match item.mark {
        Mark::Bullet(c) => c.to_string(),
        Mark::Number(n, _) if n >= LARGEST => return None,
        Mark::Number(n, after) => format!("{}{after}", n + 1),
    };
    let opened = opening(text, item.marker);
    let spaces = " ".repeat((column(text, item.column) - column(text, item.mark_end)).max(1));
    let mut begun = format!("\n{opened}{next}{spaces}");
    if item.boxed.is_some() {
        begun.push_str("[ ] ");
    }
    let mut rewrite = Rewrite::default();
    rewrite.put(at..at, begun.as_str());
    // (What was under the item, now the new one's, as far in as the new one's words.)
    let by = (opened.chars().count() + next.len() + spaces.len()) as isize - column(text, item.column) as isize;
    if by != 0 {
        let lines = tree.lines_of(found);
        rewrite.shift(text, &tree, index + 1..=*lines.end(), item.quotes, by);
    }
    let caret = rewrite.map(at);
    Some(numbered(text, &rewrite, (caret, caret)))
}

/// The item `found` (one with no words) ended: one in another item, as its mark begins its line, moved out a step, the
/// last of its list, its line left blank and the caret on a line after (words written there stand apart from the
/// list), any other, its mark taken off (the items after it in its list numbered on).
fn ended(text: &str, tree: &Tree, found: usize) -> Edit {
    let item = &tree.items[found];
    if item.parent.is_some() && item.first {
        let rewrite = outdented(text, tree, found);
        let caret = rewrite.map(item.words);
        return numbered(text, &rewrite, (caret, caret));
    }
    let line = tree.lines[tree.line_of(item.marker)].clone();
    let start = if item.first { line.start + quoted(&text[line.clone()], item.quotes) } else { item.marker };
    let list = &tree.lists[item.list].items;
    let mut rewrite = Rewrite::default();
    if list.len() > 1 && list.last() == Some(&found) {
        let opened = opening(text, start);
        rewrite.put(start..line.end, format!("\n{opened}"));
    } else {
        rewrite.put(start..line.end, "");
    }
    let caret = rewrite.map(line.end);
    numbered(text, &rewrite, (caret, caret))
}

/// Item `found` (in another, its mark beginning its line) moved out a step, with what is under it: as far in as the
/// item it was in.
fn outdented(text: &str, tree: &Tree, found: usize) -> Rewrite {
    let item = &tree.items[found];
    let parent = &tree.items[item.parent.expect("an item in another")];
    let mut rewrite = Rewrite::default();
    rewrite.shift(text, tree, tree.lines_of(found), item.quotes, column(text, parent.marker) as isize - column(text, item.marker) as isize);
    rewrite
}

/// Backspace pressed with the caret at `caret` (nothing chosen): at the start of a list's item's words, the item one
/// step further out if it is in one, else its mark (and its to-do's box) taken off: its words a paragraph of the item
/// before it (as far in as that one's words), or, the first of its list, a paragraph before the list. Its Markdown shown
/// as it reads (`formatted`: see `markdown::Shown`), where marks put away would be taken back one by one, as written:
/// at the start of a heading's words, its `#` taken off, of a quote's line's, its `>`, of a block of code's first
/// line, its fences; at the start of the line after a rule, the rule taken away, after a block of code, the caret to the
/// end of its last line. None elsewhere.
pub fn backspace(text: &str, caret: usize, formatted: bool) -> Option<Edit> {
    let tree = Tree::of(text);
    let at = byte(text, caret);
    let index = tree.line_of(at);
    let Some(found) = tree.on_line(index).rev().find(|&item| tree.items[item].words == at) else {
        return formatted.then(|| unformatted(text, &tree, at, index)).flatten();
    };
    let item = &tree.items[found];
    if item.parent.is_some() && item.first {
        let rewrite = outdented(text, &tree, found);
        let caret = rewrite.map(at);
        return Some(numbered(text, &rewrite, (caret, caret)));
    }
    let line = tree.lines[tree.line_of(item.marker)].clone();
    let start = if item.first { line.start + quoted(&text[line.clone()], item.quotes) } else { item.marker };
    let list = &tree.lists[item.list].items;
    let place = list.iter().position(|&other| other == found).expect("an item in its list");
    let mut rewrite = Rewrite::default();
    if place > 0 {
        // (Shown as written, a paragraph of the item before, as far in as its words. Shown as it reads, an item of no
        // other at the line's start, as a line of words is written: else it stood in, past nothing to be seen.)
        let before = &tree.items[list[place - 1]];
        let wanted = if formatted && item.parent.is_none() { 0 } else { column(text, before.column).saturating_sub(column(text, start)) };
        rewrite.put(start..item.words, " ".repeat(wanted));
    } else {
        rewrite.put(start..item.words, "");
        // (The list goes on from the number it began with, under a paragraph a list only if that is one: else a blank
        // line between.)
        if let Some(next) = list.get(1).map(|&next| &tree.items[next]) {
            if matches!(item.mark, Mark::Number(n, _) if n != 1) {
                let below = tree.lines[tree.line_of(next.marker)].start;
                rewrite.put(below..below, format!("{}\n", opening(text, below + quoted(&text[below..next.marker], next.quotes)).trim_end()));
            }
        }
    }
    let caret = rewrite.map(item.words);
    Some(numbered(text, &rewrite, (caret, caret)))
}

/// What Backspace does at the byte `at` (on line `index`) of a note shown as it reads, not at a list's item's words (see
/// `backspace`).
fn unformatted(text: &str, tree: &Tree, at: usize, index: usize) -> Option<Edit> {
    let shown = super::markdown::Shown::of(text);
    let line = tree.lines[index].clone();
    let units = |bytes: &Range<usize>| unit(text, bytes.start)..unit(text, bytes.end);
    if at == line.start && index > 0 {
        let before = tree.lines[index - 1].clone();
        let put_away = units(&before);
        // After a rule: the rule taken away, its line with it.
        if shown.rules.contains(&put_away) {
            let mut rewrite = Rewrite::default();
            rewrite.put(before.start..line.start, "");
            return Some(edit(text, &rewrite.apply(text), (before.start, before.start)));
        }
        // After a block of code: the caret at the end of its last line (its fence between, put away).
        if shown.codes.iter().any(|code| code.fences.len() == 2 && code.fences[1] == put_away) {
            let to = unit(text, tree.lines[index - 2].end);
            return Some(Edit { range: unit(text, at)..unit(text, at), with: String::new(), chosen: (to, to) });
        }
        // Its first line: its fences taken away (each with its line break), its lines words.
        if let Some(code) = shown.codes.iter().find(|code| code.fences.first() == Some(&put_away)) {
            let mut rewrite = Rewrite::default();
            rewrite.put(before.start..line.start, "");
            if let Some(closing) = code.fences.get(1) {
                let closing = byte(text, closing.start)..byte(text, closing.end);
                rewrite.put(closing.start - 1..closing.end, "");
            }
            let caret = rewrite.map(at);
            return Some(edit(text, &rewrite.apply(text), (caret, caret)));
        }
    }
    if tree.in_code(index) {
        return None;
    }
    let words = tree.words_of(text, index);
    // At the start of the words of a line under a list's item (not its mark's line), as far in as the item's words: the
    // line out of it, as far in as the item it is in (in none, at the line's start).
    if let Some(holding) = tree.item_holding(index).filter(|&item| tree.line_of(tree.items[item].marker) != index) {
        let item = &tree.items[holding];
        let from = line.start + quoted(&text[line.clone()], item.quotes);
        if at == words && words > from && words < line.end {
            let wanted = item.parent.map_or(0, |parent| column(text, tree.items[parent].column).saturating_sub(column(text, from)));
            let mut rewrite = Rewrite::default();
            rewrite.put(from..words, " ".repeat(wanted));
            let caret = rewrite.map(at);
            return Some(numbered(text, &rewrite, (caret, caret)));
        }
    }
    // At the start of a heading's words: its `#` taken off.
    let marks = hashes(&text[words..line.end]);
    if marks > 0 && at == words + marks {
        return Some(heading(text, (unit(text, at), unit(text, at)), 0));
    }
    // At the start of a quote's line's words: its `>` taken off.
    let quotes = tree.quotes.iter().filter(|quote| quote.start <= line.end && line.start < quote.end).count();
    let marks = quoted(&text[line.clone()], quotes);
    if marks > 0 && at == line.start + marks && shown.leads.iter().any(|lead| lead.end == unit(text, at)) {
        let mut rewrite = Rewrite::default();
        rewrite.put(line.start..at, "");
        return Some(edit(text, &rewrite.apply(text), (line.start, line.start)));
    }
    None
}

/// Delete pressed with the caret at `caret` (nothing chosen), the note shown as it reads (`formatted`): at the end of the
/// line before a rule, the rule taken away, its line with it, before a block of code's fence (put away), nothing done
/// (the fence would be taken into the line, the block undone). None elsewhere: the key's as always.
pub fn delete(text: &str, caret: usize, formatted: bool) -> Option<Edit> {
    if !formatted {
        return None;
    }
    let tree = Tree::of(text);
    let at = byte(text, caret);
    let index = tree.line_of(at);
    let line = tree.lines[index].clone();
    let after = tree.lines.get(index + 1).filter(|_| at == line.end)?.clone();
    let shown = super::markdown::Shown::of(text);
    let put_away = unit(text, after.start)..unit(text, after.end);
    if shown.rules.contains(&put_away) {
        let mut rewrite = Rewrite::default();
        rewrite.put(line.end..after.end, "");
        return Some(edit(text, &rewrite.apply(text), (at, at)));
    }
    let fence = shown.fences().any(|fence| *fence == put_away);
    fence.then(|| Edit { range: caret..caret, with: String::new(), chosen: (caret, caret) })
}

/// The first and last lines of what is chosen (from, to, bytes): a choice ending at a line's start leaves that line.
fn chosen_lines(tree: &Tree, from: usize, to: usize) -> (usize, usize) {
    let first = tree.line_of(from);
    let mut last = tree.line_of(to);
    if last > first && to == tree.lines[last].start {
        last -= 1;
    }
    (first, last)
}

/// Tab (or with Shift, out) pressed with `chosen` (from, to): the items of a list chosen (from the one whose mark begins
/// the first line chosen) moved a step in, under the item before them (as far in as its words), or out, as far in as
/// the item they are in, with what is under them, the lists numbered on. None where the first line chosen begins no
/// list's item: the key does what it always does. Where they cannot move (no item before them, none they are in),
/// nothing changes: still the key's.
pub fn indent(text: &str, chosen: (usize, usize), out: bool) -> Option<Option<Edit>> {
    let tree = Tree::of(text);
    let (from, to) = (byte(text, chosen.0.min(chosen.1)), byte(text, chosen.0.max(chosen.1)));
    let (first, last) = chosen_lines(&tree, from, to);
    let found = tree.on_line(first).find(|&item| tree.items[item].first)?;
    let item = &tree.items[found];
    let list = &tree.lists[item.list].items;
    let place = list.iter().position(|&other| other == found).expect("an item in its list");
    let moving = list[place..].iter().take_while(|&&other| tree.line_of(tree.items[other].marker) <= last).last().expect("the item itself");
    let to_column = if out {
        let Some(parent) = item.parent else { return Some(None) };
        column(text, tree.items[parent].marker)
    } else {
        let Some(&before) = place.checked_sub(1).map(|before| &list[before]) else { return Some(None) };
        column(text, tree.items[before].column)
    };
    let mut rewrite = Rewrite::default();
    rewrite.shift(text, &tree, tree.line_of(item.marker)..=*tree.lines_of(*moving).end(), item.quotes, to_column as isize - column(text, item.marker) as isize);
    let chosen = (rewrite.map(byte(text, chosen.0)), rewrite.map(byte(text, chosen.1)));
    Some(Some(numbered(text, &rewrite, chosen)))
}

/// Ctrl+Enter with the caret at `caret`: its line's to-do ticked or no longer, a list's item without a box given one,
/// any other line made a to-do. None in a block of code.
pub fn toggle_task(text: &str, caret: usize) -> Option<Edit> {
    let tree = Tree::of(text);
    let at = byte(text, caret);
    let index = tree.line_of(at);
    let mut rewrite = Rewrite::default();
    match tree.on_line(index).last().map(|item| &tree.items[item]) {
        Some(Item { boxed: Some((at, done)), .. }) => rewrite.put(at + 1..at + 2, if *done { " " } else { "x" }),
        Some(item) if item.column == item.mark_end => rewrite.put(item.column..item.column, " [ ] "),
        Some(item) => rewrite.put(item.column..item.column, "[ ] "),
        None if tree.in_code(index) => return None,
        None => {
            let words = tree.words_of(text, index);
            rewrite.put(words..words, "- [ ] ");
        }
    }
    let once = rewrite.apply(text);
    let caret = rewrite.map(at);
    Some(edit(text, &once, (caret, caret)))
}

/// Alt+Up or Alt+Down with `chosen`: the lines chosen moved past the line above them (or below), what was chosen moving
/// with them, the numbered lists they touch numbered on. None at the first line (or the last).
pub fn move_lines(text: &str, chosen: (usize, usize), up: bool) -> Option<Edit> {
    let tree = Tree::of(text);
    let (from, to) = (byte(text, chosen.0.min(chosen.1)), byte(text, chosen.0.max(chosen.1)));
    let (first, last) = chosen_lines(&tree, from, to);
    let passed = match up {
        true if first > 0 => first - 1,
        false if last + 1 < tree.lines.len() => last + 1,
        _ => return None,
    };
    // (The line passed taken from one side of them and put on the other: the lines moved are the same lines still.)
    let other = &text[tree.lines[passed].clone()];
    let mut rewrite = Rewrite::default();
    let by = if up {
        rewrite.put(tree.lines[passed].start..tree.lines[first].start, "");
        rewrite.put(tree.lines[last].end..tree.lines[last].end, format!("\n{other}"));
        -(other.len() as isize + 1)
    } else {
        rewrite.put(tree.lines[last].end..tree.lines[passed].end, "");
        rewrite.put(tree.lines[first].start..tree.lines[first].start, format!("{other}\n"));
        other.len() as isize + 1
    };
    // (What is chosen moves with the lines, a choice ending at the start of the line after them to their end. The
    // line passed is written anew on their other side: its items the same items still.)
    let shift = |at: usize| (byte(text, at).min(tree.lines[last].end) as isize + by) as usize;
    let passed_at = tree.lines[passed].start;
    let written_at = if up { tree.lines[last].end - (tree.lines[first].start - passed_at) + 1 } else { tree.lines[first].start };
    let carried = |marker: usize| tree.lines[passed].contains(&marker).then(|| written_at + marker - passed_at);
    Some(numbered_carrying(text, &rewrite, (shift(chosen.0), shift(chosen.1)), carried))
}

/// A heading of `level` (1 to 3, 0: none, a line of words) made of the lines chosen: their `#` marks written anew (a
/// list's item's after its mark), a heading underlined made one of `#` marks, its line under it taken away. Lines in
/// a block of code are left.
pub fn heading(text: &str, chosen: (usize, usize), level: u8) -> Edit {
    let tree = Tree::of(text);
    let (from, to) = (byte(text, chosen.0.min(chosen.1)), byte(text, chosen.0.max(chosen.1)));
    let (first, last) = (tree.line_of(from), tree.line_of(to));
    let marks = if level == 0 { String::new() } else { format!("{} ", "#".repeat(level as usize)) };
    let mut rewrite = Rewrite::default();
    let mut index = first;
    while index <= last {
        if let Some(underlined) = tree.underlined.iter().find(|heading| heading.start <= tree.lines[index].end && tree.lines[index].start < heading.end) {
            // (Its words on one line, the line under it taken away.)
            let (top, under) = (tree.line_of(underlined.start), tree.line_of(underlined.end - 1));
            let words = tree.words_of(text, top);
            rewrite.put(words..words, marks.as_str());
            for line in top + 1..under {
                rewrite.put(tree.lines[line - 1].end..tree.words_of(text, line), " ");
            }
            rewrite.put(tree.lines[under - 1].end..tree.lines[under].end, "");
            index = under + 1;
            continue;
        }
        if !tree.in_code(index) {
            let words = tree.words_of(text, index);
            rewrite.put(words..words + hashes(&text[words..tree.lines[index].end]), marks.as_str());
        }
        index += 1;
    }
    let once = rewrite.apply(text);
    edit(text, &once, (rewrite.map(byte(text, chosen.0)), rewrite.map(byte(text, chosen.1))))
}

/// How many bytes of the words `rest` (a line's, from where they begin) a heading's `#` marks and the spaces after them
/// take: none where they are no heading's.
fn hashes(rest: &str) -> usize {
    let hashes = rest.bytes().take_while(|&c| c == b'#').count();
    if (1..=6).contains(&hashes) && (rest.len() == hashes || rest[hashes..].starts_with([' ', '\t'])) {
        rest.len() - rest[hashes..].trim_start_matches([' ', '\t']).len()
    } else {
        0
    }
}

/// How a stretch is marked: bold (`**`), slanted (`*`), struck through (`~~`), code (`` ` ``).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Marked {
    Bold,
    Italic,
    Struck,
    Code,
}

/// The marks `marked` puts before and after `words`: code's more backticks than any run of them in its words, a space
/// within where its words begin or end with one.
fn marks(marked: Marked, words: &str) -> (String, String) {
    match marked {
        Marked::Bold => ("**".into(), "**".into()),
        Marked::Italic => ("*".into(), "*".into()),
        Marked::Struck => ("~~".into(), "~~".into()),
        Marked::Code => {
            let longest = words.split(|c| c != '`').map(str::len).max().unwrap_or(0);
            let ticks = "`".repeat(longest + 1);
            let pad = if words.starts_with('`') || words.ends_with('`') { " " } else { "" };
            (format!("{ticks}{pad}"), format!("{pad}{ticks}"))
        }
    }
}

/// `marked` put on what is chosen, or taken off: what is chosen within a stretch marked so (its words, or all of it), that
/// stretch's marks taken off, else each line's words chosen (past a list's item's mark, a heading's or a quote's,
/// less the spaces at their ends, not in a block of code) marked (see `marks`), what was chosen chosen still. Nothing
/// chosen: a pair of marks with the caret between them.
pub fn toggle(text: &str, chosen: (usize, usize), marked: Marked) -> Edit {
    let tree = Tree::of(text);
    let (start, end) = (byte(text, chosen.0.min(chosen.1)), byte(text, chosen.0.max(chosen.1)));
    let mut rewrite = Rewrite::default();
    if let Some(stretch) = marked_stretches(text, marked).into_iter().find(|stretch| stretch.whole.start <= start && end <= stretch.whole.end && (stretch.inner.start <= start && end <= stretch.inner.end || (start, end) == (stretch.whole.start, stretch.whole.end))) {
        rewrite.put(stretch.whole.start..stretch.inner.start, "");
        rewrite.put(stretch.inner.end..stretch.whole.end, "");
        let once = rewrite.apply(text);
        let kept = |at: usize| rewrite.map(at.clamp(stretch.inner.start, stretch.inner.end));
        return edit(text, &once, (kept(start), kept(end)));
    }
    if start == end {
        let (open, close) = marks(marked, "");
        rewrite.put(start..start, format!("{open}{close}"));
        let once = rewrite.apply(text);
        return edit(text, &once, (start + open.len(), start + open.len()));
    }
    let mut placed: Vec<(usize, usize, usize)> = Vec::new();
    for index in tree.line_of(start)..=tree.line_of(end) {
        let line = tree.lines[index].clone();
        let words = tree.words_of(text, index);
        let (from, to) = (start.max(words + hashes(&text[words..line.end])), end.min(line.end));
        if tree.in_code(index) || from >= to || text[from..to].trim().is_empty() {
            continue;
        }
        let words = text[from..to].trim();
        let from = from + text[from..to].find(words).expect("its own words");
        let to = from + words.len();
        let (open, close) = marks(marked, words);
        rewrite.put(from..from, open);
        placed.push((from, to, close.len()));
        rewrite.put(to..to, close);
    }
    let once = rewrite.apply(text);
    let chosen = match (placed.first(), placed.last()) {
        // (Within the marks: past those before the first words, before those after the last.)
        (Some(&(first, _, _)), Some(&(_, last, close))) => (rewrite.map(first), rewrite.map(last) - close),
        _ => (start, end),
    };
    edit(text, &once, chosen)
}

/// A stretch marked so: all of it and its words within its marks (bytes).
struct Stretch {
    whole: Range<usize>,
    inner: Range<usize>,
}

/// The stretches of `text` marked as `marked` is (bold, slanted, struck through, code), as the note is read.
fn marked_stretches(text: &str, marked: Marked) -> Vec<Stretch> {
    let mut found = Vec::new();
    for (event, range) in Parser::new_ext(text, READ).into_offset_iter() {
        let written = &text[range.clone()];
        let ticks = |c: char| written.len() - written.trim_start_matches(c).len();
        let marks = match (&event, marked) {
            (Event::Start(Tag::Strong), Marked::Bold) => 2,
            (Event::Start(Tag::Emphasis), Marked::Italic) => 1,
            (Event::Start(Tag::Strikethrough), Marked::Struck) => ticks('~'),
            (Event::Code(_), Marked::Code) => ticks('`'),
            _ => continue,
        };
        if written.len() < 2 * marks {
            continue;
        }
        let mut inner = range.start + marks..range.end - marks;
        // (Code's words within a space at each end, not all spaces: the spaces are the marks'.)
        let words = &text[inner.clone()];
        if matches!(event, Event::Code(_)) && words.len() >= 2 && words.starts_with(' ') && words.ends_with(' ') && !words.trim().is_empty() {
            inner = inner.start + 1..inner.end - 1;
        }
        found.push(Stretch { whole: range, inner });
    }
    found
}

/// A link made of what is chosen (`[chosen](to)`, the caret after it, or between its parentheses where `to` is none),
/// nothing chosen, an empty one with the caret in its square brackets. None where what is chosen runs over more than
/// one line. Square brackets in its words that pair with none are escaped, where it goes is written in angle brackets
/// if it has spaces, angle brackets, backslashes or parentheses that do not pair.
pub fn link(text: &str, chosen: (usize, usize), to: Option<&str>) -> Option<Edit> {
    let (start, end) = (byte(text, chosen.0.min(chosen.1)), byte(text, chosen.0.max(chosen.1)));
    let shown = &text[start..end];
    if shown.contains('\n') {
        return None;
    }
    let words = unpaired_escaped(shown);
    let goes = to.map(destination).unwrap_or_default();
    let with = format!("[{words}]({goes})");
    let caret = start
        + match (shown.is_empty(), to) {
            (true, _) => 1,
            (false, None) => words.len() + 3,
            (false, Some(_)) => with.len(),
        };
    let mut rewrite = Rewrite::default();
    rewrite.put(start..end, with);
    Some(edit(text, &rewrite.apply(text), (caret, caret)))
}

/// `words` with the square brackets in it that pair with none escaped (`\[`), those already escaped left, a backslash
/// at its end that escapes nothing escaped (it would escape the bracket after it).
fn unpaired_escaped(words: &str) -> String {
    let chars: Vec<char> = words.chars().collect();
    let mut open = Vec::new();
    let mut unpaired = HashSet::new();
    let mut at = 0;
    while at < chars.len() {
        match chars[at] {
            '\\' => at += 1,
            '[' => open.push(at),
            ']' => {
                if open.pop().is_none() {
                    unpaired.insert(at);
                }
            }
            _ => {}
        }
        at += 1;
    }
    unpaired.extend(open);
    let mut out = chars.iter().enumerate().fold(String::new(), |mut out, (at, &c)| {
        if unpaired.contains(&at) {
            out.push('\\');
        }
        out.push(c);
        out
    });
    if (words.len() - words.trim_end_matches('\\').len()) % 2 == 1 {
        out.push('\\');
    }
    out
}

/// `words` with every square bracket in it escaped (`\[`, `\]`: those already escaped left), a backslash at its end that
/// escapes nothing escaped: as a link's words written from what is to show, read back as they were written, brackets and
/// all (a pair left as written could read as a link of its own inside it, which a link's words cannot hold).
fn brackets_escaped(words: &str) -> String {
    let mut out = String::with_capacity(words.len());
    let mut chars = words.chars();
    while let Some(c) = chars.next() {
        match c {
            '\\' => {
                out.push(c);
                match chars.next() {
                    Some(next) => out.push(next),
                    None => out.push('\\'),
                }
            }
            '[' | ']' => {
                out.push('\\');
                out.push(c);
            }
            _ => out.push(c),
        }
    }
    out
}

/// A link's words as written (`written`) as they are to show where they are written anew (see `link_written`): their
/// escaped square brackets brackets, a backslash escaped at their end one, the rest as written.
pub fn brackets_unescaped(written: &str) -> String {
    let chars: Vec<char> = written.chars().collect();
    let mut out = String::with_capacity(written.len());
    let mut at = 0;
    while at < chars.len() {
        match (chars[at], chars.get(at + 1)) {
            ('\\', Some(&next @ ('[' | ']'))) => out.push(next),
            ('\\', Some('\\')) if at + 2 == chars.len() => out.push('\\'),
            ('\\', Some(&next)) => {
                out.push('\\');
                out.push(next);
            }
            (c, _) => {
                out.push(c);
                at += 1;
                continue;
            }
        }
        at += 2;
    }
    out
}

/// A link written of `words` (to show, as `brackets_escaped` writes them) going to `to`: `[words](to)`, read back as
/// a link of those words going there (see `destination`).
pub fn link_written(words: &str, to: &str) -> String {
    format!("[{}]({})", brackets_escaped(words), destination(to))
}

/// A link found where something is chosen (see `link_at`): all of it and its words (UTF-16 units), its words as they
/// are to show where it is written anew (see `link_written`), where it goes.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Found {
    pub span: Range<usize>,
    pub words: Range<usize>,
    pub shown: String,
    pub to: String,
}

/// The link `chosen` (from, to: UTF-16 units) is on, as the note is read: the caret in its words or at their end, or
/// what is chosen within it and some of its words. Not a table's (its words are written in its cell's own box, read
/// there as words alone). None elsewhere.
pub fn link_at(text: &str, chosen: (usize, usize)) -> Option<Found> {
    let shown = super::markdown::Shown::of(text);
    let (from, to) = (chosen.0.min(chosen.1), chosen.0.max(chosen.1));
    let units: Vec<u16> = text.encode_utf16().collect();
    // (A link in square brackets has its words from the one that opens it: as read, they begin past a backslash that
    // escapes their first character, which is theirs.)
    let words_of = |link: &super::markdown::Linked| match units.get(link.span.start) == Some(&u16::from(b'[')) {
        true => link.span.start + 1..link.words.end,
        false => link.words.clone(),
    };
    shown
        .links
        .iter()
        .filter(|link| !shown.tables.iter().any(|table| table.span.start == link.span.start))
        .map(|link| (link, words_of(link)))
        .find(|(link, words)| if from == to { words.start <= from && from <= words.end } else { link.span.start <= from && to <= link.span.end && from < words.end && to > words.start })
        .map(|(link, words)| Found { span: link.span.clone(), shown: brackets_unescaped(&String::from_utf16_lossy(&units[words.clone()])), words, to: link.to.clone() })
}

/// A link of `words` going to `to` (see `link_written`) put in place of `chosen` (from, to: UTF-16 units), the caret
/// after it.
pub fn put_link(text: &str, chosen: (usize, usize), words: &str, to: &str) -> Edit {
    let (start, end) = (byte(text, chosen.0.min(chosen.1)), byte(text, chosen.0.max(chosen.1)));
    let with = link_written(words, to);
    let caret = start + with.len();
    let mut rewrite = Rewrite::default();
    rewrite.put(start..end, with);
    edit(text, &rewrite.apply(text), (caret, caret))
}

/// The link `found` taken off its words: its marks and where it goes taken away, its words left as written, chosen.
/// None where those words would read as a link still (a web address written out).
pub fn unlink(text: &str, found: &Found) -> Option<Edit> {
    let units: Vec<u16> = text.encode_utf16().collect();
    let words = String::from_utf16_lossy(&units[found.words.clone()]);
    let (start, end) = (byte(text, found.span.start), byte(text, found.span.end));
    let mut rewrite = Rewrite::default();
    rewrite.put(start..end, words.as_str());
    let after = rewrite.apply(text);
    let length = words.encode_utf16().count();
    let still = super::markdown::Shown::of(&after).links.iter().any(|link| link.words.start < found.span.start + length && found.span.start < link.words.end);
    (!still).then(|| edit(text, &after, (start, start + words.len())))
}

/// Where a link goes, as written in its parentheses.
fn destination(to: &str) -> String {
    let mut depth = 0i32;
    let paired = to.chars().all(|c| {
        depth += match c {
            '(' => 1,
            ')' => -1,
            _ => 0,
        };
        depth >= 0
    }) && depth == 0;
    if paired && !to.is_empty() && !to.contains(|c: char| c.is_whitespace() || c.is_control() || matches!(c, '<' | '>' | '\\')) {
        return to.to_string();
    }
    let mut out = String::from("<");
    for c in to.chars() {
        if matches!(c, '<' | '>' | '\\') {
            out.push('\\');
        }
        out.push(c);
    }
    out.push('>');
    out
}

/// What the menu inserts that stands as a block of its own: a to-do, a table (two columns, a row under its head), a
/// rule, a block of code.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Block {
    Todo,
    Table,
    Rule,
    Code,
}

/// `block` inserted where the caret is (`chosen`'s end), on lines of its own (see `placed`). A to-do in a list's item
/// is another item after it (and what is under it), as far in and marked as it is. The caret where its words are to
/// be written: the to-do's, the table's first head, the code's line (after the rule, the line after it).
pub fn insert(text: &str, chosen: (usize, usize), block: Block) -> Edit {
    let at = byte(text, chosen.0.max(chosen.1));
    if block == Block::Todo {
        if let Some(made) = todo_after_item(text, at) {
            return made;
        }
    }
    let (written, into) = match block {
        Block::Todo => ("- [ ] ", 6),
        Block::Table => ("|  |  |\n| --- | --- |\n|  |  |", 2),
        Block::Rule => ("---\n", 4),
        Block::Code => ("```\n\n```", 4),
    };
    let (after, caret) = placed(text, at, written, into, block == Block::Todo, block == Block::Table, false);
    edit(text, &after, (caret, caret))
}

/// A to-do put after the list's item the byte `at` is in (after what is under it too), as far in as it and marked as
/// it is (numbered one more, those after it numbered on). None where `at` is in no item.
fn todo_after_item(text: &str, at: usize) -> Option<Edit> {
    let tree = Tree::of(text);
    let found = tree.item_holding(tree.line_of(at))?;
    let item = &tree.items[found];
    let mark = match item.mark {
        Mark::Bullet(c) => c.to_string(),
        Mark::Number(n, after) => format!("{}{after}", (n + 1).min(LARGEST)),
    };
    let end = tree.lines[*tree.lines_of(found).end()].end;
    let begun = format!("\n{}{mark} [ ] ", opening(text, item.marker));
    let mut rewrite = Rewrite::default();
    rewrite.put(end..end, begun.as_str());
    let caret = end + begun.len();
    Some(numbered(text, &rewrite, (caret, caret)))
}

/// How a line under line `index` of `text` begins, in what holds that line: block quotes' marks as written, the spaces
/// in from the mark of the item it is in.
fn continued(text: &str, tree: &Tree, index: usize) -> String {
    let line = tree.lines[index].clone();
    match tree.item_holding(index) {
        Some(item) => opening(text, tree.items[item].column),
        None => {
            let quotes = tree.quotes.iter().filter(|quote| quote.start <= line.end && line.start < quote.end).count();
            opening(text, line.start + quoted(&text[line.clone()], quotes))
        }
    }
}

/// `block` (its lines, `\n` between them) put on lines of its own where the byte `at` of `text` is: on its line where
/// nothing is written on it past its marks (in place of nothing, its first line after an item's mark), else after it
/// (after all of what it is in where that cannot be split: a block of code, a table, a heading underlined, code whose
/// fence is not closed closed first). Where `split`, words after `at` on its line are put on a line of their own
/// first, the block between them and those before. Each of its lines begun as what holds that line begins its lines
/// (block quotes' marks, the spaces in from an item's mark), a blank line between it and words before and after it
/// (none between a `todo` and an item next to it), after a `table` two line breaks at least (what is written right
/// under it would be its row). The text made, and the byte in it `into` (a byte of `block`) is.
fn placed(text: &str, at: usize, block: &str, into: usize, todo: bool, table: bool, split: bool) -> (String, usize) {
    let tree = Tree::of(text);
    let caret_line = tree.lines[tree.line_of(at)].clone();
    let holds = |range: &&Range<usize>| range.start <= caret_line.end && caret_line.start < range.end;
    let code = tree.code.iter().find(holds);
    let whole = code.or_else(|| tree.tables.iter().chain(&tree.underlined).find(holds));
    if split && whole.is_none() && !text[at..caret_line.end].trim().is_empty() {
        let mut rewrite = Rewrite::default();
        rewrite.put(at..at, format!("\n{}", continued(text, &tree, tree.line_of(at))));
        return placed(&rewrite.apply(text), at, block, into, todo, table, false);
    }
    let index = whole.map_or(tree.line_of(at), |range| tree.line_of(range.end - 1));
    let line = tree.lines[index].clone();
    let has_words = |other: usize| tree.on_line(other).next().is_some() || tree.in_code(other) || tree.words_of(text, other) < tree.lines[other].end;
    // (An item with nothing in it: the block its first.)
    let marks_only = whole.is_none() && tree.on_line(index).last().is_some_and(|item| tree.items[item].words == line.end);
    let blank = marks_only || whole.is_none() && !has_words(index);
    let first_prefix = if blank { text[line.start..tree.words_of(text, index)].to_string() } else { continued(text, &tree, index) };
    let prefix = if marks_only || !blank { continued(text, &tree, index) } else { first_prefix.clone() };
    let gap = prefix.trim_end().to_string();
    let item_line = |other: usize| other < tree.lines.len() && tree.on_line(other).next().is_some();
    let before = if marks_only { false } else if blank { index > 0 && has_words(index - 1) } else { true };
    let after = index + 1 < tree.lines.len() && has_words(index + 1);
    let gap_before = before && !(todo && item_line(if blank { index.wrapping_sub(1) } else { index }));
    // (A block ending in a line of its own has its blank line.)
    let gap_after = after && !(todo && item_line(index + 1)) && !block.ends_with('\n');
    // (Code not closed: its fence, as it opened, before what is put after it.)
    let closing = code.filter(|code| tree.fenced.contains(code)).and_then(|code| {
        let fence = text[code.start..].chars().next().filter(|c| matches!(c, '`' | '~'))?;
        let length = text[code.start..].chars().take_while(|c| *c == fence).count();
        let (first, last) = (tree.line_of(code.start), tree.line_of(code.end - 1));
        let closer = text[tree.words_of(text, last)..tree.lines[last].end].trim_end();
        let closed = last > first && closer.chars().all(|c| c == fence) && closer.len() >= length;
        (!closed).then(|| format!("\n{}{}", opening(text, code.start), fence.to_string().repeat(length)))
    });
    let mut lines: Vec<String> = Vec::new();
    if gap_before {
        lines.push(gap.clone());
    }
    let (into_line, into_column) = (block[..into].matches('\n').count(), into - block[..into].rfind('\n').map_or(0, |at| at + 1));
    let caret_index = lines.len() + into_line;
    // (Its first line where the line's marks are, written in, the others under it.)
    let begun = |nth: usize| if nth == 0 && !gap_before { first_prefix.as_str() } else { prefix.as_str() };
    let caret_prefix = begun(into_line).len();
    for (nth, written) in block.split('\n').enumerate() {
        let begun = begun(nth);
        lines.push(if written.is_empty() && nth != into_line { gap.clone() } else { format!("{begun}{written}") });
    }
    if gap_after {
        lines.push(gap.clone());
    }
    if table && !gap_after {
        // (The lines after it there are, as many as are wanted: a blank one, and one to write on.)
        let there = tree.lines.len() - 1 - index;
        for _ in there.min(2)..2 {
            lines.push(gap.clone());
        }
    }
    let range = if blank { line.clone() } else { line.end..line.end };
    let head = if blank { String::new() } else { format!("{}\n", closing.unwrap_or_default()) };
    let caret = range.start + head.len() + lines[..caret_index].iter().map(|line| line.len() + 1).sum::<usize>() + caret_prefix + into_column;
    let with = format!("{head}{}", lines.join("\n"));
    let mut rewrite = Rewrite::default();
    rewrite.put(range, with);
    (rewrite.apply(text), caret)
}

/// What is pasted: its words (line breaks `\n`), and whether they are blocks, each to stand on lines of its own (see
/// `placed`), not words among others put where the caret is.
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Pasted {
    pub text: String,
    pub blocks: bool,
}

/// `pasted` put in place of what is chosen, its line breaks `\n`: a web address alone over words chosen on one line
/// makes a link of them (see `link`), blocks are put on lines of their own (see `placed`). The caret after what is put.
pub fn paste(text: &str, chosen: (usize, usize), pasted: &Pasted) -> Edit {
    let words = pasted.text.replace("\r\n", "\n").replace('\r', "\n");
    if chosen.0 != chosen.1 && !pasted.blocks {
        if let Some(made) = web_address(&words).and_then(|to| link(text, chosen, Some(to))) {
            return made;
        }
    }
    let (start, end) = (byte(text, chosen.0.min(chosen.1)), byte(text, chosen.0.max(chosen.1)));
    let mut rewrite = Rewrite::default();
    rewrite.put(start..end, if pasted.blocks { "" } else { words.as_str() });
    let once = rewrite.apply(text);
    if !pasted.blocks {
        let caret = start + words.len();
        return edit(text, &once, (caret, caret));
    }
    let (after, caret) = placed(&once, start, &words, words.len(), false, ends_in_table(&words), true);
    edit(text, &after, (caret, caret))
}

/// Whether the last block of `markdown` is a table.
fn ends_in_table(markdown: &str) -> bool {
    let mut depth = 0usize;
    let mut last = false;
    for event in Parser::new_ext(markdown, READ) {
        match event {
            Event::Start(tag) => {
                if depth == 0 {
                    last = matches!(tag, Tag::Table(_));
                }
                depth += 1;
            }
            Event::End(_) => depth -= 1,
            _ if depth == 0 => last = false,
            _ => {}
        }
    }
    last
}

/// Whether `pasted` is a web address alone (one line, `http://` or `https://`, no spaces): pasted over what is chosen,
/// it makes a link of it (see `link`).
pub fn web_address(pasted: &str) -> Option<&str> {
    let pasted = pasted.trim();
    let lower = pasted.to_ascii_lowercase();
    ((lower.starts_with("https://") || lower.starts_with("http://")) && !pasted.contains(char::is_whitespace) && pasted.split("://").nth(1).is_some_and(|rest| !rest.is_empty())).then_some(pasted)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `text` with `edit` made, and what is chosen after it marked: `‸` the caret, `«` and `»` what is chosen.
    fn applied(text: &str, edit: &Edit) -> String {
        let mut units: Vec<u16> = text.encode_utf16().collect();
        units.splice(edit.range.clone(), edit.with.encode_utf16());
        let (from, to) = edit.chosen;
        let mut out: Vec<u16> = Vec::new();
        for at in 0..=units.len() {
            if from == to && at == from {
                out.extend("‸".encode_utf16());
            } else if from != to && at == from.min(to) {
                out.extend("«".encode_utf16());
            } else if from != to && at == from.max(to) {
                out.extend("»".encode_utf16());
            }
            if at < units.len() {
                out.push(units[at]);
            }
        }
        String::from_utf16(&out).unwrap()
    }

    /// `marked` read: the text, and where `‸` (the caret) or `«`…`»` (what is chosen) is.
    fn read(marked: &str) -> (String, (usize, usize)) {
        let mut text = String::new();
        let (mut from, mut to) = (None, None);
        for c in marked.chars() {
            let at = text.encode_utf16().count();
            match c {
                '‸' => (from, to) = (Some(at), Some(at)),
                '«' => from = Some(at),
                '»' => to = Some(at),
                _ => text.push(c),
            }
        }
        (text, (from.unwrap(), to.unwrap()))
    }

    fn enter_at(marked: &str) -> Option<String> {
        let (text, (caret, _)) = read(marked);
        enter(&text, caret).map(|edit| applied(&text, &edit))
    }

    /// Enter in a list's item begins the next item as it was: the same bullet, a number one more (the items after it
    /// numbered on), a to-do not done, in the same block quotes and as far in, the words after the caret go with it.
    #[test]
    fn enter_carries_a_list_on() {
        assert_eq!(enter_at("- 牛奶‸").as_deref(), Some("- 牛奶\n- ‸"));
        assert_eq!(enter_at("  * a‸").as_deref(), Some("  * a\n  * ‸"));
        assert_eq!(enter_at("1. a‸\n2. b").as_deref(), Some("1. a\n2. ‸\n3. b"));
        assert_eq!(enter_at("9) a‸").as_deref(), Some("9) a\n10) ‸"));
        assert_eq!(enter_at("- [x] 完成‸").as_deref(), Some("- [x] 完成\n- [ ] ‸"));
        assert_eq!(enter_at("- 前‸后").as_deref(), Some("- 前\n- ‸后"));
        assert_eq!(enter_at("1.  a‸").as_deref(), Some("1.  a\n2.  ‸"));
        assert_eq!(enter_at("> - a‸").as_deref(), Some("> - a\n> - ‸"));
        assert_eq!(enter_at("- - a‸").as_deref(), Some("- - a\n  - ‸"));
    }

    /// A number grown a digit wider keeps what is under its item under it: moved in with its words.
    #[test]
    fn a_wider_number_moves_what_is_under_it() {
        assert_eq!(enter_at("8. a‸\n9. b\n   - c").as_deref(), Some("8. a\n9. ‸\n10. b\n    - c"));
        assert_eq!(enter_at("9. b‸\n   - c").as_deref(), Some("9. b\n10. ‸\n    - c"));
    }

    /// Enter before an item's words, outside a list, on a line under an item's first, or in a block of code is a line
    /// break as always.
    #[test]
    fn enter_elsewhere_is_a_line_break() {
        assert_eq!(enter_at("-‸ a"), None);
        assert_eq!(enter_at("普通‸一行"), None);
        assert_eq!(enter_at("---‸"), None);
        assert_eq!(enter_at("-1 不是‸"), None);
        assert_eq!(enter_at("```\n- a‸\n```"), None);
        assert_eq!(enter_at("- a\n  b‸"), None);
    }

    /// Enter on an item with no words ends it: one further in comes out a step, the last of a list leaves a blank line
    /// before the caret (so what is written there is no longer the list's), any other loses its mark.
    #[test]
    fn enter_on_an_empty_item_ends_it() {
        assert_eq!(enter_at("- a\n- ‸").as_deref(), Some("- a\n\n‸"));
        assert_eq!(enter_at("- a\n  - b\n  - ‸").as_deref(), Some("- a\n  - b\n- ‸"));
        // (An item with nothing in it yet is the item it is to be, though under a paragraph CommonMark reads it as words.)
        assert_eq!(enter_at("- a\n  - ‸").as_deref(), Some("- a\n- ‸"));
        assert_eq!(enter_at("1. a\n   1. ‸").as_deref(), Some("1. a\n2. ‸"));
        assert_eq!(enter_at("- [ ] ‸").as_deref(), Some("‸"));
        assert_eq!(enter_at("1. a\n2. ‸\n3. b").as_deref(), Some("1. a\n‸\n2. b"));
        assert_eq!(enter_at("1. ‸\n2. b").as_deref(), Some("‸\n1. b"));
        assert_eq!(enter_at("> - a\n> - ‸").as_deref(), Some("> - a\n> \n> ‸"));
    }

    fn tab(marked: &str, out: bool) -> Option<Option<String>> {
        let (text, chosen) = read(marked);
        indent(&text, chosen, out).map(|edit| edit.map(|edit| applied(&text, &edit)))
    }

    /// Tab moves items under the one before them (as far in as its words), with what is under them, Shift+Tab out
    /// again, the lists they leave and come to numbered on.
    #[test]
    fn tab_moves_items_in_and_out() {
        assert_eq!(tab("- a\n- b‸", false), Some(Some("- a\n  - b‸".into())));
        assert_eq!(tab("1. a\n2. b‸", false), Some(Some("1. a\n   1. b‸".into())));
        assert_eq!(tab("- a\n- b‸\n  - c\n- d", false), Some(Some("- a\n  - b‸\n    - c\n- d".into())));
        assert_eq!(tab("1. a\n   1. x\n2. b‸\n3. c", false), Some(Some("1. a\n   1. x\n   2. b‸\n2. c".into())));
        assert_eq!(tab("1. a\n2. «b\n3. c»\n4. d", false), Some(Some("1. a\n   1. «b\n   2. c»\n2. d".into())));
        assert_eq!(tab("- a\n  - b‸", true), Some(Some("- a\n- b‸".into())));
        assert_eq!(tab("1. a\n   1. b‸\n   2. c", true), Some(Some("1. a\n2. b‸\n   1. c".into())));
        // An item just begun (Enter, then Tab), nothing in it yet.
        assert_eq!(tab("- a\n- ‸", false), Some(Some("- a\n  - ‸".into())));
        assert_eq!(tab("1. a\n2. ‸\n3. b", false), Some(Some("1. a\n   1. ‸\n2. b".into())));
        assert_eq!(tab("- [ ] a\n- [ ] ‸", false), Some(Some("- [ ] a\n  - [ ] ‸".into())));
        // The first item has none to come under, one in none none to come out of, outside a list, the key's own.
        assert_eq!(tab("- a‸", false), Some(None));
        assert_eq!(tab("- a‸", true), Some(None));
        assert_eq!(tab("words‸", false), None);
    }

    fn back_at(marked: &str) -> Option<String> {
        let (text, (caret, _)) = read(marked);
        backspace(&text, caret, true).map(|edit| applied(&text, &edit))
    }

    /// Shown as it reads, Backspace takes back marks put away one by one, as written: a heading's `#`, a quote's line's
    /// `>`, a block of code's fences; at the start of the line after a rule it takes the rule away, after a block of
    /// code it goes to the end of its last line. Delete at the end of the line before a rule takes the rule away, before
    /// a fence does nothing. Shown as written, each is the key's own.
    #[test]
    fn backspace_takes_back_marks_put_away() {
        assert_eq!(back_at("a\n## ‸标题").as_deref(), Some("a\n‸标题"));
        assert_eq!(back_at("> a\n> ‸b").as_deref(), Some("> a\n‸b"));
        assert_eq!(back_at("> > ‸b").as_deref(), Some("‸b"));
        assert_eq!(back_at("```\n‸x\ny\n```\nz").as_deref(), Some("‸x\ny\nz"));
        assert_eq!(back_at("```\n‸x").as_deref(), Some("‸x"));
        assert_eq!(back_at("a\n\n---\n‸b").as_deref(), Some("a\n\n‸b"));
        assert_eq!(back_at("```\nx\n```\n‸b").as_deref(), Some("```\nx‸\n```\nb"));
        assert_eq!(back_at("## 标‸题"), None);
        assert_eq!(back_at("```\nx\n‸y\n```"), None);
        let (text, (caret, _)) = read("a\n\n---\n‸b");
        assert_eq!(backspace(&text, caret, false), None);
        let delete_at = |marked: &str, formatted: bool| {
            let (text, (caret, _)) = read(marked);
            delete(&text, caret, formatted).map(|edit| applied(&text, &edit))
        };
        assert_eq!(delete_at("a\n\nb‸\n\n---\nc", true), None);
        assert_eq!(delete_at("a‸\n---\nc", true), None);
        assert_eq!(delete_at("a\n\n‸\n---\nc", true).as_deref(), Some("a\n\n‸\nc"));
        assert_eq!(delete_at("a\n\n‸\n```\nx\n```", true).as_deref(), Some("a\n\n‸\n```\nx\n```"));
        assert_eq!(delete_at("a\n\n‸\n---\nc", false), None);
    }

    /// Backspace at the start of an item's words moves one further in out a step, else takes its mark off (a to-do's
    /// box with it): its words a paragraph of the item before, or before the list, anywhere else it is the key's own.
    #[test]
    fn backspace_takes_a_mark_off() {
        assert_eq!(back_at("- ‸a").as_deref(), Some("‸a"));
        assert_eq!(back_at("- [ ] ‸买").as_deref(), Some("‸买"));
        assert_eq!(back_at("- a\n  - ‸b").as_deref(), Some("- a\n- ‸b"));
        assert_eq!(back_at("- a\n  - ‸").as_deref(), Some("- a\n- ‸"));
        assert_eq!(back_at("- a\n- ‸b").as_deref(), Some("- a\n‸b"));
        assert_eq!(back_at("1. a\n2. ‸b\n3. c").as_deref(), Some("1. a\n‸b\n2. c"));
        let written_at = |marked: &str| {
            let (text, (caret, _)) = read(marked);
            backspace(&text, caret, false).map(|edit| applied(&text, &edit))
        };
        assert_eq!(written_at("- a\n- ‸b").as_deref(), Some("- a\n  ‸b"));
        assert_eq!(written_at("1. a\n2. ‸b\n3. c").as_deref(), Some("1. a\n   ‸b\n2. c"));
        // Shown as it reads, at the start of a line's words under an item, the line out of it a step.
        assert_eq!(back_at("1. a\n   ‸b\n2. c").as_deref(), Some("1. a\n‸b\n2. c"));
        assert_eq!(back_at("- a\n  - b\n    ‸c").as_deref(), Some("- a\n  - b\n  ‸c"));
        assert_eq!(back_at("> - a\n>   ‸b").as_deref(), Some("> - a\n> ‸b"));
        assert_eq!(back_at("1. a\n‸b"), None);
        assert_eq!(written_at("1. a\n   ‸b"), None);
        assert_eq!(back_at("1. ‸a\n2. b").as_deref(), Some("‸a\n1. b"));
        assert_eq!(back_at("3. ‸a\n4. b").as_deref(), Some("‸a\n\n3. b"));
        assert_eq!(back_at("- ‸a\n- b").as_deref(), Some("‸a\n- b"));
        assert_eq!(back_at("- - ‸a").as_deref(), Some("- ‸a"));
        assert_eq!(back_at("- a‸b"), None);
        assert_eq!(back_at("‸a"), None);
    }

    /// Ctrl+Enter ticks a to-do or no longer, gives a list's item a box, makes any line a to-do, not in code.
    #[test]
    fn ctrl_enter_ticks_a_to_do() {
        let toggled = |marked: &str| {
            let (text, (caret, _)) = read(marked);
            toggle_task(&text, caret).map(|edit| applied(&text, &edit))
        };
        assert_eq!(toggled("- [ ] 买‸").as_deref(), Some("- [x] 买‸"));
        assert_eq!(toggled("- [x] 买‸").as_deref(), Some("- [ ] 买‸"));
        assert_eq!(toggled("- 买‸").as_deref(), Some("- [ ] 买‸"));
        assert_eq!(toggled("-‸").as_deref(), Some("- [ ] ‸"));
        assert_eq!(toggled("  买‸").as_deref(), Some("  - [ ] 买‸"));
        assert_eq!(toggled("> 买‸").as_deref(), Some("> - [ ] 买‸"));
        assert_eq!(toggled("- [ ]‸").as_deref(), Some("- [x]‸"));
        assert_eq!(toggled("```\n买‸\n```"), None);
    }

    /// Alt+Up and Alt+Down move the lines chosen past the next, numbered lists numbered on from where they began.
    #[test]
    fn alt_arrows_move_lines() {
        let moved = |marked: &str, up: bool| {
            let (text, chosen) = read(marked);
            move_lines(&text, chosen, up).map(|edit| applied(&text, &edit))
        };
        assert_eq!(moved("a\nb‸\nc", true).as_deref(), Some("b‸\na\nc"));
        assert_eq!(moved("a\nb‸\nc", false).as_deref(), Some("a\nc\nb‸"));
        assert_eq!(moved("a‸", true), None);
        assert_eq!(moved("1. a\n2. b‸", true).as_deref(), Some("1. b‸\n2. a"));
        assert_eq!(moved("«a\nb\n»c", false).as_deref(), Some("c\n«a\nb»"));
        assert_eq!(moved("x\n«a\nb\n»c", true).as_deref(), Some("«a\nb»\nx\nc"));
    }

    /// Ctrl+1 to 3 make a heading of a line (its `#` written anew, an underlined one's line under it taken away),
    /// Ctrl+0 a line of words again, code is left.
    #[test]
    fn headings_are_made_and_unmade() {
        let made = |marked: &str, level: u8| {
            let (text, chosen) = read(marked);
            applied(&text, &heading(&text, chosen, level))
        };
        assert_eq!(made("标‸题", 1), "# 标‸题");
        assert_eq!(made("## 标‸题", 3), "### 标‸题");
        assert_eq!(made("## 标‸题", 0), "标‸题");
        assert_eq!(made("- 项‸", 2), "- ## 项‸");
        assert_eq!(made("标‸题\n---", 0), "标‸题");
        assert_eq!(made("标‸题\n===\n下文", 2), "## 标‸题\n下文");
        assert_eq!(made("```\n代‸码\n```", 1), "```\n代‸码\n```");
    }

    /// Ctrl+B and the rest: what is chosen marked (each line's words past its list's mark, the spaces at their ends left
    /// out), or within a stretch marked so, that stretch's marks taken off, nothing chosen, a pair with the caret
    /// between, code by more backticks than are in its words.
    #[test]
    fn marks_are_put_on_and_taken_off() {
        let toggled = |marked: &str, how: Marked| {
            let (text, chosen) = read(marked);
            applied(&text, &toggle(&text, chosen, how))
        };
        assert_eq!(toggled("一 «粗体 »字", Marked::Bold), "一 **«粗体»** 字");
        assert_eq!(toggled("一 **«粗体»** 字", Marked::Bold), "一 «粗体» 字");
        assert_eq!(toggled("一 **粗«体»** 字", Marked::Bold), "一 粗«体» 字");
        assert_eq!(toggled("«a\n\nb»", Marked::Italic), "*«a*\n\n*b»*");
        assert_eq!(toggled("«- a\n- b»", Marked::Bold), "- **«a**\n- **b»**");
        assert_eq!(toggled("«# 题»", Marked::Struck), "# ~~«题»~~");
        assert_eq!(toggled("a‸b", Marked::Code), "a`‸`b");
        assert_eq!(toggled("x ~~«y»~~", Marked::Struck), "x «y»");
        assert_eq!(toggled("«a`b»", Marked::Code), "``«a`b»``");
        assert_eq!(toggled("«`a»", Marked::Code), "`` «`a» ``");
        assert_eq!(toggled("`` «`a» ``", Marked::Code), "«`a»");
    }

    /// A link made of what is chosen, or an empty one, brackets that pair with none escaped, a place with spaces in
    /// angle brackets, none over more than one line.
    #[test]
    fn links_are_made() {
        let made = |marked: &str, to: Option<&str>| {
            let (text, chosen) = read(marked);
            link(&text, chosen, to).map(|edit| applied(&text, &edit))
        };
        assert_eq!(made("看«文档»", Some("https://a.b")).as_deref(), Some("看[文档](https://a.b)‸"));
        assert_eq!(made("看«文档»", None).as_deref(), Some("看[文档](‸)"));
        assert_eq!(made("看‸", None).as_deref(), Some("看[‸]()"));
        assert_eq!(made("«a] [b»", Some("x")).as_deref(), Some("[a\\] \\[b](x)‸"));
        assert_eq!(made("«[a]»", Some("x")).as_deref(), Some("[[a]](x)‸"));
        assert_eq!(made("«a»", Some("C:/my file.md")).as_deref(), Some("[a](<C:/my file.md>)‸"));
        assert_eq!(made("«a»", Some("https://w.org/A_(b)")).as_deref(), Some("[a](https://w.org/A_(b))‸"));
        assert_eq!(made("«a\nb»", Some("x")), None);
        assert_eq!(web_address(" https://a.b/c \n"), Some("https://a.b/c"));
        assert_eq!(web_address("see https://a.b"), None);
    }

    /// A block goes on the caret's line where it is blank, else on lines after it, a blank line between it and words
    /// before and after it, but a to-do among a list's items goes on the list. The caret where its words go.
    #[test]
    fn blocks_are_inserted() {
        let inserted = |marked: &str, block: Block| {
            let (text, chosen) = read(marked);
            applied(&text, &insert(&text, chosen, block))
        };
        assert_eq!(inserted("‸", Block::Table), "| ‸ |  |\n| --- | --- |\n|  |  |\n\n");
        assert_eq!(inserted("文‸字", Block::Table), "文字\n\n| ‸ |  |\n| --- | --- |\n|  |  |\n\n");
        assert_eq!(inserted("a‸\n", Block::Table), "a\n\n| ‸ |  |\n| --- | --- |\n|  |  |\n\n");
        assert_eq!(inserted("a‸\nb", Block::Table), "a\n\n| ‸ |  |\n| --- | --- |\n|  |  |\n\nb");
        assert_eq!(inserted("a\n‸\nb", Block::Code), "a\n\n```\n‸\n```\n\nb");
        assert_eq!(inserted("a\n\n‸\n\nb", Block::Code), "a\n\n```\n‸\n```\n\nb");
        assert_eq!(inserted("a‸", Block::Rule), "a\n\n---\n‸");
        assert_eq!(inserted("a‸\nb", Block::Rule), "a\n\n---\n‸\nb");
        assert_eq!(inserted("a‸", Block::Todo), "a\n\n- [ ] ‸");
        assert_eq!(inserted("- [ ] a‸\n- [ ] b", Block::Todo), "- [ ] a\n- [ ] ‸\n- [ ] b");
        assert_eq!(inserted("- a\n‸", Block::Todo), "- a\n- [ ] ‸");
        assert_eq!(inserted("‸\na", Block::Todo), "- [ ] ‸\n\na");
        assert_eq!(inserted("```\nx‸\n```\nb", Block::Rule), "```\nx\n```\n\n---\n‸\nb");
        assert_eq!(inserted("```\n‸\n```", Block::Table), "```\n\n```\n\n| ‸ |  |\n| --- | --- |\n|  |  |\n\n");
        // In a list's item: a to-do another item after it (after what is under it), as far in and marked as it is,
        // numbered on.
        assert_eq!(inserted("- a\n  - b‸\n    - x\n  - c", Block::Todo), "- a\n  - b\n    - x\n  - [ ] ‸\n  - c");
        assert_eq!(inserted("* a‸", Block::Todo), "* a\n* [ ] ‸");
        assert_eq!(inserted("1. a‸\n2. b", Block::Todo), "1. a\n2. [ ] ‸\n3. b");
        // In a quote, in an item: its lines begun as theirs are.
        assert_eq!(inserted("> a‸", Block::Todo), "> a\n>\n> - [ ] ‸");
        assert_eq!(inserted("> a‸", Block::Table), "> a\n>\n> | ‸ |  |\n> | --- | --- |\n> |  |  |\n>\n>");
        assert_eq!(inserted("- a‸\n- b", Block::Code), "- a\n\n  ```\n  ‸\n  ```\n\n- b");
        assert_eq!(inserted("- ‸", Block::Rule), "- ---\n  ‸");
        // Code whose fence is not closed, closed first.
        assert_eq!(inserted("```\nx‸", Block::Table), "```\nx\n```\n\n| ‸ |  |\n| --- | --- |\n|  |  |\n\n");
        assert_eq!(inserted("> ~~~~\n> x‸", Block::Code), "> ~~~~\n> x\n> ~~~~\n>\n> ```\n> ‸\n> ```");
    }

    /// What is pasted goes in place of what is chosen with its line breaks made `\n`, the caret after it: a web
    /// address over words chosen makes a link of them.
    #[test]
    fn pasting() {
        let pasted_as = |marked: &str, what: &str, blocks: bool| {
            let (text, chosen) = read(marked);
            applied(&text, &paste(&text, chosen, &Pasted { text: what.into(), blocks }))
        };
        let pasted = |marked: &str, what: &str| pasted_as(marked, what, false);
        assert_eq!(pasted("a‸b", "x\r\ny\rz"), "ax\ny\nz‸b");
        assert_eq!(pasted("a«bc»d", "中"), "a中‸d");
        assert_eq!(pasted("看«文档»", "https://a.b/c"), "看[文档](https://a.b/c)‸");
        assert_eq!(pasted("看‸", "https://a.b/c"), "看https://a.b/c‸");
        assert_eq!(pasted("«a\nb»", "https://a.b"), "https://a.b‸");
        // Blocks on lines of their own, where the caret is: the words after it after them.
        let blocks = |marked: &str, what: &str| pasted_as(marked, what, true);
        assert_eq!(blocks("before‸after", "# 标题"), "before\n\n# 标题‸\n\nafter");
        assert_eq!(blocks("‸after", "# 标题"), "# 标题‸\n\nafter");
        assert_eq!(blocks("a«bc»", "# 标题\n\n段"), "a\n\n# 标题\n\n段‸");
        assert_eq!(blocks("> 引‸文", "| A |\n| --- |"), "> 引\n>\n> | A |\n> | --- |‸\n>\n> 文");
        assert_eq!(blocks("- ‸", "# 标题\n\n段"), "- # 标题\n\n  段‸");
        assert_eq!(blocks("x‸", "| A |\n| --- |"), "x\n\n| A |\n| --- |‸\n\n");
    }

    /// Lists numbered as they are moved: a list begun by an item moved in under another from one, a line moved past
    /// another its list's numbers as they were.
    #[test]
    fn lists_are_numbered_as_they_move() {
        assert_eq!(tab("3. a\n4. ‸b", false), Some(Some("3. a\n   1. ‸b".into())));
        let moved = |marked: &str, up: bool| {
            let (text, chosen) = read(marked);
            move_lines(&text, chosen, up).map(|edit| applied(&text, &edit))
        };
        assert_eq!(moved("1. ‸a\n2. b", false), Some("1. b\n2. ‸a".into()));
        assert_eq!(moved("1. a\n2. ‸b", true), Some("1. ‸b\n2. a".into()));
        assert_eq!(moved("x\n\n3. ‸a\n4. b", false), Some("x\n\n3. b\n4. ‸a".into()));
    }

    /// A heading underlined over several lines made one of `#` marks: all its words, on one line. Code's fence (in
    /// from the line's start) is no heading's.
    #[test]
    fn headings_underlined_over_lines() {
        let made = |marked: &str, level: u8| {
            let (text, chosen) = read(marked);
            applied(&text, &heading(&text, chosen, level))
        };
        assert_eq!(made("al‸pha\nbeta\n===", 1), "# al‸pha beta");
        assert_eq!(made("  ``‸`\nx\n  ```", 1), "  ``‸`\nx\n  ```");
    }

    /// A link written from words to show and where it goes (as the link's plate writes one), whatever they hold
    /// (brackets, a pair of them, a link's own marks, a backslash at the end, an address with spaces, parentheses
    /// paired or not, angle brackets, a backslash): read back as one link, of those words, going there, all of what
    /// was written.
    #[test]
    fn a_link_written_reads_back_as_written() {
        let words = ["文档", "a]b", "[x]", "[x](y)", "a [b", "C:\\", "a\\b", "**粗**", "一 二"];
        let places = ["https://a.b/c", "C:/my file.md", "https://w.org/A_(b)", "https://a.b/(", "x)y", "a<b>c", "a\\b", "https://a.b/?q=[1]"];
        for shown in words {
            for to in places {
                let written = link_written(shown, to);
                let text = format!("看{written}吧");
                let found = link_at(&text, (2, 2)).unwrap_or_else(|| panic!("{text}: no link"));
                assert_eq!(found.to, to, "{text}");
                assert_eq!(found.span, 1..1 + written.encode_utf16().count(), "{text}");
                if !shown.contains('*') {
                    assert_eq!(found.shown, shown, "{text}");
                }
                // Written anew from what it shows: the same.
                assert_eq!(link_written(&found.shown, &found.to), written, "{text}");
            }
        }
    }

    /// The link the caret is on (in its words, or at their end), or what is chosen within it: not one beside it, nor
    /// one in a table. A web address written out is one too.
    #[test]
    fn links_are_found_where_chosen() {
        let found = |marked: &str| {
            let (text, chosen) = read(marked);
            link_at(&text, chosen).map(|found| (found.shown, found.to))
        };
        let docs = Some(("文档".to_string(), "https://a.b".to_string()));
        assert_eq!(found("看[文‸档](https://a.b)"), docs);
        assert_eq!(found("看[‸文档](https://a.b)"), docs);
        assert_eq!(found("看[文档‸](https://a.b)"), docs);
        assert_eq!(found("看«[文档](https://a.b)»"), docs);
        assert_eq!(found("看[«文»档](https://a.b)"), docs);
        assert_eq!(found("看‸[文档](https://a.b)"), None);
        assert_eq!(found("看[文档](https://a.b)‸"), None);
        assert_eq!(found("«看[文»档](https://a.b)"), None);
        assert_eq!(found("见 https://x.y/‸z"), Some(("https://x.y/z".into(), "https://x.y/z".into())));
        assert_eq!(found("[a\\]b‸](x)"), Some(("a]b".into(), "x".into())));
        assert_eq!(found("| [a‸](x) |\n| - |"), None);
    }

    /// A link put in place of what is chosen, the caret after it. One taken off its words leaves them, chosen, but not
    /// a web address written out (it would read as a link still).
    #[test]
    fn links_are_put_and_taken_off() {
        let put = |marked: &str, words: &str, to: &str| {
            let (text, chosen) = read(marked);
            applied(&text, &put_link(&text, chosen, words, to))
        };
        assert_eq!(put("看«文档»吧", "文档", "https://a.b"), "看[文档](https://a.b)‸吧");
        assert_eq!(put("看‸", "新的 [1]", "a b"), "看[新的 \\[1\\]](<a b>)‸");
        assert_eq!(put("看[文档](https://a.b)‸", "说明", "https://c.d"), "看[文档](https://a.b)[说明](https://c.d)‸");
        let off = |marked: &str| {
            let (text, chosen) = read(marked);
            let found = link_at(&text, chosen).unwrap();
            unlink(&text, &found).map(|edit| applied(&text, &edit))
        };
        assert_eq!(off("看[文‸档](https://a.b)吧").as_deref(), Some("看«文档»吧"));
        assert_eq!(off("[**粗‸**](x)").as_deref(), Some("«**粗**»"));
        assert_eq!(off("看 https://a.b/‸c"), None);
        assert_eq!(off("看 <https://a.b/‸c>"), None);
    }

    /// A link made of words ending in a backslash: the backslash escaped, so the link is one.
    #[test]
    fn a_link_of_words_ending_in_a_backslash() {
        let (text, chosen) = read("«C:\\»");
        assert_eq!(applied(&text, &link(&text, chosen, Some("https://a.b")).unwrap()), "[C:\\\\](https://a.b)‸");
    }
}
