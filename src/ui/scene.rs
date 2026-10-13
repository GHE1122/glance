//! What is behind one of Glance's windows, as it is now, without Glance
//! hiding from captures: the windows behind it (the desktop's own among
//! them) are put together again, live, in a window of Glance's own that no
//! one sees, out of the system compositor's thumbnails of them, in their
//! order; and that window is captured (Windows.Graphics.Capture), a frame
//! each time something in it changes, none while nothing does.
//!
//! The window is cloaked (no one sees it, it is in no list of windows), and
//! owned, with no extended style: the capture refuses tool windows and
//! windows that are not to be activated. The thumbnails show what the
//! windows show, those of elevated programs too, and live (a video plays
//! on in them); not the shadows the system draws around windows.
//!
//! A frame is read back (to the pixels, lightness and digest the skins and
//! themes use) only when one is wanted, on a thread other than the one that
//! draws: as it comes, or at once if one has come meanwhile.

use std::sync::atomic::{AtomicBool, Ordering};
use std::sync::{Arc, Mutex, OnceLock};

use windows::core::{w, Interface, BOOL};
use windows::Foundation::TypedEventHandler;
use windows::Graphics::Capture::{Direct3D11CaptureFrame, Direct3D11CaptureFramePool, GraphicsCaptureItem, GraphicsCaptureSession};
use windows::Graphics::DirectX::Direct3D11::IDirect3DDevice;
use windows::Graphics::DirectX::DirectXPixelFormat;
use windows::Graphics::SizeInt32;
use windows::Win32::Foundation::{HMODULE, HWND, LPARAM, LRESULT, RECT, WPARAM};
use windows::Win32::Graphics::Direct3D::{D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP};
use windows::Win32::Graphics::Direct3D11::{
    D3D11CreateDevice, ID3D11Device, ID3D11DeviceContext, ID3D11Texture2D, D3D11_BOX, D3D11_CPU_ACCESS_READ, D3D11_CREATE_DEVICE_BGRA_SUPPORT,
    D3D11_MAPPED_SUBRESOURCE, D3D11_MAP_READ, D3D11_SDK_VERSION, D3D11_TEXTURE2D_DESC, D3D11_USAGE_STAGING,
};
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
    CreateWindowExW, DefWindowProcW, DestroyWindow, EnumWindows, GetWindowRect, IsIconic, IsWindowVisible, RegisterClassW, SetWindowPos, ShowWindow, SWP_NOACTIVATE,
    SWP_NOZORDER, SW_SHOWNOACTIVATE, WINDOW_EX_STYLE, WNDCLASSW, WS_POPUP,
};

use super::backdrop::Capture;

/// The Direct3D device every scene's frames are made and read back on (one for all: making one takes a while),
/// its context used by one reading at a time.
struct Device {
    device: ID3D11Device,
    context: Mutex<ID3D11DeviceContext>,
    direct3d: IDirect3DDevice,
}

// (A Direct3D 11 device may be used from any thread; its context is behind the lock.)
unsafe impl Send for Device {}
unsafe impl Sync for Device {}

static DEVICE: OnceLock<Option<Device>> = OnceLock::new();

/// The scenes' device: the graphics card's, or Windows' own renderer where there is none.
fn device() -> Option<&'static Device> {
    DEVICE
        .get_or_init(|| {
            [D3D_DRIVER_TYPE_HARDWARE, D3D_DRIVER_TYPE_WARP].into_iter().find_map(|kind| {
                let (mut device, mut context) = (None, None);
                unsafe { D3D11CreateDevice(None, kind, HMODULE::default(), D3D11_CREATE_DEVICE_BGRA_SUPPORT, None, D3D11_SDK_VERSION, Some(&mut device), None, Some(&mut context)) }.ok()?;
                let (device, context): (ID3D11Device, ID3D11DeviceContext) = (device?, context?);
                let dxgi: IDXGIDevice = device.cast().ok()?;
                let direct3d: IDirect3DDevice = unsafe { CreateDirect3D11DeviceFromDXGIDevice(&dxgi) }.ok()?.cast().ok()?;
                Some(Device { device, context: Mutex::new(context), direct3d })
            })
        })
        .as_ref()
}

