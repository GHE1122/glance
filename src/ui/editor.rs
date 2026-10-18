//! A box words are written in, written as Windows writes them: its RichEdit text services, windowless (see
//! docs/station-and-notes.md, sections 三 and 六). They take the keys, the input method's words, the selection, undo and
//! the clipboard; Glance hosts them in a window it draws itself, and they draw their words with Direct2D into what
//! Glance draws (`ITextServices2::TxDrawD2D`), the caret drawn by Glance where they say it is.
//!
//! Everything here is in the window's physical pixels, as the text services count; the window's thread is a
//! single-threaded apartment.

use std::cell::Cell;
use std::rc::Rc;

use windows::core::{implement, Interface, BOOL, BSTR, GUID, HSTRING};
use windows::Win32::Foundation::{COLORREF, HWND, LPARAM, LRESULT, POINT, RECT, RECTL, SIZE, WPARAM};
use windows::Win32::Graphics::Direct2D::ID2D1RenderTarget;
use windows::Win32::Graphics::Gdi::{GetDC, GetSysColor, InvalidateRect, ReleaseDC, ScreenToClient, ClientToScreen, HBITMAP, HDC, HPALETTE, HRGN, SYS_COLOR_INDEX};
use windows::Win32::System::LibraryLoader::{GetProcAddress, LoadLibraryW};
use windows::Win32::UI::Controls::RichEdit::*;
use windows::Win32::UI::Input::Ime::{ImmGetContext, ImmReleaseContext, HIMC};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetCursor, SetForegroundWindow, SetTimer, HCURSOR, SCROLLBAR_CONSTANTS, SCROLL_WINDOW_FLAGS, WS_VSCROLL};

/// What the host keeps for the text services, and what they tell it.
struct Kept {
    hwnd: HWND,
    /// Where the words are written in the window.
    rect: Cell<RECT>,
    /// The caret: where, how large, whether shown.
    caret: Cell<Option<(i32, i32, i32, i32)>>,
    caret_size: Cell<(i32, i32)>,
    caret_shown: Cell<bool>,
    /// The words' look as written from here on, and their paragraphs'.
    char_format: Box<CHARFORMAT2W>,
    para_format: Box<PARAFORMAT>,
    /// What was written changed since it was last asked.
    changed: Cell<bool>,
}

#[implement(ITextHost2, ITextHost)]
struct Host {
    kept: Rc<Kept>,
}

/// A box words are written in.
pub struct Editor {
    services: ITextServices2,
    _host: ITextHost2,
    kept: Rc<Kept>,
}

/// The text services' maker, and the interface they are asked for, as msftedit.dll gives them.
fn services_maker() -> Option<(PCreateTextServices, GUID)> {
    unsafe {
        let library = LoadLibraryW(windows::core::w!("msftedit.dll")).ok()?;
        let make: PCreateTextServices = std::mem::transmute(GetProcAddress(library, windows::core::s!("CreateTextServices"))?);
        let iid = GetProcAddress(library, windows::core::s!("IID_ITextServices2"))?;
        Some((make, *(iid as *const GUID)))
    }
}

