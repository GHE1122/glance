//! What is behind one of Glance's windows, as it is now, without Glance
//! hiding from captures: the windows behind it (the desktop's own among
//! them) are put together again, live, in a window of Glance's own that no
//! one sees, out of the system compositor's thumbnails of them, in their
//! order; and that window is captured (Windows.Graphics.Capture), a frame
//! each time something in it changes, none while nothing does.
//!
//! The window is cloaked (no one sees it, it is in no list of windows), and
//! owned, with no surface of its own and no other extended style: the capture refuses tool windows and
//! windows that are not to be activated. The thumbnails show what the
//! windows show, those of elevated programs too, and live (a video plays
//! on in them); not the shadows the system draws around windows.
//!
//! A frame is taken only when one is wanted, on a thread other than the one
//! that draws: as it comes, or at once if one has come meanwhile.
//!
//! While a frame is met as it comes, its pool is held: on Windows 10, closing
//! a frame of it or making it anew waits till the frame has been met (on 11
//! neither does). So a frame is met quickly, and read on a thread of its own;
//! and no frame is closed, nor the pool made anew, while a lock that meeting
//! a frame takes is held (the thread that draws held one and closed a frame,
//! a frame met meanwhile waited on that lock, and the panel and the widgets
//! hung for good). It stays
//! on the graphics card, copied out of the capture's own (which it takes
//! back) into a texture that is drawn from as it is; only how light it is,
//! a value for each square of two by two pixels (the card averages them),
//! is read back, for the theme and the frost. The frames are made on the
//! device Glance draws with (one device: each costs the graphics driver's
//! memory), whose use from several threads is guarded.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex};

use windows::core::{w, Interface, BOOL};
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::{HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct3D11::{
    ID3D11Device, ID3D11DeviceContext, ID3D11ShaderResourceView, ID3D11Texture2D, D3D11_BIND_RENDER_TARGET, D3D11_BIND_SHADER_RESOURCE, D3D11_BOX,
    D3D11_CPU_ACCESS_READ, D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_FLAG_DO_NOT_WAIT, D3D11_MAP_READ, D3D11_RESOURCE_MISC_GENERATE_MIPS, D3D11_TEXTURE2D_DESC,
    D3D11_USAGE_DEFAULT, D3D11_USAGE_STAGING,
};
use windows::Win32::Graphics::Dxgi::Common::{DXGI_FORMAT_B8G8R8A8_UNORM, DXGI_SAMPLE_DESC};
use windows::Win32::Graphics::Dxgi::DXGI_ERROR_WAS_STILL_DRAWING;
use windows::Win32::Graphics::Dwm::{
    DwmGetWindowAttribute, DwmRegisterThumbnail, DwmSetWindowAttribute, DwmUnregisterThumbnail, DwmUpdateThumbnailProperties, DWMWA_CLOAK, DWMWA_CLOAKED,
    DWM_THUMBNAIL_PROPERTIES, DWM_TNP_OPACITY, DWM_TNP_RECTDESTINATION, DWM_TNP_SOURCECLIENTAREAONLY, DWM_TNP_VISIBLE,
};
use windows::Win32::Graphics::Dxgi::IDXGIDevice;
use windows::Win32::System::LibraryLoader::GetModuleHandleW;
use windows::Win32::System::Performance::{QueryPerformanceCounter, QueryPerformanceFrequency};
use windows::Win32::System::WinRT::Direct3D11::{CreateDirect3D11DeviceFromDXGIDevice, IDirect3DDxgiInterfaceAccess};
use windows::Win32::System::WinRT::Graphics::Capture::IGraphicsCaptureItemInterop;
use windows::Win32::UI::WindowsAndMessaging::{
    CreateWindowExW, DefWindowProcW, DestroyWindow, EnumWindows, GetShellWindow, GetWindowRect, IsIconic, IsWindowVisible, RegisterClassW, SetWindowPos, ShowWindow, SWP_NOACTIVATE,
    SWP_NOSIZE, SWP_NOZORDER, SW_SHOWNOACTIVATE, WINDOW_EX_STYLE, WNDCLASSW, WS_EX_NOREDIRECTIONBITMAP, WS_POPUP,
};

use super::backdrop::{Capture, Spare, Texture};

/// The device a scene's frames are made and read back on: the one Glance draws with (see `gfx`), its use from
/// several threads guarded by the device itself.
struct Device {
    device: ID3D11Device,
    context: ID3D11DeviceContext,
    direct3d: IDirect3DDevice,
}

// (A Direct3D 11 device may be used from any thread; its context is guarded: see `gfx::Gfx::new`.)
unsafe impl Send for Device {}
unsafe impl Sync for Device {}

impl Device {
    /// `gfx`'s, if its context may be used from the threads frames are read on.
    fn of(gfx: &super::gfx::Gfx) -> Option<Device> {
        if !gfx.guarded {
            return None;
        }
        let device = gfx.d3d().ok()?;
        let context = unsafe { device.GetImmediateContext() }.ok()?;
        let dxgi: IDXGIDevice = device.cast().ok()?;
        let direct3d: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }.ok()?.cast().ok()?;
        Some(Device { device, context, direct3d })
    }
}

