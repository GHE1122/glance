//! A window that asks before something is done, or leaves this PC, drawn as
//! Glance's own windows are (Windows 11's dialog, light or dark): its title
//! and what it is about, and, for something sent, everything that is sent,
//! each part under its name: the sender's own words (typed here), their
//! pictures, and what Glance adds, whole, in a box that scrolls. Two buttons
//! answer it; the window it is over takes nothing till it is answered.

use std::cell::RefCell;
use std::rc::Rc;

use windows::core::{w, BOOL, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HANDLE, HGLOBAL, HWND, LPARAM, LRESULT, POINT, RECT, WPARAM};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_PIXEL_FORMAT, D2D_SIZE_U};
use windows::Win32::Graphics::Direct2D::{D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_BITMAP_OPTIONS_NONE, D2D1_BITMAP_PROPERTIES1, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_LAYER_PARAMETERS1, D2D1_ROUNDED_RECT};
use windows::Win32::Graphics::DirectWrite::{IDWriteTextLayout, DWRITE_HIT_TEST_METRICS, DWRITE_LINE_METRICS, DWRITE_TEXT_METRICS};
use windows::Win32::Graphics::Dwm::{DwmExtendFrameIntoClientArea, DwmSetWindowAttribute, DWMWA_WINDOW_CORNER_PREFERENCE, DWMWCP_ROUND};
use windows::Win32::Graphics::Dxgi::Common::DXGI_FORMAT_B8G8R8A8_UNORM;
use windows::Win32::Graphics::Gdi::{GetMonitorInfoW, InvalidateRect, MonitorFromWindow, ScreenToClient, ValidateRect, LOGFONTW, MONITORINFO, MONITOR_DEFAULTTONEAREST};
use windows::Win32::System::DataExchange::{AddClipboardFormatListener, CloseClipboard, EmptyClipboard, GetClipboardData, OpenClipboard, RemoveClipboardFormatListener, SetClipboardData};
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Memory::{GlobalAlloc, GlobalLock, GlobalUnlock, GMEM_MOVEABLE};
use windows::Win32::UI::Controls::{MARGINS, WM_MOUSELEAVE};
use windows::Win32::UI::Shell::{DragAcceptFiles, DragFinish, DragQueryFileW, HDROP};
use windows::Win32::UI::HiDpi::GetDpiForWindow;
use windows::Win32::UI::Input::Ime::{
    ImmAssociateContextEx, ImmGetContext, ImmReleaseContext, ImmSetCandidateWindow, ImmSetCompositionFontW, ImmSetCompositionWindow, CANDIDATEFORM, CFS_EXCLUDE, CFS_POINT, COMPOSITIONFORM, HIMC, IACE_DEFAULT,
};
use windows::Win32::UI::Input::KeyboardAndMouse::{
    EnableWindow, GetKeyState, ReleaseCapture, SetCapture, SetFocus, TrackMouseEvent, TME_LEAVE, TRACKMOUSEEVENT, VIRTUAL_KEY, VK_A, VK_BACK, VK_C, VK_CONTROL, VK_DELETE, VK_DOWN, VK_END, VK_ESCAPE, VK_HOME, VK_LEFT, VK_NEXT,
    VK_PRIOR, VK_RETURN, VK_RIGHT, VK_SHIFT, VK_SPACE, VK_TAB, VK_UP, VK_V, VK_X,
};
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, DispatchMessageW, GetCaretBlinkTime, GetClientRect, GetMessageW, GetWindowRect, IsWindow, KillTimer, LoadCursorW, PostQuitMessage, RegisterClassExW, SetCursor,
    SetForegroundWindow, SetTimer, SetWindowPos, ShowWindow, TranslateMessage, HTCAPTION, HTCLIENT, IDC_ARROW, IDC_IBEAM, MSG, SWP_NOACTIVATE, SWP_NOZORDER, SW_SHOW, WM_CHAR, WM_CLIPBOARDUPDATE, WM_CLOSE,
    WM_DPICHANGED, WM_IME_STARTCOMPOSITION, WM_KEYDOWN, WM_LBUTTONDOWN, WM_LBUTTONUP, WM_MOUSEMOVE, WM_MOUSEWHEEL, WM_NCCALCSIZE, WM_NCHITTEST, WM_PAINT, WM_SETCURSOR, WM_TIMER, WNDCLASSEXW,
    WS_CAPTION, WS_EX_NOREDIRECTIONBITMAP, WS_POPUP, WS_SYSMENU, ChangeWindowMessageFilterEx, MSGFLT_ALLOW, WM_COPYDATA, WM_DROPFILES,
};

use super::canvas::{Align, Canvas, Color, Family, Fill, Font, Point};
use super::gfx::{self, rect, Frame, Gfx, Surface};
use super::icons::Icon;
use super::picture::{self, Picture};
use super::view::Rect;

/// What the window asks: its title, what it is about (in as many lines as
/// that takes), the reader's own part if they may add one, what is sent
/// besides (under its name), the two answers, and whether it is dark.
pub struct Asked<'a> {
    pub title: &'a str,
    pub intro: &'a str,
    pub own: Option<Own<'a>>,
    pub sent: Option<(&'a str, &'a str)>,
    pub yes: &'a str,
    pub no: &'a str,
    pub dark: bool,
}

/// What the reader's own part is called: the heading over the box for
/// their words and what the box says while it is empty; the heading over
/// their pictures; the place that adds one (what it is, and the ways it is
/// done), its button that pastes the clipboard's picture and what it says
/// of that picture where there is no room for the button; and pictures, as
/// the file chooser lists them.
pub struct Own<'a> {
    pub words: &'a str,
    pub hint: &'a str,
    pub pictures: &'a str,
    pub add: &'a str,
    pub ways: &'a str,
    pub paste: &'a str,
    pub waiting: &'a str,
    pub kinds: &'a str,
}

/// The same, kept by the window while it is up.
#[derive(Clone)]
struct OwnText {
    words: String,
    hint: String,
    pictures: String,
    add: String,
    ways: String,
    paste: String,
    waiting: String,
    kinds: String,
}

/// `picture`'s likeness covering `tile`, its middle kept, the corners rounded by `radius`.
fn covering(frame: &Frame, picture: &Picture, tile: Rect, radius: f32, scale: f32) {
    let properties = D2D1_BITMAP_PROPERTIES1 {
        pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
        dpiX: 96.0 * scale,
        dpiY: 96.0 * scale,
        bitmapOptions: D2D1_BITMAP_OPTIONS_NONE,
        ..Default::default()
    };
    let (width, height) = picture.size;
    let Ok(bitmap) = (unsafe { frame.dc.CreateBitmap(D2D_SIZE_U { width, height }, Some(picture.small.as_ptr().cast()), width * 4, &properties) }) else { return };
    // The part of it (in DIPs, as the bitmap is laid out at this scale) of the tile's shape, about its middle.
    let (w, h) = (width as f32 / scale, height as f32 / scale);
    let fit = (w / tile.w).min(h / tile.h);
    let (cut_w, cut_h) = (tile.w * fit, tile.h * fit);
    let source = rect((w - cut_w) / 2.0, (h - cut_h) / 2.0, cut_w, cut_h);
    let Ok(mask) = (unsafe { frame.gfx.factory.CreateRoundedRectangleGeometry(&D2D1_ROUNDED_RECT { rect: rect(tile.x, tile.y, tile.w, tile.h), radiusX: radius, radiusY: radius }) }) else { return };
    let layer = D2D1_LAYER_PARAMETERS1 {
        contentBounds: rect(tile.x, tile.y, tile.w, tile.h),
        geometricMask: std::mem::ManuallyDrop::new(windows::core::Interface::cast(&mask).ok()),
        maskAntialiasMode: D2D1_ANTIALIAS_MODE_PER_PRIMITIVE,
        maskTransform: windows_numerics::Matrix3x2::identity(),
        opacity: 1.0,
        ..Default::default()
    };
    unsafe {
        frame.dc.PushLayer(&layer, None);
        frame.dc.DrawBitmap(&bitmap, Some(&rect(tile.x, tile.y, tile.w, tile.h)), 1.0, D2D1_INTERPOLATION_MODE_LINEAR, Some(&source), None);
        frame.dc.PopLayer();
    }
    drop(std::mem::ManuallyDrop::into_inner(layer.geometricMask));
}

