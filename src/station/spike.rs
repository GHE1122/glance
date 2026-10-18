//! Whether OLE drag and drop works with the windows of another thread (see docs/station-and-notes.md, section 七): a
//! drop target registered by the station's thread (an STA) for windows a multithreaded thread owns, as the panel's
//! thread owns its windows; and a drag begun on the station's thread as the button is pressed in such a window.
//!
//! It moves the pointer and presses its button: run it in the test VM, never on a desktop in use.
//! `glance_lib-*.exe --ignored --exact station::spike::drag_and_drop_across_threads --nocapture`

use std::sync::atomic::{AtomicBool, AtomicIsize, Ordering};
use std::sync::mpsc;
use std::time::{Duration, Instant};
use windows::core::w;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::System::Com::{CoInitializeEx, COINIT_MULTITHREADED};
use windows::Win32::UI::Input::KeyboardAndMouse::{SendInput, INPUT, INPUT_0, INPUT_MOUSE, MOUSEEVENTF_ABSOLUTE, MOUSEEVENTF_LEFTDOWN, MOUSEEVENTF_LEFTUP, MOUSEEVENTF_MOVE, MOUSEINPUT};
use windows::Win32::UI::WindowsAndMessaging::*;

static PRESSED: AtomicBool = AtomicBool::new(false);
static DRAG_FROM: AtomicIsize = AtomicIsize::new(0);

unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    if message == WM_LBUTTONDOWN {
        PRESSED.store(true, Ordering::Release);
    }
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

/// Moves the pointer to `at` (screen px), pressing or letting go of the left button as `buttons` says.
fn pointer(at: POINT, buttons: windows::Win32::UI::Input::KeyboardAndMouse::MOUSE_EVENT_FLAGS) {
    let (w, h) = unsafe { (GetSystemMetrics(SM_CXSCREEN), GetSystemMetrics(SM_CYSCREEN)) };
    let input = INPUT {
        r#type: INPUT_MOUSE,
        Anonymous: INPUT_0 { mi: MOUSEINPUT { dx: at.x * 65535 / (w - 1), dy: at.y * 65535 / (h - 1), dwFlags: MOUSEEVENTF_MOVE | MOUSEEVENTF_ABSOLUTE | buttons, ..Default::default() } },
    };
    unsafe { SendInput(&[input], std::mem::size_of::<INPUT>() as i32) };
}

#[test]
#[ignore = "moves the pointer and presses its button: run in the test VM"]
fn drag_and_drop_across_threads() {
    // The windows' thread: multithreaded, as the panel's is; two windows, one dragged from, one dropped on.
    let (made, windows) = mpsc::channel();
    std::thread::spawn(move || unsafe {
        let _ = CoInitializeEx(None, COINIT_MULTITHREADED);
        let class = WNDCLASSW { lpfnWndProc: Some(procedure), lpszClassName: w!("GlanceSpike"), hbrBackground: windows::Win32::Graphics::Gdi::HBRUSH(windows::Win32::Graphics::Gdi::GetStockObject(windows::Win32::Graphics::Gdi::WHITE_BRUSH).0), ..Default::default() };
        RegisterClassW(&class);
        let make = |x: i32| CreateWindowExW(WS_EX_TOPMOST, class.lpszClassName, w!("spike"), WS_POPUP | WS_VISIBLE, x, 200, 200, 200, None, None, None, None).unwrap();
        let (from, to) = (make(200), make(600));
        made.send((from.0 as isize, to.0 as isize)).unwrap();
        let mut message = MSG::default();
        while GetMessageW(&mut message, None, 0, 0).as_bool() {
            DispatchMessageW(&message);
        }
    });
    let (from, to) = windows.recv().unwrap();
    // The station's thread: an STA with OLE; registers the drop target for the other thread's window, and drags
    // from it as asked.
    let (dropped_tx, dropped) = mpsc::channel::<String>();
    let (registered_tx, registered) = mpsc::channel();
    let station = std::thread::spawn(move || unsafe {
        windows::Win32::System::Ole::OleInitialize(None).unwrap();
        let target = super::drop::Target::new(move |dragged| {
            if let super::drop::Dragged::Dropped(given) = dragged {
                let _ = dropped_tx.send(format!("{given:?}"));
            }
        });
        let said = windows::Win32::System::Ole::RegisterDragDrop(HWND(to as *mut _), &target);
        registered_tx.send(said.map_err(|error| error.to_string())).unwrap();
        // Waits for the press in the window dragged from, then drags.
        let began = Instant::now();
        while !PRESSED.load(Ordering::Acquire) && began.elapsed() < Duration::from_secs(10) {
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                DispatchMessageW(&message);
            }
            std::thread::sleep(Duration::from_millis(10));
        }
        DRAG_FROM.store(1, Ordering::Release);
        let dragged = super::drag::drag(&super::clipboard::Content::Text("across threads".into()), false);
        println!("the drag ended: {dragged:?}");
        let _ = windows::Win32::System::Ole::RevokeDragDrop(HWND(to as *mut _));
        dragged
    });
    let registered = registered.recv().unwrap();
    println!("registered for the other thread's window: {registered:?}");
    assert!(registered.is_ok(), "a drop target is registered for another thread's window");
    let middle = |window: isize| {
        let mut rect = RECT::default();
        unsafe { GetWindowRect(HWND(window as *mut _), &mut rect).unwrap() };
        POINT { x: (rect.left + rect.right) / 2, y: (rect.top + rect.bottom) / 2 }
    };
    let (a, b) = (middle(from), middle(to));
    pointer(a, Default::default());
    std::thread::sleep(Duration::from_millis(200));
    pointer(a, MOUSEEVENTF_LEFTDOWN);
    // The drag begun on the station's thread: moved over the other window in steps, and let go there.
    let began = Instant::now();
    while DRAG_FROM.load(Ordering::Acquire) == 0 && began.elapsed() < Duration::from_secs(5) {
        std::thread::sleep(Duration::from_millis(10));
    }
    std::thread::sleep(Duration::from_millis(200));
    for step in 1..=20 {
        pointer(POINT { x: a.x + (b.x - a.x) * step / 20, y: a.y + (b.y - a.y) * step / 20 }, Default::default());
        std::thread::sleep(Duration::from_millis(30));
    }
    std::thread::sleep(Duration::from_millis(200));
    pointer(b, MOUSEEVENTF_LEFTUP);
    let given = dropped.recv_timeout(Duration::from_secs(5));
    println!("dropped on the other thread's window, heard on the station's: {given:?}");
    let dragged = station.join().unwrap();
    assert!(given.is_ok_and(|given| given.contains("across threads")), "the drop came to the station's thread");
    assert_eq!(dragged, Some(super::drag::Dragged::Taken));
}
