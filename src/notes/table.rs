//! A note's tables, as GitHub's Markdown writes them: read from its words,
//! laid out to be drawn where they are not written at (in a note shown, or written in elsewhere), the same way for the
//! box a note is written in and for its face drawn (see `ui::table`), and the cell Tab goes to as one is written.
//!
//! Everything here counts in UTF-16 units, as the text services do. A table's layout is in whatever its words' size is
//! given in (the pixels a note's words are written in, as its lines' heights are: see `markdown::line_heights`).

use std::ops::Range;

use pulldown_cmark::{Event, Parser, Tag, TagEnd};

use super::markdown::{Units, READ};

/// How a column's words are set in its cells, as its delimiter row says (`:--`, `:-:`, `--:`, none said, from the left).
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Align {
    Left,
    Centre,
    Right,
}

/// A table's cell: its words as written (their spaces round them left out: where it holds none, where they would go),
/// all of it between its pipes (one not written, its row's end), its words as they read (marks off), and whether it
/// is written (a row may leave out its last cells).
#[derive(Clone, Debug, PartialEq)]
pub struct Cell {
    pub words: Range<usize>,
    pub room: Range<usize>,
    pub reads: String,
    pub written: bool,
}

/// A table: all of it (its rows, the line break after its last left out, from its line's start where it stands on its
/// own and only spaces are before it), how each column is set, and its rows (its head's first), each as many cells as
/// it has columns. Whether it stands on its own (in no quote, no list's item: drawn laid out), and how each of its
/// lines begins (the marks of what holds it, an item's as spaces).
#[derive(Clone, Debug, PartialEq)]
pub struct Table {
    pub span: Range<usize>,
    pub aligns: Vec<Align>,
    pub rows: Vec<Vec<Cell>>,
    pub own: bool,
    pub lead: String,
}

/// The tables in `text`.
pub fn tables(text: &str) -> Vec<Table> {
    let units = Units::of(text);
    let mut tables = Vec::new();
    let mut cell: Option<(Range<usize>, String)> = None;
    // (Where the row under way ends, its line break left out: a cell not written is there.)
    let mut row_end = 0;
    // (How many quotes and items what is read is in.)
    let mut held = 0usize;
    for (event, range) in Parser::new_ext(text, READ).into_offset_iter() {
        match event {
            Event::Start(Tag::BlockQuote(_) | Tag::Item) => held += 1,
            Event::End(TagEnd::BlockQuote(_) | TagEnd::Item) => held -= 1,
            Event::Start(Tag::Table(aligns)) => {
                let written = text[range.clone()].trim_end_matches(['\n', '\r']).len();
                let own = on_its_own(text, range.start, held > 0);
                let start = own.unwrap_or(range.start);
                let line = text[..range.start].rfind('\n').map_or(0, |at| at + 1);
                let lead = text[line..range.start].chars().map(|c| if c == '>' || c.is_whitespace() { c } else { ' ' }).collect();
                let aligns = aligns
                    .iter()
                    .map(|align| match align {
                        pulldown_cmark::Alignment::Center => Align::Centre,
                        pulldown_cmark::Alignment::Right => Align::Right,
                        _ => Align::Left,
                    })
                    .collect();
                tables.push(Table { span: units.range(&(start..range.start + written)), aligns, rows: Vec::new(), own: own.is_some(), lead });
            }
            Event::Start(Tag::TableHead | Tag::TableRow) => {
                row_end = range.start + text[range.clone()].trim_end_matches(['\n', '\r']).len();
                if let Some(table) = tables.last_mut() {
                    table.rows.push(Vec::new());
                }
            }
            Event::Start(Tag::TableCell) => cell = Some((range, String::new())),
            Event::End(TagEnd::TableCell) => {
                let (Some((room, reads)), Some(row)) = (cell.take(), tables.last_mut().and_then(|table| table.rows.last_mut())) else { continue };
                // (A cell not written is read at its row's end, one with nothing between its pipes before it.)
                let left_out = room.is_empty() && room.start >= row_end;
                let room = if left_out { row_end..row_end } else { room };
                let written = &text[room.clone()];
                let start = room.start + (written.len() - written.trim_start().len());
                let words = if written.trim().is_empty() {
                    // (None: where they would be written, between the spaces.)
                    let middle = room.start + written.len() / 2;
                    middle..middle
                } else {
                    start..start + written.trim().len()
                };
                row.push(Cell { words: units.range(&words), room: units.range(&room), reads, written: !left_out });
            }
            Event::Text(words) | Event::Code(words) => {
                if let Some((_, reads)) = cell.as_mut() {
                    reads.push_str(&words);
                }
            }
            _ => {}
        }
    }
    tables
}