/// What the reader gave of their own: their words (none, perhaps) and
/// their pictures, each a JPEG.
pub struct Said {
    pub words: String,
    pub pictures: Vec<Vec<u8>>,
}

/// How many characters (UTF-16 units) may be written, and how many pictures added (the site takes no more).
const WORDS_MOST: usize = 500;
pub const PICTURES_MOST: usize = 3;

/// Its measures (DIPs): its width, and its height where it has a part that
/// scrolls; the room round its content; the space between parts and under a
/// heading; the footer and its buttons; the box for words; a picture's
/// tile; the room inside a box round its text; and a turn of the wheel.
const WIDTH: f32 = 600.0;
const TALL: f32 = 760.0;
const PAD: f32 = 24.0;
const GAP: f32 = 20.0;
const HEAD: f32 = 28.0;
const FOOTER: f32 = 80.0;
const BUTTON: f32 = 32.0;
const WORDS_HEIGHT: f32 = 88.0;
const TILE: (f32, f32) = (120.0, 76.0);
const TILE_GAP: f32 = 8.0;
const INSET: (f32, f32) = (11.0, 8.0);
const WHEEL: f32 = 48.0;
/// How far down the window's top may be held to move it.
const CAPTION: f32 = 56.0;

const TITLE_FONT: Font = Font::new(Family::Segoe, 20.0, 600.0);
const BODY: Font = Font::new(Family::Segoe, 14.0, 400.0);
const HEADING: Font = Font::new(Family::Segoe, 14.0, 600.0);
const SMALL: Font = Font::new(Family::Segoe, 12.0, 400.0);

/// What Windows sends a window that files are dropped on, besides WM_DROPFILES (WM_COPYGLOBALDATA).
const COPY_GLOBAL_DATA: u32 = 0x0049;

/// The clipboard's name for text (CF_UNICODETEXT).
const TEXT: u32 = 13;

/// What a point in the window is on, and what the keyboard may be on.
#[derive(Clone, Copy, PartialEq, Debug)]
enum Hit {
    Words,
    Add,
    Paste,
    Picture(usize),
    Remove(usize),
    Sent,
    Thumb,
    Yes,
    No,
}

/// What the window is drawn in, light or dark, as Windows 11's dialogs are.
struct Colours {
    content: Color,
    footer: Color,
    text: Color,
    text2: Color,
    text3: Color,
    field: Color,
    field_stroke: Color,
    field_bottom: Color,
    sent: Color,
    button: Color,
    button_hover: Color,
    button_stroke: Color,
    accent: Color,
    selection: Color,
    scroll: Color,
    ring: Color,
    shade: Color,
}

impl Colours {
    fn new(dark: bool) -> Self {
        let white = |a| Color::hex(0xFFFFFF, a);
        let black = |a| Color::hex(0x000000, a);
        let (accent, selection) = (super::theme::accent(false), super::theme::accent(dark).alpha(0.4));
        if dark {
            Colours {
                content: Color::hex(0x2B2B2B, 1.0),
                footer: Color::hex(0x202020, 1.0),
                text: white(1.0),
                text2: white(0.76),
                text3: white(0.5),
                field: white(0.06),
                field_stroke: white(0.08),
                field_bottom: white(0.55),
                sent: black(0.18),
                button: white(0.06),
                button_hover: white(0.09),
                button_stroke: white(0.07),
                accent,
                selection,
                scroll: white(0.5),
                ring: white(1.0),
                shade: black(0.2),
            }
        } else {
            Colours {
                content: Color::hex(0xFFFFFF, 1.0),
                footer: Color::hex(0xF3F3F3, 1.0),
                text: black(0.9),
                text2: black(0.62),
                text3: black(0.45),
                field: white(1.0),
                field_stroke: black(0.1),
                field_bottom: black(0.45),
                sent: black(0.025),
                button: white(1.0),
                button_hover: Color::hex(0xF6F6F6, 1.0),
                button_stroke: black(0.1),
                accent,
                selection,
                scroll: black(0.45),
                ring: black(0.9),
                shade: black(0.12),
            }
        }
    }
}

/// The words being written: UTF-16, as Windows hands them over and
/// DirectWrite counts them; where the caret is and where the selection
/// started (the same, with none); how far the box is scrolled; and whether
/// it is to scroll to show the caret when next drawn.
#[derive(Default)]
struct Typing {
    text: Vec<u16>,
    caret: usize,
    anchor: usize,
    top: f32,
    follow: bool,
}

fn high(unit: u16) -> bool {
    (0xD800..0xDC00).contains(&unit)
}

fn low(unit: u16) -> bool {
    (0xDC00..0xE000).contains(&unit)
}

impl Typing {
    fn selection(&self) -> std::ops::Range<usize> {
        self.caret.min(self.anchor)..self.caret.max(self.anchor)
    }

    fn string(&self) -> String {
        String::from_utf16_lossy(&self.text)
    }

    /// The caret at `to`, the selection kept from where it started if `extend`.
    fn go(&mut self, to: usize, extend: bool) {
        self.caret = to.min(self.text.len());
        if !extend {
            self.anchor = self.caret;
        }
        self.follow = true;
    }

    /// From `at`, one character on (or back): a pair of surrogates is one.
    fn step(&self, at: usize, forward: bool) -> usize {
        if forward {
            if at >= self.text.len() {
                return at;
            }
            if high(self.text[at]) && self.text.get(at + 1).is_some_and(|&next| low(next)) { at + 2 } else { at + 1 }
        } else {
            if at == 0 {
                return 0;
            }
            if at >= 2 && low(self.text[at - 1]) && high(self.text[at - 2]) { at - 2 } else { at - 1 }
        }
    }

    /// `put` in place of the selection, as much of it as there is room for (no pair of surrogates cut).
    fn put(&mut self, put: &[u16]) {
        let selection = self.selection();
        let room = WORDS_MOST - (self.text.len() - selection.len());
        let mut taken = put.len().min(room);
        if taken > 0 && taken < put.len() && high(put[taken - 1]) {
            taken -= 1;
        }
        self.text.splice(selection.clone(), put[..taken].iter().copied());
        self.go(selection.start + taken, false);
    }

    /// The selection taken away, or with none, the character before (or after) the caret.
    fn erase(&mut self, forward: bool) {
        if self.caret == self.anchor {
            self.anchor = self.step(self.caret, forward);
        }
        self.put(&[]);
    }
}

/// Where in `layout` the point `(x, y)` is, as the position a caret put there would have.
fn at_point(layout: &IDWriteTextLayout, x: f32, y: f32) -> usize {
    let (mut trailing, mut inside, mut metrics) = (BOOL(0), BOOL(0), DWRITE_HIT_TEST_METRICS::default());
    let _ = unsafe { layout.HitTestPoint(x, y, &mut trailing, &mut inside, &mut metrics) };
    (metrics.textPosition + if trailing.as_bool() { metrics.length } else { 0 }) as usize
}

/// Where a caret at `at` is drawn in `layout`: its left, its top, and how tall.
fn caret_at(layout: &IDWriteTextLayout, at: usize) -> (f32, f32, f32) {
    let (mut x, mut y, mut metrics) = (0.0, 0.0, DWRITE_HIT_TEST_METRICS::default());
    let _ = unsafe { layout.HitTestTextPosition(at as u32, false, &mut x, &mut y, &mut metrics) };
    (x, metrics.top, metrics.height)
}

/// The line of `layout` that `at` is on: where it starts and ends (before its line break).
fn line_of(layout: &IDWriteTextLayout, at: usize) -> (usize, usize) {
    let mut count = 0;
    let _ = unsafe { layout.GetLineMetrics(None, &mut count) };
    let mut lines = vec![DWRITE_LINE_METRICS::default(); count as usize];
    let _ = unsafe { layout.GetLineMetrics(Some(&mut lines), &mut count) };
    let mut start = 0;
    for (index, line) in lines.iter().enumerate() {
        let end = start + line.length as usize;
        if at < end || index + 1 == lines.len() {
            return (start, end - line.newlineLength as usize);
        }
        start = end;
    }
    (0, 0)
}

