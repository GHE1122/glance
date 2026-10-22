//! Marks for the performance pass: moments of Glance's (a panel asked to open, its first frame
//! shown, a frame of the settings drawn), written with when they came to the file the GLANCE_PERF variable names, one
//! a line: the time (seconds, on the performance counter, as Python's `time.perf_counter` reads it, so that what drives
//! Glance and what Glance did are on one clock), the thread, and what. Without the variable (as Glance runs for its
//! users) nothing is written, and a mark costs a look at a flag.

use std::io::Write;
use std::sync::{Mutex, OnceLock};

use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};

static OUT: OnceLock<Option<Mutex<std::fs::File>>> = OnceLock::new();

fn out() -> Option<&'static Mutex<std::fs::File>> {
    OUT.get_or_init(|| {
        let path = std::env::var_os("GLANCE_PERF")?;
        std::fs::OpenOptions::new().create(true).append(true).open(path).ok().map(Mutex::new)
    })
    .as_ref()
}

/// Whether marks are written.
pub fn on() -> bool {
    out().is_some()
}

/// Now, in seconds on the performance counter.
pub fn now() -> f64 {
    let (mut count, mut frequency) = (0i64, 0i64);
    unsafe {
        let _ = QueryPerformanceCounter(&mut count);
        let _ = QueryPerformanceFrequency(&mut frequency);
    }
    count as f64 / frequency as f64
}

/// `what` marked as come now (made only if marks are written).
pub fn mark(what: impl FnOnce() -> String) {
    if let Some(out) = out() {
        let at = now();
        let thread = std::thread::current();
        let line = format!("{at:.6}\t{}\t{}\n", thread.name().unwrap_or("?"), what());
        let _ = out.lock().unwrap().write_all(line.as_bytes());
    }
}

/// A stretch of work timed: marked, with how long it took (ms) and `detail`, as it ends (dropped). None while marks
/// are not written.
pub struct Timed {
    what: &'static str,
    detail: String,
    began: f64,
}

impl Timed {
    pub fn start(what: &'static str, detail: impl FnOnce() -> String) -> Option<Timed> {
        on().then(|| Timed { what, detail: detail(), began: now() })
    }
}

impl Drop for Timed {
    fn drop(&mut self) {
        let took = (now() - self.began) * 1000.0;
        mark(|| format!("{} {took:.3} ms {}", self.what, self.detail));
    }
}