impl Editor {
    /// A box in `hwnd` at `rect`, its words in `face` `pixels` high, of `color`; none where the text services cannot
    /// be had.
    pub fn new(hwnd: HWND, rect: RECT, face: &str, pixels: f32, color: COLORREF) -> Option<Editor> {
        let mut char_format = Box::new(CHARFORMAT2W::default());
        char_format.Base.cbSize = std::mem::size_of::<CHARFORMAT2W>() as u32;
        char_format.Base.dwMask = CFM_SIZE | CFM_FACE | CFM_COLOR | CFM_CHARSET;
        // (Twips: the text services' DPI is the screen's, 96 per inch of pixels as they draw here.)
        char_format.Base.yHeight = (pixels * 15.0).round() as i32;
        char_format.Base.crTextColor = color;
        char_format.Base.bCharSet = windows::Win32::Graphics::Gdi::DEFAULT_CHARSET;
        for (unit, to) in face.encode_utf16().zip(char_format.Base.szFaceName.iter_mut().take(31)) {
            *to = unit;
        }
        let mut para_format = Box::new(PARAFORMAT::default());
        para_format.cbSize = std::mem::size_of::<PARAFORMAT>() as u32;
        para_format.dwMask = PFM_ALL;
        para_format.wAlignment = PFA_LEFT;
        let kept = Rc::new(Kept { hwnd, rect: Cell::new(rect), caret: Cell::new(None), caret_size: Cell::new((1, 16)), caret_shown: Cell::new(false), char_format, para_format, changed: Cell::new(false) });
        let host: ITextHost2 = Host { kept: kept.clone() }.into();
        let (make, iid) = services_maker()?;
        let make = make?;
        unsafe {
            let mut made: Option<windows::core::IUnknown> = None;
            let plain: ITextHost = host.cast().ok()?;
            make(None.into(), (&plain).into(), windows::core::OutRef::from(&mut made)).ok().ok()?;
            let made = made?;
            let mut services = std::ptr::null_mut();
            made.query(&iid, &mut services).ok().ok()?;
            let services = ITextServices2::from_raw(services);
            let mut client = rect;
            let _ = services.OnTxInPlaceActivate(&mut client);
            let _ = services.OnTxUIActivate();
            let mut result = LRESULT(0);
            let _ = services.TxSendMessage(EM_SETEVENTMASK, WPARAM(0), LPARAM(ENM_CHANGE as isize), &mut result);
            Some(Editor { services, _host: host, kept })
        }
    }

    /// What is written.
    pub fn text(&self) -> String {
        let mut text = BSTR::new();
        unsafe {
            let _ = self.services.TxGetText(&mut text);
        }
        // (The text services end each paragraph with a carriage return alone.)
        text.to_string().replace("\r\n", "\n").replace('\r', "\n")
    }

    /// `text` written in it, in place of what was.
    pub fn set_text(&self, text: &str) {
        unsafe {
            let _ = self.services.TxSetText(&HSTRING::from(text.replace('\n', "\r")));
        }
        self.kept.changed.set(false);
    }

    /// Whether what is written changed since this was last asked.
    pub fn take_changed(&self) -> bool {
        self.kept.changed.replace(false)
    }

    /// A window message for it (the keys, the input method's, the pointer's, its timers'): what it answers.
    pub fn send(&self, message: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        let mut result = LRESULT(0);
        let handled = unsafe { self.services.TxSendMessage(message, wparam, lparam, &mut result) };
        handled.is_ok().then_some(result)
    }

    /// Whether `point` (in the window) is where its words are.
    pub fn holds(&self, point: POINT) -> bool {
        let rect = self.kept.rect.get();
        point.x >= rect.left && point.x < rect.right && point.y >= rect.top && point.y < rect.bottom
    }

    /// Its words drawn on `target` (in pixels: see `gfx::Frame::in_pixels`).
    pub fn draw(&self, target: &ID2D1RenderTarget) {
        let rect = self.kept.rect.get();
        let mut bounds = RECTL { left: rect.left, top: rect.top, right: rect.right, bottom: rect.bottom };
        unsafe {
            let _ = self.services.TxDrawD2D(target, &mut bounds, std::ptr::null_mut(), TXTVIEW_ACTIVE.0);
        }
    }

    /// The caret, where it is shown now (in the window): its left, top, width and height.
    pub fn caret(&self) -> Option<(i32, i32, i32, i32)> {
        self.kept.caret_shown.get().then(|| self.kept.caret.get()).flatten()
    }
}

impl Drop for Editor {
    fn drop(&mut self) {
        unsafe {
            let _ = self.services.OnTxInPlaceDeactivate();
        }
    }
}

