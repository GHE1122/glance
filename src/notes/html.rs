//! What is pasted into a note as HTML (a web page's, Word's: see docs/station-and-notes.md, section 九 8), written as
//! the Markdown a note is: headings, bold, slanted and struck words, links, code, code blocks, lists (nested, and
//! to-dos), quotes, tables, rules, paragraphs and line breaks. What it does not know only its words are kept of, an
//! image as a link to it, and what would read as Markdown's marks in its words is escaped.
//!
//! The page is read as a browser reads it (html5ever), the fragment copied in the page round it, so a table's row
//! copied alone is a row still. Its tree is walked without calling down into itself (a page nests as deep as it likes),
//! each block written as it is closed: words as runs, each set one way, marked where the way they are set changes.

use std::rc::Rc;

use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData, RcDom};

use super::edit::Pasted;

/// The comments put round the fragment in the page it is read in (see `pasted`): only what is between them is written.
const FROM: &str = "glance:fragment";
const TO: &str = "glance:/fragment";

/// As deep as a browser builds a page's tree (Chromium's parser stops at 512): an element deeper is read as part of
/// what holds it, its tag not asked, so what is written nests no deeper.
const DEEPEST: usize = 512;

/// What the clipboard's `HTML Format` holds (`clipboard` its bytes) as Markdown: the fragment its header says
/// (`StartFragment` to `EndFragment`, UTF-8 byte offsets from its start), read in the page round it (`StartHTML` to
/// `EndHTML`, where it says there is one). None where its header says no fragment.
pub fn pasted(clipboard: &[u8]) -> Option<Pasted> {
    // (Its header is ASCII lines, before anything else.)
    let head = String::from_utf8_lossy(&clipboard[..clipboard.len().min(1024)]);
    let offset = |key: &str| head.lines().find_map(|line| line.strip_prefix(key)).and_then(|value| value.trim().parse::<i64>().ok()).and_then(|at| usize::try_from(at).ok()).filter(|at| *at <= clipboard.len());
    let (start, end) = (offset("StartFragment:")?, offset("EndFragment:")?);
    if start > end {
        return None;
    }
    let page_start = offset("StartHTML:").filter(|at| *at <= start).unwrap_or(start);
    let page_end = offset("EndHTML:").filter(|at| *at >= end).unwrap_or(end);
    let text = |range: std::ops::Range<usize>| String::from_utf8_lossy(&clipboard[range]).into_owned();
    let page = format!("{}<!--{FROM}-->{}<!--{TO}-->{}", text(page_start..start), text(start..end), text(end..page_end));
    Some(read(&page, false))
}

/// `html` read and written as Markdown: from its start where `inside`, else from the comment `FROM` to `TO`.
fn read(html: &str, inside: bool) -> Pasted {
    let dom = html5ever::parse_document(RcDom::default(), Default::default()).one(html);
    let mut reader = Reader { holders: vec![Holder::Flow(Flow::new(Flows::Page, inside, false))], inside, marking: false };
    let mut frames = vec![Frame { node: dom.document.clone(), next: 0, context: Context::default(), holder: None, end: End::Nothing }];
    while let Some(frame) = frames.last_mut() {
        let child = frame.node.children.borrow().get(frame.next).cloned();
        match child {
            Some(child) => {
                frame.next += 1;
                let context = frame.context.clone();
                let depth = frames.len();
                if let Some(frame) = reader.enter(&child, context, depth) {
                    frames.push(frame);
                }
            }
            None => {
                let frame = frames.pop().expect("a frame walked");
                reader.leave(&frame);
            }
        }
    }
    reader.finish()
}

/// How words are set where they are, as what holds them says (its tags, its `style`).
#[derive(Clone, Copy, Default, PartialEq)]
struct Style {
    bold: bool,
    italic: bool,
    struck: bool,
}

/// What words are in where they are: how they are set, the link they are in, whether they are code, whether they are
/// drawn bold as they are (a heading's, a table's head's: not marked so), whether they are Word's mark before an item
/// of its list (see `Reader::marking`).
#[derive(Clone, Default)]
struct Context {
    style: Style,
    link: Option<Rc<str>>,
    code: bool,
    heads: bool,
    marking: bool,
}

impl Context {
    fn format(&self) -> Format {
        Format { bold: self.style.bold && !self.heads, italic: self.style.italic, struck: self.style.struck, code: self.code, link: self.link.clone() }
    }
}

/// How a run of words is written: marked bold, slanted, struck, as code, in a link.
#[derive(Clone, Debug, Default, PartialEq)]
struct Format {
    bold: bool,
    italic: bool,
    struck: bool,
    code: bool,
    link: Option<Rc<str>>,
}

/// Words among others (as the page has them, spaces not yet laid out), or a line break.
#[derive(Clone)]
enum Piece {
    Words(String, Format),
    Break,
}

impl Piece {
    /// Whether it writes nothing a page shows: spaces, a line break.
    fn blank(&self) -> bool {
        match self {
            Piece::Words(text, _) => text.chars().all(char::is_whitespace),
            Piece::Break => true,
        }
    }
}

/// What a block is, as what is next to it asks: a list can follow a paragraph in an item without a blank line between
/// where it can begin under one (bullets, numbers from one), two lists one after another are told apart by their marks.
#[derive(Clone, Copy, Debug, PartialEq)]
enum Shape {
    Paragraph,
    Heading,
    Code,
    Rule,
    Quote,
    Table,
    List { ordered: bool, alternate: bool, start: u64 },
}

/// A block written: its lines, and what it is.
type Written = (Vec<String>, Shape);

/// A block to be put among others: written, or a list (written as it is put, its marks as the block before it asks).
enum Block {
    Lines(Vec<String>, Shape),
    List(List),
}

/// A list's item: a to-do's box where it has one, its blocks.
#[derive(Default)]
struct Item {
    task: Option<bool>,
    blocks: Vec<Written>,
}

/// A list: numbered (from `start`) or not, its items.
struct List {
    ordered: bool,
    start: u64,
    items: Vec<Item>,
}

/// The greatest number a list's item can be written with (nine digits).
const LARGEST: u64 = 999_999_999;

impl List {
    /// Its lines: each item's mark before its first line (where `alternate`, `*` for `-` and `)` for `.`: it follows a
    /// list of its kind, and would be read as part of it), its other lines in from where its words begin.
    fn written(&self, alternate: bool) -> Written {
        let mut number = self.start.min(LARGEST);
        let mut lines = Vec::new();
        for item in &self.items {
            let marker = match (self.ordered, alternate) {
                (true, false) => format!("{number}. "),
                (true, true) => format!("{number}) "),
                (false, false) => "- ".to_string(),
                (false, true) => "* ".to_string(),
            };
            number = (number + 1).min(LARGEST);
            let task = item.task.map(|done| if done { "[x] " } else { "[ ] " }).unwrap_or_default();
            let inner = joined(&item.blocks, true);
            let indent = " ".repeat(marker.len());
            match inner.split_first() {
                Some((first, rest)) => {
                    lines.push(format!("{marker}{task}{first}"));
                    lines.extend(rest.iter().map(|line| if line.is_empty() { String::new() } else { format!("{indent}{line}") }));
                }
                None => lines.push(format!("{marker}{task}").trim_end().to_string()),
            }
        }
        (lines, Shape::List { ordered: self.ordered, alternate, start: self.start })
    }
}