fn height_of(layout: &IDWriteTextLayout) -> f32 {
    let mut metrics = DWRITE_TEXT_METRICS::default();
    let _ = unsafe { layout.GetMetrics(&mut metrics) };
    metrics.height
}

/// The text on the clipboard, its lines broken by `\n` alone, its tabs as spaces and nothing else unprintable.
fn clipboard_text(owner: HWND) -> Vec<u16> {
    let mut taken = Vec::new();
    unsafe {
        if OpenClipboard(Some(owner)).is_err() {
            return taken;
        }
        if let Ok(held) = GetClipboardData(TEXT) {
            let memory = HGLOBAL(held.0);
            let units = GlobalLock(memory) as *const u16;
            if !units.is_null() {
                let mut at = 0;
                while *units.add(at) != 0 {
                    taken.push(*units.add(at));
                    at += 1;
                }
                let _ = GlobalUnlock(memory);
            }
        }
        let _ = CloseClipboard();
    }
    let text = String::from_utf16_lossy(&taken).replace("\r\n", "\n").replace(['\r'], "\n").replace('\t', " ");
    text.chars().filter(|&c| c == '\n' || !c.is_control()).collect::<String>().encode_utf16().collect()
}

/// `text` put on the clipboard.
fn copy_text(owner: HWND, text: &[u16]) {
    unsafe {
        if OpenClipboard(Some(owner)).is_err() {
            return;
        }
        let _ = EmptyClipboard();
        if let Ok(memory) = GlobalAlloc(GMEM_MOVEABLE, (text.len() + 1) * 2) {
            let units = GlobalLock(memory) as *mut u16;
            if !units.is_null() {
                std::ptr::copy_nonoverlapping(text.as_ptr(), units, text.len());
                *units.add(text.len()) = 0;
                let _ = GlobalUnlock(memory);
                // (The clipboard owns it from here.)
                let _ = SetClipboardData(TEXT, Some(HANDLE(memory.0)));
            }
        }
        let _ = CloseClipboard();
    }
}

/// The window while it is up.
struct Dialog {
    hwnd: HWND,
    gfx: Rc<Gfx>,
    surface: Option<Surface>,
    scale: f32,
    dark: bool,
    title: String,
    intro: String,
    own: Option<OwnText>,
    /// Whether the button that pastes the clipboard's picture was drawn (there was room for it).
    chip_shown: bool,
    sent: Option<(String, String)>,
    yes: String,
    no: String,
    typing: Typing,
    pictures: Vec<Picture>,
    /// How far what is sent is scrolled, and the most it can be.
    scroll: f32,
    scroll_most: f32,
    /// Its scroll bar: where its track starts, how long it is, and its thumb's length.
    track: (f32, f32, f32),
    pointer: Option<(f32, f32)>,
    pressed: Option<Hit>,
    /// Words being selected with the pointer, or the scroll bar held (this far down its thumb).
    selecting: bool,
    held: Option<f32>,
    focus: Hit,
    /// The focus shown, as it is once the keyboard has moved it.
    keyboard: bool,
    caret_on: bool,
    hits: Vec<(Rect, Hit)>,
    /// Those of the frame being drawn: they become `hits` once it is drawn whole (what is pointed at, while it is
    /// drawn, is what was under the pointer in the frame before).
    drawn: Vec<(Rect, Hit)>,
    /// The box the words are written in (inside its border), and where the caret was drawn.
    words_inside: Rect,
    caret: Option<Rect>,
    answer: Option<bool>,
    /// The file chooser asked for: it is shown from the window's loop, outside its messages.
    choose_files: bool,
}

thread_local! {
    static DIALOG: RefCell<Option<Dialog>> = const { RefCell::new(None) };
}

#[cfg(test)]
thread_local! {
    /// Pictures a test has the window hold as it comes up (their files).
    static GIVEN_FOR_TEST: RefCell<Vec<String>> = const { RefCell::new(Vec::new()) };
    /// Where a test has the pointer be, whatever the real one does (DIPs).
    static POINTER_FOR_TEST: std::cell::Cell<Option<(f32, f32)>> = const { std::cell::Cell::new(None) };
}

fn with<R>(act: impl FnOnce(&mut Dialog) -> R) -> Option<R> {
    DIALOG.with(|cell| cell.try_borrow_mut().ok().and_then(|mut dialog| dialog.as_mut().map(act)))
}

fn contains(r: Rect, x: f32, y: f32) -> bool {
    x >= r.x && x < r.x + r.w && y >= r.y && y < r.y + r.h
}

impl Dialog {
    fn redraw(&self) {
        unsafe {
            let _ = InvalidateRect(Some(self.hwnd), None, false);
        }
    }

    /// How tall it is, laid out `WIDTH` wide (DIPs): as tall as what it says, or, with a part that scrolls, `TALL`.
    fn height(&self) -> f32 {
        if self.own.is_some() || self.sent.is_some() {
            return TALL;
        }
        let inner = WIDTH - 2.0 * PAD;
        PAD + self.gfx.wrapped_height(&self.title, TITLE_FONT, inner) + 12.0 + self.gfx.wrapped_height(&self.intro, BODY, inner) + PAD + FOOTER
    }

    fn hit_at(&self, x: f32, y: f32) -> Option<Hit> {
        self.hits.iter().rev().find(|(r, _)| contains(*r, x, y)).map(|(_, hit)| *hit)
    }

    fn hovered(&self) -> Option<Hit> {
        #[cfg(test)]
        if let Some((x, y)) = POINTER_FOR_TEST.get() {
            return self.hit_at(x, y);
        }
        self.pointer.and_then(|(x, y)| self.hit_at(x, y))
    }

    /// Whether the window may be moved from `(x, y)`: its top, where nothing is to be pressed.
    fn caption(&self, x: f32, y: f32) -> bool {
        y < CAPTION && self.hit_at(x, y).is_none()
    }

    fn over_words(&self) -> bool {
        self.pointer.is_some_and(|(x, y)| self.hit_at(x, y) == Some(Hit::Words))
    }

    /// What the keyboard can go to, in order.
    fn stops(&self) -> Vec<Hit> {
        let mut stops = Vec::new();
        if self.own.is_some() {
            stops.push(Hit::Words);
            stops.extend((0..self.pictures.len()).map(Hit::Remove));
            if self.pictures.len() < PICTURES_MOST {
                stops.push(Hit::Add);
                if self.chip_shown {
                    stops.push(Hit::Paste);
                }
            }
        }
        if self.sent.is_some() {
            stops.push(Hit::Sent);
        }
        stops.extend([Hit::Yes, Hit::No]);
        stops
    }

    fn focus_on(&mut self, hit: Hit) {
        self.focus = hit;
        // An input method only while words are written.
        unsafe {
            if hit == Hit::Words {
                let _ = ImmAssociateContextEx(self.hwnd, HIMC::default(), IACE_DEFAULT);
            } else {
                let _ = ImmAssociateContextEx(self.hwnd, HIMC::default(), 0);
            }
        }
        self.caret_on = true;
        self.redraw();
    }

    /// The input method's words and choices put at the caret, in the box's font.
    fn place_ime(&self) {
        let Some(caret) = self.caret else { return };
        let px = |dips: f32| (dips * self.scale).round() as i32;
        unsafe {
            let context = ImmGetContext(self.hwnd);
            if context.is_invalid() {
                return;
            }
            let at = POINT { x: px(caret.x), y: px(caret.y) };
            let _ = ImmSetCompositionWindow(context, &COMPOSITIONFORM { dwStyle: CFS_POINT, ptCurrentPos: at, ..Default::default() });
            let around = RECT { left: px(caret.x), top: px(caret.y), right: px(caret.x) + 1, bottom: px(caret.y + caret.h) };
            let _ = ImmSetCandidateWindow(context, &CANDIDATEFORM { dwIndex: 0, dwStyle: CFS_EXCLUDE, ptCurrentPos: POINT { x: around.left, y: around.bottom }, rcArea: around });
            let mut face = LOGFONTW { lfHeight: -px(BODY.size), lfWeight: 400, lfCharSet: windows::Win32::Graphics::Gdi::DEFAULT_CHARSET, ..Default::default() };
            for (unit, to) in "Segoe UI".encode_utf16().zip(face.lfFaceName.iter_mut()) {
                *to = unit;
            }
            let _ = ImmSetCompositionFontW(context, &face);
            let _ = ImmReleaseContext(self.hwnd, context);
        }
    }

