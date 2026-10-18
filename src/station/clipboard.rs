//! What is copied: read from the clipboard for the station's Recently copied (as it changes), or as it is pasted on
//! the station (Ctrl+V).
//!
//! Read as one: the formats that keep it out are looked at and what it is read in the one opening of the clipboard,
//! and what is read copied into Glance's own before the clipboard is closed (the clipboard's own is the copier's, and
//! goes as the next copy comes). Its sequence number, read while it is open, tells which copy it was: told of a
//! change, Glance may find a later copy there (the one told of is then not had: copied very quickly one after
//! another, a copy between may be missed).
//!
//! What a copier asks of it is kept to (`Marks`, `judged`):
//! - `ExcludeClipboardContentFromMonitorProcessing` (a password manager's): not read at all as it changes.
//! - `CanIncludeInClipboardHistory` 0: not in Recently copied.
//! - `CanUploadToCloudClipboard` 0: kept, never to leave this PC (see `shelf::Item::stays_here`).

use std::path::PathBuf;
use windows::core::{w, Interface, PCWSTR};
use windows::Win32::Foundation::{HGLOBAL, HWND};
use windows::Win32::Graphics::Gdi::HBITMAP;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_ContainerFormatPng, GUID_WICPixelFormat32bppBGRA, IWICBitmapSource, IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapEncoderNoCache, WICBitmapPaletteTypeCustom,
    WICBitmapUseAlpha,
};
use windows::Win32::System::Com::StructuredStorage::{CreateStreamOnHGlobal, GetHGlobalFromStream};
use windows::Win32::System::Com::{CoCreateInstance, CLSCTX_INPROC_SERVER, STREAM_SEEK_END};
use windows::Win32::System::DataExchange::{CloseClipboard, GetClipboardData, GetClipboardSequenceNumber, IsClipboardFormatAvailable, OpenClipboard, RegisterClipboardFormatW};
use windows::Win32::System::Memory::{GlobalLock, GlobalSize, GlobalUnlock};
use windows::Win32::UI::Shell::{DragQueryFileW, HDROP};

/// The clipboard's own formats: a picture, text, files.
const BITMAP: u32 = 2;
const TEXT: u32 = 13;
const FILES: u32 = 15;
/// The most of a picture's or text's bytes read (more is not what the station is for: a file is).
const BYTES_MOST: usize = 256 << 20;

/// What was copied.
#[derive(Debug, PartialEq)]
pub enum Content {
    Files(Vec<PathBuf>),
    Text(String),
    /// A picture, as a PNG's bytes.
    Png(Vec<u8>),
}

/// What was copied, as Glance read it.
#[derive(Debug)]
pub struct Copied {
    pub content: Content,
    /// Never to leave this PC (see `Marks::upload`).
    pub stays_here: bool,
    /// The clipboard's sequence number as it was read (which copy it was).
    pub sequence: u32,
}

/// What its copier asked of a copy, by the formats it put with it.
#[derive(Clone, Copy, Debug, Default, PartialEq)]
pub struct Marks {
    /// `ExcludeClipboardContentFromMonitorProcessing` there (whatever it holds).
    pub excluded: bool,
    /// `CanIncludeInClipboardHistory`'s value, there.
    pub history: Option<u32>,
    /// `CanUploadToCloudClipboard`'s value, there.
    pub upload: Option<u32>,
}

/// Why the clipboard is read: it changed (for Recently copied), or it was pasted on the station by hand.
#[derive(Clone, Copy, Debug, PartialEq)]
pub enum Why {
    Changed,
    Pasted,
}

/// Whether a copy marked `marks` is taken, read for `why`: if so, whether it never leaves this PC. (Pasted by hand,
/// it is taken whatever it says of monitors and history: the person put it there.)
pub fn judged(marks: Marks, why: Why) -> Option<bool> {
    let stays_here = marks.upload == Some(0);
    match why {
        Why::Changed if marks.excluded || marks.history == Some(0) => None,
        _ => Some(stays_here),
    }
}

/// The clipboard's sequence number now.
pub fn sequence() -> u32 {
    unsafe { GetClipboardSequenceNumber() }
}

/// What is on the clipboard, read for `why` by window `owner`, as `judged` takes it: none if it is not taken, holds
/// nothing the station keeps, or the clipboard is held by another just now.
pub fn read(owner: HWND, why: Why) -> Option<Copied> {
    unsafe {
        OpenClipboard(Some(owner)).ok()?;
        let read = read_open(why);
        let _ = CloseClipboard();
        let (taken, stays_here, sequence) = read?;
        // (A picture read as the clipboard's was copied into a bitmap of Glance's; made a PNG now it is closed.)
        let content = match taken {
            Taken::Content(content) => content,
            Taken::Bitmap(bitmap) => Content::Png(png(&bitmap).ok()?),
        };
        Some(Copied { content, stays_here, sequence })
    }
}

/// What was read with the clipboard open: what it is, or a picture copied out of it to be made a PNG.
enum Taken {
    Content(Content),
    Bitmap(IWICBitmapSource),
}

