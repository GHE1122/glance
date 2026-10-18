//! What is dragged off the station into another program: given in the formats it is copied in (see
//! `clipboard::formats_of`), the receiving program choosing among them; files as a copy (the station never has them
//! moved away by a drag: moved, they would be gone from where it points to).

use super::clipboard::{self, Content};
use windows::core::{implement, BOOL, HRESULT};
use windows::Win32::Foundation::{GlobalFree, HGLOBAL};
use windows::Win32::System::Com::{IDataObject, DVASPECT_CONTENT, FORMATETC, STGMEDIUM, STGMEDIUM_0, TYMED_HGLOBAL};
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::System::Ole::{DoDragDrop, IDropSource, IDropSource_Impl, DROPEFFECT, DROPEFFECT_COPY, DROPEFFECT_NONE};
use windows::Win32::System::SystemServices::{MK_LBUTTON, MODIFIERKEYS_FLAGS};
use windows::Win32::UI::Shell::SHCreateDataObject;

/// What came of a drag off the station.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Dragged {
    /// Dropped on a program that took it.
    Taken,
    /// Let go where nothing took it, or given up (Esc).
    Not,
}

/// Drags `content` off the station (`stays_here`: see `shelf::Item::stays_here`) till it is dropped or given up, on
/// this thread (which must be the station's: in a single-threaded apartment, its messages pumped meanwhile by
/// Windows), pressed on a window of the thread `pressed_on`: whose input this thread shares meanwhile (the pointer
/// is that thread's while its button is down; not shared, the drag saw it neither move nor let go). None where it
/// could not be begun.
pub fn drag(content: &Content, stays_here: bool, pressed_on: u32) -> Option<Dragged> {
    use windows::Win32::System::Threading::{AttachThreadInput, GetCurrentThreadId};
    let this = unsafe { GetCurrentThreadId() };
    let shared = pressed_on != this && unsafe { AttachThreadInput(this, pressed_on, true) }.as_bool();
    let dragged = drag_here(content, stays_here);
    if shared {
        let _ = unsafe { AttachThreadInput(this, pressed_on, false) };
    }
    dragged
}

/// As `drag`, its input this thread's.
fn drag_here(content: &Content, stays_here: bool) -> Option<Dragged> {
    let formats = clipboard::formats_of(content, stays_here)?;
    unsafe {
        // The shell's own data object, which keeps whatever it is given (and gives the receiving program what it
        // asks of it as Explorer's do).
        let data: IDataObject = SHCreateDataObject(None, None, None).ok()?;
        for (format, bytes) in &formats {
            let Some(memory) = global(bytes) else { return None };
            let asked = FORMATETC { cfFormat: *format as u16, ptd: std::ptr::null_mut(), dwAspect: DVASPECT_CONTENT.0, lindex: -1, tymed: TYMED_HGLOBAL.0 as u32 };
            let medium = STGMEDIUM { tymed: TYMED_HGLOBAL.0 as u32, u: STGMEDIUM_0 { hGlobal: memory }, pUnkForRelease: std::mem::ManuallyDrop::new(None) };
            // (The data object owns it from here; not taken, it is let go of here.)
            if data.SetData(&asked, &medium, true).is_err() {
                let _ = GlobalFree(Some(memory));
                return None;
            }
        }
        let source: IDropSource = Source.into();
        let mut effect = DROPEFFECT_NONE;
        let said = DoDragDrop(&data, &source, DROPEFFECT_COPY, &mut effect);
        Some(if said == windows::Win32::Foundation::DRAGDROP_S_DROP && effect != DROPEFFECT_NONE { Dragged::Taken } else { Dragged::Not })
    }
}

/// `bytes` in global memory of their own (to be handed over).
unsafe fn global(bytes: &[u8]) -> Option<HGLOBAL> {
    unsafe {
        let memory = GlobalAlloc(GMEM_MOVEABLE, bytes.len().max(1)).ok()?;
        let at = GlobalLock(memory);
        if at.is_null() {
            let _ = GlobalFree(Some(memory));
            return None;
        }
        std::ptr::copy_nonoverlapping(bytes.as_ptr(), at.cast::<u8>(), bytes.len());
        let _ = GlobalUnlock(memory);
        Some(memory)
    }
}

/// The station as the source of a drag: dropped as the left button is let go, given up by Esc; the system's cursors.
#[implement(IDropSource)]
struct Source;

impl IDropSource_Impl for Source_Impl {
    fn QueryContinueDrag(&self, escape: BOOL, keys: MODIFIERKEYS_FLAGS) -> HRESULT {
        // (The button and Esc as they are, not only as the drag's messages say: pressed on another thread's window,
        // those may never come, and the drag never ended.)
        use windows::Win32::UI::Input::KeyboardAndMouse::{GetAsyncKeyState, VK_ESCAPE, VK_LBUTTON};
        let down = |key: windows::Win32::UI::Input::KeyboardAndMouse::VIRTUAL_KEY| unsafe { GetAsyncKeyState(i32::from(key.0)) } < 0;
        if escape.as_bool() || down(VK_ESCAPE) {
            windows::Win32::Foundation::DRAGDROP_S_CANCEL
        } else if keys.0 & MK_LBUTTON.0 == 0 || !down(VK_LBUTTON) {
            windows::Win32::Foundation::DRAGDROP_S_DROP
        } else {
            HRESULT(0)
        }
    }

    fn GiveFeedback(&self, _: DROPEFFECT) -> HRESULT {
        windows::Win32::Foundation::DRAGDROP_S_USEDEFAULTCURSORS
    }
}