    fn words_layout(&self) -> IDWriteTextLayout {
        self.gfx.wrapped(&self.typing.string(), BODY, self.words_inside.w)
    }

    fn add(&mut self, more: Vec<Picture>) {
        self.pictures.extend(more);
        self.pictures.truncate(PICTURES_MOST);
        self.redraw();
    }

    fn likeness(&self) -> (u32, u32) {
        ((TILE.0 * self.scale).ceil() as u32, (TILE.1 * self.scale).ceil() as u32)
    }

    fn paste_pictures(&mut self) {
        let taken = picture::from_clipboard(self.hwnd, self.likeness());
        self.add(taken);
    }

    fn activate(&mut self, hit: Hit) {
        match hit {
            Hit::Yes => self.answer = Some(true),
            Hit::No => self.answer = Some(false),
            Hit::Add => self.choose_files = true,
            Hit::Paste => self.paste_pictures(),
            Hit::Remove(index) if index < self.pictures.len() => {
                self.pictures.remove(index);
                // The keyboard on what took its place, or what comes after the pictures.
                if self.focus == Hit::Remove(index) && index >= self.pictures.len() {
                    self.focus = if self.pictures.is_empty() { Hit::Add } else { Hit::Remove(self.pictures.len() - 1) };
                }
                self.redraw();
            }
            _ => {}
        }
    }

    fn moved(&mut self, x: f32, y: f32) {
        let before = self.hovered();
        self.pointer = Some((x, y));
        if self.selecting {
            let inside = self.words_inside;
            let at = at_point(&self.words_layout(), x - inside.x, y - inside.y + self.typing.top);
            self.typing.go(at, true);
            self.redraw();
        } else if let Some(grab) = self.held {
            let (start, length, thumb) = self.track;
            if length > thumb {
                self.scroll = ((y - grab - start) / (length - thumb) * self.scroll_most).clamp(0.0, self.scroll_most);
            }
            self.redraw();
        } else if self.hovered() != before {
            self.redraw();
        }
    }

    fn press(&mut self, x: f32, y: f32, shift: bool) {
        let hit = self.hit_at(x, y);
        self.pressed = hit;
        self.keyboard = false;
        match hit {
            Some(Hit::Words) => {
                self.focus_on(Hit::Words);
                let inside = self.words_inside;
                let at = at_point(&self.words_layout(), x - inside.x, y - inside.y + self.typing.top);
                self.typing.go(at, shift);
                self.selecting = true;
            }
            Some(Hit::Thumb) => {
                let (start, length, thumb) = self.track;
                let at = start + (length - thumb) * if self.scroll_most > 0.0 { self.scroll / self.scroll_most } else { 0.0 };
                self.held = Some(y - at);
            }
            Some(hit) if hit != Hit::Thumb => self.focus_on(hit),
            _ => {}
        }
        self.redraw();
    }

    fn release(&mut self, x: f32, y: f32) {
        let pressed = self.pressed.take();
        self.selecting = false;
        self.held = None;
        if let Some(pressed) = pressed.filter(|pressed| self.hit_at(x, y) == Some(*pressed)) {
            self.activate(pressed);
        }
        self.redraw();
    }

    fn wheel(&mut self, x: f32, y: f32, turns: f32) {
        if self.hit_at(x, y) == Some(Hit::Words) {
            self.typing.top -= turns * WHEEL;
        } else {
            self.scroll = (self.scroll - turns * WHEEL).clamp(0.0, self.scroll_most);
        }
        self.redraw();
    }

    fn key(&mut self, key: VIRTUAL_KEY, shift: bool, ctrl: bool) {
        match key {
            VK_ESCAPE => self.answer = Some(false),
            VK_RETURN if ctrl => self.answer = Some(true),
            VK_TAB => {
                let stops = self.stops();
                let at = stops.iter().position(|&stop| stop == self.focus);
                let next = match at {
                    Some(at) if shift => stops[(at + stops.len() - 1) % stops.len()],
                    Some(at) => stops[(at + 1) % stops.len()],
                    None => stops[0],
                };
                self.keyboard = true;
                self.focus_on(next);
            }
            // A picture on the clipboard is taken wherever the keyboard is; text, by the box for words.
            VK_V if ctrl && self.own.is_some() && self.pictures.len() < PICTURES_MOST && picture::on_clipboard() => self.paste_pictures(),
            _ if self.focus == Hit::Words => self.type_key(key, shift, ctrl),
            VK_RETURN | VK_SPACE if !matches!(self.focus, Hit::Sent) => {
                let focus = self.focus;
                self.activate(focus);
            }
            VK_UP | VK_DOWN | VK_PRIOR | VK_NEXT | VK_HOME | VK_END if self.focus == Hit::Sent => {
                let page = self.track.1;
                self.scroll = match key {
                    VK_UP => self.scroll - WHEEL,
                    VK_DOWN => self.scroll + WHEEL,
                    VK_PRIOR => self.scroll - page,
                    VK_NEXT => self.scroll + page,
                    VK_HOME => 0.0,
                    _ => self.scroll_most,
                }
                .clamp(0.0, self.scroll_most);
                self.redraw();
            }
            _ => {}
        }
    }

    /// A key pressed while words are written: the caret moved, words taken away, copied or pasted.
    fn type_key(&mut self, key: VIRTUAL_KEY, shift: bool, ctrl: bool) {
        let typing = &mut self.typing;
        match key {
            VK_LEFT | VK_RIGHT => {
                let forward = key == VK_RIGHT;
                let selection = typing.selection();
                let to = if !shift && !selection.is_empty() { if forward { selection.end } else { selection.start } } else { typing.step(typing.caret, forward) };
                typing.go(to, shift);
            }
            VK_UP | VK_DOWN => {
                let layout = self.words_layout();
                let typing = &mut self.typing;
                let (x, top, height) = caret_at(&layout, typing.caret);
                let y = if key == VK_UP { top - height / 2.0 } else { top + height * 1.5 };
                let to = if y < 0.0 { 0 } else if y > height_of(&layout) { typing.text.len() } else { at_point(&layout, x, y) };
                typing.go(to, shift);
            }
            VK_HOME | VK_END if ctrl => typing.go(if key == VK_HOME { 0 } else { typing.text.len() }, shift),
            VK_HOME | VK_END => {
                let (start, end) = line_of(&self.words_layout(), self.typing.caret);
                self.typing.go(if key == VK_HOME { start } else { end }, shift);
            }
            VK_BACK | VK_DELETE => typing.erase(key == VK_DELETE),
            VK_RETURN => typing.put(&[u16::from(b'\n')]),
            VK_A if ctrl => {
                typing.anchor = 0;
                typing.caret = typing.text.len();
            }
            VK_C | VK_X if ctrl => {
                let selection = typing.selection();
                if !selection.is_empty() {
                    copy_text(self.hwnd, &self.typing.text[selection]);
                    if key == VK_X {
                        self.typing.put(&[]);
                    }
                }
            }
            VK_V if ctrl => {
                let pasted = clipboard_text(self.hwnd);
                self.typing.put(&pasted);
            }
            _ => return,
        }
        self.caret_on = true;
        self.redraw();
    }

    /// A character typed (or given by the input method), one UTF-16 unit at a time.
    fn char(&mut self, unit: u16) {
        // (Control characters come of keys taken as keys: Enter, Backspace, Ctrl with a letter.)
        if self.focus != Hit::Words || unit < 0x20 || unit == 0x7F {
            return;
        }
        self.typing.put(&[unit]);
        self.caret_on = true;
        self.redraw();
    }