impl ITextHost_Impl for Host_Impl {
    fn TxGetDC(&self) -> HDC {
        unsafe { GetDC(Some(self.kept.hwnd)) }
    }
    fn TxReleaseDC(&self, hdc: HDC) -> i32 {
        unsafe { ReleaseDC(Some(self.kept.hwnd), hdc) }
    }
    fn TxShowScrollBar(&self, _: i32, _: BOOL) -> BOOL {
        true.into()
    }
    fn TxEnableScrollBar(&self, _: SCROLLBAR_CONSTANTS, _: i32) -> BOOL {
        true.into()
    }
    fn TxSetScrollRange(&self, _: i32, _: i32, _: i32, _: BOOL) -> BOOL {
        true.into()
    }
    fn TxSetScrollPos(&self, _: i32, _: i32, _: BOOL) -> BOOL {
        true.into()
    }
    fn TxInvalidateRect(&self, _: *mut RECT, _: BOOL) {
        unsafe {
            let _ = InvalidateRect(Some(self.kept.hwnd), None, false);
        }
    }
    fn TxViewChange(&self, _: BOOL) {
        unsafe {
            let _ = InvalidateRect(Some(self.kept.hwnd), None, false);
        }
    }
    fn TxCreateCaret(&self, _: HBITMAP, width: i32, height: i32) -> BOOL {
        self.kept.caret_size.set((width.max(1), height));
        true.into()
    }
    fn TxShowCaret(&self, show: BOOL) -> BOOL {
        self.kept.caret_shown.set(show.as_bool());
        self.TxViewChange(false.into());
        true.into()
    }
    fn TxSetCaretPos(&self, x: i32, y: i32) -> BOOL {
        let (width, height) = self.kept.caret_size.get();
        self.kept.caret.set(Some((x, y, width, height)));
        self.TxViewChange(false.into());
        true.into()
    }
    fn TxSetTimer(&self, id: u32, timeout: u32) -> BOOL {
        unsafe { (SetTimer(Some(self.kept.hwnd), id as usize, timeout, None) != 0).into() }
    }
    fn TxKillTimer(&self, id: u32) {
        unsafe {
            let _ = KillTimer(Some(self.kept.hwnd), id as usize);
        }
    }
    fn TxScrollWindowEx(&self, _: i32, _: i32, _: *mut RECT, _: *mut RECT, _: HRGN, _: *mut RECT, _: SCROLL_WINDOW_FLAGS) {
        self.TxViewChange(false.into());
    }
    fn TxSetCapture(&self, capture: BOOL) {
        unsafe {
            if capture.as_bool() {
                SetCapture(self.kept.hwnd);
            } else {
                let _ = ReleaseCapture();
            }
        }
    }
    fn TxSetFocus(&self) {
        unsafe {
            let _ = SetFocus(Some(self.kept.hwnd));
        }
    }
    fn TxSetCursor(&self, cursor: HCURSOR, _: BOOL) {
        unsafe {
            SetCursor(Some(cursor));
        }
    }
    fn TxScreenToClient(&self, point: *mut POINT) -> BOOL {
        unsafe { ScreenToClient(self.kept.hwnd, point) }
    }
    fn TxClientToScreen(&self, point: *mut POINT) -> BOOL {
        unsafe { ClientToScreen(self.kept.hwnd, point) }
    }
    fn TxActivate(&self, _: *mut i32) -> windows::core::Result<()> {
        Ok(())
    }
    fn TxDeactivate(&self, _: i32) -> windows::core::Result<()> {
        Ok(())
    }
    fn TxGetClientRect(&self, rect: *mut RECT) -> windows::core::Result<()> {
        unsafe { *rect = self.kept.rect.get() };
        Ok(())
    }
    fn TxGetViewInset(&self, inset: *mut RECT) -> windows::core::Result<()> {
        unsafe { *inset = RECT::default() };
        Ok(())
    }
    fn TxGetCharFormat(&self, format: *const *const CHARFORMATW) -> windows::core::Result<()> {
        unsafe { *(format as *mut *const CHARFORMATW) = &self.kept.char_format.Base };
        Ok(())
    }
    fn TxGetParaFormat(&self, format: *const *const PARAFORMAT) -> windows::core::Result<()> {
        unsafe { *(format as *mut *const PARAFORMAT) = &*self.kept.para_format };
        Ok(())
    }
    fn TxGetSysColor(&self, index: SYS_COLOR_INDEX) -> COLORREF {
        unsafe { COLORREF(GetSysColor(index)) }
    }
    fn TxGetBackStyle(&self, style: *mut TXTBACKSTYLE) -> windows::core::Result<()> {
        unsafe { *style = TXTBACK_TRANSPARENT };
        Ok(())
    }
    fn TxGetMaxLength(&self, length: *mut u32) -> windows::core::Result<()> {
        unsafe { *length = 1 << 24 };
        Ok(())
    }
    fn TxGetScrollBars(&self, bars: *mut u32) -> windows::core::Result<()> {
        // (Scrolled down as it is written, by the wheel; no bar drawn of theirs.)
        unsafe { *bars = WS_VSCROLL.0 | 0x40 };
        Ok(())
    }
    fn TxGetPasswordChar(&self) -> windows::core::Result<i8> {
        Err(windows::Win32::Foundation::S_FALSE.into())
    }
    fn TxGetAcceleratorPos(&self, at: *mut i32) -> windows::core::Result<()> {
        unsafe { *at = -1 };
        Ok(())
    }
    fn TxGetExtent(&self, _: *mut SIZE) -> windows::core::Result<()> {
        Err(windows::Win32::Foundation::E_NOTIMPL.into())
    }
    fn OnTxCharFormatChange(&self, _: *const CHARFORMATW) -> windows::core::Result<()> {
        Ok(())
    }
    fn OnTxParaFormatChange(&self, _: *const PARAFORMAT) -> windows::core::Result<()> {
        Ok(())
    }
    fn TxGetPropertyBits(&self, mask: u32, bits: *mut u32) -> windows::core::Result<()> {
        let ours = TXTBIT_RICHTEXT | TXTBIT_MULTILINE | TXTBIT_WORDWRAP | TXTBIT_D2DDWRITE | TXTBIT_D2DSIMPLETYPOGRAPHY | TXTBIT_D2DPIXELSNAPPED;
        unsafe { *bits = ours & mask };
        Ok(())
    }
    fn TxNotify(&self, notify: u32, _: *mut core::ffi::c_void) -> windows::core::Result<()> {
        if notify == windows::Win32::UI::WindowsAndMessaging::EN_CHANGE {
            self.kept.changed.set(true);
        }
        Ok(())
    }
    fn TxImmGetContext(&self) -> HIMC {
        unsafe { ImmGetContext(self.kept.hwnd) }
    }
    fn TxImmReleaseContext(&self, context: HIMC) {
        unsafe {
            let _ = ImmReleaseContext(self.kept.hwnd, context);
        }
    }
    fn TxGetSelectionBarWidth(&self, width: *mut i32) -> windows::core::Result<()> {
        unsafe { *width = 0 };
        Ok(())
    }
}