/// `block` put after `blocks`.
fn place(blocks: &mut Vec<Written>, block: Block) {
    let written = match block {
        Block::Lines(lines, shape) => (lines, shape),
        Block::List(list) => {
            let alternate = matches!(blocks.last(), Some((_, Shape::List { ordered, alternate, .. })) if *ordered == list.ordered && !*alternate);
            list.written(alternate)
        }
    };
    if !written.0.is_empty() {
        blocks.push(written);
    }
}

/// `blocks`' lines, a blank line between each two: in a list's `item`, none before a list that can begin under the
/// paragraph before it.
fn joined(blocks: &[Written], item: bool) -> Vec<String> {
    let mut lines = Vec::new();
    for (at, (block, shape)) in blocks.iter().enumerate() {
        if at > 0 {
            let follows = item && blocks[at - 1].1 == Shape::Paragraph && matches!(shape, Shape::List { ordered, start, .. } if !ordered || *start == 1);
            if !follows {
                lines.push(String::new());
            }
        }
        lines.extend(block.iter().cloned());
    }
    lines
}

/// Where blocks go one after another: the page, a quote, a list's item.
#[derive(Clone, Copy, PartialEq)]
enum Flows {
    Page,
    Quote,
    Item,
}

/// Blocks one after another, and the words after the last of them (a paragraph to come), Word's list's items after it
/// (a list to come).
struct Flow {
    is: Flows,
    blocks: Vec<Written>,
    words: Vec<Piece>,
    listed: Vec<WordItem>,
    /// An item's to-do's box (a checkbox before its words).
    task: Option<bool>,
    /// Whether it is kept with nothing in it: begun in the fragment, or an item gone on with.
    keep: bool,
    /// An item begun where none was (what is in a list but in none of its items): the last item gone on with.
    implicit: bool,
    /// Whether a paragraph was ended by a block: its words were a block's, not words among others.
    bounded: bool,
}

impl Flow {
    fn new(is: Flows, keep: bool, implicit: bool) -> Flow {
        Flow { is, blocks: Vec::new(), words: Vec::new(), listed: Vec::new(), task: None, keep, implicit, bounded: false }
    }

    /// Word's list's items so far written as its lists.
    fn end_list(&mut self) {
        for block in word_lists(std::mem::take(&mut self.listed)) {
            place(&mut self.blocks, block);
        }
    }

    /// What is to come so far written: Word's list, then the paragraph its words make (ended by a block where `bounded`).
    fn flush(&mut self, bounded: bool) {
        self.end_list();
        let lines = lines(&std::mem::take(&mut self.words));
        if !lines.is_empty() {
            self.bounded |= bounded;
            self.blocks.push((lines, Shape::Paragraph));
        }
    }

    fn put(&mut self, block: Block) {
        self.flush(true);
        place(&mut self.blocks, block);
    }
}

