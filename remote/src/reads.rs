//! What Glance reads of this PC (how warm and how busy its parts are, how
//! fast its fans turn), as Glance leaves it in a piece of memory shared by
//! name: a count (odd while what follows is being written), the length of
//! what follows, and that, as JSON. Glance not running, there is none.

use std::sync::atomic::{AtomicU32, Ordering};
use std::time::{SystemTime, UNIX_EPOCH};

use serde_json::Value;
use windows::core::w;
use windows::Win32::Foundation::CloseHandle;
use windows::Win32::System::Memory::{MapViewOfFile, OpenFileMappingW, UnmapViewOfFile, FILE_MAP_READ};

/// How much of the memory there is, and how much of that comes before the text.
const SIZE: usize = 4096;
const HEAD: usize = 8;
/// How many times a text is read over, caught being written each time,
/// before there is taken to be none (its writer gone half way).
const TRIES: usize = 64;
/// How old the figures may be (ms): Glance samples every ten seconds at its
/// slowest, and older than a few of those it is not sampling at all.
const FRESH: u64 = 30_000;

/// The text out of `memory`, whole: none while it is being written.
fn text(memory: &[u8]) -> Option<Vec<u8>> {
    // (The start of a mapping, or of a test's own aligned words.)
    let count = unsafe { &*(memory.as_ptr() as *const AtomicU32) };
    let before = count.load(Ordering::SeqCst);
    let length = u32::from_le_bytes(memory[4..HEAD].try_into().unwrap()) as usize;
    let read = memory.get(HEAD..HEAD + length)?.to_vec();
    (before % 2 == 0 && count.load(Ordering::SeqCst) == before).then_some(read)
}

/// The figures Glance last left, while they are fresh: `cpu`, `gpu`, `ram`
/// and `disk`, each with a `temp` and a `load`, and `cpu_fan` and
/// `case_fan`; any of them null where Glance does not know it.
pub fn read() -> Option<Value> {
    let mapping = unsafe { OpenFileMappingW(FILE_MAP_READ.0, false, w!("Local\\Glance.Readings")) }.ok()?;
    let view = unsafe { MapViewOfFile(mapping, FILE_MAP_READ, 0, 0, SIZE) };
    let read = (!view.Value.is_null()).then(|| {
        let memory = unsafe { std::slice::from_raw_parts(view.Value as *const u8, SIZE) };
        let read = (0..TRIES).find_map(|_| text(memory).or_else(|| {
            std::thread::yield_now();
            None
        }));
        let _ = unsafe { UnmapViewOfFile(view) };
        read
    });
    let _ = unsafe { CloseHandle(mapping) };
    let figures: Value = serde_json::from_slice(&read??).ok()?;
    let taken = figures.get("t")?.as_u64()?;
    let now = SystemTime::now().duration_since(UNIX_EPOCH).map_or(0, |since| since.as_millis() as u64);
    (now.saturating_sub(taken) < FRESH).then_some(figures)
}

/// How warm the warmest of the processor and the graphics card is (°C).
pub fn hottest(figures: &Value) -> Option<i64> {
    ["cpu", "gpu"].iter().filter_map(|part| figures.get(part)?.get("temp")?.as_i64()).max()
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_text_is_read_whole_or_not_at_all() {
        let mut words = [0u32; 16];
        let memory = unsafe { std::slice::from_raw_parts_mut(words.as_mut_ptr().cast::<u8>(), 64) };
        memory[..4].copy_from_slice(&2u32.to_le_bytes());
        memory[4..8].copy_from_slice(&7u32.to_le_bytes());
        memory[8..15].copy_from_slice(b"{\"t\":1}");
        assert_eq!(text(memory).as_deref(), Some(&b"{\"t\":1}"[..]));
        // Being written: not read.
        memory[..4].copy_from_slice(&3u32.to_le_bytes());
        assert_eq!(text(memory), None);
        // A length beyond the memory: not read.
        memory[..4].copy_from_slice(&4u32.to_le_bytes());
        memory[4..8].copy_from_slice(&900u32.to_le_bytes());
        assert_eq!(text(memory), None);
    }

    #[test]
    fn the_hottest_is_of_the_processor_and_the_card() {
        let figures = serde_json::json!({ "cpu": { "temp": 58, "load": 12 }, "gpu": { "temp": 71, "load": 9 }, "disk": { "temp": 90 } });
        assert_eq!(hottest(&figures), Some(71));
        assert_eq!(hottest(&serde_json::json!({ "cpu": { "temp": null }, "gpu": { "temp": 40 } })), Some(40));
        assert_eq!(hottest(&serde_json::json!({ "cpu": { "temp": null } })), None);
    }
}
