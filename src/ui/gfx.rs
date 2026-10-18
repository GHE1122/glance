//! Drawing: Direct2D on a DirectComposition surface, text with DirectWrite.
//! Coordinates are in device-independent pixels (DIPs); the surface is in
//! physical pixels and the context scales between them.

use std::cell::{Cell, RefCell};
use std::collections::HashMap;
use std::rc::Rc;
use std::time::{Duration, Instant};

/// How long laid-out text no window draws is kept.
const TEXT_KEPT: Duration = Duration::from_secs(2);
/// How long a picture not drawn again is kept on the card (a track's cover: drawn each second while it plays).
const PICTURE_KEPT: Duration = Duration::from_secs(10);

use windows::core::{w, Interface, Result, BOOL, HSTRING, PCWSTR};
use windows::Win32::Foundation::{HWND, POINT};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_ALPHA_MODE_PREMULTIPLIED, D2D1_COLOR_F, D2D1_COMPOSITE_MODE_SOURCE_OVER, D2D1_PIXEL_FORMAT, D2D_RECT_F, D2D_SIZE_U,
};
use windows::Win32::Graphics::Direct2D::{
    D2D1CreateFactory, ID2D1Bitmap1, ID2D1Device, ID2D1DeviceContext, ID2D1Factory1, ID2D1Image, ID2D1SolidColorBrush,
    CLSID_D2D1DpiCompensation, CLSID_D2D1Shadow, D2D1_BITMAP_OPTIONS_TARGET, D2D1_DPICOMPENSATION_PROP_INPUT_DPI,
    D2D1_PROPERTY_TYPE_VECTOR2, D2D1_BITMAP_PROPERTIES1, D2D1_INTERPOLATION_MODE_LINEAR,
    D2D1_PROPERTY_TYPE_FLOAT, D2D1_PROPERTY_TYPE_VECTOR4, D2D1_SHADOW_PROP_BLUR_STANDARD_DEVIATION, D2D1_SHADOW_PROP_COLOR, D2D1_DRAW_TEXT_OPTIONS_CLIP,
    D2D1_DRAW_TEXT_OPTIONS_NONE, D2D1_FACTORY_TYPE_MULTI_THREADED, D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE,
};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP};
use windows::Win32::Graphics::Direct3D11::{D3D11CreateDevice, ID3D11Device, D3D11_CREATE_DEVICE_BGRA_SUPPORT, D3D11_SDK_VERSION};
use windows::Win32::Graphics::DirectComposition::{
    DCompositionCreateDevice2, IDCompositionDesktopDevice, IDCompositionSurface, IDCompositionTarget, IDCompositionVisual2,
};
use windows::Win32::Graphics::DirectWrite::{
    DWriteCreateFactory, IDWriteFactory6, IDWriteFontCollection, IDWriteFontSetBuilder1, IDWriteInMemoryFontFileLoader,
    IDWriteTextLayout1, DWRITE_CONTAINER_TYPE_WOFF2, DWRITE_LINE_METRICS, DWRITE_TEXT_RANGE, IDWriteTextFormat3, IDWriteTextLayout,
    DWRITE_FACTORY_TYPE_SHARED, DWRITE_FONT_AXIS_TAG_WEIGHT, DWRITE_FONT_AXIS_TAG_WIDTH, DWRITE_FONT_AXIS_VALUE,
    DWRITE_FONT_FAMILY_MODEL_TYPOGRAPHIC, DWRITE_PARAGRAPH_ALIGNMENT_NEAR, DWRITE_TEXT_ALIGNMENT_LEADING,
    DWRITE_TEXT_ALIGNMENT_TRAILING, DWRITE_TEXT_METRICS, DWRITE_TRIMMING, DWRITE_TRIMMING_GRANULARITY_CHARACTER,
    DWRITE_WORD_WRAPPING_NO_WRAP, DWRITE_WORD_WRAPPING_WRAP,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_ALPHA_MODE_PREMULTIPLIED, DXGI_FORMAT_B8G8R8A8_UNORM};
use windows::Win32::Graphics::Dxgi::{IDXGIDevice, IDXGIDevice3};
use windows::Win32::Graphics::Direct2D::Common::{
    D2D1_FIGURE_BEGIN_FILLED, D2D1_FIGURE_BEGIN_HOLLOW, D2D1_FIGURE_END_OPEN, D2D1_GRADIENT_STOP,
};
use windows::Win32::Graphics::Direct2D::{
    ID2D1PathGeometry1, ID2D1RenderTarget, D2D1_ANTIALIAS_MODE_ALIASED, D2D1_ANTIALIAS_MODE_PER_PRIMITIVE, D2D1_ELLIPSE, D2D1_EXTEND_MODE_CLAMP,
    D2D1_GAMMA_2_2, D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES, D2D1_ROUNDED_RECT,
};
use windows::Win32::Graphics::Direct2D::Common::{D2D1_BEZIER_SEGMENT, D2D1_FIGURE_END_CLOSED, D2D_SIZE_F};
use windows::Win32::Graphics::Direct2D::{
    ID2D1StrokeStyle, D2D1_ARC_SEGMENT, D2D1_ARC_SIZE_LARGE, D2D1_DASH_STYLE_CUSTOM, D2D1_ARC_SIZE_SMALL, D2D1_CAP_STYLE_ROUND, D2D1_LINE_JOIN_ROUND,
    D2D1_STROKE_STYLE_PROPERTIES1, D2D1_SWEEP_DIRECTION_CLOCKWISE, D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE,
};
use windows_numerics::{Matrix3x2, Vector2};

use super::icons::{self, Icon, Segment};

use super::canvas::{Align, Canvas, Color, Family, Fill, Font, FontKey, Point};

/// The faces shipped inside the program, Latin only (the system supplies
/// Chinese): Archivo, with its weight and width axes, for chart paper, and
/// Inter, with its weight axis, for glass.
const ARCHIVO: &[u8] = include_bytes!("../../fonts/Archivo-latin.woff2");
const INTER: &[u8] = include_bytes!("../../fonts/Inter-latin.woff2");

impl Color {
    /// For Direct2D, which takes colours straight and premultiplies itself.
    pub fn d2d(self) -> D2D1_COLOR_F {
        D2D1_COLOR_F { r: self.r, g: self.g, b: self.b, a: self.a }
    }
}

/// Devices and factories shared by every surface.
pub struct Gfx {
    /// Whether its device's immediate context may be used from several threads (see `new`).
    pub guarded: bool,
    pub factory: ID2D1Factory1,
    pub(super) device: ID2D1Device,
    dxgi: IDXGIDevice3,
    pub dcomp: IDCompositionDesktopDevice,
    write: IDWriteFactory6,
    /// Holds the shipped faces; registered with the shared factory for as
    /// long as this device lives.
    loader: IDWriteInMemoryFontFileLoader,
    /// The shipped faces: their collections and family names.
    archivo: (IDWriteFontCollection, HSTRING),
    inter: (IDWriteFontCollection, HSTRING),
    /// The serif face, once it is had and first asked for (see `serif`).
    serif: std::cell::OnceCell<Option<(IDWriteFontCollection, HSTRING)>>,
    icons: PCWSTR,
    /// Each font's format, and when it was last asked for.
    formats: RefCell<HashMap<FontKey, (IDWriteTextFormat3, Instant)>>,
    /// Laid-out text, kept while frames keep drawing it: most of a panel's
    /// words and figures are the same from one frame to the next. With when
    /// each was last drawn.
    layouts: RefCell<HashMap<LayoutKey, (IDWriteTextLayout, Instant)>>,
    /// Strokes with round ends and joins, for rings and icons.
    round: ID2D1StrokeStyle,
    /// Dashes as long as the gaps between them, each twice the line's width.
    dashed: ID2D1StrokeStyle,
    /// Each icon's figures as a geometry, made when first drawn.
    icon_paths: RefCell<HashMap<Icon, ID2D1PathGeometry1>>,
    /// The pictures drawn (see `Canvas::picture`), as bitmaps on the card, by key, and when each was last drawn.
    pictures: RefCell<HashMap<u64, (ID2D1Bitmap1, Instant)>>,
    /// Bitmaps made from nothing but their key (the glass's displacement
    /// maps, by size, radius and scale): kept while frames keep drawing
    /// them, as text is.
    made: RefCell<HashMap<[u32; 4], (ID2D1Bitmap1, Instant)>>,
    /// Bitmaps drawn on that were let go of (see `made`), to be drawn on again for what is made next (see
    /// `drawn_on`), as much as `MADE_BUDGET` of them.
    spare: RefCell<Vec<ID2D1Bitmap1>>,
    /// When a window last drew with it, and whether what drawing holds has been given back since (see
    /// `trim_when_idle`).
    last_drawn: Cell<Option<Instant>>,
    trimmed: Cell<bool>,
    /// When a layer was last painted anew, and whether what painting it took has been given back since (see
    /// `let_go_once_settled`).
    last_painted: Cell<Option<Instant>>,
    settled: Cell<bool>,
    /// How many windows show what they draw with it now (see `life::Showing`).
    showing: Cell<usize>,
    /// The thread's timer that gives back what drawing holds once it is due (see `tend`), while one runs.
    tending: Cell<usize>,
    /// The two colours each gradient fades between, as made for them (a resource of the device, any of its
    /// contexts draws with), and when each was last used.
    stops: RefCell<HashMap<[u32; 8], (windows::Win32::Graphics::Direct2D::ID2D1GradientStopCollection, Instant)>>,
    /// What a frame handed out a region of the compositor's atlas is drawn on first (see `Frame::begun`), as large
    /// as the largest such frame yet.
    scratch: RefCell<Option<(ID2D1Bitmap1, (u32, u32))>>,
}

/// How much contrast grayscale text's edges are given by `crisp_text` (the
/// system's default is 1).
const CRISP_CONTRAST: f32 = 2.0;

/// Text, font, width, at the end, and broken into lines.
type LayoutKey = (String, FontKey, u32, bool, bool);

impl Drop for Gfx {
    fn drop(&mut self) {
        if self.tending.get() != 0 {
            let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::KillTimer(None, self.tending.get()) };
        }
        let _ = unsafe { self.write.UnregisterFontFileLoader(&self.loader) };
    }
}

thread_local! {
    static CURRENT: RefCell<Option<Rc<Gfx>>> = const { RefCell::new(None) };
}

/// This thread's graphics device, which its windows share; made when first
/// asked for, and again after it was lost.
pub fn current() -> Result<Rc<Gfx>> {
    CURRENT.with(|cell| {
        if let Some(gfx) = cell.borrow().as_ref() {
            return Ok(gfx.clone());
        }
        let gfx = Rc::new(Gfx::new()?);
        *cell.borrow_mut() = Some(gfx.clone());
        Ok(gfx)
    })
}

/// Drawing failed: the device is gone (a driver update, a graphics reset).
/// The next `current` makes a new one, and each window, seeing it, makes its
/// surfaces again.
pub fn lost() {
    if CURRENT.with(|cell| cell.borrow_mut().take()).is_some() {
        crate::journal::note("the graphics device was lost; drawing on a new one");
    }
}