/// What a scene's frames meet as they come, on whatever thread they come on.
struct Shared {
    /// What its frames are of: the area, when it was last arranged anew (the system's clock of frames, in 100 ns:
    /// a frame from before is of what was there before), and where in it nothing may be clear once what is behind
    /// is put in (see `keep`).
    of: Mutex<(RECT, i64, Vec<RECT>)>,
    /// The latest frame come and not read (none was wanted as it came).
    latest: Mutex<Option<Direct3D11CaptureFrame>>,
    /// A frame is wanted: read as it comes.
    wanted: AtomicBool,
    /// It rests (see `Scene::rest`): what comes meanwhile is of nothing.
    resting: AtomicBool,
    /// What was read, to be taken.
    ready: Mutex<Option<Capture>>,
    /// Tells the thread that draws (see `Scene::new`).
    wake: Arc<dyn Fn() + Send + Sync>,
    device: Device,
    /// What a frame's lightness is made and read back through, kept for the next of the same size.
    halves: Mutex<Option<Halves>>,
    /// Textures frames were kept in, let go of: written again for the next (see `backdrop::Texture`).
    spare: Arc<Spare>,
    /// How many frames have been taken: what tells one from another.
    taken: std::sync::atomic::AtomicU64,
    /// How many frames are being read now (on whatever thread).
    reading: std::sync::atomic::AtomicU32,
}

/// For a frame `size`: a texture it is copied into whose second level the card makes of it (each texel the
/// average of a square of two by two), that level's view, and a texture the processor reads the level from.
struct Halves {
    size: (u32, u32),
    levels: ID3D11Texture2D,
    view: ID3D11ShaderResourceView,
    readable: ID3D11Texture2D,
}

// (Used under its lock only.)
unsafe impl Send for Halves {}

impl Shared {
    /// Reads `frame` back (if it is of the area as it is now), for it to be taken; whether it was.
    fn read(&self, frame: &Direct3D11CaptureFrame) -> bool {
        self.reading.fetch_add(1, Ordering::AcqRel);
        let read = self.read_counted(frame);
        self.reading.fetch_sub(1, Ordering::AcqRel);
        read
    }

    fn read_counted(&self, frame: &Direct3D11CaptureFrame) -> bool {
        let (area, arranged, covered) = self.of.lock().unwrap().clone();
        let read = (|| {
            if frame.SystemRelativeTime().ok()?.Duration < arranged {
                crate::widget::note_sparingly("scene-early", || "scene: a frame from before it was arranged passed over".into());
                return None;
            }
            let size = frame.ContentSize().ok()?;
            let (width, height) = (area.right - area.left, area.bottom - area.top);
            if size.Width < width || size.Height < height {
                crate::widget::note_sparingly("scene-small", || format!("scene: a frame of {}x{} passed over, {width}x{height} wanted", size.Width, size.Height));
                return None;
            }
            let texture: ID3D11Texture2D = unsafe { frame.Surface().ok()?.cast::<IDirect3DDxgiInterfaceAccess>().ok()?.GetInterface() }.ok()?;
            // (Its texture as large too: a pool not made larger as its window was keeps smaller ones, and what lies
            // past a texture is not copied.)
            let mut made = D3D11_TEXTURE2D_DESC::default();
            unsafe { texture.GetDesc(&mut made) };
            if made.Width < width as u32 || made.Height < height as u32 {
                crate::widget::note_sparingly("scene-texture", || format!("scene: a texture of {}x{} passed over, {width}x{height} wanted", made.Width, made.Height));
                return None;
            }
            let (kept, luma) = keep(&self.device, &mut self.halves.lock().unwrap(), &self.spare, &texture, width as u32, height as u32, area, &covered)?;
            let taken = self.taken.fetch_add(1, Ordering::Relaxed) + 1;
            Some(Capture::from_texture(area, Texture { texture: kept, spare: self.spare.clone() }, luma, taken))
        })();
        let Some(capture) = read else { return false };
        // Arranged anew (or rested) while it was read: of what was there before, not taken. (Looked at and handed on
        // under one lock: what arranges or rests it marks it so before it clears what was handed on, so that one
        // handed on meanwhile is cleared too, never left behind.)
        let of = self.of.lock().unwrap();
        if of.1 != arranged {
            return false;
        }
        *self.ready.lock().unwrap() = Some(capture);
        drop(of);
        (self.wake)();
        true
    }
}

