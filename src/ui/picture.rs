//! Pictures a user gives to go with what they send (screenshots, mostly):
//! taken from files or from the clipboard, made into JPEGs no larger than
//! what is sent may be, each with a small likeness to show in a window.

use windows::core::{Interface, Result, HSTRING};
use windows::Win32::Foundation::{GENERIC_READ, HANDLE, HGLOBAL, HWND};
use windows::Win32::Graphics::Gdi::{CreateDIBSection, DeleteObject, BITMAPINFO, BITMAPINFOHEADER, BI_RGB, DIB_RGB_COLORS, HBITMAP};
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatJpeg, GUID_WICPixelFormat24bppBGR, GUID_WICPixelFormat32bppBGRA, IWICBitmapSource, IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapEncoderNoCache,
    WICBitmapIgnoreAlpha, WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::StructuredStorage::{CreateStreamOnHGlobal, GetHGlobalFromStream};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER, STREAM_SEEK_END};
use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, IsClipboardFormatAvailable, OpenClipboard};
use windows::Win32::System::Memory::{GlobalLock, GlobalUnlock};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

/// The clipboard's names for a picture and for files (CF_BITMAP, CF_HDROP).
const BITMAP: u32 = 2;
const FILES: u32 = 15;
/// The longest side a picture is sent with (pixels): a screen's, as most are.
const SIDE_MOST: u32 = 2560;
/// The most one picture may weigh as it is sent (the site keeps none heavier).
pub const BYTES_MOST: usize = 1_400_000;

/// One picture: as it is sent, and its likeness (of the size asked for, or
/// smaller: its own shape kept).
pub struct Picture {
    pub jpeg: Vec<u8>,
    pub small: HBITMAP,
}

impl Drop for Picture {
    fn drop(&mut self) {
        unsafe {
            let _ = DeleteObject(self.small.into());
        }
    }
}

/// The picture in the file at `path`, if it is one Windows reads.
pub fn from_file(path: &str, small: (u32, u32)) -> Option<Picture> {
    unsafe {
        let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let decoder = factory.CreateDecoderFromFilename(&HSTRING::from(path), None, GENERIC_READ, WICDecodeMetadataCacheOnDemand).ok()?;
        made(&factory, &decoder.GetFrame(0).ok()?.cast().ok()?, small).ok()
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
    unsafe {
        if OpenClipboard(Some(owner)).is_err() {
            return Vec::new();
        }
        let taken = if let Ok(files) = GetClipboardData(FILES) {
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
        };
        let _ = CloseClipboard();
        taken
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn a_file_becomes_a_jpeg_and_a_likeness_within_its_room() {
        use windows::Win32::Graphics::Gdi::{GetObjectW, BITMAP};
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let picture = super::from_file(concat!(env!("CARGO_MANIFEST_DIR"), "/website/apple-touch-icon.png"), (84, 52)).expect("read");
        assert_eq!(picture.jpeg[..3], [0xFF, 0xD8, 0xFF]);
        assert!(picture.jpeg.len() <= super::BYTES_MOST);
        let mut small = BITMAP::default();
        unsafe { GetObjectW(picture.small.into(), size_of::<BITMAP>() as i32, Some((&mut small as *mut BITMAP).cast())) };
        // (A square picture: as tall as its room, and as wide as it is tall.)
        assert_eq!((small.bmWidth, small.bmHeight), (52, 52));
        assert!(super::from_file(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml"), (84, 52)).is_none());
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
        let (likeness, (w, h)) = within(factory, image, (width, height), small)?;
        let converter = factory.CreateFormatConverter()?;
        converter.Initialize(&likeness, &GUID_WICPixelFormat32bppBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
        // (Its rows from the top down, as the pixels are read.)
        let described = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: size_of::<BITMAPINFOHEADER>() as u32, biWidth: w as i32, biHeight: -(h as i32), biPlanes: 1, biBitCount: 32, biCompression: BI_RGB.0, ..Default::default() }, ..Default::default() };
        let mut pixels = std::ptr::null_mut();
        let small = CreateDIBSection(None, &described, DIB_RGB_COLORS, &mut pixels, Some(HANDLE::default()), 0)?;
        let picture = Picture { jpeg, small };
        converter.CopyPixels(std::ptr::null(), w * 4, std::slice::from_raw_parts_mut(pixels.cast::<u8>(), (w * h * 4) as usize))?;
        Ok(picture)
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