impl Gfx {
    pub fn new() -> Result<Self> {
        // The graphics card; without a working one, Windows' software renderer.
        let create = |kind| {
            let mut d3d: Option<ID3D11Device> = None;
            unsafe {
                D3D11CreateDevice(None, kind, Default::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut d3d), None, None)
            }
            .map(|_| d3d.unwrap())
        };
        let d3d = create(D3D_DRIVER_TYPE_HARDWARE).or_else(|_| create(D3D_DRIVER_TYPE_WARP))?;
        // Its immediate context guarded (the context, not the device, answers for that): the frames of what is
        // behind are read back on it from other threads (see `scene`), which they are not unless it is.
        let guarded = unsafe { d3d.GetImmediateContext() }
            .and_then(|context| context.cast::<windows::Win32::Graphics::Direct3D11::ID3D11Multithread>())
            .is_ok_and(|guard| unsafe {
                let _ = guard.SetMultithreadProtected(true);
                guard.GetMultithreadProtected().as_bool()
            });
        let dxgi: IDXGIDevice = d3d.cast()?;
        let factory: ID2D1Factory1 = unsafe { D2D1CreateFactory(D2D1_FACTORY_TYPE_MULTI_THREADED, None)? };
        let device: ID2D1Device = unsafe { factory.CreateDevice(&dxgi)? };
        // What Direct2D keeps of the textures its effects draw through (the frost's blur, shadows), past which it
        // lets go of those not in use: each size a window is drawn at has its own, and a window sized pixel by pixel
        // would otherwise keep one of each.
        unsafe { device.SetMaximumTextureMemory(EFFECTS_KEPT) };
        // Made from the Direct2D device, composition surfaces hand out device
        // contexts already aimed at themselves.
        let dcomp: IDCompositionDesktopDevice = unsafe { DCompositionCreateDevice2(&device)? };
        let write: IDWriteFactory6 = unsafe { DWriteCreateFactory(DWRITE_FACTORY_TYPE_SHARED)? };
        let loader = unsafe { write.CreateInMemoryFontFileLoader()? };
        unsafe { write.RegisterFontFileLoader(&loader)? };
        let faces = (|| {
            let archivo = shipped_face(&write, &loader, ARCHIVO)?;
            let inter = shipped_face(&write, &loader, INTER)?;
            let icons = unsafe {
                let system = write.GetSystemFontCollection(false, DWRITE_FONT_FAMILY_MODEL_TYPOGRAPHIC)?;
                let (mut index, mut fluent) = (0, BOOL(0));
                system.FindFamilyName(w!("Segoe Fluent Icons"), &mut index, &mut fluent)?;
                if fluent.as_bool() { w!("Segoe Fluent Icons") } else { w!("Segoe MDL2 Assets") }
            };
            Ok::<_, windows::core::Error>((archivo, inter, icons))
        })();
        // Failing here, the loader is not left registered with nothing to free it.
        let (archivo, inter, icons) = faces.inspect_err(|_| unsafe {
            let _ = write.UnregisterFontFileLoader(&loader);
        })?;
        let round = unsafe {
            factory.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES1 { startCap: D2D1_CAP_STYLE_ROUND, endCap: D2D1_CAP_STYLE_ROUND, dashCap: D2D1_CAP_STYLE_ROUND, lineJoin: D2D1_LINE_JOIN_ROUND, ..Default::default() },
                None,
            )?
        };
        let dashed = unsafe {
            factory.CreateStrokeStyle(
                &D2D1_STROKE_STYLE_PROPERTIES1 { lineJoin: D2D1_LINE_JOIN_ROUND, dashStyle: D2D1_DASH_STYLE_CUSTOM, ..Default::default() },
                Some(&[2.0, 2.0]),
            )?
        };
        Ok(Gfx {
            scratch: RefCell::new(None),
            guarded,
            factory,
            device,
            dxgi: dxgi.cast()?,
            dcomp,
            write,
            loader,
            archivo,
            inter,
            serif: std::cell::OnceCell::new(),
            icons,
            formats: RefCell::new(HashMap::new()),
            made: RefCell::new(HashMap::new()),
            spare: RefCell::new(Vec::new()),
            last_drawn: Cell::new(None),
            trimmed: Cell::new(true),
            last_painted: Cell::new(None),
            settled: Cell::new(true),
            showing: Cell::new(0),
            tending: Cell::new(0),
            stops: RefCell::new(HashMap::new()),
            layouts: RefCell::new(HashMap::new()),
            round: round.into(),
            dashed: dashed.into(),
            icon_paths: RefCell::new(HashMap::new()),
            pictures: RefCell::new(HashMap::new()),
        })
    }

    /// The Direct3D device it draws on.
    pub fn d3d(&self) -> Result<ID3D11Device> {
        self.dxgi.cast()
    }

    fn format(&self, font: Font) -> IDWriteTextFormat3 {
        if let Some((format, used)) = self.formats.borrow_mut().get_mut(&font.key()) {
            *used = Instant::now();
            return format.clone();
        }
        let axes = [
            DWRITE_FONT_AXIS_VALUE { axisTag: DWRITE_FONT_AXIS_TAG_WEIGHT, value: font.weight },
            DWRITE_FONT_AXIS_VALUE { axisTag: DWRITE_FONT_AXIS_TAG_WIDTH, value: font.width },
        ];
        let (name, collection): (PCWSTR, Option<&IDWriteFontCollection>) = match font.family {
            Family::Archivo => (PCWSTR(self.archivo.1.as_ptr()), Some(&self.archivo.0)),
            Family::Inter => (PCWSTR(self.inter.1.as_ptr()), Some(&self.inter.0)),
            Family::Segoe => (w!("Segoe UI Variable Text"), None),
            Family::SegoeDisplay => (w!("Segoe UI Variable Display"), None),
            Family::Icons => (self.icons, None),
            Family::YaHei => (w!("Microsoft YaHei UI"), None),
            // (Asked for only once it is had: see `serif::family`.)
            Family::Serif => match self.serif.get_or_init(|| crate::serif::face().and_then(|face| face_of(&self.write, &self.loader, face).ok())) {
                Some((collection, name)) => (PCWSTR(name.as_ptr()), Some(collection)),
                None => (w!("Microsoft YaHei UI"), None),
            },
        };
        let format = unsafe {
            self.write
                .CreateTextFormat(name, collection, &axes, font.size, w!("zh-cn"))
                .expect("text format")
        };
        unsafe {
            format.SetWordWrapping(DWRITE_WORD_WRAPPING_NO_WRAP).unwrap();
            format.SetParagraphAlignment(DWRITE_PARAGRAPH_ALIGNMENT_NEAR).unwrap();
            // Text too long for its box ends in an ellipsis.
            let trimming = DWRITE_TRIMMING { granularity: DWRITE_TRIMMING_GRANULARITY_CHARACTER, ..Default::default() };
            let sign = self.write.CreateEllipsisTrimmingSign(&format).unwrap();
            format.SetTrimming(&trimming, &sign).unwrap();
        }
        self.formats.borrow_mut().insert(font.key(), (format.clone(), Instant::now()));
        format
    }

    /// Lays `text` out on one line, `width` DIPs wide at most.
    pub fn layout(&self, text: &str, font: Font, width: f32, align: Align) -> IDWriteTextLayout {
        self.laid_out(text, font, width, align, false)
    }

    /// Lays `text` out in as many lines `width` DIPs wide as it takes,
    /// broken between words (or, in Chinese, characters).
    pub fn wrapped(&self, text: &str, font: Font, width: f32) -> IDWriteTextLayout {
        self.laid_out(text, font, width, Align::Start, true)
    }

    /// How tall `text` is laid out in lines `width` wide.
    pub fn wrapped_height(&self, text: &str, font: Font, width: f32) -> f32 {
        let mut metrics = DWRITE_TEXT_METRICS::default();
        unsafe { self.wrapped(text, font, width).GetMetrics(&mut metrics).unwrap() };
        metrics.height
    }

    fn laid_out(&self, text: &str, font: Font, width: f32, align: Align, wrap: bool) -> IDWriteTextLayout {
        let key = (text.to_string(), font.key(), width.max(0.0).to_bits(), align == Align::End, wrap);
        if let Some((layout, used)) = self.layouts.borrow_mut().get_mut(&key) {
            *used = Instant::now();
            return layout.clone();
        }
        let wide: Vec<u16> = text.encode_utf16().collect();
        let layout = unsafe { self.write.CreateTextLayout(&wide, &self.format(font), width.max(0.0), font.size * 2.0) }
            .expect("text layout");
        let alignment = if align == Align::End { DWRITE_TEXT_ALIGNMENT_TRAILING } else { DWRITE_TEXT_ALIGNMENT_LEADING };
        unsafe { layout.SetTextAlignment(alignment).unwrap() };
        if wrap {
            unsafe {
                layout.SetWordWrapping(DWRITE_WORD_WRAPPING_WRAP).unwrap();
                layout.SetTrimming(&DWRITE_TRIMMING::default(), None).unwrap();
            }
        }
        if font.tracking != 0.0 {
            // As CSS letter-spacing: added after every character.
            let range = DWRITE_TEXT_RANGE { startPosition: 0, length: wide.len() as u32 };
            let spaced: IDWriteTextLayout1 = layout.cast().unwrap();
            unsafe { spaced.SetCharacterSpacing(0.0, font.tracking * font.size, 0.0, range).unwrap() };
        }
        self.layouts.borrow_mut().insert(key, (layout.clone(), Instant::now()));
        layout
    }

    /// Where the first line's baseline falls below the top of `font`'s
    /// text, and how far its descent reaches below it (DIPs).
    pub fn baseline(&self, font: Font) -> (f32, f32) {
        let layout = self.layout("0", font, 10_000.0, Align::Start);
        let mut line = [DWRITE_LINE_METRICS::default()];
        let mut count = 0;
        unsafe { layout.GetLineMetrics(Some(&mut line), &mut count).unwrap() };
        (line[0].baseline, line[0].height - line[0].baseline)
    }

    /// Lets go of what Direct2D keeps from earlier drawing, such as the
    /// intermediate images of effects.
    pub fn clear_caches(&self) {
        unsafe { self.device.ClearResources(0) };
    }

    /// A window sized by hand has come to rest, and been drawn so (call after that frame): what was made for it
    /// frame by frame as it was sized is let go, and the graphics driver gives back the memory let go of meanwhile
    /// (a few hundred megabytes, else held a while, or for good).
    pub fn settle(&self) {
        self.scratch.borrow_mut().take();
        self.made.borrow_mut().clear();
        self.spare.borrow_mut().clear();
        unsafe { self.dxgi.Trim() };
    }

    /// One more window shows what it draws with this device (see `life::Showing`).
    pub(super) fn show_one(&self) {
        self.showing.set(self.showing.get() + 1);
    }

    /// One window fewer shows what it draws with this device. The last gone, what drawing holds is given back now,
    /// and again once nothing has drawn for `IDLE` (what the windows let go of is the driver's to give back only once
    /// the card has done with it).
    pub(super) fn hide_one(&self) {
        self.showing.set(self.showing.get().saturating_sub(1));
        if self.showing.get() == 0 {
            self.trim_now_and_again();
            self.tend();
        }
    }

    /// What drawing holds given back as it falls due, on a timer of the thread's (see `tended`): what painting
    /// layers took, once they have stayed as they are (`let_go_once_settled`); everything, once nothing is shown
    /// (`trim_when_idle`). The timer runs while either is still due.
    fn tend(&self) {
        if self.tending.get() != 0 {
            return;
        }
        let timer = unsafe { windows::Win32::UI::WindowsAndMessaging::SetTimer(None, 0, TEND_EVERY.as_millis() as u32, Some(tended)) };
        self.tending.set(timer);
    }

    /// Gives back what drawing holds on to (see `trim`) once no window has drawn with it for `IDLE`, if one has
    /// since it was last given back: whatever was drawn last (the panel as it went, a widget closed, the settings
    /// closed, what was measured as Glance started), the graphics driver keeps what it let go of till it is told to
    /// give it back.
    pub fn trim_when_idle(&self) {
        if self.showing.get() == 0 && !self.trimmed.get() && self.last_drawn.get().is_some_and(|at| at.elapsed() >= IDLE) {
            self.trim();
        }
    }

    /// What painting a layer anew took (its effects' textures, the bitmaps made for it from their keys alone) given
    /// back once no layer has been painted anew for `IDLE`, if one has since it was last given back: a panel or a
    /// widget that stays up redraws each second only what is not kept in its layers, and what painted those is
    /// otherwise held for good (a large glass widget's, tens of megabytes). Painted anew, they are made again.
    pub fn let_go_once_settled(&self) {
        if self.settled.get() || self.last_painted.get().is_none_or(|at| at.elapsed() < IDLE) {
            return;
        }
        self.settled.set(true);
        self.scratch.borrow_mut().take();
        self.made.borrow_mut().clear();
        self.spare.borrow_mut().clear();
        unsafe {
            self.device.ClearResources(0);
            if let Ok(context) = self.d3d().and_then(|device| device.GetImmediateContext()) {
                context.Flush();
            }
            self.dxgi.Trim();
        }
    }

    /// Gives back what drawing holds on to while nothing is on screen.
    pub fn trim(&self) {
        self.trimmed.set(true);
        self.scratch.borrow_mut().take();
        self.pictures.borrow_mut().clear();
        self.layouts.borrow_mut().clear();
        self.made.borrow_mut().clear();
        self.spare.borrow_mut().clear();
        self.stops.borrow_mut().clear();
        unsafe {
            self.device.ClearResources(0);
            // What was let go of is destroyed only once the card has done what was asked of it before: asked to be
            // done now, so that the driver has it to give back.
            if let Ok(context) = self.d3d().and_then(|device| device.GetImmediateContext()) {
                context.Flush();
            }
            self.dxgi.Trim();
        }
    }

    /// As `trim`, now, and again once nothing has drawn for a while (see `trim_when_idle`): for a window that has
    /// just gone, what it let go of may still be in the card's hands now.
    pub fn trim_now_and_again(&self) {
        self.trim();
        self.trimmed.set(false);
    }

    /// Forgets the text, and the fonts, no window has drawn for a while.
    pub fn sweep(&self) {
        self.pictures.borrow_mut().retain(|_, (_, used)| used.elapsed() < PICTURE_KEPT);
        self.layouts.borrow_mut().retain(|_, (_, used)| used.elapsed() < TEXT_KEPT);
        self.formats.borrow_mut().retain(|_, (_, used)| used.elapsed() < TEXT_KEPT);
        let mut gone = Vec::new();
        self.made.borrow_mut().retain(|_, (bitmap, used)| {
            let kept = used.elapsed() < TEXT_KEPT;
            if !kept {
                gone.push(bitmap.clone());
            }
            kept
        });
        gone.into_iter().for_each(|bitmap| self.let_go(bitmap));
        self.stops.borrow_mut().retain(|_, (_, used)| used.elapsed() < TEXT_KEPT);
    }

    /// The bitmap `key` stands for: the one made for it before, else made
    /// now by `make`. Those drawn least lately are let go of past
    /// `MADE_BUDGET` (a widget sized frame by frame makes one for each size).
    pub fn made(&self, key: [u32; 4], make: impl FnOnce() -> Result<ID2D1Bitmap1>) -> Result<ID2D1Bitmap1> {
        let now = Instant::now();
        if let Some((bitmap, used)) = self.made.borrow_mut().get_mut(&key) {
            *used = now;
            return Ok(bitmap.clone());
        }
        let bitmap = make()?;
        let mut made = self.made.borrow_mut();
        made.insert(key, (bitmap.clone(), now));
        let bytes = |bitmap: &ID2D1Bitmap1| {
            let size = unsafe { bitmap.GetPixelSize() };
            size.width as usize * size.height as usize * 4
        };
        let mut total: usize = made.values().map(|(bitmap, _)| bytes(bitmap)).sum();
        let mut gone = Vec::new();
        while total > MADE_BUDGET && made.len() > 1 {
            let Some(oldest) = made.iter().filter(|(k, _)| **k != key).min_by_key(|(_, (_, used))| *used).map(|(k, _)| *k) else { break };
            if let Some((bitmap, _)) = made.remove(&oldest) {
                total -= bytes(&bitmap);
                gone.push(bitmap);
            }
        }
        drop(made);
        gone.into_iter().for_each(|bitmap| self.let_go(bitmap));
        Ok(bitmap)
    }

    /// Keeps `bitmap`, let go of, to be drawn on again (see `drawn_on`), if it is one drawn on and there is room
    /// for it within `MADE_BUDGET`.
    fn let_go(&self, bitmap: ID2D1Bitmap1) {
        if unsafe { bitmap.GetOptions() }.0 & D2D1_BITMAP_OPTIONS_TARGET.0 == 0 {
            return;
        }
        let bytes = |bitmap: &ID2D1Bitmap1| {
            let size = unsafe { bitmap.GetPixelSize() };
            size.width as usize * size.height as usize * 4
        };
        let mut spare = self.spare.borrow_mut();
        if spare.iter().map(bytes).sum::<usize>() + bytes(&bitmap) <= MADE_BUDGET {
            spare.push(bitmap);
        }
    }

    /// A bitmap to draw on, at least `pixels` large, at `dpi`: one let go of before (see `made`) that holds that
    /// many pixels and no more than twice as many each way, its memory taken at `dpi`; else a new one. What is
    /// made for keys anew frame by frame (a window sized by hand) draws on the same memory, not on new memory
    /// each frame (each new one is the graphics card's, given back only a while after it is let go).
    /// `picture` as a bitmap on the card: made the first time it is drawn, and kept while it is drawn again (see
    /// `PICTURE_KEPT`).
    fn picture_bitmap(&self, dc: &ID2D1DeviceContext, picture: &super::canvas::Picture) -> Option<ID2D1Bitmap1> {
        if let Some((bitmap, used)) = self.pictures.borrow_mut().get_mut(&picture.key) {
            *used = Instant::now();
            return Some(bitmap.clone());
        }
        if picture.pixels.len() < (picture.side * picture.side * 4) as usize {
            return None;
        }
        let properties = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0,
            dpiY: 96.0,
            ..Default::default()
        };
        let size = D2D_SIZE_U { width: picture.side, height: picture.side };
        let bitmap = unsafe { dc.CreateBitmap(size, Some(picture.pixels.as_ptr().cast()), picture.side * 4, &properties) }.ok()?;
        self.pictures.borrow_mut().insert(picture.key, (bitmap.clone(), Instant::now()));
        Some(bitmap)
    }

    pub fn drawn_on(&self, dc: &ID2D1DeviceContext, pixels: D2D_SIZE_U, dpi: f32) -> Result<ID2D1Bitmap1> {
        let properties = D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: dpi,
            dpiY: dpi,
            bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
            ..Default::default()
        };
        let found = {
            let mut spare = self.spare.borrow_mut();
            let serves = |bitmap: &ID2D1Bitmap1| {
                let have = unsafe { bitmap.GetPixelSize() };
                have.width >= pixels.width && have.height >= pixels.height && have.width <= 2 * pixels.width.max(16) && have.height <= 2 * pixels.height.max(16)
            };
            spare.iter().position(serves).map(|index| spare.swap_remove(index))
        };
        unsafe {
            match found {
                Some(bitmap) => {
                    let mut had = (0.0, 0.0);
                    bitmap.GetDpi(&mut had.0, &mut had.1);
                    if had.0 == dpi {
                        Ok(bitmap)
                    } else {
                        dc.CreateBitmapFromDxgiSurface(&bitmap.GetSurface()?, Some(&properties))
                    }
                }
                None => dc.CreateBitmap(pixels, None, 0, &properties),
            }
        }
    }

    /// How far below the top of its line box `text`'s ink starts and ends.
    pub fn ink(&self, text: &str, font: Font) -> (f32, f32) {
        // Laid out in a box twice the font's size tall (see `laid_out`); the
        // overhangs say how far the ink reaches beyond it.
        let overhang = unsafe { self.layout(text, font, 10_000.0, Align::Start).GetOverhangMetrics().unwrap() };
        (-overhang.top, font.size * 2.0 + overhang.bottom)
    }

    /// How wide `text` is in `font`, in DIPs.
    pub fn measure(&self, text: &str, font: Font) -> f32 {
        let mut metrics = DWRITE_TEXT_METRICS::default();
        unsafe { self.layout(text, font, 10_000.0, Align::Start).GetMetrics(&mut metrics).unwrap() };
        metrics.widthIncludingTrailingWhitespace
    }
}

