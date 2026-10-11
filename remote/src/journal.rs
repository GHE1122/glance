//! The remote's log: what it was asked and did, what the screens and the
//! monitors did (and what woke them), and what went wrong; a line each with
//! the local time, beside its configuration. Only the latest lines are
//! kept. Nothing of the phones goes in but that there was one, and no key.

use std::collections::VecDeque;
use std::fmt::Display;
use std::io::Write;
use std::path::PathBuf;
use std::sync::{Mutex, PoisonError};

use windows::Win32::System::SystemInformation::GetLocalTime;

/// How many lines are kept, and how long each may be.
const KEPT: usize = 600;
const LINE_MOST: usize = 400;
/// The file is written anew, with only the lines kept, once it has grown
/// by this many lines beyond them.
const SLACK: usize = 200;

struct Log {
    lines: VecDeque<String>,
    /// How many of the file's lines are not among those kept.
    over: usize,
}

static LOG: Mutex<Option<Log>> = Mutex::new(None);

fn path() -> PathBuf {
    crate::config::folder().join("remote.log")
}

fn read() -> Log {
    let text = std::fs::read_to_string(path()).unwrap_or_default();
    let mut lines: VecDeque<String> = text.lines().map(|line| line.chars().take(LINE_MOST + 24).collect()).collect();
    let over = lines.len().saturating_sub(KEPT);
    lines.drain(..over);
    Log { lines, over }
}

/// Adds a line to the log.
pub fn note(what: impl Display) {
    let time = unsafe { GetLocalTime() };
    let what: String = what.to_string().replace(['\r', '\n'], " ").chars().take(LINE_MOST).collect();
    let line = format!("{:04}-{:02}-{:02} {:02}:{:02}:{:02}.{:03}  {what}", time.wYear, time.wMonth, time.wDay, time.wHour, time.wMinute, time.wSecond, time.wMilliseconds);
    let mut log = LOG.lock().unwrap_or_else(PoisonError::into_inner);
    let log = log.get_or_insert_with(read);
    log.lines.push_back(line.clone());
    if log.lines.len() > KEPT {
        log.lines.pop_front();
        log.over += 1;
    }
    let _ = std::fs::create_dir_all(crate::config::folder());
    if log.over > SLACK {
        let text: String = log.lines.iter().map(|line| format!("{line}\n")).collect();
        if std::fs::write(path(), text).is_ok() {
            log.over = 0;
        }
    } else if let Ok(mut file) = std::fs::OpenOptions::new().create(true).append(true).open(path()) {
        let _ = writeln!(file, "{line}");
    }
}

/// The latest `count` lines, oldest first.
pub fn recent(count: usize) -> Vec<String> {
    let mut log = LOG.lock().unwrap_or_else(PoisonError::into_inner);
    let log = log.get_or_insert_with(read);
    log.lines.iter().skip(log.lines.len().saturating_sub(count)).cloned().collect()
}

/// Notes a crash before the program ends: its message, and the source
/// file and line, by the file's name alone.
pub fn note_crashes() {
    let previous = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let payload = info.payload();
        let message = payload.downcast_ref::<&str>().copied().or_else(|| payload.downcast_ref::<String>().map(String::as_str)).unwrap_or("");
        let place = info.location().map(|at| {
            let file = std::path::Path::new(at.file()).file_name().map_or_else(String::new, |name| name.to_string_lossy().into_owned());
            format!(" at {file}:{}", at.line())
        });
        note(format!("crashed: {message}{}", place.unwrap_or_default()));
        previous(info);
    }));
}