/// The top left `width` × `height` of `texture` (a frame of the capture's, taken back by it), kept in a texture of
/// its own to be drawn from (one let go of before, of that size, from `spare`; else a new one); and how light it is,
/// a value (0–255) for each square of two by two pixels, as `Capture::from_pixels` makes it (the card averaging each
/// square, through `halves`: made, or made anew, for that size).
///
/// None where the frame was put together before the windows behind were (the compositor puts them in a frame or
/// so after it is asked, and says nothing of it, and a frame from just before holds the scene's own window: nothing,
/// clear). Told by `opaque` (screen coordinates; the frame's top left at `area`'s): where the desktop's own window is
/// put and a screen is, which is wholly clear in such a frame and wholly opaque once it is put in (whatever the
/// windows over it are; a live wallpaper is drawn in it too). Wholly clear there, it is passed over: what the card
/// made of each square of two by two pixels is clear only where all four are, wherever its squares fall.
#[allow(clippy::too_many_arguments)]
fn keep(device: &Device, halves: &mut Option<Halves>, spare: &Arc<Spare>, texture: &ID3D11Texture2D, width: u32, height: u32, area: RECT, opaque: &[RECT]) -> Option<(ID3D11Texture2D, Vec<u8>)> {
    unsafe {
        let make = |description: &D3D11_TEXTURE2D_DESC| -> Option<ID3D11Texture2D> {
            let mut made = None;
            device.device.CreateTexture2D(description, None, Some(&mut made)).ok()?;
            made
        };
        let plain = D3D11_TEXTURE2D_DESC {
            Width: width,
            Height: height,
            MipLevels: 1,
            ArraySize: 1,
            Format: DXGI_FORMAT_B8G8R8A8_UNORM,
            SampleDesc: DXGI_SAMPLE_DESC { Count: 1, Quality: 0 },
            Usage: D3D11_USAGE_DEFAULT,
            BindFlags: D3D11_BIND_SHADER_RESOURCE.0 as u32,
            ..Default::default()
        };
        // (Those of another size are let go of: the scene's size has changed.)
        let reused = {
            let mut spare = spare.0.lock().unwrap();
            spare.retain(|texture| {
                let mut description = D3D11_TEXTURE2D_DESC::default();
                texture.GetDesc(&mut description);
                description.Width == width && description.Height == height
            });
            spare.pop()
        };
        let kept = match reused {
            Some(texture) => texture,
            None => {
                make(&plain)?
            }
        };
        if halves.as_ref().is_none_or(|halves| halves.size != (width, height)) {
            let levels = make(&D3D11_TEXTURE2D_DESC {
                MipLevels: 2,
                BindFlags: (D3D11_BIND_SHADER_RESOURCE.0 | D3D11_BIND_RENDER_TARGET.0) as u32,
                MiscFlags: D3D11_RESOURCE_MISC_GENERATE_MIPS.0 as u32,
                ..plain
            })?;
            let mut view = None;
            device.device.CreateShaderResourceView(&levels, None, Some(&mut view)).ok()?;
            let readable = make(&D3D11_TEXTURE2D_DESC {
                Width: (width / 2).max(1),
                Height: (height / 2).max(1),
                Usage: D3D11_USAGE_STAGING,
                BindFlags: 0,
                CPUAccessFlags: D3D11_CPU_ACCESS_READ.0 as u32,
                ..plain
            })?;
            *halves = Some(Halves { size: (width, height), levels, view: view?, readable });
        }
        let Halves { levels, view, readable, .. } = halves.as_ref()?;
        let region = D3D11_BOX { left: 0, top: 0, front: 0, right: width, bottom: height, back: 1 };
        let context = &device.context;
        context.CopySubresourceRegion(&kept, 0, 0, 0, 0, texture, 0, Some(&region));
        context.CopySubresourceRegion(levels, 0, 0, 0, 0, texture, 0, Some(&region));
        context.GenerateMips(view);
        // (The second level: subresource 1.)
        context.CopySubresourceRegion(readable, 0, 0, 0, 0, levels, 1, None);
        context.Flush();
        let staging = readable;
        let (row_width, rows) = ((width / 2).max(1) as usize, (height / 2).max(1) as usize);
        // Read once the copy is done, not waited for in the call (the device is the drawing's too, and held
        // meanwhile): tried again till it is, a second at most.
        let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
        let began = std::time::Instant::now();
        loop {
            match context.Map(staging, 0, D3D11_MAP_READ, D3D11_MAP_FLAG_DO_NOT_WAIT.0 as u32, Some(&mut mapped)) {
                Ok(()) => break,
                Err(error) if error.code() == DXGI_ERROR_WAS_STILL_DRAWING && began.elapsed() < std::time::Duration::from_secs(1) => {
                    std::thread::sleep(std::time::Duration::from_millis(1))
                }
                Err(_) => return None,
            }
        }
        // As `Capture::from_pixels` reads it: a square for each two pixels each way, rounded up.
        let (columns, down) = ((width as usize).div_ceil(2), (height as usize).div_ceil(2));
        let mut luma = Vec::with_capacity(columns * down);
        // (Whether any square there was looked at, and whether any held anything.)
        let (mut looked, mut held) = (false, false);
        for y in 0..down {
            let line = std::slice::from_raw_parts((mapped.pData as *const u8).add(y.min(rows - 1) * mapped.RowPitch as usize), row_width * 4);
            // (Each square's middle, on the screen.)
            let screen_y = area.top + (2 * y.min(rows - 1)) as i32 + 1;
            for x in 0..columns {
                let i = x.min(row_width - 1) * 4;
                let lightness = (0.2126 * line[i + 2] as f64 + 0.7152 * line[i + 1] as f64 + 0.0722 * line[i] as f64) / 255.0;
                luma.push((lightness * 255.0).round() as u8);
                if !held {
                    let screen_x = area.left + (2 * x.min(row_width - 1)) as i32;
                    if opaque.iter().any(|rect| screen_x >= rect.left && screen_x + 2 <= rect.right && screen_y - 1 >= rect.top && screen_y + 1 <= rect.bottom) {
                        looked = true;
                        held = line[i + 3] > 0;
                    }
                }
            }
        }
        context.Unmap(staging, 0);
        if looked && !held {
            crate::widget::note_sparingly("scene-clear", || format!("scene: a frame of {area:?} with the desktop not yet in it passed over"));
            // (The texture it was copied into, spare again.)
            drop(Texture { texture: kept, spare: spare.clone() });
            return None;
        }
        Some((kept, luma))
    }
}