/// Words on one line: a heading's, a table's cell's, its caption's, an item of Word's list.
enum Lines {
    Heading(usize),
    Cell { span: (usize, usize), align: &'static str },
    Caption,
    WordItem { level: usize, list: String, mark: String },
}

struct Line {
    is: Lines,
    words: Vec<Piece>,
}

/// An item of Word's list: its level (from 1), the list it is in (its `mso-list` but the level), Word's mark before it,
/// its words.
struct WordItem {
    level: usize,
    list: String,
    mark: String,
    words: Vec<Piece>,
}

/// A table's cell: its words on one line (`|` escaped), how many columns and rows it takes (none: all those after it),
/// its column's delimiter as it is set.
struct Cell {
    words: String,
    span: (usize, usize),
    align: &'static str,
}

/// A table: its rows, and what is written before it (its caption).
#[derive(Default)]
struct Table {
    rows: Vec<Vec<Cell>>,
    before: Vec<Written>,
}

impl Table {
    /// Its blocks, as GitHub's Markdown writes a table: its first row its head, each column set as its head's cell says,
    /// a cell over several columns or rows in the first of them, those after it empty, as many cells in each row as
    /// in its widest.
    fn written(self) -> Vec<Written> {
        let mut grid: Vec<Vec<String>> = Vec::new();
        let mut aligns: Vec<&'static str> = Vec::new();
        // (How many rows more each column is taken by a cell above.)
        let mut taken: Vec<usize> = Vec::new();
        for (nth, row) in self.rows.into_iter().enumerate() {
            let mut line: Vec<String> = Vec::new();
            let mut cells = row.into_iter();
            loop {
                while taken.get(line.len()).is_some_and(|rows| *rows > 0) {
                    taken[line.len()] -= 1;
                    line.push(String::new());
                }
                let Some(cell) = cells.next() else { break };
                let (across, down) = cell.span;
                let column = line.len();
                if nth == 0 {
                    aligns.resize(column, "---");
                    aligns.push(cell.align);
                }
                line.push(cell.words);
                line.extend(std::iter::repeat_n(String::new(), across - 1));
                if taken.len() < column + across {
                    taken.resize(column + across, 0);
                }
                for rows in &mut taken[column..column + across] {
                    *rows = down - 1;
                }
            }
            for rows in taken.iter_mut().skip(line.len()) {
                *rows = rows.saturating_sub(1);
            }
            grid.push(line);
        }
        let mut blocks = self.before;
        let columns = grid.iter().map(Vec::len).max().unwrap_or(0);
        if columns == 0 {
            return blocks;
        }
        aligns.resize(columns, "---");
        let line = |cells: &[String]| format!("| {} |", cells.join(" | "));
        let mut lines = Vec::new();
        for (nth, mut row) in grid.into_iter().enumerate() {
            row.resize(columns, String::new());
            lines.push(line(&row));
            if nth == 0 {
                lines.push(line(&aligns.iter().map(|align| align.to_string()).collect::<Vec<_>>()));
            }
        }
        blocks.push((lines, Shape::Table));
        blocks
    }
}

/// A table's row: its cells, and whether it is kept with nothing in them (begun in the fragment).
struct Row {
    cells: Vec<Cell>,
    keep: bool,
}

/// A block of code: its text as written, its language.
struct Code {
    text: String,
    language: String,
}

impl Code {
    /// Its lines, between fences longer than any run of backticks in it. None where it has nothing in it.
    fn written(&self) -> Option<Vec<String>> {
        let code = self.text.replace("\r\n", "\n").replace('\r', "\n").replace('\u{a0}', " ");
        let code = code.strip_suffix('\n').unwrap_or(&code);
        if code.trim().is_empty() {
            return None;
        }
        let fence = "`".repeat(longest_run(code, '`').max(2) + 1);
        let mut lines = vec![format!("{fence}{}", self.language)];
        lines.extend(code.split('\n').map(str::to_string));
        lines.push(fence);
        Some(lines)
    }
}

/// What is being written, innermost last: each block open, what is read goes in the innermost.
enum Holder {
    Flow(Flow),
    Line(Line),
    List(List),
    Table(Table),
    Row(Row),
    Code(Code),
}

/// An element being walked: the next of its children to be read, what its words are in, the holder it opened (to be
/// closed after it, with those opened in it), and what ends it.
struct Frame {
    node: Handle,
    next: usize,
    context: Context,
    holder: Option<usize>,
    end: End,
}

/// What an element's end does: nothing, ends a paragraph, a line break (a block among words on one line).
#[derive(Clone, Copy, PartialEq)]
enum End {
    Nothing,
    Bound,
    Break,
}

/// Reading a page's tree into Markdown.
struct Reader {
    holders: Vec<Holder>,
    /// Whether what is read is in the fragment.
    inside: bool,
    /// Whether what is read is Word's mark before an item of its list (`<![if !supportLists]>` to `<![endif]>`): taken
    /// aside, not written.
    marking: bool,
}

/// What an element is to its Markdown: left out, a block of its own, or words among others.
#[derive(PartialEq)]
enum Kind {
    Out,
    Block,
    Inline,
}

/// What an element `name` is (see `Kind`): what it does not know is words (the page's `html` and `body` too: what is in
/// them is as it is).
fn kind(name: &str) -> Kind {
    match name {
        "head" | "title" | "meta" | "link" | "script" | "style" | "template" | "noscript" | "iframe" | "object" | "svg" | "button" | "select" | "textarea" | "colgroup" | "col" => Kind::Out,
        "p" | "div" | "section" | "article" | "header" | "footer" | "main" | "nav" | "aside" | "figure" | "figcaption" | "address" | "center" | "details" | "summary" | "fieldset"
        | "legend" | "form" | "search" | "hgroup" | "menu" | "dir" | "dl" | "dt" | "dd" | "li" | "h1" | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol" | "blockquote" | "pre" | "table" | "caption"
        | "thead" | "tbody" | "tfoot" | "tr" | "td" | "th" | "hr" => Kind::Block,
        _ => Kind::Inline,
    }
}

/// An element's tag (lower case), none for what is not one.
fn tag(node: &Handle) -> Option<String> {
    match &node.data {
        NodeData::Element { name, .. } => Some(name.local.to_string().to_ascii_lowercase()),
        _ => None,
    }
}

/// An element's attribute `name`.
fn attribute(node: &Handle, name: &str) -> Option<String> {
    match &node.data {
        NodeData::Element { attrs, .. } => attrs.borrow().iter().find(|attr| attr.name.local.to_string().eq_ignore_ascii_case(name)).map(|attr| attr.value.to_string()),
        _ => None,
    }
}

/// An element's `style` property `name` (lower case), as written.
fn css(node: &Handle, name: &str) -> Option<String> {
    let style = attribute(node, "style")?;
    style.split(';').find_map(|declaration| {
        let (key, value) = declaration.split_once(':')?;
        key.trim().eq_ignore_ascii_case(name).then(|| value.trim().to_ascii_lowercase())
    })
}

/// How the words in `node` (its tag `name`) are set, in what is set `style`: its tag's way, and its `style`'s.
fn styled(node: &Handle, name: &str, style: Style) -> Style {
    let mut style = style;
    match name {
        "b" | "strong" => style.bold = true,
        "i" | "em" | "cite" | "var" | "dfn" => style.italic = true,
        "s" | "strike" | "del" => style.struck = true,
        _ => {}
    }
    if let Some(weight) = css(node, "font-weight") {
        style.bold = matches!(weight.as_str(), "bold" | "bolder") || weight.parse::<u32>().is_ok_and(|weight| weight >= 600);
    }
    if let Some(slant) = css(node, "font-style") {
        style.italic = slant.starts_with("italic") || slant.starts_with("oblique");
    }
    if let Some(line) = css(node, "text-decoration").or_else(|| css(node, "text-decoration-line")) {
        style.struck = line.contains("line-through");
    }
    style
}

/// The level of Word's list `node` is an item of (a paragraph its `mso-list` says is: `l0 level2 lfo1`), from 1.
fn word_level(node: &Handle) -> Option<usize> {
    let list = css(node, "mso-list")?;
    list.split_whitespace().find_map(|word| word.strip_prefix("level")).and_then(|level| level.parse().ok())
}

/// The language `node`'s class says its code is in (`language-rust`, `lang-rust`), none where it says none.
fn language(node: &Handle) -> String {
    let class = attribute(node, "class").unwrap_or_default();
    class.split_whitespace().find_map(|name| name.strip_prefix("language-").or_else(|| name.strip_prefix("lang-"))).unwrap_or_default().to_string()
}

impl Reader {
    /// `node` read, its parent's children read before it, in `context`, `depth` elements in: the frame it is walked
    /// in, none where its children are not to be read.
    fn enter(&mut self, node: &Handle, context: Context, depth: usize) -> Option<Frame> {
        match &node.data {
            NodeData::Text { contents } => {
                self.text(&contents.borrow(), &context);
                None
            }
            NodeData::Comment { contents } => {
                // (Word's mark before an item of its list, between these: `<![if !supportLists]>` … `<![endif]>`.)
                match contents.trim() {
                    FROM => self.inside = true,
                    TO => self.inside = false,
                    "[if !supportLists]" => self.marking = true,
                    "[endif]" => self.marking = false,
                    _ => {}
                }
                None
            }
            NodeData::Element { .. } => self.element(node, context, depth),
            _ => None,
        }
    }