    fn draw(&mut self) {
        let mut inside = RECT::default();
        unsafe {
            let _ = GetClientRect(self.hwnd, &mut inside);
        }
        let size = (inside.right.max(0) as u32, inside.bottom.max(0) as u32);
        if size.0 == 0 || size.1 == 0 {
            return;
        }
        if self.surface.is_none() {
            if let Ok(gfx) = gfx::current() {
                self.gfx = gfx;
            }
            self.surface = Surface::new(&self.gfx, self.hwnd).ok();
        }
        let Some(mut surface) = self.surface.take() else { return };
        let gfx = self.gfx.clone();
        let (scale, area) = (self.scale, (size.0 as f32 / self.scale, size.1 as f32 / self.scale));
        if surface.draw(&gfx, size, scale, |frame| self.paint(frame, area)).is_ok() {
            self.surface = Some(surface);
        } else {
            gfx::lost();
        }
        if self.focus == Hit::Words {
            self.place_ime();
        }
    }

    fn paint(&mut self, frame: &Frame, (width, height): (f32, f32)) {
        self.drawn.clear();
        let colours = Colours::new(self.dark);
        let c = &colours;
        let inner = width - 2.0 * PAD;
        let footer = height - FOOTER;
        frame.fill(c.content, 0.0, 0.0, width, height);
        frame.fill(c.footer, 0.0, footer, width, FOOTER);
        let mut y = PAD;
        frame.text_wrapped(&self.title, TITLE_FONT, c.text, PAD, y, inner);
        y += frame.gfx.wrapped_height(&self.title, TITLE_FONT, inner) + 12.0;
        frame.text_wrapped(&self.intro, BODY, c.text, PAD, y, inner);
        y += frame.gfx.wrapped_height(&self.intro, BODY, inner) + GAP;
        if let Some(own) = self.own.clone() {
            frame.text(&own.words, HEADING, c.text, PAD, y, inner, Align::Start);
            y += HEAD;
            self.paint_words(frame, c, Rect { x: PAD, y, w: inner, h: WORDS_HEIGHT }, &own.hint);
            y += WORDS_HEIGHT + GAP;
            frame.text(&own.pictures, HEADING, c.text, PAD, y, inner, Align::Start);
            y += HEAD;
            self.paint_pictures(frame, c, PAD, y, inner, &own);
            y += TILE.1 + GAP;
        }
        if let Some((name, sent)) = self.sent.clone() {
            frame.text(&name, HEADING, c.text, PAD, y, inner, Align::Start);
            y += HEAD;
            self.paint_sent(frame, c, Rect { x: PAD, y, w: inner, h: (footer - GAP - y).max(60.0) }, &sent);
        }
        // The two answers, sharing the footer, as Windows 11's dialogs have them.
        let across = (inner - 8.0) / 2.0;
        let top = footer + (FOOTER - BUTTON) / 2.0;
        let (yes, no) = (self.yes.clone(), self.no.clone());
        self.button(frame, c, Rect { x: PAD, y: top, w: across, h: BUTTON }, &yes, Hit::Yes, true, true);
        self.button(frame, c, Rect { x: PAD + across + 8.0, y: top, w: across, h: BUTTON }, &no, Hit::No, false, true);
        // The frame's own places from now; and drawn again where what is pointed at is another among them (a
        // picture's button, shown as the picture was pointed at, now under the pointer).
        let before = self.hovered();
        self.hits = std::mem::take(&mut self.drawn);
        if self.hovered() != before {
            self.redraw();
        }
    }

    /// The ring round what the keyboard is on, once the keyboard has moved it.
    fn ring(&self, frame: &Frame, c: &Colours, r: Rect, radius: f32, hit: Hit) {
        if self.keyboard && self.focus == hit {
            frame.stroke_rounded(Fill::Solid(c.ring), r.x - 3.0, r.y - 3.0, r.w + 6.0, r.h + 6.0, radius + 3.0, 2.0);
        }
    }

    /// One line of `text`, centred in `r`.
    fn centred(frame: &Frame, text: &str, font: Font, colour: Color, r: Rect) {
        let (ascent, descent) = frame.gfx.baseline(font);
        let wide = frame.gfx.measure(text, font).min(r.w);
        frame.text(text, font, colour, r.x + (r.w - wide) / 2.0, r.y + (r.h - ascent - descent) / 2.0, wide + 1.0, Align::Start);
    }

    #[allow(clippy::too_many_arguments)]
    fn button(&mut self, frame: &Frame, c: &Colours, r: Rect, text: &str, hit: Hit, accent: bool, enabled: bool) {
        let hover = enabled && self.hovered() == Some(hit);
        let down = hover && self.pressed == Some(hit);
        if accent {
            frame.fill_rounded(c.accent.alpha(if down { 0.8 } else if hover { 0.9 } else { 1.0 }), r.x, r.y, r.w, r.h, 6.0);
            Self::centred(frame, text, BODY, Color::hex(0xFFFFFF, 1.0), r);
        } else {
            frame.fill_rounded(if hover { c.button_hover } else { c.button }, r.x, r.y, r.w, r.h, 6.0);
            frame.stroke_rounded(Fill::Solid(c.button_stroke), r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0, 5.5, 1.0);
            Self::centred(frame, text, BODY, if !enabled { c.text3 } else if down { c.text2 } else { c.text }, r);
        }
        self.ring(frame, c, r, 6.0, hit);
        if enabled {
            self.drawn.push((r, hit));
        }
    }

    fn paint_words(&mut self, frame: &Frame, c: &Colours, r: Rect, hint: &str) {
        let focused = self.focus == Hit::Words;
        frame.fill_rounded(c.field, r.x, r.y, r.w, r.h, 4.0);
        frame.stroke_rounded(Fill::Solid(c.field_stroke), r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0, 3.5, 1.0);
        // Its lower edge, in the accent and thicker while words are written in it.
        if focused {
            frame.fill(c.accent, r.x + 2.0, r.y + r.h - 2.0, r.w - 4.0, 2.0);
        } else {
            frame.fill(c.field_bottom, r.x + 2.0, r.y + r.h - 1.0, r.w - 4.0, 1.0);
        }
        let inside = Rect { x: r.x + INSET.0, y: r.y + INSET.1, w: r.w - 2.0 * INSET.0 - 6.0, h: r.h - 2.0 * INSET.1 };
        self.words_inside = inside;
        let text = self.typing.string();
        let layout = frame.gfx.wrapped(&text, BODY, inside.w);
        let (x, top, tall) = caret_at(&layout, self.typing.caret);
        // Scrolled to the caret after it moved; never past the words.
        if self.typing.follow {
            self.typing.top = self.typing.top.min(top).max(top + tall - inside.h);
            self.typing.follow = false;
        }
        let whole = height_of(&layout);
        self.typing.top = self.typing.top.clamp(0.0, (whole - inside.h).max(0.0));
        let shift = inside.y - self.typing.top;
        frame.clip(inside.x - 1.0, inside.y, inside.w + 2.0, inside.h);
        let selection = self.typing.selection();
        if !selection.is_empty() {
            let mut count = 0;
            let _ = unsafe { layout.HitTestTextRange(selection.start as u32, selection.len() as u32, 0.0, 0.0, None, &mut count) };
            let mut spans = vec![DWRITE_HIT_TEST_METRICS::default(); count as usize];
            let _ = unsafe { layout.HitTestTextRange(selection.start as u32, selection.len() as u32, 0.0, 0.0, Some(&mut spans), &mut count) };
            for span in &spans {
                frame.fill(c.selection, inside.x + span.left, shift + span.top, span.width.max(4.0), span.height);
            }
        }
        if text.is_empty() {
            frame.text_wrapped(hint, BODY, c.text3, inside.x, inside.y, inside.w);
        } else {
            frame.text_wrapped(&text, BODY, c.text, inside.x, shift, inside.w);
        }
        self.caret = focused.then_some(Rect { x: inside.x + x, y: shift + top, w: 1.0, h: tall });
        if focused && self.caret_on && selection.is_empty() {
            frame.fill(c.text, inside.x + x, shift + top, 1.0, tall);
        }
        frame.unclip();
        if whole > inside.h {
            let thumb = (inside.h * inside.h / whole).max(16.0);
            let at = inside.y + (inside.h - thumb) * self.typing.top / (whole - inside.h);
            frame.fill_rounded(c.scroll, r.x + r.w - 6.0, at, 3.0, thumb, 1.5);
        }
        self.drawn.push((r, Hit::Words));
    }

