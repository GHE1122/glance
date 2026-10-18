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

#[cfg(test)]
mod tests {
    use super::*;

    fn look_of(looks: &[(Range<usize>, Look)], at: usize) -> Vec<Look> {
        looks.iter().filter(|(range, _)| range.contains(&at)).map(|(_, look)| *look).collect()
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
}