    /// The element `node` read (see `enter`).
    fn element(&mut self, node: &Handle, mut context: Context, depth: usize) -> Option<Frame> {
        let name = tag(node)?;
        let frame = |context: Context, end: End| Some(Frame { node: node.clone(), next: 0, context, holder: None, end });
        if kind(&name) == Kind::Out {
            return None;
        }
        if depth > DEEPEST {
            return frame(context, End::Nothing);
        }
        if css(node, "mso-list").is_some_and(|list| list.starts_with("ignore")) {
            // (Word's mark before an item of its list, in a run of its own.)
            context.marking = true;
        }
        context.style = styled(node, &name, context.style);
        let in_code = matches!(self.holders.last(), Some(Holder::Code(_)));
        let in_line = matches!(self.holders.last(), Some(Holder::Line(_)));
        match name.as_str() {
            "br" => {
                if self.inside {
                    self.piece(Piece::Break);
                }
                return None;
            }
            "img" => {
                self.image(node, &context);
                return None;
            }
            "input" => {
                self.checkbox(node);
                return None;
            }
            "hr" => {
                if in_line {
                    self.piece(Piece::Break);
                } else if self.inside && !in_code {
                    self.put(Block::Lines(vec!["---".to_string()], Shape::Rule));
                }
                return None;
            }
            "a" => {
                if let Some(href) = attribute(node, "href").filter(|href| web(href) || href.trim().to_ascii_lowercase().starts_with("mailto:")) {
                    context.link = Some(Rc::from(href.trim()));
                }
            }
            "code" | "kbd" | "samp" | "tt" => {
                context.code = true;
                if let Some(Holder::Code(code)) = self.holders.last_mut() {
                    if code.language.is_empty() {
                        code.language = language(node);
                    }
                }
            }
            _ => {}
        }
        if in_code || kind(&name) == Kind::Inline {
            return frame(context, End::Nothing);
        }
        // A block among words on one line (a heading's, a cell's): its words apart from those round it.
        if in_line {
            self.piece(Piece::Break);
            return frame(context, End::Break);
        }
        if let Some(level) = word_level(node) {
            self.flow();
            if let Some(Holder::Flow(flow)) = self.holders.last_mut() {
                // (Spaces between its items leave the list going.)
                if flow.words.iter().all(Piece::blank) {
                    flow.words.clear();
                } else {
                    flow.flush(true);
                }
            }
            let list = css(node, "mso-list").unwrap_or_default().split_whitespace().filter(|word| !word.starts_with("level")).collect::<Vec<_>>().join(" ");
            return self.open(Holder::Line(Line { is: Lines::WordItem { level, list, mark: String::new() }, words: Vec::new() }), node, context);
        }
        let top_list = matches!(self.holders.last(), Some(Holder::List(_)));
        let top_table = matches!(self.holders.last(), Some(Holder::Table(_)));
        match name.as_str() {
            heading @ ("h1" | "h2" | "h3" | "h4" | "h5" | "h6") => {
                let level = usize::from(heading.as_bytes()[1] - b'0');
                self.flow();
                self.bound();
                context.heads = true;
                self.open(Holder::Line(Line { is: Lines::Heading(level), words: Vec::new() }), node, context)
            }
            list @ ("ul" | "ol") => {
                self.flow();
                self.bound();
                let start = attribute(node, "start").and_then(|start| start.trim().parse::<u64>().ok()).unwrap_or(1);
                self.open(Holder::List(List { ordered: list == "ol", start, items: Vec::new() }), node, context)
            }
            "li" if top_list || self.in_implicit() => {
                while self.in_implicit() {
                    self.close();
                }
                let keep = self.inside;
                self.open(Holder::Flow(Flow::new(Flows::Item, keep, false)), node, context)
            }
            "blockquote" => {
                self.flow();
                self.bound();
                let keep = self.inside;
                self.open(Holder::Flow(Flow::new(Flows::Quote, keep, false)), node, context)
            }
            "pre" => {
                self.flow();
                self.bound();
                self.open(Holder::Code(Code { text: String::new(), language: language(node) }), node, context)
            }
            "table" => {
                self.flow();
                self.bound();
                self.open(Holder::Table(Table::default()), node, context)
            }
            "tr" if top_table => {
                let keep = self.inside;
                self.open(Holder::Row(Row { cells: Vec::new(), keep }), node, context)
            }
            "td" | "th" if matches!(self.holders.last(), Some(Holder::Row(_))) => {
                let number = |name: &str| attribute(node, name).and_then(|value| value.trim().parse::<usize>().ok());
                let across = number("colspan").unwrap_or(1).clamp(1, 1000);
                let down = match number("rowspan").unwrap_or(1).min(65534) {
                    0 => usize::MAX,
                    down => down,
                };
                let align = attribute(node, "align").map(|align| align.to_ascii_lowercase()).or_else(|| css(node, "text-align"));
                let align = match align.as_deref() {
                    Some("center") => ":-:",
                    Some("right" | "end") => "--:",
                    Some("left" | "start") => ":--",
                    _ => "---",
                };
                // (A head's cell not marked bold: a table's head is drawn bold as it is.)
                context.heads |= name == "th";
                self.open(Holder::Line(Line { is: Lines::Cell { span: (across, down), align }, words: Vec::new() }), node, context)
            }
            "caption" if top_table => self.open(Holder::Line(Line { is: Lines::Caption, words: Vec::new() }), node, context),
            _ => {
                self.bound();
                frame(context, End::Bound)
            }
        }
    }

    /// `holder` opened for `node`, closed when it is.
    fn open(&mut self, holder: Holder, node: &Handle, context: Context) -> Option<Frame> {
        self.holders.push(holder);
        Some(Frame { node: node.clone(), next: 0, context, holder: Some(self.holders.len() - 1), end: End::Nothing })
    }

    /// The element of `frame` ended: what it ends, the holders opened in it closed.
    fn leave(&mut self, frame: &Frame) {
        match frame.end {
            End::Bound => self.bound(),
            End::Break => self.piece(Piece::Break),
            End::Nothing => {}
        }
        if let Some(at) = frame.holder {
            while self.holders.len() > at {
                self.close();
            }
        }
    }

    /// Whether the innermost holder is an item begun where none was (see `Flow::implicit`).
    fn in_implicit(&self) -> bool {
        matches!(self.holders.last(), Some(Holder::Flow(Flow { implicit: true, .. })))
    }

    /// In a list but in none of its items, its last item gone on with (or one begun): what is read goes in it.
    fn flow(&mut self) {
        if let Some(Holder::List(list)) = self.holders.last_mut() {
            let item = list.items.pop();
            let keep = item.is_some();
            let item = item.unwrap_or_default();
            let mut flow = Flow::new(Flows::Item, keep, true);
            flow.task = item.task;
            flow.blocks = item.blocks;
            self.holders.push(Holder::Flow(flow));
        }
    }

    /// A paragraph ended (where words are a paragraph).
    fn bound(&mut self) {
        if let Some(Holder::Flow(flow)) = self.holders.last_mut() {
            flow.flush(true);
        }
    }

    /// `block` put where the innermost holder has blocks.
    fn put(&mut self, block: Block) {
        self.flow();
        if let Some(Holder::Flow(flow)) = self.holders.last_mut() {
            flow.put(block);
        }
    }

    /// The text `text` read, in `context`.
    fn text(&mut self, text: &str, context: &Context) {
        if !self.inside {
            return;
        }
        let marking = self.marking || context.marking;
        match self.holders.last_mut() {
            Some(Holder::Code(code)) => code.text.push_str(text),
            Some(Holder::Line(Line { is: Lines::WordItem { mark, .. }, .. })) if marking => mark.push_str(text),
            _ if marking => {}
            _ => self.piece(Piece::Words(text.to_string(), context.format())),
        }
    }

    /// `piece` put where the innermost holder has words.
    fn piece(&mut self, piece: Piece) {
        let blank = piece.blank();
        if matches!(self.holders.last(), Some(Holder::List(_))) {
            if blank {
                return;
            }
            self.flow();
        }
        match self.holders.last_mut() {
            Some(Holder::Flow(flow)) => {
                if !blank {
                    flow.end_list();
                }
                flow.words.push(piece);
            }
            Some(Holder::Line(line)) => line.words.push(piece),
            Some(Holder::Code(code)) => match piece {
                Piece::Words(text, _) => code.text.push_str(&text),
                Piece::Break => code.text.push('\n'),
            },
            _ => {}
        }
    }