/// Where the line a block beginning at `at` (bytes) is on begins, where it stands on its own (not `contained` in a quote
/// or a list's item) and only spaces are before it there: from there it is put away, where it is drawn other than as
/// written. None where it is in a quote or a list's item: it is left as written.
pub(crate) fn on_its_own(text: &str, at: usize, contained: bool) -> Option<usize> {
    let start = text[..at].rfind('\n').map_or(0, |at| at + 1);
    (!contained && text[start..at].trim().is_empty()).then_some(start)
}

/// How far a cell's words are from its edges, at its sides and above and below them, over the words' size.
pub const PAD_SIDE: f32 = 0.6;
pub const PAD_TOP: f32 = 0.35;

/// A table laid out (see `lay`): each column's width and each row's height, whole, each with the line before it (its
/// left, its top), the line after the last column and the last row besides.
#[derive(Clone, Debug, PartialEq)]
pub struct Laid {
    pub columns: Vec<f32>,
    pub rows: Vec<f32>,
}

impl Laid {
    /// How wide it is, its lines with it.
    pub fn width(&self) -> f32 {
        self.columns.iter().sum::<f32>() + 1.0
    }

    /// How tall it is, its lines round it with it.
    pub fn height(&self) -> f32 {
        self.rows.iter().sum::<f32>() + 1.0
    }
}

/// `table` laid out, its words `words` high, in `room` across (its lines and the room round its cells' words, `PAD_SIDE`
/// and `PAD_TOP`, with them): each column as wide as its widest cell's words on one line, where they all fit. Where they do not, the columns narrower than a fair share of the room keep their width,
/// the rest share what is left alike (again while one of them needs less than its share), their words on as many lines
/// as they take. Each row as tall as its tallest cell's words, a line's height at least (`markdown::LEADING`).
/// `measure` says how wide and how tall words are (bold or not), on lines as wide as asked, or on one.
pub fn lay(table: &Table, words: f32, room: f32, measure: impl Fn(&str, bool, Option<f32>) -> (f32, f32)) -> Laid {
    let count = table.aligns.len();
    let (side, top) = ((PAD_SIDE * words).round(), (PAD_TOP * words).round());
    let bold = |row: usize| row == 0;
    let natural: Vec<f32> = (0..count)
        .map(|column| {
            let widest = table.rows.iter().enumerate().filter_map(|(row, cells)| cells.get(column).map(|cell| measure(&cell.reads, bold(row), None).0)).fold(0.0, f32::max);
            (widest + 2.0 * side).ceil() + 1.0
        })
        .collect();
    let room = room - 1.0;
    let columns = if natural.iter().sum::<f32>() <= room {
        natural
    } else {
        let mut kept: Vec<Option<f32>> = vec![None; count];
        loop {
            let left = room - kept.iter().flatten().sum::<f32>();
            let sharing = kept.iter().filter(|kept| kept.is_none()).count();
            let share = left / sharing.max(1) as f32;
            let mut more = false;
            for column in 0..count {
                if kept[column].is_none() && natural[column] <= share {
                    kept[column] = Some(natural[column]);
                    more = true;
                }
            }
            if !more || sharing == 0 {
                // (Not narrower than a character's room, however little room there is: drawn past it, cut off.)
                break kept.iter().map(|kept| kept.unwrap_or(share.floor().max((words + 2.0 * side).ceil() + 1.0))).collect();
            }
        }
    };
    let line = (words * super::markdown::LEADING).round();
    let rows = table
        .rows
        .iter()
        .enumerate()
        .map(|(row, cells)| {
            let tallest = cells.iter().zip(&columns).map(|(cell, width)| measure(&cell.reads, bold(row), Some(width - 1.0 - 2.0 * side)).1).fold(0.0, f32::max);
            (tallest + 2.0 * top).ceil().max(line) + 1.0
        })
        .collect();
    Laid { columns, rows }
}

