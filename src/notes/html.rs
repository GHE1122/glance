//! What is pasted into a note as HTML (a web page's, Word's: see docs/station-and-notes.md, section 九 8), written as
//! the Markdown a note is: headings, bold, slanted and struck words, links, code, code blocks, lists (nested, and
//! to-dos), quotes, tables, rules, paragraphs and line breaks. What it does not know only its words are kept of, an
//! image as a link to it, and what would read as Markdown's marks in its words is escaped.

use std::cell::{Cell, RefCell};

use html5ever::tendril::TendrilSink;
use markup5ever_rcdom::{Handle, NodeData, RcDom};

/// The fragment the clipboard's `HTML Format` holds, `clipboard` its bytes: from its header's `StartFragment` to its
/// `EndFragment` (UTF-8 byte offsets from its start). None where its header says none.
pub fn fragment(clipboard: &[u8]) -> Option<String> {
    // (Its header is ASCII lines, before anything else.)
    let head = String::from_utf8_lossy(&clipboard[..clipboard.len().min(1024)]);
    let offset = |key: &str| head.lines().find_map(|line| line.strip_prefix(key)).and_then(|value| value.trim().parse::<usize>().ok());
    let (start, end) = (offset("StartFragment:")?, offset("EndFragment:")?);
    (start <= end && end <= clipboard.len()).then(|| String::from_utf8_lossy(&clipboard[start..end]).into_owned())
}

/// `html` written as Markdown (its line breaks `\n`, nothing before or after it).
pub fn markdown(html: &str) -> String {
    let dom = html5ever::parse_document(RcDom::default(), Default::default()).one(html);
    let reader = Reader::default();
    let blocks = reader.blocks(&dom.document.children.borrow(), Style::default());
    blocks.into_iter().map(|block| block.join("\n")).collect::<Vec<_>>().join("\n\n")
}

/// A block written: its lines.
type Block = Vec<String>;

/// How words are set where they are, as what holds them says (its tags, its `style`).
#[derive(Clone, Copy, Default, PartialEq)]
struct Style {
    bold: bool,
    italic: bool,
    struck: bool,
}

/// Reading a page's tree: Word's mark before a list's item (its number or bullet, written in what only Word's own
/// lists leave out) taken aside, not written, while it is read.
#[derive(Default)]
struct Reader {
    marking: Cell<bool>,
    mark: RefCell<String>,
}

/// What an element is to its Markdown: left out, a block of its own, or words among others.
#[derive(PartialEq)]
enum Kind {
    Out,
    Block,
    Inline,
}