    /// The pictures given (pointed at, a button on each takes it away again), and the place to add more, as
    /// wide as is left of `width`.
    fn paint_pictures(&mut self, frame: &Frame, c: &Colours, left: f32, top: f32, width: f32, own: &OwnText) {
        let hovered = self.hovered();
        let white = Color::hex(0xFFFFFF, 1.0);
        let mut x = left;
        for (index, picture) in self.pictures.iter().enumerate() {
            let tile = Rect { x, y: top, w: TILE.0, h: TILE.1 };
            frame.fill_rounded(c.sent, tile.x, tile.y, tile.w, tile.h, 6.0);
            covering(frame, picture, tile, 6.0, self.scale);
            frame.stroke_rounded(Fill::Solid(c.field_stroke), tile.x + 0.5, tile.y + 0.5, tile.w - 1.0, tile.h - 1.0, 5.5, 1.0);
            self.drawn.push((tile, Hit::Picture(index)));
            let keyed = self.keyboard && self.focus == Hit::Remove(index);
            if keyed || matches!(hovered, Some(Hit::Picture(at) | Hit::Remove(at)) if at == index) {
                frame.fill_rounded(c.shade, tile.x, tile.y, tile.w, tile.h, 6.0);
                // Its button to take it away, in the corner.
                let centre = Point { x: tile.x + tile.w - 15.0, y: tile.y + 15.0 };
                let on = hovered == Some(Hit::Remove(index));
                frame.fill_circle(Color::hex(0x000000, if on { 0.82 } else { 0.6 }), centre, 11.0);
                frame.icon(Icon::Close, centre, 13.0, white);
                let badge = Rect { x: centre.x - 13.0, y: centre.y - 13.0, w: 26.0, h: 26.0 };
                self.ring(frame, c, badge, 13.0, Hit::Remove(index));
                self.drawn.push((badge, Hit::Remove(index)));
            }
            x += TILE.0 + TILE_GAP;
        }
        self.chip_shown = false;
        if self.pictures.len() >= PICTURES_MOST {
            return;
        }
        // The place to add one: pressed, the file chooser; it says the other ways (dropping a file on the window,
        // Ctrl+V), and, with a picture on the clipboard, offers it.
        let zone = Rect { x, y: top, w: left + width - x, h: TILE.1 };
        let over = hovered == Some(Hit::Add);
        frame.fill_rounded(if over { c.button_hover } else { c.field }, zone.x, zone.y, zone.w, zone.h, 6.0);
        frame.stroke_rounded_dashed(if over { c.accent } else { c.field_bottom.alpha(0.6) }, zone.x + 0.5, zone.y + 0.5, zone.w - 1.0, zone.h - 1.0, 5.5, 1.0);
        self.ring(frame, c, zone, 6.0, Hit::Add);
        self.drawn.push((zone, Hit::Add));
        let middle = zone.y + zone.h / 2.0;
        frame.icon(Icon::ImagePlus, Point { x: zone.x + 26.0, y: middle }, 22.0, if over { c.accent } else { c.text2 });
        let words = zone.x + 50.0;
        let pasting = picture::on_clipboard();
        let chip = frame.gfx.measure(&own.paste, BODY) + 28.0;
        let said = frame.gfx.measure(&own.add, BODY).max(frame.gfx.measure(&own.ways, SMALL));
        self.chip_shown = pasting && words + said + 16.0 + chip + 12.0 <= zone.x + zone.w;
        let room = zone.x + zone.w - words - if self.chip_shown { chip + 24.0 } else { 12.0 };
        frame.text(&own.add, BODY, c.text, words, middle - 20.0, room, Align::Start);
        frame.text(if pasting && !self.chip_shown { &own.waiting } else { &own.ways }, SMALL, c.text3, words, middle + 2.0, room, Align::Start);
        if self.chip_shown {
            let paste = own.paste.clone();
            self.button(frame, c, Rect { x: zone.x + zone.w - 12.0 - chip, y: middle - 15.0, w: chip, h: 30.0 }, &paste, Hit::Paste, false, true);
        }
    }

    fn paint_sent(&mut self, frame: &Frame, c: &Colours, r: Rect, sent: &str) {
        frame.fill_rounded(c.sent, r.x, r.y, r.w, r.h, 4.0);
        frame.stroke_rounded(Fill::Solid(c.field_stroke), r.x + 0.5, r.y + 0.5, r.w - 1.0, r.h - 1.0, 3.5, 1.0);
        let inside = Rect { x: r.x + INSET.0, y: r.y + INSET.1, w: r.w - 2.0 * INSET.0 - 8.0, h: r.h - 2.0 * INSET.1 };
        let whole = frame.gfx.wrapped_height(sent, SMALL, inside.w);
        self.scroll_most = (whole - inside.h).max(0.0);
        self.scroll = self.scroll.clamp(0.0, self.scroll_most);
        frame.clip(inside.x, inside.y, inside.w, inside.h);
        frame.text_wrapped(sent, SMALL, c.text2, inside.x, inside.y - self.scroll, inside.w);
        frame.unclip();
        self.ring(frame, c, r, 4.0, Hit::Sent);
        self.drawn.push((r, Hit::Sent));
        if self.scroll_most > 0.0 {
            let thumb = (inside.h * inside.h / whole).max(24.0);
            let at = inside.y + (inside.h - thumb) * self.scroll / self.scroll_most;
            self.track = (inside.y, inside.h, thumb);
            let wide = matches!(self.hovered(), Some(Hit::Thumb)) || self.held.is_some();
            let across = if wide { 6.0 } else { 3.0 };
            frame.fill_rounded(c.scroll, r.x + r.w - 5.0 - across, at, across, thumb, across / 2.0);
            self.drawn.push((Rect { x: r.x + r.w - 16.0, y: at, w: 16.0, h: thumb }, Hit::Thumb));
        } else {
            self.track = (inside.y, inside.h, inside.h);
        }
    }
}

/// The picture files the reader chooses (in Windows' own chooser, over
/// `owner`; `kinds` is what it calls them): their paths.
fn chosen(owner: HWND, kinds: &str) -> Vec<String> {
    use windows::Win32::UI::Controls::Dialogs::{GetOpenFileNameW, OFN_ALLOWMULTISELECT, OFN_EXPLORER, OFN_FILEMUSTEXIST, OFN_HIDEREADONLY, OFN_NOCHANGEDIR, OPENFILENAMEW};
    let filter: Vec<u16> = format!("{kinds}\0*.png;*.jpg;*.jpeg;*.bmp;*.gif;*.webp\0\0").encode_utf16().collect();
    let mut paths = vec![0u16; 16 << 10];
    let mut asked = OPENFILENAMEW {
        lStructSize: size_of::<OPENFILENAMEW>() as u32,
        hwndOwner: owner,
        lpstrFilter: PCWSTR(filter.as_ptr()),
        lpstrFile: windows::core::PWSTR(paths.as_mut_ptr()),
        nMaxFile: paths.len() as u32,
        Flags: OFN_EXPLORER | OFN_ALLOWMULTISELECT | OFN_FILEMUSTEXIST | OFN_HIDEREADONLY | OFN_NOCHANGEDIR,
        ..Default::default()
    };
    if !unsafe { GetOpenFileNameW(&mut asked) }.as_bool() {
        return Vec::new();
    }
    // One file chosen is its whole path; several are their folder, then each one's name.
    let named: Vec<String> = paths.split(|&c| c == 0).take_while(|part| !part.is_empty()).map(String::from_utf16_lossy).collect();
    match named.split_first() {
        Some((folder, names)) if !names.is_empty() => names.iter().map(|name| format!("{}\\{name}", folder.trim_end_matches('\\'))).collect(),
        _ => named,
    }
}

