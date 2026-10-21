//! How a note's Markdown shows as it is written (see docs/station-and-notes.md, sections 二 and 六): what each stretch
//! of it looks like, for the box it is written in to dress it, the words themselves left as they are. Parsed with
//! pulldown-cmark, as CommonMark (and its task lists and strike-through) reads it.
//!
//! Its marks (a heading's `#`, emphasis's `*`, a list's `-`, code's `` ` ``) stay where they are, faint; what they
//! mark shows as it will read: a heading larger, emphasis bold or slanted, code in a fixed face, a link in the accent,
//! a quote and a done task faint.

use pulldown_cmark::{Event, HeadingLevel, Options, Parser, Tag, TagEnd};
use std::ops::Range;

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
    for (event, range) in Parser::new_ext(text, Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH).into_offset_iter() {
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
    // A link's words in the accent, over their plain: found as the words a link holds.
    let mut in_link = Vec::new();
    for (event, range) in Parser::new_ext(text, Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH).into_offset_iter() {
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
    for (event, range) in Parser::new_ext(text, Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH).into_offset_iter() {
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

/// The to-dos' boxes in `text` (`[ ]`, `[x]`: where each is, in UTF-16 units, and whether it is done).
pub fn tasks(text: &str) -> Vec<(Range<usize>, bool)> {
    let units = Units::of(text);
    Parser::new_ext(text, Options::ENABLE_TASKLISTS)
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
    for (event, range) in Parser::new_ext(text, Options::ENABLE_TASKLISTS | Options::ENABLE_STRIKETHROUGH).into_offset_iter() {
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