    /// An image read: a link to it, named as its `alt` says (else its file's name). In a link, its name alone (the
    /// link goes where that one does). A picture not on the web, its `alt` alone.
    fn image(&mut self, node: &Handle, context: &Context) {
        if !self.inside {
            return;
        }
        let alt = attribute(node, "alt").unwrap_or_default();
        let src = attribute(node, "src").filter(|src| web(src)).map(|src| src.trim().to_string());
        let named = |src: &str| if alt.trim().is_empty() { src.rsplit('/').find(|part| !part.is_empty()).unwrap_or(src).to_string() } else { alt.clone() };
        let mut format = context.format();
        let words = match (&context.link, src) {
            (Some(_), src) => src.map_or_else(|| alt.clone(), |src| named(&src)),
            (None, Some(src)) => {
                let words = named(&src);
                format.link = Some(Rc::from(src.as_str()));
                words
            }
            (None, None) => alt.clone(),
        };
        if !words.trim().is_empty() {
            self.piece(Piece::Words(words, format));
        }
    }

    /// A checkbox read: before an item's words, the item's to-do's box.
    fn checkbox(&mut self, node: &Handle) {
        if !self.inside || !attribute(node, "type").is_some_and(|kind| kind.eq_ignore_ascii_case("checkbox")) {
            return;
        }
        if let Some(Holder::Flow(flow)) = self.holders.last_mut() {
            if flow.is == Flows::Item && flow.task.is_none() && flow.blocks.is_empty() && flow.words.iter().all(Piece::blank) {
                flow.task = Some(attribute(node, "checked").is_some());
            }
        }
    }

    /// The innermost holder closed: what it holds written into the one round it.
    fn close(&mut self) {
        let Some(holder) = self.holders.pop() else { return };
        match holder {
            Holder::Flow(mut flow) => {
                flow.flush(false);
                match flow.is {
                    Flows::Quote => {
                        let lines = joined(&flow.blocks, false);
                        if !lines.is_empty() {
                            let lines = lines.into_iter().map(|line| if line.is_empty() { ">".to_string() } else { format!("> {line}") }).collect();
                            self.put(Block::Lines(lines, Shape::Quote));
                        }
                    }
                    Flows::Item => {
                        if let Some(Holder::List(list)) = self.holders.last_mut() {
                            if flow.keep || flow.task.is_some() || !flow.blocks.is_empty() {
                                list.items.push(Item { task: flow.task, blocks: flow.blocks });
                            }
                        }
                    }
                    Flows::Page => {}
                }
            }
            Holder::Line(line) => match line.is {
                Lines::Heading(level) => {
                    let words = heading_words(&line.words);
                    if !words.is_empty() {
                        self.put(Block::Lines(vec![format!("{} {words}", "#".repeat(level))], Shape::Heading));
                    }
                }
                Lines::Cell { span, align } => {
                    if let Some(Holder::Row(row)) = self.holders.last_mut() {
                        row.cells.push(Cell { words: one_line(&line.words).replace('|', "\\|"), span, align });
                    }
                }
                Lines::Caption => {
                    let lines = lines(&line.words);
                    if let Some(Holder::Table(table)) = self.holders.last_mut() {
                        if !lines.is_empty() {
                            table.before.push((lines, Shape::Paragraph));
                        }
                    }
                }
                Lines::WordItem { level, list, mark } => {
                    if let Some(Holder::Flow(flow)) = self.holders.last_mut() {
                        flow.listed.push(WordItem { level, list, mark, words: line.words });
                    }
                }
            },
            Holder::List(list) => {
                if !list.items.is_empty() {
                    self.put(Block::List(list));
                }
            }
            Holder::Table(table) => {
                for (lines, shape) in table.written() {
                    self.put(Block::Lines(lines, shape));
                }
            }
            Holder::Row(row) => {
                if let Some(Holder::Table(table)) = self.holders.last_mut() {
                    if row.keep || row.cells.iter().any(|cell| !cell.words.is_empty()) {
                        table.rows.push(row.cells);
                    }
                }
            }
            Holder::Code(code) => {
                if let Some(lines) = code.written() {
                    self.put(Block::Lines(lines, Shape::Code));
                }
            }
        }
    }

