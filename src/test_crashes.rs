//! In test builds only: a fault of the process (an access violation, from code outside Rust's checks: a system
//! library, a driver's, a `unsafe` call) said before the process ends, with the test whose thread it was (the test
//! harness names each test's thread by it) and the module and offset where it was. Without it, a test process that
//! dies under load says only that it died.

use std::ffi::c_void;

#[repr(C)]
struct ExceptionRecord {
    code: u32,
    flags: u32,
    record: *mut ExceptionRecord,
    address: *mut c_void,
    count: u32,
    information: [usize; 15],
}

#[repr(C)]
struct ExceptionPointers {
    record: *mut ExceptionRecord,
    context: *mut c_void,
}

#[link(name = "kernel32")]
unsafe extern "system" {
    fn AddVectoredExceptionHandler(first: u32, handler: unsafe extern "system" fn(*mut ExceptionPointers) -> i32) -> *mut c_void;
}

const ACCESS_VIOLATION: u32 = 0xC000_0005;
/// Let the search go on: the process ends as it would have.
const CONTINUE_SEARCH: i32 = 0;

unsafe extern "system" fn said(pointers: *mut ExceptionPointers) -> i32 {
    let record = unsafe { &*(*pointers).record };
    if record.code != ACCESS_VIOLATION {
        return CONTINUE_SEARCH;
    }
    use windows::Win32::System::LibraryLoader::{GetModuleFileNameW, GetModuleHandleExW, GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS, GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT};
    let mut module = windows::Win32::Foundation::HMODULE::default();
    let found = unsafe {
        GetModuleHandleExW(GET_MODULE_HANDLE_EX_FLAG_FROM_ADDRESS | GET_MODULE_HANDLE_EX_FLAG_UNCHANGED_REFCOUNT, windows::core::PCWSTR(record.address as *const u16), &mut module)
    }
    .is_ok();
    let mut name = [0u16; 260];
    let length = if found { (unsafe { GetModuleFileNameW(Some(module), &mut name) }) as usize } else { 0 };
    let offset = record.address as usize - module.0 as usize;
    let thread = std::thread::current();
    // (Straight to the error stream: the test harness holds what a test's thread prints, and it goes with the
    // process.)
    let said = format!(
        "ACCESS VIOLATION in thread {:?}: at {:?}, {} + {offset:#x}, {} {:#x}
",
        thread.name().unwrap_or("(unnamed)"),
        record.address,
        String::from_utf16_lossy(&name[..length]),
        if record.information[0] == 0 { "reading" } else { "writing" },
        record.information[1],
    );
    use std::io::Write;
    let _ = std::io::stderr().lock().write_all(said.as_bytes());
    CONTINUE_SEARCH
}

/// Run as the test process starts (with the C runtime's initialisers).
#[used]
#[unsafe(link_section = ".CRT$XCU")]
static WATCHED: extern "C" fn() = {
    extern "C" fn watch() {
        unsafe {
            AddVectoredExceptionHandler(1, said);
        }
    }
    watch
};

#[cfg(test)]
mod tests {
    /// The fault is said: run alone, it ends this process as a fault does.
    #[test]
    #[ignore = "faults on purpose, ending the process: run alone, to see the fault said"]
    fn a_fault_is_said() {
        let nowhere = std::hint::black_box(std::ptr::null::<u64>());
        let _ = unsafe { std::ptr::read_volatile(nowhere) };
    }
}
