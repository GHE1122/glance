//! Each thing on the station's picture: a file's as Explorer shows it (its thumbnail: a picture's, a video's, a
//! PDF's; else its icon), through the shell, on a thread of its own (a thumbnail may take a while to be made: never
//! on a thread that draws), kept once made.

use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::mpsc::{Receiver, Sender};
use crate::ui::canvas::Picture;
use std::sync::{Arc, Mutex, OnceLock};
use windows::core::HSTRING;
use windows::Win32::Foundation::SIZE;
use windows::Win32::Graphics::Gdi::{DeleteObject, GetDIBits, GetObjectW, BITMAP, BITMAPINFO, BITMAPINFOHEADER, DIB_RGB_COLORS, HBITMAP, HGDIOBJ};
use windows::Win32::UI::Shell::{IShellItemImageFactory, SHCreateItemFromParsingName, SIIGBF_BIGGERSIZEOK, SIIGBF_ICONONLY, SIIGBF_RESIZETOFIT};

/// A picture: its pixels (BGRA, premultiplied, top row first) and how many across and down.
#[derive(Clone, Debug)]
struct Thumb {
    pixels: Vec<u8>,
    size: (u32, u32),
}

/// What a picture is asked for: the file, when it was last written (a file written again is pictured again), the
/// side (physical px) it is to fill, and how (see `Made`).
#[derive(Clone, Debug, PartialEq, Eq, Hash)]
struct Key {
    path: PathBuf,
    written: Option<std::time::SystemTime>,
    side: u32,
    made: Made,
}

/// How a picture is made: as Explorer shows the file (its thumbnail, else its icon), its shape kept in the square;
/// filling the square (a picture's middle, as much as the square holds: for a box it is to fill); its icon alone.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
enum Made {
    Fit,
    Fill,
    Icon,
}

/// The most pictures kept (the oldest asked for go first).
const KEPT_MOST: usize = 256;

struct Kept {
    made: HashMap<Key, Option<Picture>>,
    /// Asked for, in the order they were (the oldest first), and being made.
    order: Vec<Key>,
    asked: HashSet<Key>,
}

static KEPT: Mutex<Option<Kept>> = Mutex::new(None);
static ASKS: OnceLock<Mutex<Sender<Key>>> = OnceLock::new();
/// Called (on the pictures' thread) as one is made.
static TOLD: OnceLock<Box<dyn Fn() + Send + Sync>> = OnceLock::new();

/// Starts the pictures' thread; `told` is called as each is made.
pub fn start(told: impl Fn() + Send + Sync + 'static) {
    let _ = TOLD.set(Box::new(told));
    let (sender, asks) = std::sync::mpsc::channel();
    if ASKS.set(Mutex::new(sender)).is_err() {
        return;
    }
    *KEPT.lock().unwrap() = Some(Kept { made: HashMap::new(), order: Vec::new(), asked: HashSet::new() });
    std::thread::Builder::new().name("glance: station pictures".into()).spawn(move || make(asks)).expect("a thread started");
}

/// The picture of the file at `path`, `side` physical px square (its own shape in its middle): as made; none yet
/// (asked for, and told when it is), or none to be had.
pub fn of(path: &Path, side: u32) -> Option<Picture> {
    asked(path, side, Made::Fit)
}

/// The picture of the picture at `path`, `side` physical px square, filling it (its middle).
pub fn filling(path: &Path, side: u32) -> Option<Picture> {
    asked(path, side, Made::Fill)
}

/// The icon of the file (or folder) at `path`, `side` physical px square: what Explorer shows of a file of its kind.
pub fn icon(path: &Path, side: u32) -> Option<Picture> {
    asked(path, side, Made::Icon)
}

fn asked(path: &Path, side: u32, made: Made) -> Option<Picture> {
    let key = Key { path: path.to_path_buf(), written: std::fs::metadata(path).and_then(|meta| meta.modified()).ok(), side, made };
    let mut kept = KEPT.lock().unwrap();
    let kept = kept.as_mut()?;
    if let Some(made) = kept.made.get(&key) {
        return made.clone();
    }
    if kept.asked.insert(key.clone()) {
        if let Some(asks) = ASKS.get() {
            let _ = asks.lock().unwrap().send(key);
        }
    }
    None
}