/// Makes the scenes' device on another thread now, for the first scene not to wait for it on the thread that
/// draws.
pub fn prepare() {
    std::thread::spawn(|| {
        device();
    });
}

/// What a scene's frames meet as they come, on whatever thread they come on.
struct Shared {
    /// What its frames are of: the area, and when it was last arranged anew (the system's clock of frames, in
    /// 100 ns): a frame from before is of what was there before.
    of: Mutex<(RECT, i64)>,
    /// The latest frame come and not read (none was wanted as it came).
    latest: Mutex<Option<Direct3D11CaptureFrame>>,
    /// A frame is wanted: read as it comes.
    wanted: AtomicBool,
    /// What was read, to be taken.
    ready: Mutex<Option<Capture>>,
    /// Tells the thread that draws (see `Scene::new`).
    wake: Arc<dyn Fn() + Send + Sync>,
    /// What frames are read back through, kept for the next of the same size.
    staging: Mutex<Option<Staging>>,
}

/// A texture the processor may read, and its size.
struct Staging(ID3D11Texture2D, u32, u32);

// (Used under its lock only.)
unsafe impl Send for Staging {}

impl Shared {
    /// Reads `frame` back (if it is of the area as it is now), for it to be taken; whether it was.
    fn read(&self, frame: &Direct3D11CaptureFrame) -> bool {
        let (area, arranged) = *self.of.lock().unwrap();
        let read = (|| {
            if frame.SystemRelativeTime().ok()?.Duration < arranged {
                return None;
            }
            let size = frame.ContentSize().ok()?;
            let (width, height) = (area.right - area.left, area.bottom - area.top);
            if size.Width < width || size.Height < height {
                return None;
            }
            let texture: ID3D11Texture2D = unsafe { frame.Surface().ok()?.cast::<IDirect3DDxgiInterfaceAccess>().ok()?.GetInterface() }.ok()?;
            let pixels = read_back(device()?, &mut self.staging.lock().unwrap(), &texture, width as u32, height as u32)?;
            {
                static DUMPED: std::sync::atomic::AtomicU32 = std::sync::atomic::AtomicU32::new(0);
                let n = DUMPED.fetch_add(1, Ordering::Relaxed);
                if n < 12 {
                    let (w, h) = (width as usize, height as usize);
                    let mut out = Vec::new();
                    out.extend_from_slice(b"BM");
                    out.extend_from_slice(&((54 + w * h * 4) as u32).to_le_bytes());
                    out.extend_from_slice(&0u32.to_le_bytes());
                    out.extend_from_slice(&54u32.to_le_bytes());
                    out.extend_from_slice(&40u32.to_le_bytes());
                    out.extend_from_slice(&(w as i32).to_le_bytes());
                    out.extend_from_slice(&(-(h as i32)).to_le_bytes());
                    out.extend_from_slice(&1u16.to_le_bytes());
                    out.extend_from_slice(&32u16.to_le_bytes());
                    out.extend_from_slice(&[0u8; 24]);
                    out.extend_from_slice(&pixels);
                    let _ = std::fs::write(std::env::temp_dir().join(format!("scene-{n}-{}x{}-at{}_{}.bmp", w, h, area.left, area.top)), out);
                }
            }
            Some(Capture::from_pixels(area, pixels))
        })();
        let Some(capture) = read else { return false };
        *self.ready.lock().unwrap() = Some(capture);
        (self.wake)();
        true
    }
}

