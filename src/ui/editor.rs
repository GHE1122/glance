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
use windows::Win32::UI::HiDpi::{SetThreadDpiAwarenessContext, DPI_AWARENESS_CONTEXT_UNAWARE};
use windows::Win32::UI::Input::Ime::{ImmGetContext, ImmReleaseContext, HIMC};
use windows::Win32::UI::Input::KeyboardAndMouse::{ReleaseCapture, SetCapture, SetFocus};
use windows::Win32::UI::WindowsAndMessaging::{KillTimer, SetCursor, SetForegroundWindow, SetTimer, HCURSOR, SCROLLBAR_CONSTANTS, SCROLL_WINDOW_FLAGS, WS_VSCROLL};

/// The edit control's message asking which character is at a point (winuser.h's `EM_CHARFROMPOS`; the bindings leave it
/// out): a rich edit box answers with the character's place.
const EM_CHARFROMPOS: u32 = 0x00D7;

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

/// The colours and faces a box's words are dressed in (see `Editor::dress`): its words', faint, the accent; its size
/// (points, as its words are written); its face, and code's.
pub struct Palette {
    pub text: COLORREF,
    pub faint: COLORREF,
    pub accent: COLORREF,
    pub points: f32,
    pub face: String,
    pub code_face: String,
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
            // (Made as a thread unaware of its screen's scale would make them: they take the scale they count in as
            // they are made, and count in pixels at 96 per inch as the words are drawn (see `draw`). Made at the
            // window's scale, they would place the caret, and the input method's window, at its multiple of where the
            // words are.)
            let was = SetThreadDpiAwarenessContext(DPI_AWARENESS_CONTEXT_UNAWARE);
            let made_now = make(None.into(), (&plain).into(), windows::core::OutRef::from(&mut made));
            SetThreadDpiAwarenessContext(was);
            made_now.ok().ok()?;
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
        // (Each line break one carriage return, as the text services keep them: Windows' pairs as one too.)
        unsafe {
            let _ = self.services.TxSetText(&HSTRING::from(text.replace("\r\n", "\n").replace('\n', "\r")));
        }
        self.kept.changed.set(false);
    }

    /// Whether what is written changed since this was last asked.
    pub fn take_changed(&self) -> bool {
        self.kept.changed.replace(false)
    }

    /// A window message for it (the keys, the input method's, the pointer's, its timers'): what it answers.
    /// (None: not theirs, `S_FALSE`, for the window's own handling to see to, as an input method's message may be.)
    pub fn send(&self, message: u32, wparam: WPARAM, lparam: LPARAM) -> Option<LRESULT> {
        let mut result = LRESULT(0);
        // (Called through its table, its answer kept whole: the bindings' call takes S_FALSE for done.)
        let answer = unsafe { (Interface::vtable(&self.services).base__.TxSendMessage)(Interface::as_raw(&self.services), message, wparam, lparam, &mut result) };
        (answer == windows::Win32::Foundation::S_OK).then_some(result)
    }

    /// Its words dressed as `looks` says (see `notes::markdown`), in `palette`'s colours and faces: what they are
    /// left as it is, the dressing kept out of what undo takes back (and drawn once, done).
    pub fn dress(&self, looks: &[(std::ops::Range<usize>, crate::notes::markdown::Look)], palette: &Palette) {
        use crate::notes::markdown::Look;
        let Ok(document) = self.services.cast::<ITextDocument>() else { return };
        let face = BSTR::from(palette.face.as_str());
        let code = BSTR::from(palette.code_face.as_str());
        let color = |c: COLORREF| c.0 as i32;
        unsafe {
            let _ = document.Undo(tomSuspend.0);
            let _ = document.Freeze();
            // All as plain words first; then each stretch as it looks.
            if let Ok(font) = document.Range(0, i32::MAX).and_then(|all| all.GetFont()) {
                let _ = font.SetBold(tomFalse.0);
                let _ = font.SetItalic(tomFalse.0);
                let _ = font.SetStrikeThrough(tomFalse.0);
                let _ = font.SetSize(palette.points);
                let _ = font.SetName(&face);
                let _ = font.SetForeColor(color(palette.text));
            }
            for (range, look) in looks {
                let Ok(font) = document.Range(range.start as i32, range.end as i32).and_then(|range| range.GetFont()) else { continue };
                let _ = match look {
                    Look::Heading(level) => {
                        let _ = font.SetBold(tomTrue.0);
                        font.SetSize(palette.points * match level {
                            1 => 1.6,
                            2 => 1.35,
                            3 => 1.2,
                            _ => 1.08,
                        })
                    }
                    Look::Bold => font.SetBold(tomTrue.0),
                    Look::Italic => font.SetItalic(tomTrue.0),
                    Look::Struck => font.SetStrikeThrough(tomTrue.0),
                    Look::Code => font.SetName(&code),
                    Look::Link => font.SetForeColor(color(palette.accent)),
                    Look::Faint => font.SetForeColor(color(palette.faint)),
                    Look::Plain => font.SetForeColor(color(palette.text)),
                };
            }
            let _ = document.Unfreeze();
            let _ = document.Undo(tomResume.0);
        }
        // (Dressing is no writing.)
        self.kept.changed.set(false);
    }

    /// Where among its words (in UTF-16 units) the point `point` (in the window) is.
    pub fn at(&self, point: POINT) -> usize {
        let mut at = windows::Win32::Foundation::POINTL { x: point.x, y: point.y };
        self.send(EM_CHARFROMPOS, WPARAM(0), LPARAM(&mut at as *mut _ as isize)).map_or(0, |found| found.0.max(0) as usize)
    }

    /// The words from `start` to `end` (UTF-16 units) written over with `text`, as if typed (undo takes it back).
    pub fn replace(&self, start: usize, end: usize, text: &str) {
        let Ok(document) = self.services.cast::<ITextDocument>() else { return };
        unsafe {
            if let Ok(range) = document.Range(start as i32, end as i32) {
                let _ = range.SetText(&BSTR::from(text));
            }
        }
        self.kept.changed.set(true);
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
    /// (Not shut down with `ShutdownTextServices`: their host is counted, held as long as they are, so a reference to
    /// them kept elsewhere finds it still there; shut down, msftedit lets go of the host a second time as the last such
    /// reference goes.)
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

#[cfg(test)]
mod tests {
    use super::*;
    use crate::notes::markdown::looks;
    use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, DestroyWindow, WINDOW_EX_STYLE, WS_POPUP};

    /// The caret where the words drawn end, the window aware of its screen's scale as Glance's are (on a screen scaled
    /// past 100%: the text services count in the window's pixels whatever the scale, as Glance draws them).
    #[test]
    #[ignore = "makes a window of this session's"]
    fn puts_the_caret_where_the_words_end() {
        use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_PIXEL_FORMAT};
        use windows::Win32::Graphics::Direct2D::{D2D1CreateFactory, ID2D1Factory, D2D1_FACTORY_TYPE_SINGLE_THREADED, D2D1_RENDER_TARGET_PROPERTIES, D2D1_RENDER_TARGET_TYPE_DEFAULT};
        use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
        use windows::Win32::Graphics::Imaging::{CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory, WICBitmapCacheOnLoad};
        unsafe {
            let _ = windows::Win32::UI::HiDpi::SetThreadDpiAwarenessContext(windows::Win32::UI::HiDpi::DPI_AWARENESS_CONTEXT_PER_MONITOR_AWARE_V2);
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
            let hwnd = CreateWindowExW(WINDOW_EX_STYLE(0), windows::core::w!("STATIC"), windows::core::w!(""), WS_POPUP, 0, 0, 800, 300, None, None, None, None).unwrap();
            let (wide, tall) = (800u32, 300u32);
            let editor = Editor::new(hwnd, RECT { left: 20, top: 30, right: 780, bottom: 280 }, "Segoe UI", 22.0, COLORREF(0)).expect("text services");
            editor.set_text("MMMMMMMMMM second line");
            let _ = editor.send(windows::Win32::UI::WindowsAndMessaging::WM_SETFOCUS, WPARAM(0), LPARAM(0));
            let document: ITextDocument = editor.services.cast().unwrap();
            let _ = document.Range(i32::MAX, i32::MAX).unwrap().Select();
            let wic: IWICImagingFactory = windows::Win32::System::Com::CoCreateInstance(&CLSID_WICImagingFactory, None, windows::Win32::System::Com::CLSCTX_INPROC_SERVER).unwrap();
            let bitmap = wic.CreateBitmap(wide, tall, &GUID_WICPixelFormat32bppPBGRA, WICBitmapCacheOnLoad).unwrap();
            let factory: ID2D1Factory = D2D1CreateFactory(D2D1_FACTORY_TYPE_SINGLE_THREADED, None).unwrap();
            let properties = D2D1_RENDER_TARGET_PROPERTIES { r#type: D2D1_RENDER_TARGET_TYPE_DEFAULT, pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED }, dpiX: 96.0, dpiY: 96.0, ..Default::default() };
            let target = factory.CreateWicBitmapRenderTarget(&bitmap, &properties).unwrap();
            target.BeginDraw();
            target.Clear(Some(&D2D1_COLOR_F { r: 1.0, g: 1.0, b: 1.0, a: 1.0 }));
            editor.draw(&target);
            target.EndDraw(None, None).unwrap();
            let mut pixels = vec![0u8; (wide * tall * 4) as usize];
            bitmap.CopyPixels(std::ptr::null(), wide * 4, &mut pixels).unwrap();
            let inked = |x: u32, y: u32| pixels[((y * wide + x) * 4) as usize] < 128;
            // The words' right end, and their lines' top and bottom.
            let right = (0..wide).rev().find(|&x| (0..tall).any(|y| inked(x, y))).expect("words drawn");
            let bottom = (0..tall).rev().find(|&y| (0..wide).any(|x| inked(x, y))).unwrap();
            let caret = editor.caret().expect("a caret");
            drop(editor);
            let _ = DestroyWindow(hwnd);
            assert!((caret.0 - right as i32).abs() <= 6, "caret at x {} where the words end at {right}", caret.0);
            assert!(caret.1 <= bottom as i32 && caret.1 + caret.3 >= bottom as i32 - 4, "caret at y {}..{} where the words' last line ends at {bottom}", caret.1, caret.1 + caret.3);
        }
    }

    /// The text services hosted here take keys as written and give the words back; dressed as Markdown, a heading is
    /// bold and larger, its words otherwise as typed; undo takes back what was typed, not the dressing.
    #[test]
    #[ignore = "makes a window of this session's"]
    fn writes_dresses_and_undoes() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
            let hwnd = CreateWindowExW(WINDOW_EX_STYLE(0), windows::core::w!("STATIC"), windows::core::w!(""), WS_POPUP, 0, 0, 400, 300, None, None, None, None).unwrap();
            let editor = Editor::new(hwnd, RECT { left: 0, top: 0, right: 400, bottom: 300 }, "Microsoft YaHei UI", 20.0, COLORREF(0)).expect("text services");
            let palette = Palette { text: COLORREF(0x202020), faint: COLORREF(0x909090), accent: COLORREF(0xD07000), points: 15.0, face: "Microsoft YaHei UI".into(), code_face: "Cascadia Mono".into() };
            editor.set_text("# Title\nbody");
            editor.dress(&looks(&editor.text()), &palette);
            let document: ITextDocument = editor.services.cast().unwrap();
            let font_at = |at: i32| document.Range(at, at + 1).unwrap().GetFont().unwrap();
            // "T" (2) bold and larger; "b" of body (8) neither.
            assert_eq!(font_at(2).GetBold().unwrap(), tomTrue.0);
            assert!(font_at(2).GetSize().unwrap() > 20.0);
            assert_eq!(font_at(8).GetBold().unwrap(), tomFalse.0);
            // "#" (0) faint.
            assert_eq!(font_at(0).GetForeColor().unwrap(), 0x909090);
            // Typed at its end, dressed again, then undone: what was typed goes, the dressing stays.
            let _ = editor.send(windows::Win32::UI::WindowsAndMessaging::WM_SETFOCUS, WPARAM(0), LPARAM(0));
            let _ = document.Range(i32::MAX, i32::MAX).unwrap().Select();
            for c in "!".encode_utf16() {
                let _ = editor.send(windows::Win32::UI::WindowsAndMessaging::WM_CHAR, WPARAM(usize::from(c)), LPARAM(0));
            }
            assert_eq!(editor.text().trim_end(), "# Title\nbody!");
            editor.dress(&looks(&editor.text()), &palette);
            let _ = editor.send(windows::Win32::UI::WindowsAndMessaging::WM_UNDO, WPARAM(0), LPARAM(0));
            assert_eq!(editor.text().trim_end(), "# Title\nbody");
            assert_eq!(font_at(2).GetBold().unwrap(), tomTrue.0);
            // A to-do's box ticked as if typed: undo takes it back.
            editor.set_text("- [ ] 买牛奶");
            let (range, done) = crate::notes::markdown::tasks(&editor.text()).remove(0);
            editor.replace(range.start, range.end, if done { "[ ]" } else { "[x]" });
            assert_eq!(editor.text().trim_end(), "- [x] 买牛奶");
            let _ = editor.send(windows::Win32::UI::WindowsAndMessaging::WM_UNDO, WPARAM(0), LPARAM(0));
            assert_eq!(editor.text().trim_end(), "- [ ] 买牛奶");
            // Chinese and an emoji written and given back as written.
            editor.set_text("你好 😀");
            assert_eq!(editor.text().trim_end(), "你好 😀");
            // A file written on Windows: its line breaks one each, not two.
            editor.set_text("一\r\n二");
            assert_eq!(editor.text().trim_end(), "一\n二");
            drop(editor);
            let _ = DestroyWindow(hwnd);
        }
    }
}