/// Where Tab (or Shift+Tab, `back`) goes from what is chosen, `chosen` (UTF-16 units), in a table of `text`: the next
/// cell's words chosen (the one before's), in order along its rows and down them (the delimiter row's line counted
/// between its head and its first row), one its row leaves out written (with those before it left out, the caret in
/// it), after the last a row more made under it (as many cells as it has columns, begun as its lines are, the caret in
/// its first), before the first nowhere. None where what is chosen begins in no table: Tab is not its.
pub fn tab(text: &str, chosen: (usize, usize), back: bool) -> Option<Option<super::edit::Edit>> {
    let at = chosen.0.min(chosen.1);
    let table = tables(text).into_iter().find(|table| table.span.start <= at && at <= table.span.end)?;
    let cells: Vec<(usize, usize, &Cell)> = table.rows.iter().enumerate().flat_map(|(row, cells)| cells.iter().enumerate().map(move |(column, cell)| (row, column, cell))).collect();
    let within = cells.iter().position(|(.., cell)| cell.written && cell.room.start <= at && at <= cell.room.end);
    let to = match (within, back) {
        (Some(index), false) => Some(index + 1),
        (Some(index), true) => index.checked_sub(1),
        // (Between cells, or after a row's last pipe: the next that begins there or after, one left out among them, or
        // past the last.)
        (None, false) => Some(cells.iter().position(|(.., cell)| cell.room.start >= at).unwrap_or(cells.len())),
        (None, true) => cells.iter().rposition(|(.., cell)| cell.room.end < at),
    };
    let choose = |range: &Range<usize>| super::edit::Edit { range: range.start..range.start, with: String::new(), chosen: (range.start, range.end) };
    Some(match to {
        Some(index) if index < cells.len() && cells[index].2.written => Some(choose(&cells[index].2.words)),
        // (One its row leaves out: written, with those before it, after the row's last pipe.)
        Some(index) if index < cells.len() => {
            let (row, column, cell) = cells[index];
            let row = &table.rows[row];
            let written = row.iter().filter(|cell| cell.written).count();
            let piped = row[..written].last().is_some_and(|last| last.room.end < cell.room.start);
            let with = format!("{}{}", if piped { "" } else { " |" }, "  |".repeat(row.len() - written));
            let caret = cell.room.start + if piped { 0 } else { 2 } + 3 * (column - written) + 1;
            Some(super::edit::Edit { range: cell.room.start..cell.room.start, with, chosen: (caret, caret) })
        }
        // (Past the last: a row more.)
        Some(_) => {
            let row = format!("\n{}|{}", table.lead, "  |".repeat(table.aligns.len()));
            let caret = table.span.end + 1 + table.lead.encode_utf16().count() + 2;
            Some(super::edit::Edit { range: table.span.end..table.span.end, with: row, chosen: (caret, caret) })
        }
        None => None,
    })
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A table's rows and cells where they are written, its words as they read, each column set as its delimiter row
    /// says, a cell with nothing in it where its words would go, one not written where its row ends.
    #[test]
    fn reads_a_table() {
        let text = "a\n\n| 甲 | **乙** |\n| :-- | --: |\n| 1 |  |\n|x\n\nb";
        let units: Vec<u16> = text.encode_utf16().collect();
        let of = |range: &Range<usize>| String::from_utf16(&units[range.clone()]).unwrap();
        let found = tables(text);
        assert_eq!(found.len(), 1);
        let table = &found[0];
        assert_eq!(of(&table.span), "| 甲 | **乙** |\n| :-- | --: |\n| 1 |  |\n|x");
        assert_eq!(table.aligns, [Align::Left, Align::Right]);
        let reads: Vec<Vec<&str>> = table.rows.iter().map(|row| row.iter().map(|cell| cell.reads.as_str()).collect()).collect();
        assert_eq!(reads, [vec!["甲", "乙"], vec!["1", ""], vec!["x", ""]]);
        assert_eq!(of(&table.rows[0][1].words), "**乙**");
        assert_eq!(of(&table.rows[1][1].room), "  ");
        assert_eq!(table.rows[1][1].words, table.rows[1][1].room.start + 1..table.rows[1][1].room.start + 1);
        assert!(table.rows[2][1].room.is_empty());
        assert_eq!(table.rows[2][1].room.start, table.span.end);
        assert!(tables("| a\nb |").is_empty());
    }

    /// Each column as wide as its widest words where all fit, else the narrow kept and the rest sharing what is left,
    /// each row as tall as its tallest words, a line's height at least.
    #[test]
    fn lays_a_table_out() {
        let table = &tables("| a | bb |\n| - | - |\n| cccc | d |")[0];
        // (Each character 10 wide, 20 tall on each line it takes: bold or not.)
        let measure = |words: &str, _bold: bool, wide: Option<f32>| {
            let width = words.chars().count() as f32 * 10.0;
            let lines = wide.map_or(1.0, |wide| (width / wide.max(10.0)).ceil().max(1.0));
            (width.min(wide.unwrap_or(f32::MAX)), lines * 20.0)
        };
        // (Words 10 high: 6 at the sides, 4 over and under, a line 16 high.)
        let laid = lay(table, 10.0, 200.0, measure);
        assert_eq!(laid.columns, [53.0, 33.0]);
        assert_eq!(laid.rows, [29.0, 29.0]);
        assert_eq!((laid.width(), laid.height()), (87.0, 59.0));
        let wide = &tables("| a | bbbbbbbbbbbbbbbbbbbb |\n| - | - |\n| c | d |")[0];
        let laid = lay(wide, 10.0, 101.0, measure);
        assert_eq!(laid.columns, [23.0, 77.0]);
        // (The second column's 20 characters on 64 across: four lines.)
        assert_eq!(laid.rows, [89.0, 29.0]);
    }

    /// Tab goes along a table's cells and down its rows, choosing each one's words, past the last a row more. Shift+Tab
    /// goes back, before the first nowhere. Outside a table it is not Tab's.
    #[test]
    fn tab_goes_from_cell_to_cell() {
        let text = "| 甲 | 乙 |\n| --- | --- |\n| 1 |  |";
        let units: Vec<u16> = text.encode_utf16().collect();
        let of = |range: (usize, usize)| String::from_utf16(&units[range.0..range.1]).unwrap();
        let unit = |what: &str| text[..text.find(what).unwrap()].encode_utf16().count();
        let went = |at: usize, back: bool| tab(text, (at, at), back).map(|edit| edit.map(|edit| edit.chosen));
        assert_eq!(went(unit("甲"), false).flatten().map(of).as_deref(), Some("乙"));
        assert_eq!(went(unit("乙"), false).flatten().map(of).as_deref(), Some("1"));
        assert_eq!(went(unit("--- |\n|"), false).flatten().map(of).as_deref(), Some("1"));
        assert_eq!(went(unit("--- |\n|"), true).flatten().map(of).as_deref(), Some("乙"));
        let empty = went(unit("1"), false).flatten().unwrap();
        assert_eq!(empty.0, empty.1);
        assert_eq!(went(unit("甲"), true), Some(None));
        let more = tab(text, (units.len() - 2, units.len() - 2), false).unwrap().unwrap();
        assert_eq!((more.range.clone(), more.with.as_str(), more.chosen), (units.len()..units.len(), "\n|  |  |", (units.len() + 3, units.len() + 3)));
        assert_eq!(tab("words\n\n| a |\n| - |", (2, 2), false), None);
    }

    /// A cell with nothing between its pipes is where it is written: Tab goes to it. One its row leaves out is written
    /// as Tab goes to it (with those before it), the caret in it, and Tab goes on from it. A row more in a quote is the
    /// quote's.
    #[test]
    fn tab_through_cells_empty_and_left_out() {
        let text = "| A | B | C |\n| --- | --- | --- |\n|x||z|";
        let at = |what: &str| text.find(what).unwrap();
        let empty = tab(text, (at("x"), at("x")), false).unwrap().unwrap();
        assert_eq!(empty.chosen, (at("||") + 1, at("||") + 1));
        assert_eq!(&tables(text)[0].rows[1][1].room, &(at("||") + 1..at("||") + 1));
        let left = "| A | B | C |\n| --- | --- | --- |\n|x";
        let made = tab(left, (left.len(), left.len()), false).unwrap().unwrap();
        let mut after = left.to_string();
        after.replace_range(made.range.clone(), &made.with);
        assert_eq!(after, "| A | B | C |\n| --- | --- | --- |\n|x |  |  |");
        assert_eq!(&after[made.chosen.0 - 1..made.chosen.0 + 1], "  ");
        let next = tab(&after, made.chosen, false).unwrap().unwrap();
        assert_eq!(next.chosen.0, after.rfind("  |").unwrap() + 1);
        let piped = "| A | B |\n| --- | --- |\n| x |";
        let made = tab(piped, (piped.len() - 2, piped.len() - 2), false).unwrap().unwrap();
        assert_eq!((made.with.as_str(), made.chosen), ("  |", (piped.len() + 1, piped.len() + 1)));
        let quoted = "> | a | b |\n> | - | - |\n> | c | d |";
        let more = tab(quoted, (quoted.len() - 2, quoted.len() - 2), false).unwrap().unwrap();
        assert_eq!((more.with.as_str(), more.chosen), ("\n> |  |  |", (quoted.len() + 5, quoted.len() + 5)));
        let listed = "- i\n\n  | a |\n  | - |\n  | b |";
        assert!(!tables(listed)[0].own);
        let more = tab(listed, (listed.len() - 2, listed.len() - 2), false).unwrap().unwrap();
        assert_eq!(more.with, "\n  |  |");
        // After a row's last pipe, as the caret is once it has been typed: the next row's first cell, else a row more.
        let typed = "| a | b |\n| - | - |\n| c | d |\n| e | f |";
        let end = typed.find("\n| e").unwrap();
        let next = tab(typed, (end, end), false).unwrap().unwrap();
        assert_eq!(&typed[next.chosen.0..next.chosen.1], "e");
        let more = tab(typed, (typed.len(), typed.len()), false).unwrap().unwrap();
        assert_eq!((more.with.as_str(), more.chosen), ("\n|  |  |", (typed.len() + 3, typed.len() + 3)));
        let left = tab(piped, (piped.len(), piped.len()), false).unwrap().unwrap();
        assert_eq!((left.with.as_str(), left.chosen), ("  |", (piped.len() + 1, piped.len() + 1)));
    }
}