/// The top left `width` × `height` of `texture`, read back through `staging` (made, or made anew, for that size):
/// BGRA, rows top to bottom, made opaque.
fn read_back(device: &Device, staging: &mut Option<Staging>, texture: &ID3D11Texture2D, width: u32, height: u32) -> Option<Vec<u8>> {
    unsafe {
        if staging.as_ref().is_none_or(|Staging(_, w, h)| (*w, *h) != (width, height)) {
            let mut description = D3D11_TEXTURE2D_DESC::default();
            texture.GetDesc(&mut description);
            description.Width = width;
            description.Height = height;
            description.Usage = D3D11_USAGE_STAGING;
            description.BindFlags = 0;
            description.CPUAccessFlags = D3D11_CPU_ACCESS_READ.0 as u32;
            description.MiscFlags = 0;
            let mut made = None;
            device.device.CreateTexture2D(&description, None, Some(&mut made)).ok()?;
            *staging = Some(Staging(made?, width, height));
        }
        let Staging(staging, ..) = staging.as_ref()?;
        let region = D3D11_BOX { left: 0, top: 0, front: 0, right: width, bottom: height, back: 1 };
        let row = width as usize * 4;
        let mut pixels = Vec::with_capacity(row * height as usize);
        {
            let context = device.context.lock().unwrap();
            context.CopySubresourceRegion(staging, 0, 0, 0, 0, texture, 0, Some(&region));
            let mut mapped = D3D11_MAPPED_SUBRESOURCE::default();
            context.Map(staging, 0, D3D11_MAP_READ, 0, Some(&mut mapped)).ok()?;
            for y in 0..height as usize {
                pixels.extend_from_slice(std::slice::from_raw_parts((mapped.pData as *const u8).add(y * mapped.RowPitch as usize), row));
            }
            context.Unmap(staging, 0);
        }
        // (What no window covers is the scene's own window: drawn opaque, as the screen is.)
        for alpha in pixels.iter_mut().skip(3).step_by(4) {
            *alpha = 255;
        }
        Some(pixels)
    }
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
    /// The size the frames are made at.
    size: SizeInt32,
    shared: Arc<Shared>,
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

impl Scene {
    /// A scene standing for `area`, empty till it is arranged; `wake` is called (on another thread) as a frame
    /// wanted has been read (see `ready`), and as one comes unwanted after none had (see `fresh`).
    pub fn new(area: RECT, wake: Arc<dyn Fn() + Send + Sync>) -> Option<Scene> {
        let device = device()?;
        unsafe {
            let module = GetModuleHandleW(None).ok()?;
            let class = WNDCLASSW { lpfnWndProc: Some(procedure), hInstance: module.into(), lpszClassName: w!("GlanceScene"), ..Default::default() };
            RegisterClassW(&class);
            let (width, height) = ((area.right - area.left).max(1), (area.bottom - area.top).max(1));
            let owner = CreateWindowExW(WINDOW_EX_STYLE(0), class.lpszClassName, w!(""), WS_POPUP, 0, 0, 0, 0, None, None, Some(module.into()), None).ok()?;
            let window = match CreateWindowExW(WINDOW_EX_STYLE(0), class.lpszClassName, w!("Glance scene"), WS_POPUP, area.left, area.top, width, height, Some(owner), None, Some(module.into()), None) {
                Ok(window) => window,
                Err(_) => {
                    let _ = DestroyWindow(owner);
                    return None;
                }
            };
            let shared = Arc::new(Shared { of: Mutex::new((area, now_100ns())), latest: Mutex::new(None), wanted: AtomicBool::new(false), ready: Mutex::new(None), wake, staging: Mutex::new(None) });
            let made = (|| {
                let cloak: BOOL = true.into();
                DwmSetWindowAttribute(window, DWMWA_CLOAK, &cloak as *const BOOL as *const _, 4).ok()?;
                let _ = ShowWindow(window, SW_SHOWNOACTIVATE);
                let item: GraphicsCaptureItem = windows::core::factory::<GraphicsCaptureItem, IGraphicsCaptureItemInterop>().ok()?.CreateForWindow(window).ok()?;
                let size = item.Size().ok()?;
                let pool = Direct3D11CaptureFramePool::CreateFreeThreaded(&device.direct3d, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size).ok()?;
                pool.FrameArrived(&TypedEventHandler::new({
                    let shared = shared.clone();
                    move |pool: windows::core::Ref<Direct3D11CaptureFramePool>, _| {
                        let Ok(pool) = pool.ok() else { return Ok(()) };
                        let mut newest = None;
                        while let Ok(frame) = pool.TryGetNextFrame() {
                            newest = Some(frame);
                        }
                        let Some(frame) = newest else { return Ok(()) };
                        // Wanted, read now (one of what was there before is not, and the next is wanted still); else
                        // kept, the thread that draws told once.
                        if shared.wanted.swap(false, Ordering::AcqRel) {
                            if !shared.read(&frame) {
                                shared.wanted.store(true, Ordering::Release);
                            }
                        } else if shared.latest.lock().unwrap().replace(frame).is_none() {
                            (shared.wake)();
                        }
                        Ok(())
                    }
                }))
                .ok()?;
                let session = pool.CreateCaptureSession(&item).ok()?;
                // (Not on every system: a border no one sees is drawn then.)
                let _ = session.SetIsBorderRequired(false);
                let _ = session.SetIsCursorCaptureEnabled(false);
                session.StartCapture().ok()?;
                Some((pool, session, size))
            })();
            let Some((pool, session, size)) = made else {
                let _ = DestroyWindow(window);
                let _ = DestroyWindow(owner);
                return None;
            };
            Some(Scene { owner, window, area, shown: Vec::new(), pool, session, size, shared })
        }
    }

    /// Puts together in it, for `area`, the windows behind `over` (one of Glance's: those under it, the desktop's
    /// own among them) that meet the area, as they are now; what has not changed is left as it is. Whether anything
    /// had (then a frame of it as it is now comes).
    pub fn arrange(&mut self, area: RECT, over: HWND) -> bool {
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
        if area != self.area {
            let (width, height) = ((area.right - area.left).max(1), (area.bottom - area.top).max(1));
            unsafe {
                let _ = SetWindowPos(self.window, None, area.left, area.top, width, height, SWP_NOACTIVATE | SWP_NOZORDER);
            }
            self.area = area;
            let size = SizeInt32 { Width: width, Height: height };
            if size != self.size {
                if let Some(device) = device() {
                    if self.pool.Recreate(&device.direct3d, DirectXPixelFormat::B8G8R8A8UIntNormalized, 2, size).is_ok() {
                        self.size = size;
                    }
                }
            }
        }
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
        // is being put in place).
        *self.shared.of.lock().unwrap() = (area, now_100ns());
        *self.shared.latest.lock().unwrap() = None;
        true
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

    /// Shows nothing till it is arranged again (nor does the compositor put anything together in it meanwhile): for
    /// whoever wants what is behind only as it was when last looked at, not as it changes.
    pub fn rest(&mut self) {
        self.forget();
        // (Nor is the frame of it emptied wanted, nor any come meanwhile.)
        self.shared.wanted.store(false, Ordering::Release);
        *self.shared.latest.lock().unwrap() = None;
        *self.shared.ready.lock().unwrap() = None;
    }

    /// Whether a frame has come that was not wanted as it came (and is not read yet).
    pub fn fresh(&self) -> bool {
        self.shared.latest.lock().unwrap().is_some()
    }

    /// Wants a frame: one come already, read at once (on another thread); else the next, as it comes. Told as it
    /// has been read (see `ready`).
    pub fn want(&self) {
        if self.shared.ready.lock().unwrap().is_some() {
            return;
        }
        match self.shared.latest.lock().unwrap().take() {
            Some(frame) => {
                let shared = self.shared.clone();
                std::thread::spawn(move || {
                    if !shared.read(&frame) {
                        shared.wanted.store(true, Ordering::Release);
                    }
                });
            }
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
        let _ = self.session.Close();
        let _ = self.pool.Close();
        self.forget();
        unsafe {
            let _ = DestroyWindow(self.window);
            let _ = DestroyWindow(self.owner);
        }
    }
}