/// A capture of `item` (a scene's window) at `size`: its frame pool, made on the scene's device, each frame met as it
/// comes (see `Shared`), and its session, started.
fn capture(shared: &Arc<Shared>, item: &GraphicsCaptureItem, size: SizeInt32) -> Option<(Direct3D11CaptureFramePool, GraphicsCaptureSession, i64)> {
    let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&shared.device.direct3d, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size).ok()?;
    let arrived = pool.FrameArrived(&TypedEventHandler::new({
        let shared = shared.clone();
        move |pool: windows::core::Ref<Direct3D11CaptureFramePool>, _| {
            let Ok(pool) = pool.ok() else { return Ok(()) };
            let mut newest: Option<Direct3D11CaptureFrame> = None;
            while let Ok(frame) = pool.TryGetNextFrame() {
                // (Those passed over closed.)
                close(newest.replace(frame));
            }
            let Some(frame) = newest else { return Ok(()) };
            if shared.resting.load(Ordering::Acquire) {
                return Ok(());
            }
            // Wanted, read now, on a thread of its own (see the top); else kept, the thread that draws told once.
            if shared.wanted.swap(false, Ordering::AcqRel) {
                read_apart(&shared, frame);
            } else {
                let older = shared.latest.lock().unwrap().replace(frame);
                if older.is_none() {
                    (shared.wake)();
                }
                close(older);
            }
            Ok(())
        }
    }))
    .ok()?;
    let session = pool.CreateCaptureSession(item).ok()?;
    // (Not on every system: a border no one sees is drawn then.)
    let _ = session.SetIsBorderRequired(false);
    let _ = session.SetIsCursorCaptureEnabled(false);
    session.StartCapture().ok()?;
    Some((pool, session, arrived))
}

/// A window of Glance's own that no one sees, the windows behind an area put together in it, and its capture.
pub struct Scene {
    owner: HWND,
    window: HWND,
    /// The area it stands for (physical screen coordinates).
    area: RECT,
    /// The windows shown in it, bottom first: each with its thumbnail and where it was.
    shown: Vec<(HWND, isize, RECT)>,
    pool: Direct3D11CaptureFramePool,
    session: GraphicsCaptureSession,
    /// Its handler of frames as they come, as the pool has it.
    arrived: i64,
    /// The size the frames are made at.
    size: SizeInt32,
    shared: Arc<Shared>,
}

/// `frame` read on a thread of its own, then closed (one of what was there before is not read, and the next is
/// wanted still).
fn read_apart(shared: &Arc<Shared>, frame: Direct3D11CaptureFrame) {
    let shared = shared.clone();
    std::thread::spawn(move || {
        if !shared.read(&frame) {
            shared.wanted.store(true, Ordering::Release);
        }
        close(Some(frame));
    });
}

/// A frame done with, closed: its buffer back in its pool at once, not once the last reference to it goes. Never
/// with a lock held that meeting a frame takes (see the top).
fn close(frame: Option<Direct3D11CaptureFrame>) {
    if let Some(frame) = frame {
        let _ = frame.Close();
    }
}

/// The system's clock of frames, now (100 ns).
fn now_100ns() -> i64 {
    let (mut count, mut frequency) = (0i64, 0i64);
    unsafe {
        let _ = QueryPerformanceCounter(&mut count);
        let _ = QueryPerformanceFrequency(&mut frequency);
    }
    (count as i128 * 10_000_000 / frequency.max(1) as i128) as i64
}

unsafe extern "system" fn procedure(window: HWND, message: u32, wparam: WPARAM, lparam: LPARAM) -> LRESULT {
    unsafe { DefWindowProcW(window, message, wparam, lparam) }
}

unsafe extern "system" fn each(window: HWND, found: LPARAM) -> BOOL {
    unsafe { (*(found.0 as *mut Vec<HWND>)).push(window) };
    true.into()
}

fn cloaked(window: HWND) -> bool {
    let mut cloaked = 0u32;
    unsafe { DwmGetWindowAttribute(window, DWMWA_CLOAKED, &mut cloaked as *mut u32 as *mut _, 4) }.is_ok() && cloaked != 0
}

/// Whether the system can capture a window (Windows.Graphics.Capture: Windows 10 1903 and later).
pub fn supported() -> bool {
    static SUPPORTED: std::sync::OnceLock<bool> = std::sync::OnceLock::new();
    *SUPPORTED.get_or_init(|| GraphicsCaptureSession::IsSupported().unwrap_or(false))
}

/// Glance's windows left out of every scene, while they are: what is not part of what is behind (the panel as a
/// widget torn off it takes its place, the hint of where a widget would stick).
static LEFT_OUT: Mutex<Vec<isize>> = Mutex::new(Vec::new());