/// A shipped face, unpacked from its WOFF2 container, as a collection of
/// its own, and the family name it goes by.
fn shipped_face(write: &IDWriteFactory6, loader: &IDWriteInMemoryFontFileLoader, packed: &[u8]) -> Result<(IDWriteFontCollection, HSTRING)> {
    unsafe {
        let stream = write.UnpackFontFile(DWRITE_CONTAINER_TYPE_WOFF2, packed.as_ptr().cast(), packed.len() as u32)?;
        let size = stream.GetFileSize()?;
        let (mut start, mut context) = (std::ptr::null_mut(), std::ptr::null_mut());
        stream.ReadFileFragment(&mut start, 0, size, &mut context)?;
        let face = face_of(write, loader, std::slice::from_raw_parts(start.cast::<u8>(), size as usize));
        stream.ReleaseFileFragment(context);
        face
    }
}

/// A face from its file's bytes (OpenType), as a collection of its own, and the family name it goes by.
fn face_of(write: &IDWriteFactory6, loader: &IDWriteInMemoryFontFileLoader, bytes: &[u8]) -> Result<(IDWriteFontCollection, HSTRING)> {
    unsafe {
        // Without an owner, the loader keeps a copy of the data.
        let file = loader.CreateInMemoryFontFileReference(write, bytes.as_ptr().cast(), bytes.len() as u32, None);
        let builder = write.CreateFontSetBuilder()?;
        IDWriteFontSetBuilder1::AddFontFile(&builder, &file?)?;
        let collection: IDWriteFontCollection =
            write.CreateFontCollectionFromFontSet(&builder.CreateFontSet()?, DWRITE_FONT_FAMILY_MODEL_TYPOGRAPHIC)?.cast()?;
        let names = collection.GetFontFamily(0)?.GetFamilyNames()?;
        let mut name = vec![0u16; names.GetStringLength(0)? as usize + 1];
        names.GetString(0, &mut name)?;
        name.pop();
        Ok((collection, HSTRING::from_wide(&name)))
    }
}

impl Gfx {
    /// `icon`'s figures as one geometry, on its 24-unit grid; made once.
    fn icon_path(&self, icon: Icon) -> Option<ID2D1PathGeometry1> {
        if let Some(path) = self.icon_paths.borrow().get(&icon) {
            return Some(path.clone());
        }
        let point = |(x, y): (f32, f32)| Vector2 { X: x, Y: y };
        let path = (|| -> Result<ID2D1PathGeometry1> {
            unsafe {
                let path = self.factory.CreatePathGeometry()?;
                let sink = path.Open()?;
                for figure in icon.figures() {
                    sink.BeginFigure(point(figure.start), D2D1_FIGURE_BEGIN_HOLLOW);
                    for segment in &figure.segments {
                        match *segment {
                            Segment::Line(to) => sink.AddLine(point(to)),
                            Segment::Arc { to, radii, rotation, large, clockwise } => sink.AddArc(&D2D1_ARC_SEGMENT {
                                point: point(to),
                                size: D2D_SIZE_F { width: radii.0, height: radii.1 },
                                rotationAngle: rotation,
                                sweepDirection: if clockwise { D2D1_SWEEP_DIRECTION_CLOCKWISE } else { D2D1_SWEEP_DIRECTION_COUNTER_CLOCKWISE },
                                arcSize: if large { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL },
                            }),
                            Segment::Cubic(c1, c2, to) => sink.AddBezier(&D2D1_BEZIER_SEGMENT { point1: point(c1), point2: point(c2), point3: point(to) }),
                        }
                    }
                    sink.EndFigure(if figure.closed { D2D1_FIGURE_END_CLOSED } else { D2D1_FIGURE_END_OPEN });
                }
                sink.Close()?;
                Ok(path)
            }
        })()
        .ok()?;
        self.icon_paths.borrow_mut().insert(icon, path.clone());
        Some(path)
    }
}