fn make(asks: Receiver<Key>) {
    // (The shell's thumbnail makers expect a single-threaded apartment.)
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
    }
    while let Ok(key) = asks.recv() {
        let number = {
            use std::hash::{Hash, Hasher};
            let mut hasher = std::collections::hash_map::DefaultHasher::new();
            key.hash(&mut hasher);
            hasher.finish()
        };
        let made = picture(&key.path, key.side, key.made == Made::Icon).map(|thumb| match key.made {
            Made::Fill => squared(&middle(&thumb), key.side, number),
            Made::Fit | Made::Icon => squared(&thumb, key.side, number),
        });
        {
            let mut kept = KEPT.lock().unwrap();
            let Some(kept) = kept.as_mut() else { continue };
            kept.asked.remove(&key);
            kept.order.push(key.clone());
            kept.made.insert(key, made);
            while kept.order.len() > KEPT_MOST {
                let oldest = kept.order.remove(0);
                kept.made.remove(&oldest);
            }
        }
        if let Some(told) = TOLD.get() {
            told();
        }
    }
}

/// `thumb` made no larger than `side` square, its shape kept: each pixel of the smaller the mean of those of the larger
/// in its box (the shell, asked for one size, may give a larger); one less than half as large made as large as fits
/// (a Store app's icon comes small), each pixel between the four nearest.
fn fitted(thumb: &Thumb, side: u32) -> std::borrow::Cow<'_, Thumb> {
    let (w, h) = thumb.size;
    if w.max(h) * 2 < side && w > 0 && h > 0 {
        return std::borrow::Cow::Owned(enlarged(thumb, side));
    }
    if w <= side && h <= side {
        return std::borrow::Cow::Borrowed(thumb);
    }
    let k = side as f64 / w.max(h) as f64;
    let (to_w, to_h) = (((w as f64 * k).round() as u32).clamp(1, side), ((h as f64 * k).round() as u32).clamp(1, side));
    let mut pixels = vec![0u8; (to_w * to_h * 4) as usize];
    for ty in 0..to_h {
        let (y0, y1) = (ty * h / to_h, ((ty + 1) * h / to_h).max(ty * h / to_h + 1));
        for tx in 0..to_w {
            let (x0, x1) = (tx * w / to_w, ((tx + 1) * w / to_w).max(tx * w / to_w + 1));
            let mut sum = [0u32; 4];
            for y in y0..y1 {
                for x in x0..x1 {
                    let at = ((y * w + x) * 4) as usize;
                    for (channel, total) in sum.iter_mut().enumerate() {
                        *total += u32::from(thumb.pixels[at + channel]);
                    }
                }
            }
            let count = (y1 - y0) * (x1 - x0);
            let to = ((ty * to_w + tx) * 4) as usize;
            for (channel, total) in sum.into_iter().enumerate() {
                pixels[to + channel] = ((total + count / 2) / count) as u8;
            }
        }
    }
    std::borrow::Cow::Owned(Thumb { pixels, size: (to_w, to_h) })
}

/// `thumb` made as large as fits `side` square, its shape kept, each pixel blended from the four nearest of it.
fn enlarged(thumb: &Thumb, side: u32) -> Thumb {
    let (w, h) = thumb.size;
    let k = side as f64 / w.max(h) as f64;
    let (to_w, to_h) = (((w as f64 * k).round() as u32).clamp(1, side), ((h as f64 * k).round() as u32).clamp(1, side));
    let mut pixels = vec![0u8; (to_w * to_h * 4) as usize];
    let at = |x: u32, y: u32, channel: usize| f64::from(thumb.pixels[((y * w + x) * 4) as usize + channel]);
    for ty in 0..to_h {
        let sy = ((ty as f64 + 0.5) / k - 0.5).clamp(0.0, (h - 1) as f64);
        let (y0, fy) = (sy.floor() as u32, sy.fract());
        let y1 = (y0 + 1).min(h - 1);
        for tx in 0..to_w {
            let sx = ((tx as f64 + 0.5) / k - 0.5).clamp(0.0, (w - 1) as f64);
            let (x0, fx) = (sx.floor() as u32, sx.fract());
            let x1 = (x0 + 1).min(w - 1);
            let to = ((ty * to_w + tx) * 4) as usize;
            for channel in 0..4 {
                let top = at(x0, y0, channel) * (1.0 - fx) + at(x1, y0, channel) * fx;
                let bottom = at(x0, y1, channel) * (1.0 - fx) + at(x1, y1, channel) * fx;
                pixels[to + channel] = (top * (1.0 - fy) + bottom * fy).round() as u8;
            }
        }
    }
    Thumb { pixels, size: (to_w, to_h) }
}