    /// The page written: its blocks, a blank line between each two. A paragraph alone is words among others (pasted
    /// where the caret is), the spaces at its ends kept (one each) where they were words of no block's.
    fn finish(self) -> Pasted {
        let Some(Holder::Flow(mut page)) = self.holders.into_iter().next() else { unreachable!("the page is read into its flow") };
        let spaced = |piece: Option<&Piece>, first: bool| match piece {
            Some(Piece::Words(text, _)) => if first { text.starts_with(char::is_whitespace) } else { text.ends_with(char::is_whitespace) },
            _ => false,
        };
        let edges = page.blocks.is_empty() && page.listed.is_empty();
        let (before, after) = (spaced(page.words.first(), true), spaced(page.words.last(), false));
        page.flush(false);
        let text = joined(&page.blocks, false).join("\n");
        let alone = page.blocks.len() == 1 && page.blocks[0].1 == Shape::Paragraph;
        if alone && edges && !page.bounded {
            let space = |spaced: bool| if spaced { " " } else { "" };
            return Pasted { text: format!("{}{text}{}", space(before), space(after)), blocks: false };
        }
        Pasted { blocks: !alone && !text.is_empty(), text }
    }
}

/// Word's list's items `items` as lists: each level in from the item it is under, numbered where Word's mark is a
/// number or a letter (from the number it is, from 1 for a letter), Word's own mark left out, a list begun anew where
/// Word's begins another.
fn word_lists(items: Vec<WordItem>) -> Vec<Block> {
    let mut made = Vec::new();
    // (Each list open, the innermost last, and its level.)
    let mut open: Vec<(usize, List)> = Vec::new();
    // (The innermost list closed: under the last item of the one round it, or made where it is in none.)
    let shut = |open: &mut Vec<(usize, List)>, made: &mut Vec<Block>| {
        let (_, list) = open.pop().expect("a list open");
        match open.last_mut() {
            Some((_, round)) => place(&mut round.items.last_mut().expect("a list open has an item").blocks, Block::List(list)),
            None => made.push(Block::List(list)),
        }
    };
    let mut was: Option<String> = None;
    for item in items {
        let (ordered, number) = word_mark(&item.mark);
        if was.as_ref() != Some(&item.list) {
            while !open.is_empty() {
                shut(&mut open, &mut made);
            }
            was = Some(item.list.clone());
        }
        while open.last().is_some_and(|(level, _)| *level > item.level) {
            shut(&mut open, &mut made);
        }
        if open.last().is_some_and(|(level, list)| *level == item.level && list.ordered != ordered) {
            shut(&mut open, &mut made);
        }
        if open.last().is_none_or(|(level, _)| *level < item.level) {
            open.push((item.level, List { ordered, start: number.unwrap_or(1), items: Vec::new() }));
        }
        let lines = lines(&item.words);
        let blocks = if lines.is_empty() { Vec::new() } else { vec![(lines, Shape::Paragraph)] };
        open.last_mut().expect("a list open").1.items.push(Item { task: None, blocks });
    }
    while !open.is_empty() {
        shut(&mut open, &mut made);
    }
    made
}

/// What Word's mark before an item says: whether it is a number (or a letter: `1.`, `a)`, `(3)`, `一、`), and the
/// number where it is one.
fn word_mark(mark: &str) -> (bool, Option<u64>) {
    let mark = mark.trim_matches(char::is_whitespace);
    let mark = mark.strip_prefix(['(', '（']).unwrap_or(mark);
    match mark.strip_suffix(['.', ')', '、', '．', '）']) {
        Some(body) if !body.is_empty() && body.chars().count() <= 4 && body.chars().all(char::is_alphanumeric) => (true, body.parse().ok()),
        _ => (false, None),
    }
}

/// A run of words written one way.
struct Run {
    text: String,
    format: Format,
}

/// `words` laid out as a page lays them out, at their line breaks: each run of spaces (and line breaks in the words)
/// one space, a no-break space a space, none at a line's ends.
fn laid(words: &[Piece]) -> Vec<Vec<Run>> {
    let mut lines: Vec<Vec<Run>> = vec![Vec::new()];
    let mut spaced = false;
    let push = |line: &mut Vec<Run>, c: char, format: &Format| match line.last_mut() {
        Some(run) if run.format == *format => run.text.push(c),
        _ => line.push(Run { text: c.to_string(), format: format.clone() }),
    };
    for piece in words {
        match piece {
            Piece::Break => {
                lines.push(Vec::new());
                spaced = false;
            }
            Piece::Words(text, format) => {
                for c in text.chars() {
                    if matches!(c, ' ' | '\t' | '\n' | '\r' | '\u{c}') {
                        spaced = true;
                        continue;
                    }
                    let line = lines.last_mut().expect("a line");
                    if std::mem::take(&mut spaced) && !line.is_empty() {
                        push(line, ' ', format);
                    }
                    push(line, if c == '\u{a0}' { ' ' } else { c }, format);
                }
            }
        }
    }
    for line in &mut lines {
        while let Some(run) = line.first_mut().filter(|run| !run.format.code) {
            let spaces = run.text.len() - run.text.trim_start_matches(' ').len();
            run.text.drain(..spaces);
            if !run.text.is_empty() {
                break;
            }
            line.remove(0);
        }
        while let Some(run) = line.last_mut().filter(|run| !run.format.code) {
            let words = run.text.trim_end_matches(' ').len();
            run.text.truncate(words);
            if !run.text.is_empty() {
                break;
            }
            line.pop();
        }
    }
    lines
}

/// The paragraph `words` make: its lines, what would begin a block at a line's start escaped, the empty ones left out.
fn lines(words: &[Piece]) -> Vec<String> {
    laid(words).iter().map(|runs| escape_line(&written(runs))).filter(|line| !line.is_empty()).collect()
}

/// `words` on one line (its line breaks spaces).
fn one_line(words: &[Piece]) -> String {
    let flat: Vec<Piece> = words.iter().map(|piece| if let Piece::Break = piece { Piece::Words(" ".to_string(), Format::default()) } else { piece.clone() }).collect();
    laid(&flat).first().map(|runs| written(runs)).unwrap_or_default()
}

/// A heading's words on one line, `#` marks at their end escaped (they would be read as closing it).
fn heading_words(words: &[Piece]) -> String {
    let words = one_line(words);
    let before = words.trim_end_matches('#');
    if before.len() < words.len() && (before.is_empty() || before.ends_with(' ')) {
        return format!("{before}\\{}", &words[before.len()..]);
    }
    words
}

/// A mark round words: a link's, struck, bold, slanted.
#[derive(Clone, PartialEq)]
enum Mark {
    Link(Rc<str>),
    Struck,
    Bold,
    Italic,
}

impl Mark {
    fn opening(&self) -> &'static str {
        match self {
            Mark::Link(_) => "[",
            Mark::Struck => "~~",
            Mark::Bold => "**",
            Mark::Italic => "*",
        }
    }

    fn closing(&self) -> String {
        match self {
            Mark::Link(to) => format!("]({})", destination(to)),
            other => other.opening().to_string(),
        }
    }
}

/// The marks words written `format` are in.
fn marks(format: &Format) -> Vec<Mark> {
    let mut marks = Vec::new();
    if let Some(link) = &format.link {
        marks.push(Mark::Link(link.clone()));
    }
    for (on, mark) in [(format.struck, Mark::Struck), (format.bold, Mark::Bold), (format.italic, Mark::Italic)] {
        if on {
            marks.push(mark);
        }
    }
    marks
}

/// A line's runs written: each mark opened where the words begin to be in it and closed where they end (runs set the
/// same way one after another in one, spaces between runs in those round them), those on longer opened first (so
/// one ending closes none on after it), spaces at a mark's ends outside it. Their words escaped, code as code.
fn written(runs: &[Run]) -> String {
    let mut out = String::new();
    let mut open: Vec<Mark> = Vec::new();
    let blank = |run: &Run| !run.format.code && run.text.trim_matches(' ').is_empty();
    let close = |out: &mut String, open: &mut Vec<Mark>, from: usize| {
        if from >= open.len() {
            return;
        }
        let spaces = out.len() - out.trim_end_matches(' ').len();
        out.truncate(out.len() - spaces);
        for mark in open.drain(from..).rev() {
            out.push_str(&mark.closing());
        }
        out.push_str(&" ".repeat(spaces));
    };
    for (at, run) in runs.iter().enumerate() {
        if blank(run) {
            out.push_str(&run.text);
            continue;
        }
        let wanted = marks(&run.format);
        if let Some(from) = open.iter().position(|mark| !wanted.contains(mark)) {
            close(&mut out, &mut open, from);
        }
        let lasting = |mark: &Mark| runs[at..].iter().take_while(|run| blank(run) || marks(&run.format).contains(mark)).count();
        let mut opened: Vec<Mark> = wanted.into_iter().filter(|mark| !open.contains(mark)).collect();
        opened.sort_by_key(|mark| std::cmp::Reverse(lasting(mark)));
        let words = if run.format.code {
            run.text.as_str()
        } else {
            let words = run.text.trim_start_matches(' ');
            out.push_str(&run.text[..run.text.len() - words.len()]);
            words
        };
        for mark in &opened {
            out.push_str(mark.opening());
        }
        open.extend(opened);
        out.push_str(&if run.format.code { code_span(words) } else { escape(words) });
    }
    close(&mut out, &mut open, 0);
    out
}

/// `code` written as code: between runs of backticks longer than any in it, a space inside them where it begins or
/// ends with one (or with spaces at both ends, which would be taken off).
fn code_span(code: &str) -> String {
    let ticks = "`".repeat(longest_run(code, '`') + 1);
    let padded = code.starts_with('`') || code.ends_with('`') || code.starts_with(' ') && code.ends_with(' ') && !code.trim_matches(' ').is_empty();
    let pad = if padded { " " } else { "" };
    format!("{ticks}{pad}{code}{pad}{ticks}")
}