#[cfg(feature = "studio")]
impl Gfx {
    /// Draws `paint` into a picture `size` pixels large, at `scale` pixels
    /// per DIP, off screen; returns its pixels, rows top to bottom,
    /// premultiplied BGRA. For the studio, which films the panel.
    pub fn draw_offscreen(&self, size: (u32, u32), scale: f32, paint: impl FnOnce(&Frame)) -> Result<Vec<u8>> {
        use windows::Win32::Graphics::Direct2D::{
            D2D1_BITMAP_OPTIONS, D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_CPU_READ,
            D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_MAP_OPTIONS_READ,
        };
        let properties = |options: D2D1_BITMAP_OPTIONS| D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0 * scale,
            dpiY: 96.0 * scale,
            bitmapOptions: options,
            ..Default::default()
        };
        let pixels = D2D_SIZE_U { width: size.0, height: size.1 };
        unsafe {
            let dc = self.device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            let target = dc.CreateBitmap(pixels, None, 0, &properties(D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW))?;
            dc.SetTarget(&target);
            dc.SetDpi(96.0 * scale, 96.0 * scale);
            dc.BeginDraw();
            dc.SetTransform(&Matrix3x2::identity());
            dc.Clear(Some(&D2D1_COLOR_F::default()));
            let brush = dc.CreateSolidColorBrush(&D2D1_COLOR_F::default(), None)?;
            paint(&Frame { gfx: self, dc: dc.clone(), brush, base: Matrix3x2::identity(), atlas: None });
            dc.EndDraw(None, None)?;
            let read = dc.CreateBitmap(pixels, None, 0, &properties(D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW))?;
            read.CopyFromBitmap(None, &target, None)?;
            let mapped = read.Map(D2D1_MAP_OPTIONS_READ)?;
            let row = size.0 as usize * 4;
            let mut out = Vec::with_capacity(row * size.1 as usize);
            for y in 0..size.1 as usize {
                out.extend_from_slice(std::slice::from_raw_parts(mapped.bits.add(y * mapped.pitch as usize), row));
            }
            read.Unmap()?;
            Ok(out)
        }
    }

    /// As `draw_offscreen`, the picture drawn as a frame handed a region `size` large at `offset` (physical pixels)
    /// of a larger atlas is (see `Frame::begun`): that region's pixels, as `draw_offscreen` gives them. (For tests.)
    #[cfg(test)]
    pub fn draw_in_atlas(&self, size: (u32, u32), offset: (i32, i32), scale: f32, paint: impl FnOnce(&Frame)) -> Result<Vec<u8>> {
        use windows::Win32::Graphics::Direct2D::{
            D2D1_BITMAP_OPTIONS, D2D1_BITMAP_OPTIONS_CANNOT_DRAW, D2D1_BITMAP_OPTIONS_CPU_READ, D2D1_DEVICE_CONTEXT_OPTIONS_NONE, D2D1_MAP_OPTIONS_READ,
        };
        let properties = |options: D2D1_BITMAP_OPTIONS| D2D1_BITMAP_PROPERTIES1 {
            pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
            dpiX: 96.0,
            dpiY: 96.0,
            bitmapOptions: options,
            ..Default::default()
        };
        let whole = D2D_SIZE_U { width: size.0 + offset.0 as u32, height: size.1 + offset.1 as u32 };
        unsafe {
            let dc = self.device.CreateDeviceContext(D2D1_DEVICE_CONTEXT_OPTIONS_NONE)?;
            let atlas = dc.CreateBitmap(whole, None, 0, &properties(D2D1_BITMAP_OPTIONS_TARGET | D2D1_BITMAP_OPTIONS_CANNOT_DRAW))?;
            dc.SetTarget(&atlas);
            dc.BeginDraw();
            let frame = Frame::begun(self, dc.clone(), POINT { x: offset.0, y: offset.1 }, size, scale)?;
            paint(&frame);
            frame.finish()?;
            dc.EndDraw(None, None)?;
            let read = dc.CreateBitmap(whole, None, 0, &properties(D2D1_BITMAP_OPTIONS_CPU_READ | D2D1_BITMAP_OPTIONS_CANNOT_DRAW))?;
            read.CopyFromBitmap(None, &atlas, None)?;
            let mapped = read.Map(D2D1_MAP_OPTIONS_READ)?;
            let row = size.0 as usize * 4;
            let mut out = Vec::with_capacity(row * size.1 as usize);
            for y in 0..size.1 as usize {
                let line = mapped.bits.add((y + offset.1 as usize) * mapped.pitch as usize + offset.0 as usize * 4);
                out.extend_from_slice(std::slice::from_raw_parts(line, row));
            }
            read.Unmap()?;
            Ok(out)
        }
    }
}

/// One frame being drawn onto a surface.
pub struct Frame<'a> {
    pub gfx: &'a Gfx,
    pub dc: ID2D1DeviceContext,
    brush: ID2D1SolidColorBrush,
    /// Where the surface's region starts (DIPs).
    base: Matrix3x2,
    /// Handed a region of the atlas: the atlas, the bitmap drawn on meanwhile, and the region's corner and size
    /// (physical pixels; see `begun`).
    atlas: Option<(ID2D1Image, ID2D1Bitmap1, POINT, (u32, u32))>,
}

impl Canvas for Frame<'_> {
    fn text(&self, text: &str, font: Font, color: Color, x: f32, y: f32, width: f32, align: Align) {
        Frame::text(self, text, font, color, x, y, width, align);
    }

    fn text_scaled(&self, text: &str, font: Font, color: Color, x: f32, y: f32, width: f32, align: Align, scale: f32) {
        // Laid out at its own size and drawn larger or smaller about its
        // corner: no font, nor glyphs, made for a size passed through.
        let mut parent = Matrix3x2::default();
        unsafe {
            self.dc.GetTransform(&mut parent);
            let about = Matrix3x2::translation(-x, -y) * Matrix3x2::scale(scale, scale) * Matrix3x2::translation(x, y);
            self.dc.SetTransform(&(about * parent));
        }
        Frame::text(self, text, font, color, x, y, width / scale.max(0.01), align);
        unsafe { self.dc.SetTransform(&parent) };
    }

    fn measure(&self, text: &str, font: Font) -> f32 {
        self.gfx.measure(text, font)
    }

    fn baseline(&self, font: Font) -> (f32, f32) {
        self.gfx.baseline(font)
    }

    fn ink(&self, text: &str, font: Font) -> (f32, f32) {
        self.gfx.ink(text, font)
    }

    fn clip(&self, x: f32, y: f32, width: f32, height: f32) {
        // (On whole pixels, as what is filled in it is: else a fill to its edge stopped half a pixel short of the same
        // fill unclipped, and what was under showed there.)
        let (x, y, width, height) = self.on_pixels(x, y, width, height);
        unsafe { self.dc.PushAxisAlignedClip(&rect(x, y, width, height), D2D1_ANTIALIAS_MODE_PER_PRIMITIVE) };
    }

    fn unclip(&self) {
        unsafe { self.dc.PopAxisAlignedClip() };
    }

    fn fill(&self, color: Color, x: f32, y: f32, width: f32, height: f32) {
        let (x, y, width, height) = self.on_pixels(x, y, width, height);
        unsafe { self.dc.FillRectangle(&rect(x, y, width, height), self.brush(color)) };
    }

    fn fill_rounded(&self, color: Color, x: f32, y: f32, width: f32, height: f32, radius: f32) {
        if radius == 0.0 {
            return Canvas::fill(self, color, x, y, width, height);
        }
        let (x, y, width, height) = self.on_pixels(x, y, width, height);
        let radius = radius.min(width / 2.0).min(height / 2.0);
        let shape = D2D1_ROUNDED_RECT { rect: rect(x, y, width, height), radiusX: radius, radiusY: radius };
        unsafe { self.dc.FillRoundedRectangle(&shape, self.brush(color)) };
    }

    fn fill_circle(&self, color: Color, centre: Point, radius: f32) {
        // (Its box on whole pixels, as a rectangle's: drawn with a ring round it or a glyph in it, which are, it kept
        // its own place between pixels, and they moved apart.)
        let (x, y, width, height) = self.on_pixels(centre.x - radius, centre.y - radius, 2.0 * radius, 2.0 * radius);
        let dot = D2D1_ELLIPSE { point: Vector2 { X: x + width / 2.0, Y: y + height / 2.0 }, radiusX: width / 2.0, radiusY: height / 2.0 };
        unsafe { self.dc.FillEllipse(&dot, self.brush(color)) };
    }

    fn stroke(&self, points: &[Point], color: Color, width: f32) {
        if let Some(path) = self.path(points, false) {
            // (Its corners round: a sharp one is drawn out to a point, which on a chart's line turning at nothing reaches below nothing.)
            unsafe { self.dc.DrawGeometry(&path, self.brush(color), width, &self.gfx.round) };
        }
    }

    fn stroke_dashed(&self, points: &[Point], color: Color, width: f32) {
        if let Some(path) = self.path(points, false) {
            unsafe { self.dc.DrawGeometry(&path, self.brush(color), width, &self.gfx.dashed) };
        }
    }

    fn fill_shape(&self, points: &[Point], fill: Fill) {
        let Some(path) = self.path(points, true) else { return };
        if let Some(brush) = self.paint_with(fill) {
            unsafe { self.dc.FillGeometry(&path, &brush, None) };
        }
    }

    fn arc(&self, centre: Point, radius: f32, from: f32, sweep: f32, color: Color, width: f32) {
        let brush = self.brush(color);
        if sweep >= std::f32::consts::TAU {
            let circle = D2D1_ELLIPSE { point: Vector2 { X: centre.x, Y: centre.y }, radiusX: radius, radiusY: radius };
            unsafe { self.dc.DrawEllipse(&circle, brush, width, None) };
            return;
        }
        if sweep <= 0.0 {
            return;
        }
        let at = |angle: f32| Vector2 { X: centre.x + radius * angle.sin(), Y: centre.y - radius * angle.cos() };
        let made = (|| -> Result<ID2D1PathGeometry1> {
            unsafe {
                let path = self.gfx.factory.CreatePathGeometry()?;
                let sink = path.Open()?;
                sink.BeginFigure(at(from), D2D1_FIGURE_BEGIN_HOLLOW);
                sink.AddArc(&D2D1_ARC_SEGMENT {
                    point: at(from + sweep),
                    size: D2D_SIZE_F { width: radius, height: radius },
                    rotationAngle: 0.0,
                    sweepDirection: D2D1_SWEEP_DIRECTION_CLOCKWISE,
                    arcSize: if sweep > std::f32::consts::PI { D2D1_ARC_SIZE_LARGE } else { D2D1_ARC_SIZE_SMALL },
                });
                sink.EndFigure(D2D1_FIGURE_END_OPEN);
                sink.Close()?;
                Ok(path)
            }
        })();
        if let Ok(path) = made {
            unsafe { self.dc.DrawGeometry(&path, brush, width, &self.gfx.round) };
        }
    }

    fn icon(&self, icon: Icon, centre: Point, size: f32, color: Color) {
        let Some(path) = self.gfx.icon_path(icon) else { return };
        let scale = size / 24.0;
        // (Its box's corner on a whole pixel: drawn between two, every stroke of it was spread over both, and blurred.)
        let (left, top) = self.on_pixel(centre.x - size / 2.0, centre.y - size / 2.0);
        let mut parent = Matrix3x2::default();
        unsafe {
            self.dc.GetTransform(&mut parent);
            let placed = Matrix3x2::scale(scale, scale) * Matrix3x2::translation(left, top) * parent;
            self.dc.SetTransform(&placed);
            self.dc.DrawGeometry(&path, self.brush(color), icons::STROKE, &self.gfx.round);
            self.dc.SetTransform(&parent);
        }
    }

    fn picture(&self, picture: &super::canvas::Picture, x: f32, y: f32, size: f32, radius: f32, opacity: f32) {
        let Some(bitmap) = self.gfx.picture_bitmap(&self.dc, picture) else { return };
        // (On whole pixels, its edges sharp; its size kept, so as not to scale it a second time.)
        let (x, y) = self.on_pixel(x, y);
        let scale = size / picture.side.max(1) as f32;
        unsafe {
            let properties = windows::Win32::Graphics::Direct2D::D2D1_BITMAP_BRUSH_PROPERTIES1 {
                extendModeX: windows::Win32::Graphics::Direct2D::D2D1_EXTEND_MODE_CLAMP,
                extendModeY: windows::Win32::Graphics::Direct2D::D2D1_EXTEND_MODE_CLAMP,
                interpolationMode: windows::Win32::Graphics::Direct2D::D2D1_INTERPOLATION_MODE_LINEAR,
            };
            let placed = windows::Win32::Graphics::Direct2D::D2D1_BRUSH_PROPERTIES { opacity, transform: Matrix3x2::scale(scale, scale) * Matrix3x2::translation(x, y) };
            let Ok(brush) = self.dc.CreateBitmapBrush(&bitmap, Some(&properties), Some(&placed)) else { return };
            let radius = radius.max(0.0).min(size / 2.0);
            self.dc.FillRoundedRectangle(&D2D1_ROUNDED_RECT { rect: rect(x, y, size, size), radiusX: radius, radiusY: radius }, &brush);
        }
    }

    fn stroke_rounded(&self, fill: Fill, x: f32, y: f32, width: f32, height: f32, radius: f32, line: f32) {
        let (x, y, width, height) = self.stroked(x, y, width, height, line);
        let radius = radius.max(0.0).min(width / 2.0).min(height / 2.0);
        let shape = D2D1_ROUNDED_RECT { rect: rect(x, y, width, height), radiusX: radius, radiusY: radius };
        if let Some(brush) = self.paint_with(fill) {
            unsafe { self.dc.DrawRoundedRectangle(&shape, &brush, line, None) };
        }
    }

    fn shadow(&self, color: Color, x: f32, y: f32, width: f32, height: f32, radius: f32, blur: f32, drop: f32) {
        use windows::Win32::Graphics::Direct2D::{CLSID_D2D1Composite, D2D1_COMPOSITE_PROP_MODE, D2D1_PROPERTY_TYPE_ENUM};
        use windows::Win32::Graphics::Direct2D::Common::D2D1_COMPOSITE_MODE_SOURCE_OUT;
        let dc = &self.dc;
        // The rectangle as a picture of its own, recorded in the frame's
        // DIPs (drawn back through the frame's transform), where `down`
        // below its place.
        let shape = |down: f32| -> Result<ID2D1Image> {
            unsafe {
                let list = dc.CreateCommandList()?;
                let (target, mut transform) = (dc.GetTarget()?, Matrix3x2::default());
                dc.GetTransform(&mut transform);
                dc.SetTarget(&list);
                dc.SetTransform(&Matrix3x2::identity());
                Canvas::fill_rounded(self, Color::hex(0, 1.0), x, y + down, width, height, radius);
                dc.SetTarget(&target);
                dc.SetTransform(&transform);
                list.Close()?;
                list.cast()
            }
        };
        // Without its pictures (the device going, which the frame's end
        // tells), no shadow this time.
        let _ = (|| -> Result<()> {
            unsafe {
                let cast = dc.CreateEffect(&CLSID_D2D1Shadow)?;
                cast.SetInput(0, &shape(drop)?, true);
                cast.SetValue(D2D1_SHADOW_PROP_BLUR_STANDARD_DEVIATION.0 as u32, D2D1_PROPERTY_TYPE_FLOAT, &blur.to_ne_bytes())?;
                let rgba = [color.r, color.g, color.b, color.a];
                cast.SetValue(D2D1_SHADOW_PROP_COLOR.0 as u32, D2D1_PROPERTY_TYPE_VECTOR4, std::slice::from_raw_parts(rgba.as_ptr().cast(), 16))?;
                // The shadow where the rectangle is not: the rectangle as the
                // destination, the shadow kept outside it.
                let outside = dc.CreateEffect(&CLSID_D2D1Composite)?;
                outside.SetInput(0, &shape(0.0)?, true);
                outside.SetInput(1, &cast.GetOutput()?, true);
                outside.SetValue(D2D1_COMPOSITE_PROP_MODE.0 as u32, D2D1_PROPERTY_TYPE_ENUM, &(D2D1_COMPOSITE_MODE_SOURCE_OUT.0 as u32).to_ne_bytes())?;
                dc.DrawImage(&outside.GetOutput()?, None, None, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_SOURCE_OVER);
            }
            Ok(())
        })();
    }
}

