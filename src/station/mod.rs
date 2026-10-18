//! The station: a shelf to put things on for a while (files, pictures, text, links) and take them off again into
//! another program, and what was copied lately (see docs/station-and-notes.md).

pub mod clipboard;
pub mod drag;
pub mod drop;
pub mod folder;
pub mod keep;
pub mod run;
pub mod shelf;

/// Now, in ms since 1970 (as the shelf keeps when things were put on it).
pub fn now_ms() -> u64 {
    std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).unwrap_or_default().as_millis() as u64
}