fn point(lparam: LPARAM) -> (f32, f32) {
    ((lparam.0 & 0xFFFF) as u16 as i16 as f32, ((lparam.0 >> 16) & 0xFFFF) as u16 as i16 as f32)
}

unsafe extern "system" fn procedure(hwnd: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    let held = |key: VIRTUAL_KEY| unsafe { GetKeyState(key.0 as i32) } < 0;
    match message {
        // All of it is the window's own to draw: no frame (Windows keeps its shadow and round corners).
        WM_NCCALCSIZE if wparam.0 != 0 => LRESULT(0),
        WM_NCHITTEST => {
            let (x, y) = point(lparam);
            let mut at = POINT { x: x as i32, y: y as i32 };
            unsafe {
                let _ = ScreenToClient(hwnd, &mut at);
            }
            let caption = with(|dialog| dialog.caption(at.x as f32 / dialog.scale, at.y as f32 / dialog.scale)).unwrap_or(false);
            LRESULT(if caption { HTCAPTION } else { HTCLIENT } as isize)
        }
        WM_PAINT => {
            unsafe {
                let _ = ValidateRect(Some(hwnd), None);
            }
            with(Dialog::draw);
            LRESULT(0)
        }
        WM_SETCURSOR if (lparam.0 & 0xFFFF) as u32 == HTCLIENT => {
            let typing = with(|dialog| dialog.over_words()).unwrap_or(false);
            unsafe { SetCursor(LoadCursorW(None, if typing { IDC_IBEAM } else { IDC_ARROW }).ok()) };
            LRESULT(1)
        }
        WM_MOUSEMOVE => {
            let (x, y) = point(lparam);
            with(|dialog| {
                if dialog.pointer.is_none() {
                    let mut track = TRACKMOUSEEVENT { cbSize: size_of::<TRACKMOUSEEVENT>() as u32, dwFlags: TME_LEAVE, hwndTrack: hwnd, dwHoverTime: 0 };
                    let _ = unsafe { TrackMouseEvent(&mut track) };
                }
                dialog.moved(x / dialog.scale, y / dialog.scale);
            });
            LRESULT(0)
        }
        WM_MOUSELEAVE => {
            with(|dialog| {
                dialog.pointer = None;
                dialog.redraw();
            });
            LRESULT(0)
        }
        WM_LBUTTONDOWN => {
            let (x, y) = point(lparam);
            unsafe { SetCapture(hwnd) };
            with(|dialog| dialog.press(x / dialog.scale, y / dialog.scale, held(VK_SHIFT)));
            LRESULT(0)
        }
        WM_LBUTTONUP => {
            let (x, y) = point(lparam);
            unsafe {
                let _ = ReleaseCapture();
            }
            with(|dialog| dialog.release(x / dialog.scale, y / dialog.scale));
            LRESULT(0)
        }
        WM_MOUSEWHEEL => {
            let (x, y) = point(lparam);
            let mut at = POINT { x: x as i32, y: y as i32 };
            unsafe {
                let _ = ScreenToClient(hwnd, &mut at);
            }
            let turns = ((wparam.0 >> 16) & 0xFFFF) as u16 as i16 as f32 / 120.0;
            with(|dialog| dialog.wheel(at.x as f32 / dialog.scale, at.y as f32 / dialog.scale, turns));
            LRESULT(0)
        }
        WM_KEYDOWN => {
            with(|dialog| dialog.key(VIRTUAL_KEY(wparam.0 as u16), held(VK_SHIFT), held(VK_CONTROL)));
            LRESULT(0)
        }
        WM_CHAR => {
            with(|dialog| dialog.char(wparam.0 as u16));
            LRESULT(0)
        }
        // The caret blinks.
        WM_TIMER => {
            with(|dialog| {
                dialog.caret_on = !dialog.caret_on;
                if dialog.focus == Hit::Words {
                    dialog.redraw();
                }
            });
            LRESULT(0)
        }
        // Files dropped on the window: those that are pictures are added.
        WM_DROPFILES => {
            let files = HDROP(wparam.0 as *mut _);
            let paths: Vec<String> = (0..unsafe { DragQueryFileW(files, u32::MAX, None) })
                .map(|index| {
                    let mut path = vec![0u16; unsafe { DragQueryFileW(files, index, None) } as usize + 1];
                    let length = unsafe { DragQueryFileW(files, index, Some(&mut path)) } as usize;
                    String::from_utf16_lossy(&path[..length])
                })
                .collect();
            unsafe { DragFinish(files) };
            with(|dialog| {
                if dialog.own.is_some() {
                    let small = dialog.likeness();
                    let more = paths.iter().filter_map(|path| picture::from_file(path, small)).collect();
                    dialog.add(more);
                }
            });
            LRESULT(0)
        }
        // (Whether a picture may be pasted.)
        WM_CLIPBOARDUPDATE => {
            with(|dialog| dialog.redraw());
            LRESULT(0)
        }
        WM_IME_STARTCOMPOSITION => {
            with(|dialog| dialog.place_ime());
            unsafe { DefWindowProcW(hwnd, message, wparam, lparam) }
        }
        WM_DPICHANGED => {
            let suggested = unsafe { *(lparam.0 as *const RECT) };
            with(|dialog| dialog.scale = (wparam.0 & 0xFFFF) as f32 / 96.0);
            unsafe {
                let _ = SetWindowPos(hwnd, None, suggested.left, suggested.top, suggested.right - suggested.left, suggested.bottom - suggested.top, SWP_NOZORDER | SWP_NOACTIVATE);
            }
            with(|dialog| dialog.redraw());
            LRESULT(0)
        }
        WM_CLOSE => {
            with(|dialog| dialog.answer = Some(false));
            LRESULT(0)
        }
        _ => unsafe { DefWindowProcW(hwnd, message, wparam, lparam) },
    }
}