impl ITextHost2_Impl for Host_Impl {
    fn TxIsDoubleClickPending(&self) -> BOOL {
        false.into()
    }
    fn TxGetWindow(&self, hwnd: *mut HWND) -> windows::core::Result<()> {
        unsafe { *hwnd = self.kept.hwnd };
        Ok(())
    }
    fn TxSetForegroundWindow(&self) -> windows::core::Result<()> {
        unsafe {
            let _ = SetForegroundWindow(self.kept.hwnd);
        }
        Ok(())
    }
    fn TxGetPalette(&self) -> HPALETTE {
        HPALETTE::default()
    }
    fn TxGetEastAsianFlags(&self, flags: *mut i32) -> windows::core::Result<()> {
        unsafe { *flags = 0 };
        Ok(())
    }
    fn TxSetCursor2(&self, cursor: HCURSOR, _: BOOL) -> HCURSOR {
        unsafe { SetCursor(Some(cursor)) }
    }
    fn TxFreeTextServicesNotification(&self) {}
    fn TxGetEditStyle(&self, _: u32, data: *mut u32) -> windows::core::Result<()> {
        unsafe { *data = 0 };
        Ok(())
    }
    fn TxGetWindowStyles(&self, style: *mut u32, extended: *mut u32) -> windows::core::Result<()> {
        unsafe {
            *style = 0;
            *extended = 0;
        }
        Ok(())
    }
    fn TxShowDropCaret(&self, _: BOOL, _: HDC, _: *mut RECT) -> windows::core::Result<()> {
        Ok(())
    }
    fn TxDestroyCaret(&self) -> windows::core::Result<()> {
        self.kept.caret.set(None);
        Ok(())
    }
    fn TxGetHorzExtent(&self, extent: *mut i32) -> windows::core::Result<()> {
        unsafe { *extent = 0 };
        Ok(())
    }
}