/// `thumb` without the clear rows and columns round what it shows (an icon is drawn within room round it, a Store app's
/// in much room): what it shows made as large as the tile, as every picture's is.
fn trimmed(thumb: &Thumb) -> std::borrow::Cow<'_, Thumb> {
    let (w, h) = thumb.size;
    let seen = |x: u32, y: u32| thumb.pixels[((y * w + x) * 4 + 3) as usize] > 0;
    let rows: Vec<u32> = (0..h).filter(|y| (0..w).any(|x| seen(x, *y))).collect();
    let columns: Vec<u32> = (0..w).filter(|x| (0..h).any(|y| seen(*x, y))).collect();
    let (Some(&top), Some(&bottom), Some(&left), Some(&right)) = (rows.first(), rows.last(), columns.first(), columns.last()) else {
        return std::borrow::Cow::Borrowed(thumb);
    };
    if (top, left, bottom, right) == (0, 0, h - 1, w - 1) {
        return std::borrow::Cow::Borrowed(thumb);
    }
    let (to_w, to_h) = (right - left + 1, bottom - top + 1);
    let pixels = (top..=bottom).flat_map(|y| thumb.pixels[((y * w + left) * 4) as usize..((y * w + right + 1) * 4) as usize].iter().copied()).collect();
    std::borrow::Cow::Owned(Thumb { pixels, size: (to_w, to_h) })
}

/// `thumb` in the middle of a square `side` across (made smaller to fit where it is larger), clear round it, as a
/// picture known by `key`.
fn squared(thumb: &Thumb, side: u32, key: u64) -> Picture {
    let thumb = trimmed(thumb);
    let thumb = fitted(&thumb, side);
    let (w, h) = thumb.size;
    let (left, top) = ((side - w) / 2, (side - h) / 2);
    let mut pixels = vec![0u8; (side * side * 4) as usize];
    for row in 0..h {
        let from = (row * thumb.size.0 * 4) as usize;
        let to = (((top + row) * side + left) * 4) as usize;
        pixels[to..to + (w * 4) as usize].copy_from_slice(&thumb.pixels[from..from + (w * 4) as usize]);
    }
    Picture { key, side, pixels: Arc::new(pixels) }
}

/// The square in the middle of `thumb`, as large as it holds (what fills a square of its picture).
fn middle(thumb: &Thumb) -> Thumb {
    let (w, h) = thumb.size;
    let side = w.min(h);
    let (left, top) = ((w - side) / 2, (h - side) / 2);
    let mut pixels = Vec::with_capacity((side * side * 4) as usize);
    for row in top..top + side {
        let from = ((row * w + left) * 4) as usize;
        pixels.extend_from_slice(&thumb.pixels[from..from + (side * 4) as usize]);
    }
    Thumb { pixels, size: (side, side) }
}

/// The picture of the file at `path`, as large as fits `side` square (its shape kept), as Explorer shows it (`icon`:
/// its icon alone, not a thumbnail of what it holds).
fn picture(path: &Path, side: u32, icon: bool) -> Option<Thumb> {
    unsafe {
        // (A program the Start menu names by its file's path, "shell:AppsFolder\D:\App\a.exe", which the shell
        // does not read by that name: by the file itself.)
        let named = path.to_string_lossy();
        let path = match named.strip_prefix("shell:AppsFolder\\").map(Path::new) {
            Some(file) if file.is_absolute() => file.to_path_buf(),
            // (Its parts joined as Windows writes paths: the shell does not read one written with '/'.)
            _ => path.components().collect(),
        };
        let factory: IShellItemImageFactory = SHCreateItemFromParsingName(&HSTRING::from(path.as_os_str()), None).ok()?;
        let side = side.max(16) as i32;
        let how = if icon { SIIGBF_ICONONLY | SIIGBF_BIGGERSIZEOK } else { SIIGBF_RESIZETOFIT | SIIGBF_BIGGERSIZEOK };
        let bitmap = factory.GetImage(SIZE { cx: side, cy: side }, how).ok()?;
        let thumb = pixels_of(bitmap);
        let _ = DeleteObject(HGDIOBJ(bitmap.0));
        thumb
    }
}