impl<'a> Frame<'a> {
    /// A frame on a composition surface's context just handed out for
    /// drawing, at `offset` (physical pixels) in it, `size` (physical pixels) large: cleared, at `scale`
    /// physical pixels per DIP. Drawn, it is to be `finish`ed before the surface's drawing ends.
    ///
    /// Handed a region of a larger atlas, it is drawn from its own corner on a bitmap of its own first, then put in
    /// its region pixel for pixel (see `finish`). Drawn straight into the region, its place there is reckoned in DIPs
    /// (355 px at 1.5 is 236.666…), rarely exactly, and where it was put by a hair either way, text's baselines,
    /// which DirectWrite puts on whole pixels, and its glyphs' places along the line went to one pixel or step or the
    /// other as the region did from frame to frame: the overlay's words shook by a pixel as its readings changed.
    pub(super) fn begun(gfx: &'a Gfx, dc: ID2D1DeviceContext, offset: POINT, size: (u32, u32), scale: f32) -> Result<Self> {
        let atlas = if offset.x != 0 || offset.y != 0 {
            let target = unsafe { dc.GetTarget()? };
            let mut kept = gfx.scratch.borrow_mut();
            let fits = kept.as_ref().is_some_and(|(_, has)| has.0 >= size.0 && has.1 >= size.1);
            if !fits {
                let has = kept.as_ref().map_or((0, 0), |(_, has)| *has);
                let made = (has.0.max(size.0).max(1), has.1.max(size.1).max(1));
                let properties = D2D1_BITMAP_PROPERTIES1 {
                    pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                    dpiX: 96.0,
                    dpiY: 96.0,
                    bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
                    ..Default::default()
                };
                let bitmap = unsafe { dc.CreateBitmap(D2D_SIZE_U { width: made.0, height: made.1 }, None, 0, &properties)? };
                *kept = Some((bitmap, made));
            }
            let scratch = kept.as_ref().unwrap().0.clone();
            unsafe { dc.SetTarget(&scratch) };
            Some((target, scratch, offset, size))
        } else {
            None
        };
        let base = Matrix3x2::identity();
        unsafe {
            dc.SetDpi(96.0 * scale, 96.0 * scale);
            dc.SetTransform(&base);
            // A transparent surface takes no ClearType.
            dc.SetTextAntialiasMode(D2D1_TEXT_ANTIALIAS_MODE_GRAYSCALE);
            // The system's text rendering, unless the frame asks for crisp
            // text: the context may be one another surface had it set on.
            dc.SetTextRenderingParams(None);
            dc.Clear(Some(&D2D1_COLOR_F::default()));
        }
        let brush = unsafe { dc.CreateSolidColorBrush(&D2D1_COLOR_F::default(), None)? };
        Ok(Frame { gfx, dc, brush, base, atlas })
    }

    /// What was drawn put in the region of the atlas the frame was handed (see `begun`), pixel for pixel: its
    /// place given in pixels, no DIPs between.
    pub(super) fn finish(&self) -> Result<()> {
        let Some((target, scratch, offset, size)) = &self.atlas else { return Ok(()) };
        use windows::Win32::Graphics::Direct2D::Common::D2D1_COMPOSITE_MODE_SOURCE_COPY;
        use windows::Win32::Graphics::Direct2D::{D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_UNIT_MODE_DIPS, D2D1_UNIT_MODE_PIXELS};
        unsafe {
            let dc = &self.dc;
            dc.SetTarget(target);
            dc.SetUnitMode(D2D1_UNIT_MODE_PIXELS);
            dc.SetTransform(&Matrix3x2::identity());
            let from = D2D_RECT_F { left: 0.0, top: 0.0, right: size.0 as f32, bottom: size.1 as f32 };
            let image: ID2D1Image = scratch.cast()?;
            dc.DrawImage(&image, Some(&Vector2 { X: offset.x as f32, Y: offset.y as f32 }), Some(&from), D2D1_INTERPOLATION_MODE_NEAREST_NEIGHBOR, D2D1_COMPOSITE_MODE_SOURCE_COPY);
            dc.SetUnitMode(D2D1_UNIT_MODE_DIPS);
        }
        Ok(())
    }

    /// A brush for `fill`; none if Direct2D cannot make it.
    fn paint_with(&self, fill: Fill) -> Option<windows::Win32::Graphics::Direct2D::ID2D1Brush> {
        match fill {
            Fill::Solid(color) => self.brush(color).cast().ok(),
            Fill::Down { top, from, bottom, to } => self.gradient(Vector2 { X: 0.0, Y: top }, from, Vector2 { X: 0.0, Y: bottom }, to),
        }
    }

    /// A brush fading from `from` at `start` to `to` at `end`.
    pub(super) fn gradient(&self, start: Vector2, from: Color, end: Vector2, to: Color) -> Option<windows::Win32::Graphics::Direct2D::ID2D1Brush> {
        // The colours' stops, made once for them while they are drawn with.
        let key = [from.r, from.g, from.b, from.a, to.r, to.g, to.b, to.a].map(f32::to_bits);
        let now = Instant::now();
        let collection = match self.gfx.stops.borrow_mut().get_mut(&key) {
            Some((collection, used)) => {
                *used = now;
                Some(collection.clone())
            }
            None => None,
        };
        let collection = match collection {
            Some(collection) => collection,
            None => {
                let stops = [D2D1_GRADIENT_STOP { position: 0.0, color: from.d2d() }, D2D1_GRADIENT_STOP { position: 1.0, color: to.d2d() }];
                let made = unsafe { ID2D1RenderTarget::CreateGradientStopCollection(&self.dc, &stops, D2D1_GAMMA_2_2, D2D1_EXTEND_MODE_CLAMP) }.ok()?;
                self.gfx.stops.borrow_mut().insert(key, (made.clone(), now));
                made
            }
        };
        unsafe {
            let line = D2D1_LINEAR_GRADIENT_BRUSH_PROPERTIES { startPoint: start, endPoint: end };
            self.dc.CreateLinearGradientBrush(&line, None, &collection).ok()?.cast().ok()
        }
    }

    /// The path through `points`, closed as a shape or open as a line;
    /// `None` without points, or if Direct2D cannot make it.
    fn path(&self, points: &[Point], closed: bool) -> Option<ID2D1PathGeometry1> {
        let (first, rest) = points.split_first()?;
        let rest: Vec<Vector2> = rest.iter().map(|p| Vector2 { X: p.x, Y: p.y }).collect();
        unsafe {
            let path = self.gfx.factory.CreatePathGeometry().ok()?;
            let sink = path.Open().ok()?;
            sink.BeginFigure(Vector2 { X: first.x, Y: first.y }, if closed { D2D1_FIGURE_BEGIN_FILLED } else { D2D1_FIGURE_BEGIN_HOLLOW });
            // A lone point (a reading between two gaps) has no lines: an empty
            // slice's pointer is a dangling placeholder, which Direct2D reads
            // all the same, and faults on.
            if !rest.is_empty() {
                sink.AddLines(&rest);
            }
            sink.EndFigure(D2D1_FIGURE_END_OPEN);
            sink.Close().ok()?;
            Some(path)
        }
    }