/// Leaves `window` out of every scene from now on (`out`), or no longer.
pub fn leave_out(window: HWND, out: bool) {
    let mut left_out = LEFT_OUT.lock().unwrap();
    left_out.retain(|kept| *kept != window.0 as isize);
    if out {
        left_out.push(window.0 as isize);
    }
}

fn meet(a: RECT, b: RECT) -> bool {
    a.left < b.right && b.left < a.right && a.top < b.bottom && b.top < a.bottom
}

/// Where `a` and `b` meet, if they do.
fn meeting(a: RECT, b: RECT) -> Option<RECT> {
    meet(a, b).then(|| RECT { left: a.left.max(b.left), top: a.top.max(b.top), right: a.right.min(b.right), bottom: a.bottom.min(b.bottom) })
}

impl Scene {
    /// A scene standing for `area`, its frames made on `gfx`'s device, empty till it is arranged; `wake` is called
    /// (on another thread) as a frame wanted has been read (see `ready`), and as one comes unwanted after none had
    /// (see `fresh`).
    pub fn new(area: RECT, wake: Arc<dyn Fn() + Send + Sync>, gfx: &super::gfx::Gfx) -> Option<Scene> {
        let device = Device::of(gfx)?;
        unsafe {
            let module = GetModuleHandleW(None).ok()?;
            let class = WNDCLASSW { lpfnWndProc: Some(procedure), hInstance: module.into(), lpszClassName: w!("GlanceScene"), ..Default::default() };
            RegisterClassW(&class);
            let (width, height) = ((area.right - area.left).max(1), (area.bottom - area.top).max(1));
            let owner = CreateWindowExW(WINDOW_EX_STYLE(0), class.lpszClassName, w!(""), WS_POPUP, 0, 0, 0, 0, None, None, Some(module.into()), None).ok()?;
            // With no surface of its own (nothing draws it): where no window behind is put, it holds nothing (clear),
            // never what its surface would hold, which no one wrote (what was in that memory before, on some cards).
            let window = match CreateWindowExW(WS_EX_NOREDIRECTIONBITMAP, class.lpszClassName, w!("Glance scene"), WS_POPUP, area.left, area.top, width, height, Some(owner), None, Some(module.into()), None) {
                Ok(window) => window,
                Err(_) => {
                    let _ = DestroyWindow(owner);
                    return None;
                }
            };
            let shared = Arc::new(Shared { of: Mutex::new((area, now_100ns(), Vec::new())), latest: Mutex::new(None), wanted: AtomicBool::new(false), ready: Mutex::new(None), wake, device, halves: Mutex::new(None), spare: Arc::default(), resting: AtomicBool::new(false), taken: std::sync::atomic::AtomicU64::new(0), reading: std::sync::atomic::AtomicU32::new(0) });
            let made = (|| {
                let cloak: BOOL = true.into();
                DwmSetWindowAttribute(window, DWMWA_CLOAK, &cloak as *const BOOL as *const _, 4).ok()?;
                let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
                let item: GraphicsCaptureItem = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().ok()?.CreateForWindow(window).ok()?;
                let size = item.Size().ok()?;
                let (pool, session, arrived) = capture(&shared, &item, size)?;
                Some((pool, session, arrived, size))
            })();
            let Some((pool, session, arrived, size)) = made else {
                let _ = DestroyWindow(window);
                let _ = DestroyWindow(owner);
                return None;
            };
            Some(Scene { owner, window, area, shown: Vec::new(), pool, session, arrived, size, shared })
        }
    }