/// Asks `asked` in a window over `owner`: what the reader gave of their
/// own (nothing, where they were not asked for it), if the answer was its
/// `yes`; none, if not.
pub fn show(owner: HWND, asked: &Asked) -> Option<Said> {
    let gfx = gfx::current().ok()?;
    unsafe {
        let module = GetModuleHandleW(None).ok()?;
        let class = w!("GlanceAsk");
        // (Registered once; again is refused, which is as well.)
        RegisterClassExW(&WNDCLASSEXW { cbSize: size_of::<WNDCLASSEXW>() as u32, lpfnWndProc: Some(procedure), hInstance: module.into(), lpszClassName: class, ..Default::default() });
        let scale = GetDpiForWindow(owner).max(96) as f32 / 96.0;
        let own = asked.own.as_ref().map(|own| OwnText {
            words: own.words.into(),
            hint: own.hint.into(),
            pictures: own.pictures.into(),
            add: own.add.into(),
            ways: own.ways.into(),
            paste: own.paste.into(),
            waiting: own.waiting.into(),
            kinds: own.kinds.into(),
        });
        let mut dialog = Dialog {
            hwnd: HWND::default(),
            gfx,
            surface: None,
            scale,
            dark: asked.dark,
            title: asked.title.into(),
            intro: asked.intro.into(),
            own,
            chip_shown: false,
            sent: asked.sent.map(|(name, text)| (name.into(), text.into())),
            yes: asked.yes.into(),
            no: asked.no.into(),
            typing: Typing::default(),
            pictures: Vec::new(),
            scroll: 0.0,
            scroll_most: 0.0,
            track: (0.0, 0.0, 0.0),
            pointer: None,
            pressed: None,
            selecting: false,
            held: None,
            focus: if asked.own.is_some() { Hit::Words } else { Hit::Yes },
            keyboard: false,
            caret_on: true,
            hits: Vec::new(),
            drawn: Vec::new(),
            words_inside: Rect { x: 0.0, y: 0.0, w: 200.0, h: 0.0 },
            caret: None,
            answer: None,
            choose_files: false,
        };
        // In the middle of the window it is over, within that window's screen, no taller than the screen (what
        // is sent scrolls the more).
        let mut over = RECT::default();
        let _ = GetWindowRect(owner, &mut over);
        let mut screen = MONITORINFO { cbSize: size_of::<MONITORINFO>() as u32, ..Default::default() };
        let _ = GetMonitorInfoW(MonitorFromWindow(owner, MONITOR_DEFAULTTONEAREST), &mut screen);
        let room = screen.rcWork;
        let wide = ((WIDTH * scale) as i32).min(room.right - room.left);
        let tall = ((dialog.height() * scale) as i32).min((room.bottom - room.top) * 19 / 20);
        let (x, y) = (((over.left + over.right - wide) / 2).clamp(room.left, room.right - wide), ((over.top + over.bottom - tall) / 2).clamp(room.top, room.bottom - tall));
        let window = CreateWindowExW(WS_EX_NOREDIRECTIONBITMAP, class, &HSTRING::from(asked.title), WS_POPUP | WS_CAPTION | WS_SYSMENU, x, y, wide, tall, Some(owner), None, Some(module.into()), None).ok()?;
        dialog.hwnd = window;
        // Its shadow and (on Windows 11) its round corners, as Windows gives a window with a frame.
        let _ = DwmExtendFrameIntoClientArea(window, &MARGINS { cxLeftWidth: 1, cxRightWidth: 1, cyTopHeight: 1, cyBottomHeight: 1 });
        let round = DWMWCP_ROUND;
        let _ = DwmSetWindowAttribute(window, DWMWA_WINDOW_CORNER_PREFERENCE, &round as *const _ as *const _, size_of_val(&round) as u32);
        // (Its frame gone at once: the size it was made at is the size it is.)
        let _ = SetWindowPos(window, None, x, y, wide, tall, SWP_NOZORDER | SWP_NOACTIVATE | windows::Win32::UI::WindowsAndMessaging::SWP_FRAMECHANGED);
        #[cfg(test)]
        for path in GIVEN_FOR_TEST.take() {
            dialog.pictures.extend(picture::from_file(&path, dialog.likeness()));
        }
        let focus = dialog.focus;
        DIALOG.set(Some(dialog));
        with(|dialog| {
            dialog.focus_on(focus);
            dialog.draw();
        });
        let _ = AddClipboardFormatListener(window);
        // Files dropped on it, from Explorer too (which is not elevated, as Glance is: what it sends is let through).
        if asked.own.is_some() {
            DragAcceptFiles(window, true);
            for message in [WM_DROPFILES, WM_COPYDATA, COPY_GLOBAL_DATA] {
                let _ = ChangeWindowMessageFilterEx(window, message, MSGFLT_ALLOW, None);
            }
        }
        let _ = SetTimer(Some(window), 1, GetCaretBlinkTime(), None);

        let _ = EnableWindow(owner, false);
        let _ = ShowWindow(window, SW_SHOW);
        let _ = SetForegroundWindow(window);
        let _ = SetFocus(Some(window));
        // Its own turn at the messages, till it is answered (or gone).
        let mut message = MSG::default();
        let mut quit = None;
        while with(|dialog| dialog.answer.is_none()).unwrap_or(false) && IsWindow(Some(window)).as_bool() {
            if !GetMessageW(&mut message, None, 0, 0).as_bool() {
                quit = Some(message.wParam.0 as i32);
                break;
            }
            let _ = TranslateMessage(&message);
            DispatchMessageW(&message);
            // The file chooser, which has its own turn at the messages: shown with nothing of the window's held.
            if with(|dialog| std::mem::take(&mut dialog.choose_files)).unwrap_or(false) {
                let asked = with(|dialog| (dialog.own.as_ref().map(|own| own.kinds.clone()).unwrap_or_default(), dialog.likeness()));
                if let Some((kinds, small)) = asked {
                    let more = chosen(window, &kinds).iter().filter_map(|path| picture::from_file(path, small)).collect();
                    with(|dialog| dialog.add(more));
                }
            }
        }
        let dialog = DIALOG.take()?;
        let _ = KillTimer(Some(window), 1);
        let _ = RemoveClipboardFormatListener(window);
        let _ = EnableWindow(owner, true);
        let _ = DestroyWindow(window);
        let _ = SetForegroundWindow(owner);
        if let Some(code) = quit {
            PostQuitMessage(code);
        }
        let words = dialog.typing.string().trim().to_string();
        (dialog.answer == Some(true)).then(|| Said { words, pictures: dialog.pictures.into_iter().map(|picture| picture.jpeg).collect() })
    }
}

#[cfg(test)]
mod tests {
    use super::Typing;

    fn typed(text: &str) -> Typing {
        let mut typing = Typing::default();
        typing.put(&text.encode_utf16().collect::<Vec<_>>());
        typing
    }

    #[test]
    fn words_are_written_and_taken_away_a_character_at_a_time() {
        // (One character outside the basic plane: two units, taken as one.)
        let mut typing = typed("主板😀");
        assert_eq!(typing.caret, 4);
        typing.erase(false);
        assert_eq!(typing.string(), "主板");
        typing.go(0, false);
        typing.go(typing.step(0, true), true);
        typing.put(&"CPU".encode_utf16().collect::<Vec<_>>());
        assert_eq!(typing.string(), "CPU板");
        assert_eq!((typing.caret, typing.anchor), (3, 3));
        typing.erase(true);
        assert_eq!(typing.string(), "CPU");
    }

    #[test]
    fn no_more_is_written_than_there_is_room_for() {
        let mut typing = typed(&"字".repeat(super::WORDS_MOST - 1));
        // The last unit free: a character of two units does not fit, nor is it cut.
        typing.put(&"😀".encode_utf16().collect::<Vec<_>>());
        assert_eq!(typing.text.len(), super::WORDS_MOST - 1);
        typing.put(&"ab".encode_utf16().collect::<Vec<_>>());
        assert_eq!(typing.text.len(), super::WORDS_MOST);
        assert!(typing.string().ends_with('a'));
    }

    /// Puts the window up for a few seconds, to be looked at: `cargo test -- --ignored shows_the_window`.
    #[test]
    #[ignore]
    fn shows_the_window() {
        use windows::core::w;
        use windows::Win32::UI::WindowsAndMessaging::{CreateWindowExW, FindWindowW, PostMessageW, WM_CLOSE, WS_POPUP, WS_VISIBLE};
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
            let owner = CreateWindowExW(Default::default(), w!("STATIC"), w!(""), WS_POPUP | WS_VISIBLE, 300, 200, 900, 700, None, None, None, None).unwrap();
            std::thread::spawn(|| {
                std::thread::sleep(std::time::Duration::from_secs(std::env::var("FOR").ok().and_then(|s| s.parse().ok()).unwrap_or(8)));
                let _ = PostMessageW(FindWindowW(w!("GlanceAsk"), None).ok(), WM_CLOSE, Default::default(), Default::default());
            });
            let dark = std::env::var("DARK").is_ok();
            // Pictures to hold, by their files, as `PICTURES` names them (split by `;`).
            super::POINTER_FOR_TEST.set(std::env::var("POINTER").ok().and_then(|at| at.split_once(',').and_then(|(x, y)| Some((x.parse().ok()?, y.parse().ok()?)))));
            super::GIVEN_FOR_TEST.set(std::env::var("PICTURES").map(|paths| paths.split(';').map(str::to_string).collect()).unwrap_or_default());
            let text = (0..60).map(|line| format!("Line {line} of what is sent")).collect::<Vec<_>>().join("\n");
            let own = super::Own { words: "你想说的话（可不填）", hint: "比如：主板温度没有显示；风扇转速和别的软件里看到的不一样", pictures: "截图（可不添，最多 3 张）", add: "添加截图", ways: "点击选择、拖进窗口，或按 Ctrl+V 粘贴", paste: "粘贴剪贴板里的截图", waiting: "剪贴板里有截图，按 Ctrl+V 即可粘贴", kinds: "图片" };
            let asked = super::Asked { title: "发送诊断信息", intro: "下面是将要发送给 Glance（glancepc.com）的全部内容：你写的话、你添加的截图，和最下面的诊断信息。不包含你的文件、账号或网络地址。", own: Some(own), sent: Some(("诊断信息", &text)), yes: "发送", no: "取消", dark };
            assert!(super::show(owner, &asked).is_none());
        }
    }
}