    /// Draws text from here on as sharply as grayscale antialiasing allows
    /// (a surface with nothing opaque beneath its text has no ClearType):
    /// stems fitted to the pixel grid, edges given more contrast.
    pub fn crisp_text(&self) {
        let params = unsafe {
            use windows::Win32::Graphics::DirectWrite::{DWRITE_GRID_FIT_MODE_ENABLED, DWRITE_PIXEL_GEOMETRY_FLAT, DWRITE_RENDERING_MODE1_GDI_CLASSIC};
            // GDI's own rendering: stems on whole pixels, smoothed across only,
            // as the system draws small text; sharpest at 100% scale.
            self.gfx.write.CreateCustomRenderingParams(1.8, 0.5, CRISP_CONTRAST, 0.0, DWRITE_PIXEL_GEOMETRY_FLAT, DWRITE_RENDERING_MODE1_GDI_CLASSIC, DWRITE_GRID_FIT_MODE_ENABLED)
        };
        if let Ok(params) = params {
            unsafe { self.dc.SetTextRenderingParams(&params) };
        }
    }

    /// The transform drawn through now and how many of the target's pixels a DIP is, where the transform keeps axes
    /// as they are (no turn, skew or mirror): what whole pixels are reckoned by.
    fn grid(&self) -> Option<(Matrix3x2, f32)> {
        let mut t = Matrix3x2::default();
        let mut dpi = (0.0f32, 0.0f32);
        unsafe {
            self.dc.GetTransform(&mut t);
            self.dc.GetDpi(&mut dpi.0, &mut dpi.1);
        }
        (t.M12 == 0.0 && t.M21 == 0.0 && t.M11 > 0.0 && t.M22 > 0.0 && dpi.0 > 0.0).then_some((t, dpi.0 / 96.0))
    }

    /// Where `(x, y)` (DIPs, as drawn now) lands on a whole pixel of what is drawn on, moved the least to (see `grid`);
    /// as it is where the transform turns, skews or mirrors, or for a place too far to count (as a whole frame's
    /// bounds are given).
    pub fn on_pixel(&self, x: f32, y: f32) -> (f32, f32) {
        let Some((t, device)) = self.grid() else { return (x, y) };
        let whole = |v: f32, scale: f32, offset: f32| {
            if !v.is_finite() || v.abs() > 1.0e6 {
                return v;
            }
            (whole_pixel((v * scale + offset) * device) / device - offset) / scale
        };
        (whole(x, t.M11, t.M31), whole(y, t.M22, t.M32))
    }

    /// The path `(x, y, width, height)` (DIPs) a line `line` wide is drawn along (its middle), put where the line
    /// covers whole pixels (see `grid`): a line an odd number of pixels wide on the middles of pixels, one given on
    /// their edges half a pixel in (a hairline on an edge was drawn across two pixels, at half strength); one even, on
    /// their edges. Given on the middles already (as the menus give theirs), it stays there.
    pub fn stroked(&self, x: f32, y: f32, width: f32, height: f32, line: f32) -> (f32, f32, f32, f32) {
        let Some((t, device)) = self.grid() else { return (x, y, width, height) };
        let odd = ((line * t.M11 * device).round() as i32) % 2 == 1;
        // (In the target's pixels and back.)
        let put = |v: f32, scale: f32, offset: f32, far: bool| {
            if !v.is_finite() || v.abs() > 1.0e6 {
                return v;
            }
            // (Taken to a 64th of a pixel first, as `whole_pixel` takes it: the same wherever in the atlas.)
            let at = ((v * scale + offset) * device * 64.0).round() / 64.0;
            let at = match (odd, far) {
                (true, false) => at.floor() + 0.5,
                (true, true) => at.ceil() - 0.5,
                (false, _) => whole_pixel(at),
            };
            (at / device - offset) / scale
        };
        let (left, right) = (put(x, t.M11, t.M31, false), put(x + width, t.M11, t.M31, true));
        let (top, bottom) = (put(y, t.M22, t.M32, false), put(y + height, t.M22, t.M32, true));
        (left, top, (right - left).max(0.0), (bottom - top).max(0.0))
    }

    /// The rectangle `(x, y, width, height)` (DIPs) with its edges on whole pixels (see `on_pixel`), none shrunk away:
    /// a side that had length keeps at least a pixel's.
    pub fn on_pixels(&self, x: f32, y: f32, width: f32, height: f32) -> (f32, f32, f32, f32) {
        let Some((t, device)) = self.grid() else { return (x, y, width, height) };
        let (left, top) = self.on_pixel(x, y);
        let (right, bottom) = self.on_pixel(x + width, y + height);
        let (one_x, one_y) = (1.0 / (device * t.M11), 1.0 / (device * t.M22));
        let width = if width > 0.0 { (right - left).max(one_x.min(width)) } else { right - left };
        let height = if height > 0.0 { (bottom - top).max(one_y.min(height)) } else { bottom - top };
        (left, top, width, height)
    }

    /// Draws from here on with `(x, y)`, in DIPs from the surface's corner, as the origin.
    pub fn origin(&self, x: f32, y: f32) {
        self.place(Matrix3x2::translation(x, y));
    }

    /// Draws from here on through `transform`, from the surface's corner.
    pub fn place(&self, transform: Matrix3x2) {
        unsafe { self.dc.SetTransform(&(transform * self.base)) };
    }

    /// Draws what `paint` draws `scale` times as large about `(x, y)` (DIPs), placed as it is now otherwise.
    pub fn scaled(&self, x: f32, y: f32, scale: f32, paint: impl FnOnce(&Frame)) {
        let mut was = Matrix3x2::default();
        unsafe { self.dc.GetTransform(&mut was) };
        let about = Matrix3x2::translation(-x, -y) * Matrix3x2 { M11: scale, M12: 0.0, M21: 0.0, M22: scale, M31: 0.0, M32: 0.0 } * Matrix3x2::translation(x, y);
        unsafe { self.dc.SetTransform(&(about * was)) };
        paint(self);
        unsafe { self.dc.SetTransform(&was) };
    }

    /// Draws what `paint` draws turned by `angle` (radians, clockwise) and stretched `sx`, `sy` times about `(x, y)`
    /// (DIPs), placed as it is now otherwise.
    pub fn turned(&self, x: f32, y: f32, angle: f32, (sx, sy): (f32, f32), paint: impl FnOnce(&Frame)) {
        let mut was = Matrix3x2::default();
        unsafe { self.dc.GetTransform(&mut was) };
        let (sin, cos) = angle.sin_cos();
        let turn = Matrix3x2 { M11: sx * cos, M12: sx * sin, M21: -sy * sin, M22: sy * cos, M31: 0.0, M32: 0.0 };
        unsafe { self.dc.SetTransform(&(Matrix3x2::translation(-x, -y) * turn * Matrix3x2::translation(x, y) * was)) };
        paint(self);
        unsafe { self.dc.SetTransform(&was) };
    }

    pub fn brush(&self, color: Color) -> &ID2D1SolidColorBrush {
        unsafe { self.brush.SetColor(&color.d2d()) };
        &self.brush
    }

    /// A glow of `color` round what `paint` draws (which is not itself drawn): its shape blurred by a deviation
    /// of `blur` DIPs. Nothing, where the device is going.
    pub fn glow(&self, color: Color, blur: f32, paint: impl FnOnce(&Frame)) {
        let dc = &self.dc;
        let _ = (|| -> Result<()> {
            unsafe {
                // What is drawn as a picture of its own, in the frame's DIPs (drawn back through its transform).
                let list = dc.CreateCommandList()?;
                let (target, mut transform) = (dc.GetTarget()?, Matrix3x2::default());
                dc.GetTransform(&mut transform);
                dc.SetTarget(&list);
                dc.SetTransform(&Matrix3x2::identity());
                paint(self);
                dc.SetTarget(&target);
                dc.SetTransform(&transform);
                list.Close()?;
                let cast = dc.CreateEffect(&CLSID_D2D1Shadow)?;
                cast.SetInput(0, &list.cast::<ID2D1Image>()?, true);
                cast.SetValue(D2D1_SHADOW_PROP_BLUR_STANDARD_DEVIATION.0 as u32, D2D1_PROPERTY_TYPE_FLOAT, &blur.to_ne_bytes())?;
                let rgba = [color.r, color.g, color.b, color.a];
                cast.SetValue(D2D1_SHADOW_PROP_COLOR.0 as u32, D2D1_PROPERTY_TYPE_VECTOR4, std::slice::from_raw_parts(rgba.as_ptr().cast(), 16))?;
                dc.DrawImage(&cast.GetOutput()?, None, None, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_SOURCE_OVER);
            }
            Ok(())
        })();
    }

    /// The outline of the rounded rectangle, dashed, a line `width` DIPs wide centred on it.
    #[allow(clippy::too_many_arguments)]
    pub fn stroke_rounded_dashed(&self, color: Color, x: f32, y: f32, w: f32, h: f32, radius: f32, width: f32) {
        let (x, y, w, h) = self.stroked(x, y, w, h, width);
        let shape = D2D1_ROUNDED_RECT { rect: rect(x, y, w, h), radiusX: radius, radiusY: radius };
        unsafe { self.dc.DrawRoundedRectangle(&shape, self.brush(color), width, &self.gfx.dashed) };
    }

    /// Draws `text` in lines `width` wide, the first line's box at `(x, y)` (on a whole pixel: see `text`).
    pub fn text_wrapped(&self, text: &str, font: Font, color: Color, x: f32, y: f32, width: f32) {
        let (x, y) = self.on_pixel(x, y);
        let layout = self.gfx.wrapped(text, font, width);
        unsafe { self.dc.DrawTextLayout(windows_numerics::Vector2 { X: x, Y: y }, &layout, self.brush(color), D2D1_DRAW_TEXT_OPTIONS_NONE) };
    }

    /// Draws `text` with its first baseline-box at `(x, y)`, in a box `width` wide.
    /// As `text`, its glyphs where they fall, not put on whole pixels: words moving by less than a pixel a frame
    /// (put on whole pixels, they stood still, then jumped).
    pub fn text_moving(&self, text: &str, font: Font, color: Color, x: f32, y: f32, width: f32, align: Align) {
        let layout = self.gfx.layout(text, font, width, align);
        unsafe {
            self.dc.DrawTextLayout(
                windows_numerics::Vector2 { X: x, Y: y },
                &layout,
                self.brush(color),
                windows::Win32::Graphics::Direct2D::D2D1_DRAW_TEXT_OPTIONS_NO_SNAP,
            )
        };
    }

    /// Draws `text` with its first baseline-box at `(x, y)`, put on a whole pixel (still text: drawn between two,
    /// its stems were spread over both, and it looked softer than the same words elsewhere), in a box `width` wide.
    pub fn text(&self, text: &str, font: Font, color: Color, x: f32, y: f32, width: f32, align: Align) {
        let (x, y) = self.on_pixel(x, y);
        let layout = self.gfx.layout(text, font, width, align);
        unsafe {
            self.dc.DrawTextLayout(
                windows_numerics::Vector2 { X: x, Y: y },
                &layout,
                self.brush(color),
                D2D1_DRAW_TEXT_OPTIONS_CLIP | D2D1_DRAW_TEXT_OPTIONS_NONE,
            )
        };
    }
}

/// How much the bitmaps made from their keys alone may hold (bytes; see
/// `Gfx::made`).
const MADE_BUDGET: usize = 32 << 20;

/// How long no window draws before what drawing holds is given back (see `Gfx::trim_when_idle`): longer than
/// anything shown waits between frames (a widget draws each sample, a second apart).
const IDLE: Duration = Duration::from_secs(3);

/// How often the thread's timer looks whether giving back what drawing holds is due (see `Gfx::tend`).
const TEND_EVERY: Duration = Duration::from_secs(1);

/// The thread's timer (see `Gfx::tend`): what is due given back, and the timer stopped once nothing more is.
unsafe extern "system" fn tended(_: HWND, _: u32, timer: usize, _: u32) {
    // (A device lost and made anew has a timer of its own, if any.)
    let Some(gfx) = CURRENT.with(|cell| cell.borrow().clone()).filter(|gfx| gfx.tending.get() == timer) else {
        let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::KillTimer(None, timer) };
        return;
    };
    gfx.let_go_once_settled();
    gfx.trim_when_idle();
    let idle_due = gfx.showing.get() == 0 && !gfx.trimmed.get();
    if gfx.settled.get() && !idle_due {
        let _ = unsafe { windows::Win32::UI::WindowsAndMessaging::KillTimer(None, timer) };
        gfx.tending.set(0);
    }
}