/// What a node is (see `Kind`): text and what it does not know are words.
fn kind(node: &Handle) -> Kind {
    match tag(node).as_deref() {
        Some("head" | "title" | "meta" | "link" | "script" | "style" | "template" | "noscript" | "iframe" | "object" | "svg" | "button" | "select" | "textarea") => Kind::Out,
        Some(
            "html" | "body" | "p" | "div" | "section" | "article" | "header" | "footer" | "main" | "nav" | "aside" | "figure" | "figcaption" | "address" | "center" | "details" | "summary" | "fieldset" | "form" | "dl" | "dt" | "dd" | "li" | "h1"
            | "h2" | "h3" | "h4" | "h5" | "h6" | "ul" | "ol" | "blockquote" | "pre" | "table" | "hr",
        ) => Kind::Block,
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

/// How the words in `node` are set, in what is set `style`: its tag's way, and its `style`'s.
fn styled(node: &Handle, style: Style) -> Style {
    let mut style = style;
    match tag(node).as_deref() {
        Some("b" | "strong") => style.bold = true,
        Some("i" | "em" | "cite" | "var" | "dfn") => style.italic = true,
        Some("s" | "strike" | "del") => style.struck = true,
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

/// Whether `node` holds nothing written: spaces, or a comment.
fn blank(node: &Handle) -> bool {
    match &node.data {
        NodeData::Text { contents } => contents.borrow().trim().is_empty(),
        NodeData::Comment { .. } => true,
        _ => false,
    }
}

impl Reader {
    /// The blocks `nodes` make, in what is set `style`: the words among them a paragraph (each run between blocks), each
    /// block its own, Word's list's items one list.
    fn blocks(&self, nodes: &[Handle], style: Style) -> Vec<Block> {
        let mut blocks = Vec::new();
        let mut words = String::new();
        let mut at = 0;
        while at < nodes.len() {
            let node = &nodes[at];
            if word_level(node).is_some() {
                blocks.extend(paragraph(&std::mem::take(&mut words)));
                // (The items one after another, with nothing written between them.)
                let mut items = Vec::new();
                while at < nodes.len() && (word_level(&nodes[at]).is_some() || blank(&nodes[at]) && nodes[at + 1..].iter().find(|node| !blank(node)).is_some_and(|node| word_level(node).is_some())) {
                    if word_level(&nodes[at]).is_some() {
                        items.push(nodes[at].clone());
                    }
                    at += 1;
                }
                blocks.push(self.word_list(&items, style));
                continue;
            }
            match kind(node) {
                Kind::Out => {}
                Kind::Block => {
                    blocks.extend(paragraph(&std::mem::take(&mut words)));
                    blocks.extend(self.block(node, style));
                }
                Kind::Inline => words.push_str(&self.inline(node, style)),
            }
            at += 1;
        }
        blocks.extend(paragraph(&words));
        blocks
    }

    /// The blocks the block element `node` makes.
    fn block(&self, node: &Handle, style: Style) -> Vec<Block> {
        let style = styled(node, style);
        let children = node.children.borrow();
        match tag(node).as_deref() {
            Some(heading @ ("h1" | "h2" | "h3" | "h4" | "h5" | "h6")) => {
                let level = usize::from(heading.as_bytes()[1] - b'0');
                // (A heading's words bold already: not marked so again.)
                let words = one_line(&self.inlines(&children, Style { bold: true, ..style }));
                if words.is_empty() {
                    return Vec::new();
                }
                vec![vec![format!("{} {words}", "#".repeat(level))]]
            }
            Some(list @ ("ul" | "ol")) => {
                let block = self.list(node, list == "ol", style);
                if block.is_empty() { Vec::new() } else { vec![block] }
            }
            Some("blockquote") => {
                let inner = self.blocks(&children, style);
                if inner.is_empty() {
                    return Vec::new();
                }
                let lines = inner.join(&String::new()).into_iter().map(|line| if line.is_empty() { ">".to_string() } else { format!("> {line}") });
                vec![lines.collect()]
            }
            Some("pre") => code_block(node),
            Some("table") => {
                let block = self.table(node, style);
                if block.is_empty() { Vec::new() } else { vec![block] }
            }
            Some("hr") => vec![vec!["---".to_string()]],
            _ => self.blocks(&children, style),
        }
    }

    /// The list `node` is (numbered where `ordered`, from its `start`): each item's marker before its first line, its
    /// other lines in from where its words begin, a to-do's box where its item has one.
    fn list(&self, node: &Handle, ordered: bool, style: Style) -> Block {
        let mut number = attribute(node, "start").and_then(|start| start.trim().parse::<u64>().ok()).unwrap_or(1);
        let mut lines: Block = Vec::new();
        for child in node.children.borrow().iter() {
            match tag(child).as_deref() {
                Some("li") => {
                    let marker = if ordered { format!("{number}. ") } else { "- ".to_string() };
                    number += 1;
                    let task = checkbox(child).map(|done| if done { "[x] " } else { "[ ] " }).unwrap_or_default();
                    let inner: Block = self.blocks(&child.children.borrow(), style).concat();
                    let indent = " ".repeat(marker.len());
                    match inner.split_first() {
                        Some((first, rest)) => {
                            lines.push(format!("{marker}{task}{first}"));
                            lines.extend(rest.iter().map(|line| if line.is_empty() { String::new() } else { format!("{indent}{line}") }));
                        }
                        None => lines.push(format!("{marker}{task}").trim_end().to_string()),
                    }
                }
                // (A list right in a list, not in an item: the item before's.)
                Some(inner @ ("ul" | "ol")) => {
                    let indent = if lines.is_empty() { String::new() } else { " ".repeat(if ordered { format!("{}. ", number - 1).len() } else { 2 }) };
                    lines.extend(self.list(child, inner == "ol", style).into_iter().map(|line| format!("{indent}{line}")));
                }
                _ => {}
            }
        }
        lines
    }

    /// Word's list, its items `items` (paragraphs, each its level), as Markdown's: each level in from the item it is
    /// under, numbered where Word's mark is a number or a letter, from 1 at each level, Word's own mark left out.
    fn word_list(&self, items: &[Handle], style: Style) -> Block {
        // (Each level open: its level, how far in it is, how far its last item's words are in from it, its number.)
        let mut open: Vec<(usize, usize, usize, u64)> = Vec::new();
        let mut lines = Vec::new();
        for item in items {
            let level = word_level(item).unwrap_or(1);
            self.mark.borrow_mut().clear();
            let words = one_line(&self.inlines(&item.children.borrow(), styled(item, style)));
            let mark = self.mark.borrow().trim_matches(|c: char| c.is_whitespace()).to_string();
            let ordered = mark.len() <= 5 && (mark.ends_with('.') || mark.ends_with(')')) && mark[..mark.len().saturating_sub(1)].chars().all(|c| c.is_alphanumeric()) && mark.len() > 1;
            while open.last().is_some_and(|(at, ..)| *at > level) {
                open.pop();
            }
            if open.last().is_none_or(|(at, ..)| *at < level) {
                let indent = open.last().map_or(0, |(_, indent, width, _)| indent + width);
                open.push((level, indent, 0, 0));
            }
            let Some((_, indent, width, number)) = open.last_mut() else { continue };
            *number += 1;
            let marker = if ordered { format!("{number}. ") } else { "- ".to_string() };
            *width = marker.len();
            lines.push(format!("{}{marker}{words}", " ".repeat(*indent)).trim_end().to_string());
        }
        lines
    }

    /// The table `node` is, as GitHub's Markdown writes one: its first row its head, each column set as its head's cell
    /// says (`align`, `text-align`), as many cells in each row as in its widest.
    fn table(&self, node: &Handle, style: Style) -> Block {
        let mut rows: Vec<Vec<(String, &'static str)>> = Vec::new();
        self.rows(node, style, &mut rows);
        let columns = rows.iter().map(Vec::len).max().unwrap_or(0);
        if columns == 0 {
            return Vec::new();
        }
        let line = |cells: Vec<String>| format!("| {} |", cells.join(" | "));
        let mut lines = Vec::new();
        for (nth, row) in rows.iter().enumerate() {
            let mut cells: Vec<String> = row.iter().map(|(words, _)| words.clone()).collect();
            cells.resize(columns, String::new());
            lines.push(line(cells));
            if nth == 0 {
                let delimiters = (0..columns).map(|column| row.get(column).map_or("---", |(_, align)| align).to_string()).collect();
                lines.push(line(delimiters));
            }
        }
        lines
    }

    /// The rows in `node` (its body's, head's and foot's, not a table's in one of its cells), each cell's words on one
    /// line (`|` escaped) and its delimiter.
    fn rows(&self, node: &Handle, style: Style, rows: &mut Vec<Vec<(String, &'static str)>>) {
        for child in node.children.borrow().iter() {
            match tag(child).as_deref() {
                Some("tr") => {
                    let cells = child.children.borrow().iter().filter(|cell| matches!(tag(cell).as_deref(), Some("td" | "th"))).map(|cell| self.cell(cell, style)).collect();
                    rows.push(cells);
                }
                Some("thead" | "tbody" | "tfoot") => self.rows(child, style, rows),
                _ => {}
            }
        }
    }

    /// A table's cell: its words on one line, and its column's delimiter as it is set.
    fn cell(&self, cell: &Handle, style: Style) -> (String, &'static str) {
        // (A head's cell not marked bold: a table's head is drawn bold as it is.)
        let style = styled(cell, Style { bold: style.bold || tag(cell).as_deref() == Some("th"), ..style });
        let words = self.blocks(&cell.children.borrow(), style).concat().iter().map(|line| line.trim()).filter(|line| !line.is_empty()).collect::<Vec<_>>().join(" ");
        let align = attribute(cell, "align").map(|align| align.to_ascii_lowercase()).or_else(|| css(cell, "text-align"));
        let delimiter = match align.as_deref() {
            Some("center") => ":-:",
            Some("right" | "end") => "--:",
            Some("left" | "start") => ":--",
            _ => "---",
        };
        (words.replace('|', "\\|"), delimiter)
    }

    /// The words `nodes` make together (see `inline`).
    fn inlines(&self, nodes: &[Handle], style: Style) -> String {
        nodes.iter().map(|node| self.inline(node, style)).collect()
    }

    /// The words `node` makes among others, in what is set `style`: its spaces as one, Markdown's marks in them escaped,
    /// marked bold, slanted or struck where it begins to be, a line break `\n`, a link, code, an image as Markdown
    /// writes them. Word's list's mark taken aside (see `Reader`).
    fn inline(&self, node: &Handle, style: Style) -> String {
        match &node.data {
            NodeData::Text { contents } => {
                let contents = contents.borrow();
                if self.marking.get() {
                    self.mark.borrow_mut().push_str(&contents);
                    return String::new();
                }
                escape(&spaced(&contents))
            }
            NodeData::Comment { contents } => {
                // (Word's mark before an item of its list, between these: `<![if !supportLists]>` … `<![endif]>`.)
                match contents.trim() {
                    "[if !supportLists]" => self.marking.set(true),
                    "[endif]" => self.marking.set(false),
                    _ => {}
                }
                String::new()
            }
            NodeData::Element { .. } => self.element(node, style),
            _ => String::new(),
        }
    }

    /// The words the element `node` makes among others (see `inline`).
    fn element(&self, node: &Handle, style: Style) -> String {
        let children = node.children.borrow();
        if css(node, "mso-list").is_some_and(|list| list.starts_with("ignore")) {
            // (Word's mark before an item of its list, in a run of its own.)
            let was = self.marking.replace(true);
            let _ = self.inlines(&children, style);
            self.marking.set(was);
            return String::new();
        }
        match tag(node).as_deref() {
            _ if kind(node) == Kind::Out => String::new(),
            Some("br") => "\n".to_string(),
            Some("img") => {
                let alt = attribute(node, "alt").map(|alt| spaced(&alt).trim().to_string()).unwrap_or_default();
                match attribute(node, "src").filter(|src| web(src)) {
                    Some(src) => {
                        let named = if alt.is_empty() { src.rsplit('/').find(|part| !part.is_empty()).unwrap_or(&src).to_string() } else { alt };
                        format!("[{}]({})", escape(&named), destination(&src))
                    }
                    None => escape(&alt),
                }
            }
            Some("a") => {
                let words = self.inlines(&children, style);
                match attribute(node, "href").filter(|href| web(href) || href.to_ascii_lowercase().starts_with("mailto:")) {
                    Some(href) if !words.trim().is_empty() => around(&words, |inner| format!("[{inner}]({})", destination(&href))),
                    _ => words,
                }
            }
            Some("code" | "kbd" | "samp" | "tt") => {
                let code = spaced(&text_of(node));
                if code.trim().is_empty() {
                    return code;
                }
                around(&code, |inner| {
                    let ticks = "`".repeat(longest_run(inner, '`') + 1);
                    let pad = if inner.starts_with('`') || inner.ends_with('`') { " " } else { "" };
                    format!("{ticks}{pad}{inner}{pad}{ticks}")
                })
            }
            // A block among words (a list in a paragraph's run, a cell's paragraphs): its words on lines of their own.
            _ if kind(node) == Kind::Block => format!("\n{}\n", self.blocks(&children, style).concat().join("\n")),
            _ => {
                let now = styled(node, style);
                let mut words = self.inlines(&children, now);
                for (on, was, mark) in [(now.struck, style.struck, "~~"), (now.italic, style.italic, "*"), (now.bold, style.bold, "**")] {
                    if on && !was {
                        words = words.split('\n').map(|line| around(line, |inner| format!("{mark}{inner}{mark}"))).collect::<Vec<_>>().join("\n");
                    }
                }
                words
            }
        }
    }
}

/// The paragraph `words` make (see `Reader::inline`): its lines (at its line breaks) without spaces at their ends or
/// two together, the empty ones left out, what would begin a block at a line's start escaped. None where there are none.
fn paragraph(words: &str) -> Option<Block> {
    let lines: Block = words.split('\n').map(|line| escape_start(&one_space(line))).filter(|line| !line.is_empty()).collect();
    (!lines.is_empty()).then_some(lines)
}

/// `words` on one line (its line breaks spaces), spaces as one, none at its ends.
fn one_line(words: &str) -> String {
    one_space(&words.replace('\n', " "))
}

/// `line`'s spaces, one where there were several, none at its ends.
fn one_space(line: &str) -> String {
    line.split(' ').filter(|word| !word.is_empty()).collect::<Vec<_>>().join(" ")
}

/// `text`'s spaces (a run of any, line breaks and no-break spaces among them) each one space, as a page shows them.
fn spaced(text: &str) -> String {
    let mut spaced = String::with_capacity(text.len());
    for c in text.chars() {
        if c.is_whitespace() {
            if !spaced.ends_with(' ') {
                spaced.push(' ');
            }
        } else {
            spaced.push(c);
        }
    }
    spaced
}

/// `words` put in `made` without the spaces at their ends, those kept outside it (`**a** b`, not `**a **b`). Nothing
/// made of words that are only spaces.
fn around(words: &str, made: impl FnOnce(&str) -> String) -> String {
    let inner = words.trim_matches(' ');
    if inner.is_empty() {
        return words.to_string();
    }
    let before = &words[..words.len() - words.trim_start_matches(' ').len()];
    let after = &words[words.trim_end_matches(' ').len()..];
    format!("{before}{}{after}", made(inner))
}

/// `text` with what would read as Markdown's marks escaped: `\`, `*`, `` ` ``, `[`, `]`, `<`, `~` always, `_` where it
/// could begin or end emphasis (not within a word, as `snake_case`).
fn escape(text: &str) -> String {
    let chars: Vec<char> = text.chars().collect();
    let mut escaped = String::with_capacity(text.len());
    for (at, &c) in chars.iter().enumerate() {
        let word = |at: Option<usize>| at.and_then(|at| chars.get(at)).is_some_and(|c| c.is_alphanumeric());
        let marks = match c {
            '\\' | '*' | '`' | '[' | ']' | '<' | '~' => true,
            '_' => !(word(at.checked_sub(1)) && word(Some(at + 1))),
            _ => false,
        };
        if marks {
            escaped.push('\\');
        }
        escaped.push(c);
    }
    escaped
}

/// `line` with what would begin a block at its start escaped: a heading's `#`, a quote's `>`, a list's `-`, `+` or
/// number.
fn escape_start(line: &str) -> String {
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

/// All the text in `node`, as written (a line break `\n`).
fn text_of(node: &Handle) -> String {
    match &node.data {
        NodeData::Text { contents } => contents.borrow().to_string(),
        NodeData::Element { .. } if tag(node).as_deref() == Some("br") => "\n".to_string(),
        _ => node.children.borrow().iter().map(text_of).collect(),
    }
}

/// The code block `pre` is: its text as written between fences longer than any run of backticks in it, its language
/// as its code's class says (`language-rust`, `lang-rust`).
fn code_block(pre: &Handle) -> Vec<Block> {
    let code = text_of(pre);
    let code = code.strip_suffix('\n').unwrap_or(&code).replace("\r\n", "\n").replace('\u{a0}', " ");
    let language = std::iter::once(pre.clone())
        .chain(pre.children.borrow().iter().cloned())
        .filter_map(|node| attribute(&node, "class"))
        .flat_map(|class| class.split_whitespace().filter_map(|name| name.strip_prefix("language-").or_else(|| name.strip_prefix("lang-")).map(str::to_string)).collect::<Vec<_>>())
        .next()
        .unwrap_or_default();
    let fence = "`".repeat(longest_run(&code, '`').max(2) + 1);
    let mut lines = vec![format!("{fence}{language}")];
    lines.extend(code.split('\n').map(str::to_string));
    lines.push(fence);
    vec![lines]
}

/// The to-do's box a list's item has (a checkbox in it before its words): ticked or not. None where it has none.
fn checkbox(item: &Handle) -> Option<bool> {
    let mut found = None;
    let mut stack: Vec<Handle> = item.children.borrow().iter().rev().cloned().collect();
    while let Some(node) = stack.pop() {
        if tag(&node).as_deref() == Some("input") && attribute(&node, "type").is_some_and(|kind| kind.eq_ignore_ascii_case("checkbox")) {
            found = Some(attribute(&node, "checked").is_some());
            break;
        }
        // (Not in a list within it: that list's items' own.)
        if !matches!(tag(&node).as_deref(), Some("ul" | "ol")) {
            stack.extend(node.children.borrow().iter().rev().cloned());
        }
    }
    found
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The fragment of the clipboard's `HTML Format`, cut where its header says (bytes, Chinese among them).
    #[test]
    fn takes_the_fragment() {
        let body = "<html><body><!--StartFragment--><b>粗</b><!--EndFragment--></body></html>";
        let header_of = |start: usize, end: usize| format!("Version:0.9\r\nStartHTML:{:010}\r\nEndHTML:{:010}\r\nStartFragment:{start:010}\r\nEndFragment:{end:010}\r\n", 0, 0);
        let length = header_of(0, 0).len();
        let start = length + body.find("<b>").unwrap();
        let end = length + body.find("<!--EndFragment").unwrap();
        let clipboard = format!("{}{body}", header_of(start, end));
        assert_eq!(fragment(clipboard.as_bytes()).as_deref(), Some("<b>粗</b>"));
        assert_eq!(fragment(b"no header"), None);
    }

    /// A page copied from a browser: its headings, paragraphs, bold, slanted and struck words, links, code, a line
    /// break, a rule, an image, and what it does not know only its words.
    #[test]
    fn writes_a_page_s_words() {
        let html = r#"<h2>标题 <b>二</b></h2><p>一段 <strong>粗体</strong>和<em>斜体</em>，<del>删去</del>，<a href="https://a.b/c">链接</a>，<code>a_b</code><br>下一行</p><hr><p><img src="https://a.b/p.png" alt="图"><span class="x">别的</span><script>x()</script></p>"#;
        assert_eq!(markdown(html), "## 标题 二\n\n一段 **粗体**和*斜体*，~~删去~~，[链接](https://a.b/c)，`a_b`\n下一行\n\n---\n\n[图](https://a.b/p.png)别的");
    }

    /// Lists, nested, numbered from where they start, to-dos with their boxes, a quote, a code block with its language.
    #[test]
    fn writes_lists_quotes_and_code() {
        let html = r#"<ul><li>一<ul><li>一一</li></ul></li><li><input type="checkbox" checked> 做完</li><li><input type="checkbox"> 没做</li></ul><ol start="3"><li>三</li><li>四</li></ol><blockquote><p>引一</p><p>引二</p></blockquote><pre><code class="language-rust">fn a() {
    b();
}</code></pre>"#;
        assert_eq!(markdown(html), "- 一\n  - 一一\n- [x] 做完\n- [ ] 没做\n\n3. 三\n4. 四\n\n> 引一\n>\n> 引二\n\n```rust\nfn a() {\n    b();\n}\n```");
    }

    /// A table: its head's row first, each column set as its head says, a cell's `|` escaped, a short row filled out.
    #[test]
    fn writes_a_table() {
        let html = r#"<table><thead><tr><th>名</th><th align="center">数</th><th style="text-align: right">注</th></tr></thead><tbody><tr><td>a|b</td><td>1</td><td><p>x</p><p>y</p></td></tr><tr><td>c</td></tr></tbody></table>"#;
        assert_eq!(markdown(html), "| 名 | 数 | 注 |\n| --- | :-: | --: |\n| a\\|b | 1 | x y |\n| c |  |  |");
    }

    /// Words set bold, slanted or struck by their `style` (as Word and many pages set them), not their tags. What would
    /// read as Markdown in the words escaped, at a line's start and anywhere.
    #[test]
    fn writes_styles_and_escapes_marks() {
        let html = r#"<p><span style="font-weight:700">粗</span> <span style="font-style:italic">斜</span> <span style="text-decoration: line-through">删</span></p><p># 不是标题</p><p>1. 不是列表</p><p>a*b* [c] _d_ e_f &lt;g&gt;</p>"#;
        assert_eq!(markdown(html), "**粗** *斜* ~~删~~\n\n\\# 不是标题\n\n1\\. 不是列表\n\na\\*b\\* \\[c\\] \\_d\\_ e_f \\<g>");
    }

    /// Word's list (paragraphs its `mso-list` says are items, at their levels, Word's own mark before each): a list,
    /// numbered where Word's mark is a number, nested by level, the mark left out.
    #[test]
    fn writes_word_s_lists() {
        let html = concat!(
            "<p class=MsoNormal>前</p>\n",
            "<p class=MsoListParagraphCxSpFirst style='mso-list:l0 level1 lfo1'><![if !supportLists]><span style='mso-list:Ignore'>1.<span style='font:7.0pt \"Times New Roman\"'>&nbsp;&nbsp; </span></span><![endif]>第一</p>\n",
            "<p class=MsoListParagraphCxSpMiddle style='mso-list:l0 level2 lfo1'><![if !supportLists]><span style='font-family:Symbol'><span style='mso-list:Ignore'>·<span>&nbsp;&nbsp;</span></span></span><![endif]><b>其中</b></p>\n",
            "<p class=MsoListParagraphCxSpLast style='mso-list:l0 level1 lfo1'><![if !supportLists]><span style='mso-list:Ignore'>2.<span>&nbsp;</span></span><![endif]>第二</p>\n",
            "<p class=MsoNormal>后</p>",
        );
        assert_eq!(markdown(html), "前\n\n1. 第一\n   - **其中**\n2. 第二\n\n后");
    }

    /// Words alone (a few copied from a line) are pasted as they are: no paragraph's breaks round them.
    #[test]
    fn words_alone_stay_words() {
        assert_eq!(markdown("<span>几个字</span>"), "几个字");
        assert_eq!(markdown(""), "");
    }
}