/// `text` with what would read as Markdown's marks escaped: `\`, `*`, `` ` ``, `[`, `]`, `<`, `~` always, `_` where it
/// could begin or end emphasis (not within a word, as `snake_case`), `&` where it begins what would be read as a
/// character's name or number (an entity).
fn escape(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut escaped = String::with_capacity(text.len());
    for (at, &c) in chars.iter().enumerate() {
        let word = |at: Option<usize>| at.and_then(|at| chars.get(at)).is_some_and(|c| c.is_alphanumeric());
        let marks = match c {
            '\\' | '*' | '`' | '[' | ']' | '<' | '~' => true,
            '_' => !(word(at.checked_sub(1)) && word(Some(at + 1))),
            '&' => entity(&chars[at + 1..]),
            _ => false,
        };
        if marks {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// Whether `rest` (what follows a `&`) is what Markdown reads as a character's name or number: letters and digits, or
/// `#` and digits, or `#x` and hexadecimal digits, then a semicolon.
fn entity(rest: &[char]) -> bool {
    let end = if rest.first() == Some(&'#') {
        let hex = matches!(rest.get(1), Some('x' | 'X'));
        let from = if hex { 2 } else { 1 };
        let digits = rest.get(from..).unwrap_or_default().iter().take_while(|c| if hex { c.is_ascii_hexdigit() } else { c.is_ascii_digit() }).count();
        (digits > 0).then_some(from + digits)
    } else {
        let named = rest.iter().take_while(|c| c.is_ascii_alphanumeric()).count();
        (named > 0).then_some(named)
    };
    end.is_some_and(|end| rest.get(end) == Some(&';'))
}

/// A paragraph's `line` with what would make it a block, or end the paragraph above it, escaped: a line of `=` or of
/// `-` (a heading's underline, a rule), a table's delimiter row, a heading's `#`, a quote's `>`, a list's `-`, `+` or
/// number.
fn escape_line(line: &str) -> String {
    let only = |marks: &[char]| line.chars().all(|c| c == ' ' || marks.contains(&c));
    if line.contains('=') && only(&['=']) || line.contains('-') && only(&['-', ':', '|']) {
        let at = line.find(['=', '-']).expect("a mark");
        return format!("{}\\{}", &line[..at], &line[at..]);
    }
    let spaced_after = |rest: &str| rest.is_empty() || rest.starts_with(' ');
    let hashes = line.len() - line.trim_start_matches('#').len();
    if (1..=6).contains(&hashes) && spaced_after(&line[hashes..]) || line.starts_with('>') {
        return format!("\\{line}");
    }
    if (line.starts_with('-') || line.starts_with('+')) && spaced_after(&line[1..]) {
        return format!("\\{line}");
    }
    let digits = line.len() - line.trim_start_matches(|c: char| c.is_ascii_digit()).len();
    if (1..=9).contains(&digits) && (line[digits..].starts_with('.') || line[digits..].starts_with(')')) && spaced_after(&line[digits + 1..]) {
        return format!("{}\\{}", &line[..digits], &line[digits..]);
    }
    line.to_string()
}

/// Whether `address` is a web page's (`http://`, `https://`).
fn web(address: &str) -> bool {
    let lower = address.trim().to_ascii_lowercase();
    lower.starts_with("http://") || lower.starts_with("https://")
}

/// `address` as a link's destination: in `<>` where it has spaces or brackets in it.
fn destination(address: &str) -> String {
    let address = address.trim();
    if address.contains([' ', '(', ')', '<', '>']) {
        format!("<{}>", address.replace('<', "%3C").replace('>', "%3E"))
    } else {
        address.to_string()
    }
}

/// How many of `c` are together at most in `text`.
fn longest_run(text: &str, c: char) -> usize {
    text.split(|other| other != c).map(str::len).max().unwrap_or(0)
}

#[cfg(test)]
mod tests {
    use super::*;

    /// `html` (all of it) written as Markdown.
    fn markdown(html: &str) -> Pasted {
        read(html, true)
    }

    fn text(html: &str) -> String {
        markdown(html).text
    }

    /// The clipboard's `HTML Format`: `before`, `fragment` and `after` the page round it, as Windows writes one.
    fn clipboard(before: &str, fragment: &str, after: &str) -> Vec<u8> {
        let header_of = |offsets: [usize; 4]| format!("Version:0.9\r\nStartHTML:{:010}\r\nEndHTML:{:010}\r\nStartFragment:{:010}\r\nEndFragment:{:010}\r\n", offsets[0], offsets[1], offsets[2], offsets[3]);
        let length = header_of([0; 4]).len();
        let offsets = [length, length + before.len() + fragment.len() + after.len(), length + before.len(), length + before.len() + fragment.len()];
        format!("{}{before}{fragment}{after}", header_of(offsets)).into_bytes()
    }

    /// The fragment of the clipboard's `HTML Format`, cut where its header says (bytes, Chinese among them), read in
    /// the page round it: a table's row copied alone a table's row still.
    #[test]
    fn takes_the_fragment_in_its_page() {
        let bytes = clipboard("<html><body><p>不要</p><!--StartFragment-->", "<b>粗</b>", "<!--EndFragment--><p>也不要</p></body></html>");
        assert_eq!(pasted(&bytes), Some(Pasted { text: "**粗**".into(), blocks: false }));
        let bytes = clipboard("<html><body><table><tbody><!--StartFragment-->", "<tr><td>A</td><td>B</td></tr>", "<!--EndFragment--></tbody></table></body></html>");
        assert_eq!(pasted(&bytes).map(|pasted| pasted.text).as_deref(), Some("| A | B |\n| --- | --- |"));
        assert_eq!(pasted(b"no header"), None);
    }

    /// A page copied from a browser: its headings, paragraphs, bold, slanted and struck words, links, code, a line
    /// break, a rule, an image, and what it does not know only its words.
    #[test]
    fn writes_a_page_s_words() {
        let html = r#"<h2>标题 <b>二</b></h2><p>一段 <strong>粗体</strong>和<em>斜体</em>，<del>删去</del>，<a href="https://a.b/c">链接</a>，<code>a_b</code><br>下一行</p><hr><p><img src="https://a.b/p.png" alt="图"><span class="x">别的</span><script>x()</script></p>"#;
        assert_eq!(markdown(html), Pasted { text: "## 标题 二\n\n一段 **粗体**和*斜体*，~~删去~~，[链接](https://a.b/c)，`a_b`\n下一行\n\n---\n\n[图](https://a.b/p.png)别的".into(), blocks: true });
    }

    /// Lists, nested, numbered from where they start, to-dos with their boxes, a quote, a code block with its language.
    #[test]
    fn writes_lists_quotes_and_code() {
        let html = r#"<ul><li>一<ul><li>一一</li></ul></li><li><input type="checkbox" checked> 做完</li><li><input type="checkbox"> 没做</li></ul><ol start="3"><li>三</li><li>四</li></ol><blockquote><p>引一</p><p>引二</p></blockquote><pre><code class="language-rust">fn a() {
    b();
}</code></pre>"#;
        assert_eq!(text(html), "- 一\n  - 一一\n- [x] 做完\n- [ ] 没做\n\n3. 三\n4. 四\n\n> 引一\n>\n> 引二\n\n```rust\nfn a() {\n    b();\n}\n```");
    }

    /// An item's blocks a blank line apart where the one after would not be read as its own (a list numbered from
    /// three under a paragraph, a paragraph after a paragraph), two lists one after another told apart.
    #[test]
    fn keeps_an_item_s_blocks_apart() {
        assert_eq!(text(r#"<ul><li><p>前言</p><ol start="3"><li>三</li></ol></li></ul>"#), "- 前言\n\n  3. 三");
        assert_eq!(text("<ul><li><p>一</p><p>二</p></li></ul>"), "- 一\n\n  二");
        assert_eq!(text("<ul><li>甲</li></ul><ul><li>乙</li></ul><ol><li>丙</li></ol><ol><li>丁</li></ol>"), "- 甲\n\n* 乙\n\n1. 丙\n\n1) 丁");
    }

    /// A table: its head's row first, each column set as its head says, a cell's `|` escaped, a short row filled out,
    /// cells over several columns or rows in the first of them.
    #[test]
    fn writes_a_table() {
        let html = r#"<table><thead><tr><th>名</th><th align="center">数</th><th style="text-align: right">注</th></tr></thead><tbody><tr><td>a|b</td><td>1</td><td><p>x</p><p>y</p></td></tr><tr><td>c</td></tr></tbody></table>"#;
        assert_eq!(text(html), "| 名 | 数 | 注 |\n| --- | :-: | --: |\n| a\\|b | 1 | x y |\n| c |  |  |");
        let html = r#"<table><tr><th>A</th><th>B</th><th>C</th></tr><tr><td colspan="2">合</td><td>99</td></tr><tr><td rowspan="2">竖</td><td>1</td><td>2</td></tr><tr><td>3</td><td>4</td></tr></table>"#;
        assert_eq!(text(html), "| A | B | C |\n| --- | --- | --- |\n| 合 |  | 99 |\n| 竖 | 1 | 2 |\n|  | 3 | 4 |");
    }

    /// Words set bold, slanted or struck by their `style` (as Word and many pages set them), not their tags, a block's
    /// style its words', a style taken off where it is. What would read as Markdown in the words escaped, at a line's
    /// start and anywhere.
    #[test]
    fn writes_styles_and_escapes_marks() {
        let html = r#"<p><span style="font-weight:700">粗</span> <span style="font-style:italic">斜</span> <span style="text-decoration: line-through">删</span></p><p># 不是标题</p><p>1. 不是列表</p><p>a*b* [c] _d_ e_f &lt;g&gt;</p>"#;
        assert_eq!(text(html), "**粗** *斜* ~~删~~\n\n\\# 不是标题\n\n1\\. 不是列表\n\na\\*b\\* \\[c\\] \\_d\\_ e_f \\<g>");
        assert_eq!(text(r#"<p style="font-weight:700">整段</p>"#), "**整段**");
        assert_eq!(text(r#"<b>A<span style="font-weight:400">B</span>C</b>"#), "**A**B**C**");
        assert_eq!(text("<b>A</b><b>B</b> <b>C</b>"), "**AB C**");
        assert_eq!(text("<b><i>斜粗</i>粗</b>"), "***斜粗*粗**");
        assert_eq!(text("<p>---</p><p>A<br>===</p><p>&amp;copy; AT&amp;T</p><p>| - |</p>"), "\\---\n\nA\n\\===\n\n\\&copy; AT&T\n\n| \\- |");
    }

    /// Spaces as a page shows them: at words' ends kept for words alone (pasted among others), no-break spaces kept in
    /// code.
    #[test]
    fn keeps_spaces_as_shown() {
        assert_eq!(markdown("<span> world </span>"), Pasted { text: " world ".into(), blocks: false });
        assert_eq!(text("<p><code>a&nbsp;&nbsp;b</code> <b> 粗 </b>字</p>"), "`a  b` **粗** 字");
    }

    /// A block in what is words on a page (Google Docs' `<b style="font-weight:normal">` round all it copies) a block
    /// still.
    #[test]
    fn a_block_in_words_is_a_block() {
        assert_eq!(text(r#"<b style="font-weight:normal" id="docs-internal-guid-1"><h2>标题</h2><ul><li>一</li></ul><p><span style="font-weight:700">粗</span></p></b>"#), "## 标题\n\n- 一\n\n**粗**");
    }

    /// A picture in a link: its name the link's words, the link going where it does.
    #[test]
    fn a_picture_in_a_link() {
        assert_eq!(text(r#"<a href="https://site/page"><img src="https://site/p.png" alt="照片"></a>"#), "[照片](https://site/page)");
    }

    /// A page nested far deeper than any is: read without end, its words kept.
    #[test]
    fn reads_a_page_nested_without_end() {
        let deep = 20_000;
        let html = format!("{}深{}", "<span><div><blockquote>".repeat(deep), "</blockquote></div></span>".repeat(deep));
        let written = text(&html);
        assert!(written.ends_with("深"));
        assert!(written.matches('>').count() <= DEEPEST);
    }

    /// Word's list (paragraphs its `mso-list` says are items, at their levels, Word's own mark before each): a list,
    /// numbered where Word's mark is a number (from the number it is), nested by level, the mark left out, another of
    /// Word's lists another list.
    #[test]
    fn writes_word_s_lists() {
        let html = concat!(
            "<p class=MsoNormal>前</p>\n",
            "<p class=MsoListParagraphCxSpFirst style='mso-list:l0 level1 lfo1'><![if !supportLists]><span style='mso-list:Ignore'>1.<span style='font:7.0pt \"Times New Roman\"'>&nbsp;&nbsp; </span></span><![endif]>第一</p>\n",
            "<p class=MsoListParagraphCxSpMiddle style='mso-list:l0 level2 lfo1'><![if !supportLists]><span style='font-family:Symbol'><span style='mso-list:Ignore'>·<span>&nbsp;&nbsp;</span></span></span><![endif]><b>其中</b></p>\n",
            "<p class=MsoListParagraphCxSpLast style='mso-list:l0 level1 lfo1'><![if !supportLists]><span style='mso-list:Ignore'>2.<span>&nbsp;</span></span><![endif]>第二</p>\n",
            "<p class=MsoNormal>后</p>",
        );
        assert_eq!(text(html), "前\n\n1. 第一\n   - **其中**\n2. 第二\n\n后");
        let html = concat!(
            "<p style='mso-list:l0 level1 lfo1'><![if !supportLists]><span style='mso-list:Ignore'>7.<span>&nbsp;</span></span><![endif]>七</p>\n",
            "<p style='mso-list:l0 level1 lfo1'><![if !supportLists]><span style='mso-list:Ignore'>8.<span>&nbsp;</span></span><![endif]>八</p>\n",
            "<p style='mso-list:l0 level1 lfo2'><![if !supportLists]><span style='mso-list:Ignore'>1.<span>&nbsp;</span></span><![endif]>另一</p>",
        );
        assert_eq!(text(html), "7. 七\n8. 八\n\n1) 另一");
    }

    /// Words alone (a few copied from a line) are pasted as they are: no paragraph's breaks round them. A paragraph
    /// alone is words too, blocks are blocks.
    #[test]
    fn words_alone_stay_words() {
        assert_eq!(markdown("<span>几个字</span>"), Pasted { text: "几个字".into(), blocks: false });
        assert_eq!(markdown("<p>一段</p>"), Pasted { text: "一段".into(), blocks: false });
        assert_eq!(markdown("<h1>标题</h1>"), Pasted { text: "# 标题".into(), blocks: true });
        assert_eq!(markdown(""), Pasted { text: String::new(), blocks: false });
    }
}