    /// Puts together in it, for `area`, the windows behind `over` (one of Glance's: those under it, the desktop's
    /// own among them) that meet the area, as they are now; what has not changed is left as it is. Whether anything
    /// had (then a frame of it as it is now comes). What it stands for, and its frames, may reach past `area` (see
    /// `gfx::room`): a capture says where it was taken.
    pub fn arrange(&mut self, area: RECT, over: HWND) -> bool {
        // As large as it is while that serves, else with room to spare (see `gfx::room`): a window sized pixel by
        // pixel makes its frames, and what they are read through and kept in, anew a few times, not at each size.
        let want = ((area.right - area.left).max(1) as u32, (area.bottom - area.top).max(1) as u32);
        let have = (self.size.Width as u32, self.size.Height as u32);
        let (width, height) = super::gfx::room(Some(have), want).unwrap_or(have);
        // The room to spare on the desktop (what reaches past it is of no screen): on whichever side there is room,
        // all of what was asked held. Against the desktop's edge (a panel, a widget stuck to it), it spares room
        // towards the middle. Not on one screen alone: what is behind a window across two is behind it on both.
        let screen = Some(crate::mouse::desktop());
        let span = |from: i32, to: i32, length: i32, low: Option<i32>, high: Option<i32>| {
            // As far as it goes from `from`, moved back from the screen's far side, never past where it begins.
            let start = high.map_or(from, |high| from.min(high - length));
            let start = low.map_or(start, |low| start.max(low)).min(from);
            // (Larger than the screen: as far as the screen goes.)
            let end = high.map_or(start + length, |high| (start + length).min(high.max(to)));
            (start, end.max(to))
        };
        let (left, right) = span(area.left, area.right, width as i32, screen.map(|s| s.left), screen.map(|s| s.right));
        let (top, bottom) = span(area.top, area.bottom, height as i32, screen.map(|s| s.top), screen.map(|s| s.bottom));
        let area = RECT { left, top, right, bottom };
        let mut all: Vec<HWND> = Vec::new();
        unsafe {
            let _ = EnumWindows(Some(each), LPARAM(&mut all as *mut _ as isize));
        }
        // Top first: those after `over`; then bottom first.
        let left_out = LEFT_OUT.lock().unwrap().clone();
        let mut behind: Vec<(HWND, RECT)> = all
            .into_iter()
            .skip_while(|window| *window != over)
            .skip(1)
            .filter(|window| !left_out.contains(&(window.0 as isize)))
            .filter(|window| unsafe { IsWindowVisible(*window).as_bool() && !IsIconic(*window).as_bool() } && !cloaked(*window))
            .filter_map(|window| {
                let mut rect = RECT::default();
                unsafe { GetWindowRect(window, &mut rect) }.ok()?;
                meet(rect, area).then_some((window, rect))
            })
            .collect();
        behind.reverse();
        let same_windows = behind.len() == self.shown.len() && behind.iter().zip(&self.shown).all(|((a, _), (b, ..))| a == b);
        if same_windows && area == self.area && behind.iter().zip(&self.shown).all(|((_, a), (.., b))| a == b) {
            return false;
        }
        // Where the desktop's own window will be put, on a screen (see `keep`): worked out before anything is put in
        // place, so that nothing comes between the last of that and when frames count from. (Between screens it
        // holds nothing. Not behind, as while Explorer is not running: nothing then tells such a frame.)
        let shell = unsafe { GetShellWindow() };
        let opaque: Vec<RECT> = behind.iter().find(|(window, _)| *window == shell).map_or_else(Vec::new, |(_, desktop)| {
            crate::screens::monitors().into_iter().filter_map(|monitor| meeting(meeting(*desktop, monitor)?, area)).collect()
        });
        let (width, height) = ((area.right - area.left).max(1), (area.bottom - area.top).max(1));
        if area != self.area {
            // (Only moved, where its size is as it was: sizing a window costs more.)
            let same_size = (area.right - area.left, area.bottom - area.top) == (self.area.right - self.area.left, self.area.bottom - self.area.top);
            let flags = SWP_NOACTIVATE | SWP_NOZORDER | if same_size { SWP_NOSIZE } else { Default::default() };
            unsafe {
                let _ = SetWindowPos(self.window, None, area.left, area.top, width, height, flags);
            }
            self.area = area;
        }
        let screen_size = screen.map(|s| SizeInt32 { Width: s.right - s.left, Height: s.bottom - s.top });
        // (Not made larger, the frames that come are too small, and are not taken: see `Shared::read`.)
        let _ = self.fit(SizeInt32 { Width: width, Height: height }, screen_size);
        // The thumbnails are drawn in the order they were made in: another order, or other windows, and they are
        // made anew, bottom first; the same windows, only moved, are moved.
        if !same_windows {
            self.forget();
            for (window, rect) in behind {
                if let Ok(thumbnail) = unsafe { DwmRegisterThumbnail(self.window, window) } {
                    self.shown.push((window, thumbnail, RECT::default()));
                    self.place(self.shown.len() - 1, rect);
                }
            }
        } else {
            for (index, (_, rect)) in behind.into_iter().enumerate() {
                self.place(index, rect);
            }
        }
        // Frames from before all of it was in place are not of what is there now (one may be put together while it
        // is being put in place); nor what was read of it before and not taken yet. Cleared and made so under one
        // lock, as what reads a frame looks and hands on under it: one read meanwhile is of before, or kept.
        let from = now_100ns();
        let stale = {
            let mut of = self.shared.of.lock().unwrap();
            *self.shared.ready.lock().unwrap() = None;
            let stale = self.shared.latest.lock().unwrap().take();
            *of = (area, from, opaque);
            self.shared.resting.store(false, Ordering::Release);
            stale
        };
        close(stale);
        true
    }

    /// Holds nothing till it is arranged again: no thumbnails (the compositor puts nothing together in it), its window
    /// a pixel, and what its frames were read through given back (its pool of frames kept: see `fit`). For whoever
    /// wants what is behind only as it was when last looked at, not as it changes, or is hidden (see `life`);
    /// arranged again, it is as it was, and as quickly.
    pub fn rest(&mut self) {
        let least = RECT { left: self.area.left, top: self.area.top, right: self.area.left + 1, bottom: self.area.top + 1 };
        // (A frame being read meanwhile is of what was there: marked so first, see `Shared::read`.)
        *self.shared.of.lock().unwrap() = (least, now_100ns(), Vec::new());
        self.shared.resting.store(true, Ordering::Release);
        self.forget();
        self.shared.wanted.store(false, Ordering::Release);
        let stale = self.shared.latest.lock().unwrap().take();
        close(stale);
        *self.shared.ready.lock().unwrap() = None;
        // (Not while a frame is read through them on another thread, which can wait a while on the card: the
        // thread that draws never waits on it. Let go of at the next rest, or with the scene.)
        if let Ok(mut halves) = self.shared.halves.try_lock() {
            *halves = None;
        }
        self.shared.spare.0.lock().unwrap().clear();
        unsafe {
            let _ = SetWindowPos(self.window, None, least.left, least.top, 1, 1, SWP_NOACTIVATE | SWP_NOZORDER);
        }
        self.area = least;
        // (Its pool of frames stays as it is, see `fit`: made smaller, and larger again as it is arranged, would be
        // two more the system keeps.)
    }

