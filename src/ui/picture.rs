//! Pictures a user gives to go with what they send (screenshots, mostly):
//! taken from files or from the clipboard, made into JPEGs no larger than
//! what is sent may be, each with a small likeness to show in a window.

use windows::core::{Interface, Result, HSTRING};
use windows::Win32::Foundation::{GENERIC_READ, HGLOBAL, HWND};
use windows::Win32::Graphics::Gdi::HBITMAP;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatJpeg, GUID_WICPixelFormat24bppBGR, GUID_WICPixelFormat32bppBGRA, IWICBitmapSource, IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapEncoderNoCache,
    WICBitmapIgnoreAlpha, WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::StructuredStorage::{CreateStreamOnHGlobal, GetHGlobalFromStream};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER, STREAM_SEEK_END};
use windows::Win32::System::DataExchange::{GetClipboardData, IsClipboardFormatAvailable};
use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
use windows::Win32::UI::Shell::{DragQueryFileW, SHCreateMemStream, HDROP};

/// The clipboard's names for a picture and for files (CF_BITMAP, CF_HDROP).
const BITMAP: u32 = 2;
const FILES: u32 = 15;
/// The longest side a picture is sent with (pixels): a screen's, as most are.
const SIDE_MOST: u32 = 2560;
/// The most one picture may weigh as it is sent (the site keeps none heavier).
pub const BYTES_MOST: usize = 1_400_000;

/// One picture: as it is sent, and its likeness (as small as covers the
/// size asked for, its own shape kept): its pixels (BGRA, opaque) and how
/// many across and down.
pub struct Picture {
    pub jpeg: Vec<u8>,
    pub small: Vec<u8>,
    pub size: (u32, u32),
}

/// The picture in the file at `path`, if it is one Windows reads.
pub fn from_file(path: &str, small: (u32, u32)) -> Option<Picture> {
    unsafe {
        let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let decoder = factory.CreateDecoderFromFilename(&HSTRING::from(path), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand).ok()?;
        made(&factory, &decoder.GetFrame(0).ok()?.cast().ok()?, small).ok()
    }
}

/// The JPEG `jpeg` as a picture, itself as it is, with its likeness: one kept of what was sent, to be shown again.
pub fn from_jpeg(jpeg: &[u8], small: (u32, u32)) -> Option<Picture> {
    unsafe {
        let (factory, image) = decoded(jpeg)?;
        let (mut width, mut height) = (0, 0);
        image.GetSize(&mut width, &mut height).ok()?;
        let (small, size) = likeness(&factory, &image, (width, height), small).ok()?;
        Some(Picture { jpeg: jpeg.to_vec(), small, size })
    }
}

/// The JPEG `jpeg` made again no larger than `side` either way (its shape kept): a copy small enough to keep.
pub fn shrunk(jpeg: &[u8], side: u32) -> Option<Vec<u8>> {
    unsafe {
        let (factory, image) = decoded(jpeg)?;
        let (mut width, mut height) = (0, 0);
        image.GetSize(&mut width, &mut height).ok()?;
        self::jpeg(&factory, &within(&factory, &image, (width, height), (side, side)).ok()?.0).ok()
    }
}

/// The picture in `bytes` (any kind Windows reads), with the factory it was read with.
unsafe fn decoded(bytes: &[u8]) -> Option<(IWICImagingFactory, IWICBitmapSource)> {
    unsafe {
        let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let stream = SHCreateMemStream(Some(bytes))?;
        let decoder = factory.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand).ok()?;
        let image = decoder.GetFrame(0).ok()?.cast().ok()?;
        Some((factory, image))
    }
}

/// Whether the clipboard holds a picture, or files (which may be pictures).
pub fn on_clipboard() -> bool {
    unsafe { IsClipboardFormatAvailable(BITMAP).is_ok() || IsClipboardFormatAvailable(FILES).is_ok() }
}