/// How much of the graphics card's memory Direct2D keeps for its effects' textures (bytes; see `Gfx::new`): a few
/// of a large panel's.
const EFFECTS_KEPT: u64 = 96 << 20;

/// The step a surface's size is made in (physical px; see `room`).
const SURFACE_STEP: u32 = 128;

thread_local! {
    /// Whether what this thread draws now is being sized by hand (see `room`).
    static SIZING: Cell<bool> = const { Cell::new(false) };
}

/// What this thread draws while the returned guard is held is being sized by hand (a panel, a widget or the
/// settings held by their edges), or is not; once it is dropped, what is drawn is not (another window drawn on the
/// thread after it is drawn as it is).
#[must_use]
pub fn sizing(on: bool) -> Sizing {
    SIZING.with(|sizing| sizing.set(on));
    Sizing
}

/// See `sizing`.
pub struct Sizing;

impl Drop for Sizing {
    fn drop(&mut self) {
        SIZING.with(|sizing| sizing.set(false));
    }
}

/// How much larger than asked room at rest may be (physical px each way), and the grain it is made in.
const AT_REST_SLACK: u32 = 64;
const AT_REST_GRAIN: u32 = 16;

/// Room for drawing that is sized as it is used (a window's surface, a layer, what is behind a window): whether
/// what was made `have` (physical px) still serves for `want`, and if not, how large to make it anew. While what is
/// drawn is sized by hand (see `sizing`), it serves while it holds `want` and is no more than twice as large, give
/// or take a step, each way, and is made anew with a quarter more than `want`, in steps: a window sized pixel by
/// pixel makes its room anew a few times, not at each size (each made anew is memory of the card's, given back
/// only a while after it is let go). At rest, it is as large as asked, near enough: a large window holds a dozen
/// such, and room to spare in each would be a third as much again of the card's memory.
pub fn room(have: Option<(u32, u32)>, want: (u32, u32)) -> Option<(u32, u32)> {
    let sizing = SIZING.with(Cell::get);
    let serves = |have: u32, want: u32| have >= want && if sizing { have <= 2 * want + SURFACE_STEP } else { have - want < AT_REST_SLACK };
    if have.is_some_and(|have| serves(have.0, want.0) && serves(have.1, want.1)) {
        return None;
    }
    let made = |want: u32| {
        if sizing {
            (want.max(1) + want / 4).div_ceil(SURFACE_STEP) * SURFACE_STEP
        } else {
            want.max(1).div_ceil(AT_REST_GRAIN) * AT_REST_GRAIN
        }
    };
    Some((made(want.0), made(want.1)))
}

/// A window's content: a composition surface the size of the window, or of
/// the part of it drawn in (see `offset`).
pub struct Surface {
    _target: IDCompositionTarget,
    visual: IDCompositionVisual2,
    surface: Option<IDCompositionSurface>,
    size: (u32, u32),
    /// Whether the window shows `surface` (one made anew is shown once it is first drawn on: see `draw`).
    shown: bool,
    /// Where in the window it is drawn (physical px), and where it was last.
    at: (i32, i32),
    placed: (i32, i32),
    /// Shown no further than the size it is drawn at, where its window is larger than that (see `clipped`): the size
    /// it was last cut to.
    clips: bool,
    clipped_to: (u32, u32),
    /// How long the last frame drawn on it took, part by part.
    pub took: Took,
}

/// A frame's time on a surface: till the compositor gave the surface to
/// draw on, what was drawn on it, and till it was handed back and the
/// compositor told.
#[derive(Clone, Copy, Default, Debug)]
pub struct Took {
    pub given: std::time::Duration,
    pub painted: std::time::Duration,
    pub handed: std::time::Duration,
}

impl Surface {
    pub fn new(gfx: &Gfx, hwnd: HWND) -> Result<Self> {
        let target = unsafe { gfx.dcomp.CreateTargetForHwnd(hwnd, true)? };
        let visual: IDCompositionVisual2 = unsafe { gfx.dcomp.CreateVisual()? };
        unsafe { target.SetRoot(&visual)? };
        Ok(Surface { _target: target, visual, surface: None, size: (0, 0), shown: false, at: (0, 0), placed: (0, 0), clips: false, clipped_to: (0, 0), took: Default::default() })
    }

    /// Shown no further than the size each frame is drawn at, for a window larger than what is drawn in it (the
    /// panel's): the room a surface is made with past that, and anything drawn into it, is not shown.
    pub fn clipped(mut self) -> Self {
        self.clips = true;
        self
    }

    /// Draws from now on at `at` (physical px) in its window, with the next
    /// frame: a window larger than what is drawn in it (the panel's) has only
    /// that much surface, and it is moved with what it shows, in the same
    /// frame of the compositor's.
    pub fn offset(&mut self, at: (i32, i32)) {
        self.at = at;
    }

    /// Draws one frame, at `scale` physical pixels per DIP, on a surface
    /// `size` physical pixels large.
    pub fn draw(&mut self, gfx: &Gfx, size: (u32, u32), scale: f32, paint: impl FnOnce(&Frame)) -> Result<()> {
        // Made with room to spare, and kept while it serves (see `room`): a
        // window sized frame by frame (a widget settling, sticking, sized)
        // draws on one surface, not a new one each frame. What lies past the
        // window is not shown.
        // (A surface made anew is shown only once it is drawn on: shown at once, anything committed meanwhile, by
        // this window or another on the device, showed it empty for a frame, and the window flashed out.)
        if let Some(made) = room(self.surface.as_ref().map(|_| self.size), size) {
            let surface = unsafe { gfx.dcomp.CreateSurface(made.0, made.1, DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_ALPHA_MODE_PREMULTIPLIED)? };
            self.surface = Some(surface);
            self.size = made;
            self.shown = false;
        }
        if self.at != self.placed {
            unsafe {
                self.visual.SetOffsetX2(self.at.0 as f32)?;
                self.visual.SetOffsetY2(self.at.1 as f32)?;
            }
            self.placed = self.at;
        }
        if self.clips && self.clipped_to != size {
            let clip = D2D_RECT_F { left: 0.0, top: 0.0, right: size.0 as f32, bottom: size.1 as f32 };
            unsafe { self.visual.SetClip2(&clip)? };
            self.clipped_to = size;
        }
        gfx.last_drawn.set(Some(Instant::now()));
        gfx.trimmed.set(false);
        let surface = self.surface.as_ref().unwrap();
        let mut offset = POINT::default();
        let began = std::time::Instant::now();
        let dc: ID2D1DeviceContext = unsafe { surface.BeginDraw(None, &mut offset)? };
        let frame = Frame::begun(gfx, dc, offset, self.size, scale)?;
        let given = began.elapsed();
        paint(&frame);
        frame.finish()?;
        let painted = began.elapsed();
        unsafe {
            surface.EndDraw()?;
            if !self.shown {
                self.visual.SetContent(surface)?;
                self.shown = true;
            }
            gfx.dcomp.Commit()?;
        }
        self.took = Took { given, painted: painted - given, handed: began.elapsed() - painted };
        Ok(())
    }

    /// Lets go of the drawing memory while the window is hidden.
    pub fn release(&mut self, gfx: &Gfx) {
        if self.surface.take().is_some() {
            unsafe {
                let _ = self.visual.SetContent(None);
                let _ = gfx.dcomp.Commit();
            }
        }
    }
}

/// The whole pixel `at` (physical px) is put on: the nearest, a half going up; first taken to a 64th of a pixel,
/// so that what is drawn at the same place in a window comes to the same pixel wherever in the compositor's atlas
/// the frame is drawn (its place there, in DIPs, is rarely exact: 355 px at 1.5 is 236.666…, and a place half
/// way between two pixels went to one or the other as the frames went, the text shaking by a pixel).
fn whole_pixel(at: f32) -> f32 {
    ((at * 64.0).round() / 64.0 + 0.5).floor()
}

/// The faint one-unit glow `halo` rings what `drawn` draws with (as CSS's `text-shadow: 0 0 1px`; see `Layer::draw`).
unsafe fn glow(dc: &ID2D1DeviceContext, drawn: &windows::Win32::Graphics::Direct2D::ID2D1CommandList, halo: Color) -> Result<ID2D1Image> {
    let glow = dc.CreateEffect(&CLSID_D2D1Shadow)?;
    glow.SetInput(0, drawn, true);
    glow.SetValue(D2D1_SHADOW_PROP_BLUR_STANDARD_DEVIATION.0 as u32, D2D1_PROPERTY_TYPE_FLOAT, &0.5f32.to_ne_bytes())?;
    let rgba = [halo.r, halo.g, halo.b, halo.a];
    glow.SetValue(D2D1_SHADOW_PROP_COLOR.0 as u32, D2D1_PROPERTY_TYPE_VECTOR4, std::slice::from_raw_parts(rgba.as_ptr().cast(), 16))?;
    glow.GetOutput()
}

/// Drawing kept in a bitmap of its own and painted again only when what it
/// shows changes, told by a key.
#[derive(Default)]
pub struct Layer {
    bitmap: Option<(u64, ID2D1Bitmap1)>,
}