    /// The thumbnail `index` shown where its window is, `rect`, within the area.
    fn place(&mut self, index: usize, rect: RECT) {
        let (_, thumbnail, at) = &mut self.shown[index];
        let (x, y) = (self.area.left, self.area.top);
        let properties = DWM_THUMBNAIL_PROPERTIES {
            dwFlags: DWM_TNP_RECTDESTINATION | DWM_TNP_VISIBLE | DWM_TNP_OPACITY | DWM_TNP_SOURCECLIENTAREAONLY,
            rcDestination: RECT { left: rect.left - x, top: rect.top - y, right: rect.right - x, bottom: rect.bottom - y },
            fVisible: true.into(),
            opacity: 255,
            fSourceClientAreaOnly: false.into(),
            ..Default::default()
        };
        let _ = unsafe { DwmUpdateThumbnailProperties(*thumbnail, &properties) };
        *at = rect;
    }

    fn forget(&mut self) {
        for (_, thumbnail, _) in self.shown.drain(..) {
            let _ = unsafe { DwmUnregisterThumbnail(thumbnail) };
        }
    }

    /// Its pool's handler taken away, and what it holds of its frames let go of: the handler holds `shared`, which
    /// may hold a frame of the pool's (which holds the pool).
    fn unhook(&mut self) {
        let _ = self.pool.RemoveFrameArrived(self.arrived);
        let stale = self.shared.latest.lock().unwrap().take();
        close(stale);
        *self.shared.ready.lock().unwrap() = None;
        // (Not while a frame is read through them on another thread, which can wait a while on the card: the
        // thread that draws never waits on it. Let go of at the next rest, or with the scene.)
        if let Ok(mut halves) = self.shared.halves.try_lock() {
            *halves = None;
        }
        self.shared.spare.0.lock().unwrap().clear();
    }

    /// Its frames made large enough for `size` from now on, on a desktop of `screen`'s size (none: not known);
    /// whether they are. Each pool of frames made is memory the system keeps for good (till Glance ends, whatever is
    /// let go of: measured, a frame's worth each time): so it is made only larger, never smaller, and a quarter
    /// larger than asked, as large as the desktop at most (what a scene stands for is on it); for a window moved
    /// and sized about, it is made a few times at most, then serves.
    fn fit(&mut self, size: SizeInt32, screen: Option<SizeInt32>) -> bool {
        if size.Width <= self.size.Width && size.Height <= self.size.Height {
            return true;
        }
        let grown = |want: i32, have: i32, most: Option<i32>| {
            let roomy = want + want / 4;
            have.max(most.map_or(roomy, |most| roomy.min(most.max(want))))
        };
        let made = SizeInt32 {
            Width: grown(size.Width, self.size.Width, screen.map(|screen| screen.Width)),
            Height: grown(size.Height, self.size.Height, screen.map(|screen| screen.Height)),
        };
        if self.pool.Recreate(&self.shared.device.direct3d, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, made).is_err() {
            return false;
        }
        self.size = made;
        true
    }

    /// Whether a frame has come that was not wanted as it came (and is not read yet).
    /// Whether a frame is being read now (to be told as it is: see `ready`).
    pub fn reading(&self) -> bool {
        self.shared.reading.load(Ordering::Acquire) > 0
    }

    pub fn fresh(&self) -> bool {
        self.shared.latest.lock().unwrap().is_some()
    }

    /// Wants a frame: one come already, read at once (on another thread); else the next, as it comes. Told as it
    /// has been read (see `ready`).
    pub fn want(&self) {
        if self.shared.ready.lock().unwrap().is_some() {
            return;
        }
        let come = self.shared.latest.lock().unwrap().take();
        match come {
            Some(frame) => read_apart(&self.shared, frame),
            None => self.shared.wanted.store(true, Ordering::Release),
        }
    }

    /// A frame wanted, read: what is behind as it showed (a capture of the area).
    pub fn ready(&self) -> Option<Capture> {
        self.shared.ready.lock().unwrap().take()
    }
}