/// The pictures on the clipboard: the files copied there that are
/// pictures, or the one picture copied there (a screenshot, as Windows
/// takes them). `owner` is the window asking.
pub fn from_clipboard(owner: HWND, small: (u32, u32)) -> Vec<Picture> {
    let Some(_opened) = crate::clipboard::open(owner) else { return Vec::new() };
    unsafe {
        if let Ok(files) = GetClipboardData(FILES) {
            let files = HDROP(files.0);
            (0..DragQueryFileW(files, u32::MAX, None))
                .filter_map(|index| {
                    let mut path = vec![0u16; DragQueryFileW(files, index, None) as usize + 1];
                    let length = DragQueryFileW(files, index, Some(&mut path)) as usize;
                    from_file(&String::from_utf16_lossy(&path[..length]), small)
                })
                .collect()
        } else if let Ok(bitmap) = GetClipboardData(BITMAP) {
            let read = || -> Result<Picture> {
                let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
                made(&factory, &factory.CreateBitmapFromHBITMAP(HBITMAP(bitmap.0), Default::default(), WICBitmapIgnoreAlpha)?.cast()?, small)
            };
            read().into_iter().collect()
        } else {
            Vec::new()
        }
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_file_becomes_a_jpeg_and_a_likeness_within_its_room() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let picture = super::from_file(concat!(env!("CARGO_MANIFEST_DIR"), "/icons/128x128@2x.png"), (84, 52)).expect("read");
        assert_eq!(picture.jpeg[..3], [0xFF, 0xD8, 0xFF]);
        assert!(picture.jpeg.len() <= super::BYTES_MOST);
        // (A square picture: as wide as its room, and as tall as it is wide.)
        assert_eq!(picture.size, (84, 84));
        assert_eq!(picture.small.len(), 84 * 84 * 4);
        assert!(super::from_file(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"), (84, 52)).is_none());
        // Made smaller to be kept, and read again as it was kept.
        let kept = super::shrunk(&picture.jpeg, 64).expect("shrunk");
        let again = super::from_jpeg(&kept, (32, 20)).expect("read again");
        assert_eq!((again.jpeg, again.size), (kept, (32, 32)));
        assert!(super::from_jpeg(b"not a picture", (32, 20)).is_none());
    }
}

/// `image` as a picture to send, and its likeness.
unsafe fn made(factory: &IWICImagingFactory, image: &IWICBitmapSource, small: (u32, u32)) -> Result<Picture> {
    unsafe {
        let (mut width, mut height) = (0, 0);
        image.GetSize(&mut width, &mut height)?;
        // No larger than is sent, and then no heavier: at half the size, and half again, while it is.
        let mut side = SIDE_MOST;
        let jpeg = loop {
            let jpeg = jpeg(factory, &within(factory, image, (width, height), (side, side))?.0)?;
            if jpeg.len() <= BYTES_MOST {
                break jpeg;
            }
            side /= 2;
        };
        let (small, size) = likeness(factory, image, (width, height), small)?;
        Ok(Picture { jpeg, small, size })
    }
}

/// `image`'s (of `size`) likeness, as small as covers `room`: its pixels (BGRA) and how many across and down.
unsafe fn likeness(factory: &IWICImagingFactory, image: &IWICBitmapSource, size: (u32, u32), room: (u32, u32)) -> Result<(Vec<u8>, (u32, u32))> {
    unsafe {
        let (likeness, (w, h)) = covering(factory, image, size, room)?;
        let converter = factory.CreateFormatConverter()?;
        converter.Initialize(&likeness, &GUID_WICPixelFormat32bppBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
        let mut small = vec![0u8; (w * h * 4) as usize];
        converter.CopyPixels(std::ptr::null(), w * 4, &mut small)?;
        Ok((small, (w, h)))
    }
}

/// `image` (of `size`) no larger than `most` either way, its shape kept:
/// itself, if it is within that already. And the size it then has.
unsafe fn within(factory: &IWICImagingFactory, image: &IWICBitmapSource, size: (u32, u32), most: (u32, u32)) -> Result<(IWICBitmapSource, (u32, u32))> {
    let fits = (most.0 as f64 / size.0 as f64).min(most.1 as f64 / size.1 as f64);
    if fits >= 1.0 {
        return Ok((image.clone(), size));
    }
    let scaled = (((size.0 as f64 * fits).round() as u32).max(1), ((size.1 as f64 * fits).round() as u32).max(1));
    unsafe {
        let scaler = factory.CreateBitmapScaler()?;
        scaler.Initialize(image, scaled.0, scaled.1, WICBitmapInterpolationModeFant)?;
        Ok((scaler.cast()?, scaled))
    }
}

/// `image` (of `size`) as small as covers `room`, its shape kept (itself, if it is no larger), and the size
/// it then has.
unsafe fn covering(factory: &IWICImagingFactory, image: &IWICBitmapSource, size: (u32, u32), room: (u32, u32)) -> Result<(IWICBitmapSource, (u32, u32))> {
    let covers = (room.0 as f64 / size.0 as f64).max(room.1 as f64 / size.1 as f64).min(1.0);
    let most = (((size.0 as f64 * covers).ceil() as u32).max(1), ((size.1 as f64 * covers).ceil() as u32).max(1));
    unsafe { within(factory, image, size, most) }
}

/// `image` as a JPEG's bytes.
unsafe fn jpeg(factory: &IWICImagingFactory, image: &IWICBitmapSource) -> Result<Vec<u8>> {
    unsafe {
        let (mut width, mut height) = (0, 0);
        image.GetSize(&mut width, &mut height)?;
        let converter = factory.CreateFormatConverter()?;
        converter.Initialize(image, &GUID_WICPixelFormat24bppBGR, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
        let stream = CreateStreamOnHGlobal(HGLOBAL::default(), true)?;
        let encoder = factory.CreateEncoder(&GUID_ContainerFormatJpeg, std::ptr::null())?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
        let mut frame = None;
        encoder.CreateNewFrame(&mut frame, std::ptr::null_mut())?;
        let frame = frame.ok_or_else(windows::core::Error::empty)?;
        frame.Initialize(None)?;
        frame.SetSize(width, height)?;
        let mut format = GUID_WICPixelFormat24bppBGR;
        frame.SetPixelFormat(&mut format)?;
        frame.WriteSource(&converter, std::ptr::null())?;
        frame.Commit()?;
        encoder.Commit()?;
        let mut length = 0u64;
        stream.Seek(0, STREAM_SEEK_END, Some(&mut length))?;
        let memory = GetHGlobalFromStream(&stream)?;
        let bytes = GlobalLock(memory);
        let jpeg = std::slice::from_raw_parts(bytes.cast::<u8>(), length as usize).to_vec();
        let _ = GlobalUnlock(memory);
        Ok(jpeg)
    }
}