impl Layer {
    /// Draws the layer: what `paint` draws in its own coordinates, inside
    /// `area` of them, placed by `local` within the frame's current
    /// transform, and kept at `scale` physical pixels per unit. It is painted
    /// again first only if `key` is not what it shows. A `halo` colour rings
    /// what is drawn with a faint one-unit glow, as CSS's `text-shadow: 0 0 1px`.
    #[allow(clippy::too_many_arguments)]
    pub fn draw(
        &mut self,
        frame: &Frame,
        key: u64,
        local: Matrix3x2,
        area: (f32, f32, f32, f32),
        scale: f32,
        halo: Option<Color>,
        paint: impl FnOnce(&Frame) -> Result<()>,
    ) -> Result<()> {
        let (left, top, width, height) = area;
        let size = (width, height);
        let dc = &frame.dc;
        // Being sized by hand (see `sizing`), what it shows changes each frame, and keeping it keeps nothing: it is
        // drawn where it shows, and its bitmap let go (a window's size of the card's memory, each layer), to be made
        // again at the size it comes to rest at.
        if SIZING.with(Cell::get) {
            self.bitmap = None;
            return unsafe { Self::draw_through(frame, local, area, halo, paint) };
        }
        if self.bitmap.as_ref().is_none_or(|(shown, _)| *shown != key) {
            let pixels = D2D_SIZE_U { width: (size.0 * scale).ceil() as u32, height: (size.1 * scale).ceil() as u32 };
            // The same pixels serve while they serve (see `room`): a layer
            // sized frame by frame keeps one bitmap's memory, not one for
            // each size; at another scale (a panel sized is drawn larger),
            // the same memory is taken at the new one.
            let had = self.bitmap.take().map(|(_, bitmap)| bitmap);
            let made = room(had.as_ref().map(|bitmap| unsafe { bitmap.GetPixelSize() }).map(|have| (have.width, have.height)), (pixels.width, pixels.height));
            let reusable = had.filter(|_| made.is_none());
            let pixels = made.map_or(pixels, |(width, height)| D2D_SIZE_U { width, height });
            let properties = D2D1_BITMAP_PROPERTIES1 {
                pixelFormat: D2D1_PIXEL_FORMAT { format: DXGI_FORMAT_B8G8R8A8_UNORM, alphaMode: D2D1_ALPHA_MODE_PREMULTIPLIED },
                dpiX: 96.0 * scale,
                dpiY: 96.0 * scale,
                bitmapOptions: D2D1_BITMAP_OPTIONS_TARGET,
                ..Default::default()
            };
            let bitmap = match reusable {
                Some(bitmap) => {
                    let mut dpi = (0.0, 0.0);
                    unsafe { bitmap.GetDpi(&mut dpi.0, &mut dpi.1) };
                    if dpi.0 == 96.0 * scale {
                        bitmap
                    } else {
                        unsafe { dc.CreateBitmapFromDxgiSurface(&bitmap.GetSurface()?, Some(&properties))? }
                    }
                }
                None => {
                    unsafe { dc.CreateBitmap(pixels, None, 0, &properties)? }
                }
            };
            unsafe {
                // A new target brings its own DPI to the context; the
                // frame's is put back with the frame's target.
                let target = dc.GetTarget()?;
                let mut transform = Matrix3x2::default();
                dc.GetTransform(&mut transform);
                let mut dpi = (0.0, 0.0);
                dc.GetDpi(&mut dpi.0, &mut dpi.1);
                dc.SetTarget(&bitmap);
                dc.SetDpi(96.0 * scale, 96.0 * scale);
                dc.SetTransform(&Matrix3x2::translation(-left, -top));
                dc.Clear(Some(&D2D1_COLOR_F::default()));
                // Its bitmap may be larger than asked (see above): what is
                // drawn stays inside what was.
                dc.PushAxisAlignedClip(&rect(left, top, width, height), D2D1_ANTIALIAS_MODE_ALIASED);
                // Ringed by a halo, the halo is painted in with it, once (not each time the layer is drawn): what is
                // drawn recorded, then drawn over its glow.
                let painted = match halo {
                    None => paint(frame),
                    Some(color) => {
                        let list = dc.CreateCommandList()?;
                        dc.SetTarget(&list);
                        // (Setting a target can change the context's DPI: the layer's, as it is painted at.)
                        dc.SetDpi(96.0 * scale, 96.0 * scale);
                        let painted = paint(frame);
                        list.Close()?;
                        dc.SetTarget(&bitmap);
                        dc.DrawImage(&glow(dc, &list, color)?, None, None, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_SOURCE_OVER);
                        dc.DrawImage(&list, None, None, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_SOURCE_OVER);
                        painted
                    }
                };
                dc.PopAxisAlignedClip();
                dc.SetTarget(&target);
                dc.SetDpi(dpi.0, dpi.1);
                dc.SetTransform(&transform);
                painted?;
            }
            self.bitmap = Some((key, bitmap));
            frame.gfx.last_painted.set(Some(Instant::now()));
            frame.gfx.settled.set(false);
            frame.gfx.tend();
        }
        let image: ID2D1Image = self.bitmap.as_ref().unwrap().1.cast()?;
        let at = windows_numerics::Vector2 { X: left, Y: top };
        let mut parent = Matrix3x2::default();
        unsafe { dc.GetTransform(&mut parent) };
        unsafe {
            let mut placed = local * parent;
            // Drawn pixel for pixel (moved only, at the scale it was kept at), it is put on whole pixels: a bitmap put
            // between them is resampled, and what is in it (its text) blurred. (A widget sized by its left or top
            // edge keeps what it shows to its right and bottom ones: rarely on a whole pixel.)
            let mut dpi = (0.0, 0.0);
            dc.GetDpi(&mut dpi.0, &mut dpi.1);
            let device = dpi.0 / 96.0;
            let moved_only = (placed.M11 - 1.0).abs() < 1e-4 && (placed.M22 - 1.0).abs() < 1e-4 && placed.M12 == 0.0 && placed.M21 == 0.0;
            if moved_only && (device - scale).abs() < 1e-4 {
                placed.M31 = whole_pixel((left + placed.M31) * device) / device - left;
                placed.M32 = whole_pixel((top + placed.M32) * device) / device - top;
            }
            dc.SetTransform(&placed);
            dc.DrawImage(&image, Some(&at), None, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_SOURCE_OVER);
            dc.SetTransform(&parent);
        }
        Ok(())
    }

    /// As `draw`, kept nowhere: drawn where it shows, through what `paint` draws recorded (a command list, not a
    /// bitmap) when it is ringed by a `halo`.
    unsafe fn draw_through(frame: &Frame, local: Matrix3x2, area: (f32, f32, f32, f32), halo: Option<Color>, paint: impl FnOnce(&Frame) -> Result<()>) -> Result<()> {
        let dc = &frame.dc;
        let (left, top, width, height) = area;
        let mut parent = Matrix3x2::default();
        dc.GetTransform(&mut parent);
        let placed = local * parent;
        let painted = match halo {
            None => {
                dc.SetTransform(&placed);
                dc.PushAxisAlignedClip(&rect(left, top, width, height), D2D1_ANTIALIAS_MODE_ALIASED);
                let painted = paint(frame);
                dc.PopAxisAlignedClip();
                painted
            }
            Some(color) => {
                let list = dc.CreateCommandList()?;
                let target = dc.GetTarget()?;
                // (Setting a target can change the context's DPI; it is put back.)
                let mut dpi = (0.0, 0.0);
                dc.GetDpi(&mut dpi.0, &mut dpi.1);
                dc.SetTarget(&list);
                dc.SetDpi(dpi.0, dpi.1);
                dc.SetTransform(&Matrix3x2::identity());
                dc.PushAxisAlignedClip(&rect(left, top, width, height), D2D1_ANTIALIAS_MODE_ALIASED);
                let painted = paint(frame);
                dc.PopAxisAlignedClip();
                list.Close()?;
                dc.SetTarget(&target);
                dc.SetDpi(dpi.0, dpi.1);
                dc.SetTransform(&placed);
                dc.DrawImage(&glow(dc, &list, color)?, None, None, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_SOURCE_OVER);
                dc.DrawImage(&list, None, None, D2D1_INTERPOLATION_MODE_LINEAR, D2D1_COMPOSITE_MODE_SOURCE_OVER);
                painted
            }
        };
        dc.SetTransform(&parent);
        painted
    }

    pub fn release(&mut self) {
        self.bitmap = None;
    }
}

/// A bitmap as an effect's input, at its own DPI: effects read their input
/// bitmaps' pixels at the context's DPI otherwise.
pub fn effect_input(dc: &ID2D1DeviceContext, bitmap: &ID2D1Bitmap1) -> Result<ID2D1Image> {
    unsafe {
        let mut dpi = (0.0f32, 0.0f32);
        bitmap.GetDpi(&mut dpi.0, &mut dpi.1);
        let compensation = dc.CreateEffect(&CLSID_D2D1DpiCompensation)?;
        compensation.SetInput(0, &bitmap.cast::<ID2D1Image>()?, true);
        let input_dpi = [dpi.0, dpi.1];
        compensation.SetValue(
            D2D1_DPICOMPENSATION_PROP_INPUT_DPI.0 as u32,
            D2D1_PROPERTY_TYPE_VECTOR2,
            std::slice::from_raw_parts(input_dpi.as_ptr().cast(), 8),
        )?;
        compensation.GetOutput()
    }
}

pub fn rect(left: f32, top: f32, width: f32, height: f32) -> D2D_RECT_F {
    D2D_RECT_F { left, top, right: left + width, bottom: top + height }
}

#[cfg(test)]
mod tests {
    use super::room;

    /// Drawn off screen at 100%, `size` pixels square: each pixel's alpha, rows top to bottom.
    #[cfg(feature = "studio")]
    fn alphas(size: u32, paint: impl FnOnce(&super::Frame)) -> Vec<Vec<u8>> {
        let gfx = super::Gfx::new().expect("graphics");
        let pixels = gfx.draw_offscreen((size, size), 1.0, paint).expect("drawn");
        pixels.chunks(size as usize * 4).map(|row| row.chunks(4).map(|pixel| pixel[3]).collect()).collect()
    }

    /// A hairline round a box covers one whole pixel at each edge, inside the box, whether it is given at the box's
    /// edge or half a pixel in (as the menus give theirs); not two pixels at half strength, nor a pixel further in.
    #[test]
    #[cfg(feature = "studio")]
    fn a_hairline_covers_whole_pixels() {
        use super::super::canvas::{Canvas, Color, Fill};
        for (x, side) in [(2.0, 10.0), (2.5, 9.0)] {
            let seen = alphas(16, |frame| frame.stroke_rounded(Fill::Solid(Color::hex(0xffffff, 1.0)), x, x, side, side, 0.0, 1.0));
            assert_eq!(seen[6][2], 255, "given at {x}: its left edge's pixel");
            assert_eq!(seen[6][1], 0, "given at {x}: the pixel outside it");
            assert_eq!(seen[6][3], 0, "given at {x}: the pixel inside it");
            assert_eq!(seen[11][6], 255, "given at {x}: its bottom edge's pixel");
            assert_eq!(seen[12][6], 0, "given at {x}: the pixel below it");
        }
    }

    /// What is put on a whole pixel comes to the same pixel of its surface wherever in the compositor's atlas the
    /// surface's region is (its place there, in DIPs, rarely exact: 355 px at 1.5 is 236.666…): a place half way
    /// between two pixels went to one or the other as the frames went, and the overlay's words shook by a pixel.
    #[test]
    #[cfg(feature = "studio")]
    fn whole_pixels_do_not_follow_the_atlas() {
        let gfx = super::Gfx::new().expect("graphics");
        for scale in [1.25f32, 1.5, 1.75] {
            gfx.draw_offscreen((4, 4), scale, |frame| {
                // Places half way between two pixels of the surface, and others.
                for local in [1.0 / (2.0 * scale), 7.0 / (2.0 * scale), 0.3, 13.37] {
                    let landed: std::collections::BTreeSet<i64> = (0..800)
                        .map(|offset| {
                            let base = offset as f32 / scale;
                            frame.place(windows_numerics::Matrix3x2::translation(base, base));
                            let (x, _) = frame.on_pixel(local, local);
                            // (Which pixel of the surface's own region, in 64ths: the atlas's offset taken off.)
                            (((x + base) * scale - offset as f32) * 64.0).round() as i64
                        })
                        .collect();
                    assert_eq!(landed.len(), 1, "at {scale}, {local} DIPs went to {landed:?} (64ths of a pixel)");
                }
            })
            .expect("drawn");
        }
    }

    /// A fill clipped to a box half a pixel off the grid covers what the same fill unclipped does at that edge (both
    /// put on whole pixels): nothing of what is under shows along it.
    #[test]
    #[cfg(feature = "studio")]
    fn a_clipped_fill_meets_its_clip() {
        use super::super::canvas::{Canvas, Color};
        let seen = alphas(16, |frame| {
            frame.clip(2.0, 2.0, 10.0, 7.5);
            frame.fill(Color::hex(0xffffff, 1.0), 2.0, 2.0, 10.0, 7.5);
            frame.unclip();
        });
        assert_eq!(seen[9][5], 255, "the fill's last row, inside the clip");
        assert_eq!(seen[10][5], 0, "past both");
    }

    #[test]
    fn room_is_made_anew_a_few_times_as_a_window_is_sized() {
        // Grown pixel by pixel from 300 to 2000 wide, then back: made anew a few times each way, not at each size.
        let sizing = super::sizing(true);
        let mut have = None;
        let mut made = 0;
        for width in (300..=2000).chain((300..2000).rev()) {
            if let Some(new) = room(have, (width, 400)) {
                have = Some(new);
                made += 1;
            }
            let (w, h) = have.unwrap();
            assert!(w >= width && h >= 400, "it holds what is asked");
            assert!(w <= 2 * width + 128, "it is never much larger than asked");
        }
        assert!(made <= 16, "made {made} times");
        // At rest: as large as asked, near enough.
        drop(sizing);
        let (w, h) = room(have, (1000, 400)).expect("made anew at rest");
        assert!(w >= 1000 && w < 1000 + 64 && h >= 400 && h < 400 + 64);
        assert_eq!(room(Some((w, h)), (1000, 400)), None);
    }
}