impl Drop for Scene {
    fn drop(&mut self) {
        self.unhook();
        let _ = self.session.Close();
        let _ = self.pool.Close();
        self.forget();
        unsafe {
            let _ = DestroyWindow(self.window);
            let _ = DestroyWindow(self.owner);
        }
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// A scene arranged again and again at another place and size (as a widget moved and shown), one frame taken
    /// after each: how many of those were not yet of what is behind (dark, where the windows behind are light).
    /// `cargo test --lib -- --ignored scene_first_frames --nocapture`.
    #[test]
    #[ignore = "captures this PC's desktop"]
    fn scene_first_frames() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let gfx = super::super::gfx::Gfx::new().expect("graphics");
        let mut all: Vec<HWND> = Vec::new();
        unsafe {
            let _ = EnumWindows(Some(each), LPARAM(&mut all as *mut _ as isize));
        }
        let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
        let mut scene = Scene::new(RECT { left: 100, top: 100, right: 400, bottom: 300 }, wake, &gfx).expect("scene");
        let pump = || unsafe {
            use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE};
            let mut message = MSG::default();
            while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                let _ = TranslateMessage(&message);
                DispatchMessageW(&message);
            }
        };
        let mut means = Vec::new();
        let mut arranged = Vec::new();
        for step in 0..60 {
            let (x, grow) = (100 + (step % 7) * 53, (step % 5) * 61);
            let area = RECT { left: x, top: 100 + grow / 2, right: x + 300 + grow, bottom: 300 + grow };
            if step % 10 == 9 {
                scene.rest();
            }
            let arranging = std::time::Instant::now();
            scene.arrange(area, all[0]);
            arranged.push(arranging.elapsed().as_secs_f64() * 1000.0);
            let began = std::time::Instant::now();
            scene.want();
            let mut capture = None;
            while began.elapsed() < std::time::Duration::from_secs(2) && capture.is_none() {
                pump();
                capture = scene.ready();
                std::thread::sleep(std::time::Duration::from_millis(1));
            }
            let Some(capture) = capture else {
                means.push(-1.0);
                continue;
            };
            let seen = capture.luminances(area, RECT::default(), 7);
            means.push(seen.iter().sum::<f32>() / seen.len() as f32);
        }
        let dark = means.iter().filter(|mean| (0.0..0.02).contains(*mean)).count();
        arranged.sort_by(f64::total_cmp);
        println!("arranging took {:.2} ms at the median, {:.2} at most", arranged[arranged.len() / 2], arranged[arranged.len() - 1]);
        println!("first frames: {dark} of {} dark; none {}; {:?}", means.len(), means.iter().filter(|m| **m < 0.0).count(), means.iter().map(|m| (m * 100.0).round() as i32).collect::<Vec<_>>());
    }

    /// What a scene with nothing behind it holds (its window's own surface, which nothing draws), beside one with the
    /// desktop behind it: `cargo test --lib -- --ignored scene_shows --nocapture`.
    #[test]
    #[ignore = "captures this PC's desktop"]
    fn scene_shows() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let gfx = super::super::gfx::Gfx::new().expect("graphics");
        let area = RECT { left: 100, top: 100, right: 700, bottom: 500 };
        for (name, over) in [("bare", HWND(usize::MAX as *mut _)), ("desktop", HWND::default())] {
            let wake: Arc<dyn Fn() + Send + Sync> = Arc::new(|| {});
            let mut scene = Scene::new(area, wake, &gfx).expect("scene");
            // (Over nothing: the windows after the first are all behind it.)
            let over = if over.is_invalid() {
                let mut all: Vec<HWND> = Vec::new();
                unsafe {
                    let _ = EnumWindows(Some(each), LPARAM(&mut all as *mut _ as isize));
                }
                all[0]
            } else {
                over
            };
            let changed = scene.arrange(area, over);
            println!("{name}: arranged {changed}, {} thumbnails, area {:?}, frames {}x{}", scene.shown.len(), scene.area, scene.size.Width, scene.size.Height);
            for (window, _, rect) in &scene.shown {
                let mut title = [0u16; 80];
                let n = unsafe { windows::Win32::UI::WindowsAndMessaging::GetWindowTextW(*window, &mut title) };
                let mut class = [0u16; 80];
                let c = unsafe { windows::Win32::UI::WindowsAndMessaging::GetClassNameW(*window, &mut class) };
                println!("   behind: {:?} [{}] {:?}", String::from_utf16_lossy(&title[..n as usize]), String::from_utf16_lossy(&class[..c as usize]), rect);
            }
            let arranged = std::time::Instant::now();
            for taken in 0..3 {
                let began = std::time::Instant::now();
                scene.want();
                let mut capture = None;
                while began.elapsed() < std::time::Duration::from_secs(3) && capture.is_none() {
                    capture = scene.ready();
                    // (Its window's messages handled, as the thread that draws handles them.)
                    unsafe {
                        use windows::Win32::UI::WindowsAndMessaging::{DispatchMessageW, PeekMessageW, TranslateMessage, MSG, PM_REMOVE};
                        let mut message = MSG::default();
                        while PeekMessageW(&mut message, None, 0, 0, PM_REMOVE).as_bool() {
                            let _ = TranslateMessage(&message);
                            DispatchMessageW(&message);
                        }
                    }
                    std::thread::sleep(std::time::Duration::from_millis(2));
                }
                let Some(capture) = capture else {
                    println!("{name} {taken}: no frame in 3 s");
                    continue;
                };
                let seen = capture.luminances(capture.rect, RECT::default(), 9);
                let mean = seen.iter().sum::<f32>() / seen.len() as f32;
                let spread = (seen.iter().map(|l| (l - mean).powi(2)).sum::<f32>() / seen.len() as f32).sqrt();
                println!("{name} {taken}: at {:?} after arranging, mean {mean:.3}, spread {spread:.3}", arranged.elapsed());
            }
        }
    }
}