/// A bitmap's pixels (32 bits, as the shell makes them: premultiplied BGRA), top row first.
unsafe fn pixels_of(bitmap: HBITMAP) -> Option<Thumb> {
    unsafe {
        let mut described = BITMAP::default();
        if GetObjectW(HGDIOBJ(bitmap.0), std::mem::size_of::<BITMAP>() as i32, Some((&mut described as *mut BITMAP).cast())) == 0 {
            return None;
        }
        let (w, h) = (described.bmWidth, described.bmHeight.abs());
        let mut info = BITMAPINFO { bmiHeader: BITMAPINFOHEADER { biSize: std::mem::size_of::<BITMAPINFOHEADER>() as u32, biWidth: w, biHeight: -h, biPlanes: 1, biBitCount: 32, ..Default::default() }, ..Default::default() };
        let mut pixels = vec![0u8; (w * h * 4) as usize];
        let screen = windows::Win32::Graphics::Gdi::GetDC(None);
        let lines = GetDIBits(screen, bitmap, 0, h as u32, Some(pixels.as_mut_ptr().cast()), &mut info, DIB_RGB_COLORS);
        windows::Win32::Graphics::Gdi::ReleaseDC(None, screen);
        if lines != h {
            return None;
        }
        // (A picture with no alpha of its own, as an icon of old may be, wholly opaque.)
        if described.bmBitsPixel != 32 || pixels.chunks_exact(4).all(|pixel| pixel[3] == 0) {
            pixels.chunks_exact_mut(4).for_each(|pixel| pixel[3] = 255);
        }
        Some(Thumb { pixels, size: (w as u32, h as u32) })
    }
}

#[cfg(test)]
mod made {
    use super::*;

    /// A thumbnail larger than the side asked for is made smaller to fit, its shape and colours kept, and centred:
    /// never cut down to its corner.
    #[test]
    fn a_larger_thumbnail_is_fitted_whole() {
        // 256 × 128: its left half red, its right half blue (BGRA).
        let pixels = (0..128).flat_map(|_| (0..256).flat_map(|x| if x < 128 { [0, 0, 255, 255] } else { [255, 0, 0, 255] })).collect();
        let picture = squared(&Thumb { pixels, size: (256, 128) }, 128, 7);
        let at = |x: u32, y: u32| &picture.pixels[((y * 128 + x) * 4) as usize..((y * 128 + x) * 4 + 4) as usize];
        // 128 × 64 in the middle: clear above and below, red on its left, blue on its right.
        assert_eq!(at(10, 10), [0, 0, 0, 0]);
        assert_eq!(at(10, 64), [0, 0, 255, 255]);
        assert_eq!(at(120, 64), [255, 0, 0, 255]);
        assert_eq!(at(120, 120), [0, 0, 0, 0]);
    }

    /// An icon drawn small in much clear room is made as large as the tile: the clear room taken off first.
    #[test]
    fn an_icon_in_much_room_fills_the_tile() {
        // 128 × 128, clear but for 16 × 16 red in its middle.
        let pixels = (0..128u32).flat_map(|y| (0..128u32).flat_map(move |x| if (56..72).contains(&x) && (56..72).contains(&y) { [0, 0, 255, 255] } else { [0, 0, 0, 0] })).collect();
        let picture = squared(&Thumb { pixels, size: (128, 128) }, 128, 7);
        assert_eq!(&picture.pixels[((4 * 128 + 4) * 4) as usize..((4 * 128 + 4) * 4 + 4) as usize], [0, 0, 255, 255]);
    }

    /// A thumbnail less than half as large as the side asked for is made as large as fits, its colours kept.
    #[test]
    fn a_small_thumbnail_is_made_larger() {
        let pixels = (0..16).flat_map(|_| (0..32).flat_map(|x| if x < 16 { [0, 0, 255, 255] } else { [255, 0, 0, 255] })).collect();
        let picture = squared(&Thumb { pixels, size: (32, 16) }, 128, 7);
        let at = |x: u32, y: u32| &picture.pixels[((y * 128 + x) * 4) as usize..((y * 128 + x) * 4 + 4) as usize];
        // 128 × 64 in the middle.
        assert_eq!(at(10, 10), [0, 0, 0, 0]);
        assert_eq!(at(10, 64), [0, 0, 255, 255]);
        assert_eq!(at(120, 64), [255, 0, 0, 255]);
    }
}

#[cfg(test)]
mod tests {
    /// A picture's file gives its picture, another file its icon; each within the side asked for.
    #[test]
    #[ignore = "asks this PC's shell"]
    fn files_are_pictured_as_explorer_shows_them() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_APARTMENTTHREADED);
        }
        let picture = super::picture(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/website/apple-touch-icon.png")), 96, false).expect("a picture's");
        assert!(picture.size.0 <= 96 && picture.size.1 <= 96 && picture.size.0.max(picture.size.1) >= 48, "{:?}", picture.size);
        let icon = super::picture(std::path::Path::new(concat!(env!("CARGO_MANIFEST_DIR"), "/Cargo.toml")), 48, false).expect("an icon");
        assert!(icon.pixels.chunks_exact(4).any(|pixel| pixel[3] > 0));
        println!("picture {:?}, icon {:?}", picture.size, icon.size);
    }
}
