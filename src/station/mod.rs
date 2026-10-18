//! The station: a shelf to put things on for a while (files, pictures, text, links) and take them off again into
//! another program, and what was copied lately (see docs/station-and-notes.md).

pub mod clipboard;
pub mod drag;
pub mod drop;
pub mod folder;
pub mod keep;
pub mod run;
pub mod shelf;
pub mod thumbs;
#[cfg(test)]
mod spike;

/// A press on what a station's lane shows (see `view::Hit`), in the panel and in a widget alike: done; whether it was
/// one. A thing on it, or copied lately, is put on the clipboard; its head's word clears it.
pub fn pressed(hit: crate::ui::view::Hit) -> bool {
    use crate::ui::view::Hit;
    match hit {
        Hit::Shelf(id) | Hit::Copied(id) => run::ask(run::Ask::Copy(id)),
        Hit::ShelfClear => run::ask(run::Ask::Clear),
        _ => return false,
    }
    true
}

/// Now, in ms since 1970 (as the shelf keeps when things were put on it).
pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}