/// As `read`, the clipboard open.
unsafe fn read_open(why: Why) -> Option<(Taken, bool, u32)> {
    unsafe {
        let sequence = GetClipboardSequenceNumber();
        let format = |name: PCWSTR| RegisterClipboardFormatW(name);
        let value = |name: PCWSTR| -> Option<u32> {
            let bytes = bytes_of(format(name))?;
            Some(u32::from_le_bytes(bytes.get(..4)?.try_into().ok()?))
        };
        let marks = Marks {
            excluded: IsClipboardFormatAvailable(format(w!("ExcludeClipboardContentFromMonitorProcessing"))).is_ok(),
            history: value(w!("CanIncludeInClipboardHistory")),
            upload: value(w!("CanUploadToCloudClipboard")),
        };
        let stays_here = judged(marks, why)?;
        // Files before text (a file copied in Explorer is also its name), text before a picture (cells copied in a
        // sheet are also their picture).
        let taken = if let Ok(files) = GetClipboardData(FILES) {
            let files = HDROP(files.0);
            let paths: Vec<PathBuf> = (0..DragQueryFileW(files, u32::MAX, None))
                .map(|index| {
                    let mut path = vec![0u16; DragQueryFileW(files, index, None) as usize + 1];
                    let length = DragQueryFileW(files, index, Some(&mut path)) as usize;
                    PathBuf::from(String::from_utf16_lossy(&path[..length]))
                })
                .collect();
            Taken::Content(Content::Files(paths))
        } else if let Some(text) = bytes_of(TEXT) {
            let units: Vec<u16> = text.chunks_exact(2).map(|pair| u16::from_le_bytes([pair[0], pair[1]])).take_while(|unit| *unit != 0).collect();
            Taken::Content(Content::Text(String::from_utf16_lossy(&units)))
        } else if let Some(png) = bytes_of(format(w!("PNG"))) {
            Taken::Content(Content::Png(png))
        } else if let Ok(bitmap) = GetClipboardData(BITMAP) {
            let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
            // (Its pixels copied into the bitmap made: the clipboard's goes as it is closed.)
            Taken::Bitmap(factory.CreateBitmapFromHBITMAP(HBITMAP(bitmap.0), Default::default(), WICBitmapUseAlpha).ok()?.cast().ok()?)
        } else {
            return None;
        };
        Some((taken, stays_here, sequence))
    }
}

/// The bytes of the clipboard's `format` (open), copied: none if it has none, or more than `BYTES_MOST`.
unsafe fn bytes_of(format: u32) -> Option<Vec<u8>> {
    unsafe {
        if format == 0 || IsClipboardFormatAvailable(format).is_err() {
            return None;
        }
        let memory = HGLOBAL(GetClipboardData(format).ok()?.0);
        let size = GlobalSize(memory);
        if size > BYTES_MOST {
            return None;
        }
        let at = GlobalLock(memory);
        if at.is_null() {
            return None;
        }
        let bytes = std::slice::from_raw_parts(at.cast::<u8>(), size).to_vec();
        let _ = GlobalUnlock(memory);
        Some(bytes)
    }
}

/// `image` as a PNG's bytes.
unsafe fn png(image: &IWICBitmapSource) -> windows::core::Result<Vec<u8>> {
    unsafe {
        let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER)?;
        let (mut width, mut height) = (0, 0);
        image.GetSize(&mut width, &mut height)?;
        let converter = factory.CreateFormatConverter()?;
        converter.Initialize(image, &GUID_WICPixelFormat32bppBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom)?;
        let stream = CreateStreamOnHGlobal(HGLOBAL::default(), true)?;
        let encoder = factory.CreateEncoder(&GUID_ContainerFormatPng, std::ptr::null())?;
        encoder.Initialize(&stream, WICBitmapEncoderNoCache)?;
        let mut frame = None;
        encoder.CreateNewFrame(&mut frame, std::ptr::null_mut())?;
        let frame = frame.ok_or_else(windows::core::Error::empty)?;
        frame.Initialize(None)?;
        frame.SetSize(width, height)?;
        let mut format = GUID_WICPixelFormat32bppBGRA;
        frame.SetPixelFormat(&mut format)?;
        frame.WriteSource(&converter, std::ptr::null())?;
        frame.Commit()?;
        encoder.Commit()?;
        let mut length = 0u64;
        stream.Seek(0, STREAM_SEEK_END, Some(&mut length))?;
        let memory = GetHGlobalFromStream(&stream)?;
        let bytes = GlobalLock(memory);
        let png = std::slice::from_raw_parts(bytes.cast::<u8>(), length as usize).to_vec();
        let _ = GlobalUnlock(memory);
        Ok(png)
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn a_copier_is_kept_to() {
        let plain = Marks::default();
        assert_eq!(judged(plain, Why::Changed), Some(false));
        // A password manager's: not read as it changes; pasted by hand, the person's.
        let excluded = Marks { excluded: true, ..plain };
        assert_eq!(judged(excluded, Why::Changed), None);
        assert_eq!(judged(excluded, Why::Pasted), Some(false));
        // Not for the history: not in Recently copied (1 is as none).
        assert_eq!(judged(Marks { history: Some(0), ..plain }, Why::Changed), None);
        assert_eq!(judged(Marks { history: Some(1), ..plain }, Why::Changed), Some(false));
        // Not to be uploaded: kept, never to leave this PC.
        assert_eq!(judged(Marks { upload: Some(0), ..plain }, Why::Changed), Some(true));
        assert_eq!(judged(Marks { upload: Some(0), excluded: true, ..plain }, Why::Pasted), Some(true));
    }
}
