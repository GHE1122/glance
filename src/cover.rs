//! The cover of what plays, as its player gives it to Windows: a small copy
//! to lay behind the lyrics, and the colours to draw them in (the cover's
//! most telling colour and a second that goes with it). Read once for each
//! track, on a thread of its own, only while something asks for it.

use std::sync::{Arc, Mutex};

use windows::core::Interface;
use windows::Win32::Graphics::Imaging::{
    CLSID_WICImagingFactory, GUID_WICPixelFormat32bppPBGRA, IWICImagingFactory, WICBitmapDitherTypeNone, WICBitmapInterpolationModeFant, WICBitmapPaletteTypeCustom, WICDecodeMetadataCacheOnDemand,
};
use windows::Win32::System::Com::{CoCreateInstance, IStream, CLSCTX_INPROC_SERVER};
use windows::Win32::System::WinRT::CreateStreamOverRandomAccessStream;

/// The side of the copy kept (pixels, square).
pub const SIDE: u32 = 192;

/// A track's cover: its copy (premultiplied BGRA, `SIDE` square, opaque), and its colours (straight RGB, 0 to 1).
pub struct Cover {
    pub pixels: Arc<Vec<u8>>,
    pub base: [f32; 3],
    pub accent: [f32; 3],
}

/// How long after its player gave no cover (or the cover of another track) it is asked again: a player just started,
/// or just changed tracks, gives its cover a moment after the rest.
const AGAIN_AFTER: std::time::Duration = std::time::Duration::from_secs(4);

/// What is known of a track's cover: being read, read (when: see `AGAIN_AFTER`) and none had, or it.
enum Known {
    Reading,
    None(std::time::Instant),
    Found(Arc<Cover>),
}

/// The track the cover was asked for (its title and artist), and what is known of it.
static KNOWN: Mutex<Option<(String, Known)>> = Mutex::new(None);

/// The cover of the music playing, the track `title` by `artist`, as its player gives it; read (on a thread of its
/// own) the first time it is asked for, and again a while after its player gave none.
pub fn of(title: &str, artist: &str) -> Option<Arc<Cover>> {
    let key = format!("{title}\u{1}{artist}");
    let mut known = KNOWN.lock().unwrap();
    match known.as_ref() {
        Some((of, Known::Found(cover))) if *of == key => return Some(cover.clone()),
        Some((of, Known::Reading)) if *of == key => return None,
        Some((of, Known::None(since))) if *of == key && since.elapsed() < AGAIN_AFTER => return None,
        _ => {}
    }
    *known = Some((key.clone(), Known::Reading));
    let title = title.to_string();
    std::thread::spawn(move || {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let read = read(&title).map(Arc::new);
        let mut known = KNOWN.lock().unwrap();
        // (Kept only if the track is still the one asked about.)
        if known.as_ref().is_some_and(|(of, _)| *of == key) {
            *known = Some((key, read.map_or_else(|| Known::None(std::time::Instant::now()), Known::Found)));
        }
    });
    None
}

/// The cover of the music playing (as the player followed for music gives it), if it is the track `title`'s (a
/// player changing tracks tells the new title a moment before the new cover).
pub(crate) fn read(title: &str) -> Option<Cover> {
    let session = crate::media::session_of(crate::media::Follow::Music).ok()?;
    let properties = session.TryGetMediaPropertiesAsync().ok()?.join().ok()?;
    if properties.Title().ok()?.to_string() != title {
        return None;
    }
    let thumbnail = properties.Thumbnail().ok()?;
    let stream = thumbnail.OpenReadAsync().ok()?.join().ok()?;
    unsafe {
        let stream: IStream = CreateStreamOverRandomAccessStream(&stream.cast::<windows::core::IUnknown>().ok()?).ok()?;
        let factory: IWICImagingFactory = CoCreateInstance(&CLSID_WICImagingFactory, None, CLSCTX_INPROC_SERVER).ok()?;
        let decoder = factory.CreateDecoderFromStream(&stream, std::ptr::null(), WICDecodeMetadataCacheOnDemand).ok()?;
        let frame = decoder.GetFrame(0).ok()?;
        // Squared about its middle (most covers are square already), at `SIDE`.
        let (mut width, mut height) = (0, 0);
        frame.GetSize(&mut width, &mut height).ok()?;
        let side = width.min(height);
        let clipper = factory.CreateBitmapClipper().ok()?;
        clipper.Initialize(&frame, &windows::Win32::Graphics::Imaging::WICRect { X: ((width - side) / 2) as i32, Y: ((height - side) / 2) as i32, Width: side as i32, Height: side as i32 }).ok()?;
        let scaler = factory.CreateBitmapScaler().ok()?;
        scaler.Initialize(&clipper, SIDE, SIDE, WICBitmapInterpolationModeFant).ok()?;
        let converter = factory.CreateFormatConverter().ok()?;
        converter.Initialize(&scaler, &GUID_WICPixelFormat32bppPBGRA, WICBitmapDitherTypeNone, None, 0.0, WICBitmapPaletteTypeCustom).ok()?;
        let mut pixels = vec![0u8; (SIDE * SIDE * 4) as usize];
        converter.CopyPixels(std::ptr::null(), SIDE * 4, &mut pixels).ok()?;
        // (Opaque: a cover with clear parts is laid on black.)
        for pixel in pixels.chunks_mut(4) {
            pixel[3] = 255;
        }
        let (base, accent) = colours(&pixels);
        Some(Cover { pixels: Arc::new(pixels), base, accent })
    }
}

/// The colours of a cover's pixels (BGRA): the most telling of its main colours (vivid, neither near black nor
/// near white), and a second of another hue that goes with it (else the first turned round the wheel).
pub fn colours(pixels: &[u8]) -> ([f32; 3], [f32; 3]) {
    let main = median_cut(pixels, 8);
    let telling = |rgb: [f32; 3]| {
        let (_, saturation, lightness) = hsl(rgb);
        saturation * (1.0 - (lightness - 0.5).abs() * 1.2)
    };
    let base = main.iter().map(|(rgb, _)| *rgb).max_by(|a, b| telling(*a).total_cmp(&telling(*b))).unwrap_or([0.5, 0.5, 0.5]);
    let (hue, saturation, lightness) = hsl(base);
    let apart = |rgb: [f32; 3]| {
        let away = (hsl(rgb).0 - hue).abs();
        away.min(360.0 - away)
    };
    let accent = main.iter().map(|(rgb, _)| *rgb).filter(|rgb| (25.0..=160.0).contains(&apart(*rgb)) && hsl(*rgb).1 > 0.15).max_by(|a, b| telling(*a).total_cmp(&telling(*b)));
    (base, accent.unwrap_or_else(|| rgb((hue + 40.0) % 360.0, saturation, lightness)))
}

/// A cover's main colours and how much of it each is: its pixels (at four bits a channel) split, box by box, along
/// the widest range of the box that holds the most range and weight, into `count` boxes, each by its weighted mean.
fn median_cut(pixels: &[u8], count: usize) -> Vec<([f32; 3], f32)> {
    let mut counted = vec![0u32; 4096];
    for pixel in pixels.chunks(4) {
        counted[(pixel[2] as usize >> 4) << 8 | (pixel[1] as usize >> 4) << 4 | pixel[0] as usize >> 4] += 1;
    }
    let colours: Vec<([u8; 3], u32)> = counted.iter().enumerate().filter(|(_, n)| **n > 0).map(|(i, n)| ([(i >> 8) as u8, (i >> 4 & 15) as u8, (i & 15) as u8], *n)).collect();
    let mut boxes = vec![colours];
    while boxes.len() < count {
        let range = |colours: &[([u8; 3], u32)], channel: usize| colours.iter().map(|c| c.0[channel]).max().unwrap_or(0) - colours.iter().map(|c| c.0[channel]).min().unwrap_or(0);
        let worth = |colours: &[([u8; 3], u32)]| (0..3).map(|c| range(colours, c)).max().unwrap_or(0) as f32 * (colours.iter().map(|c| c.1).sum::<u32>() as f32).sqrt();
        let Some((at, _)) = boxes.iter().enumerate().filter(|(_, b)| b.len() > 1).max_by(|a, b| worth(a.1).total_cmp(&worth(b.1))) else { break };
        let mut split = boxes.swap_remove(at);
        let channel = (0..3).max_by_key(|c| range(&split, *c)).unwrap_or(0);
        split.sort_by_key(|c| c.0[channel]);
        let total: u32 = split.iter().map(|c| c.1).sum();
        let mut so_far = 0;
        let middle = split.iter().position(|c| {
            so_far += c.1;
            so_far * 2 >= total
        });
        let middle = middle.unwrap_or(0).clamp(1, split.len() - 1);
        let other = split.split_off(middle);
        boxes.push(split);
        boxes.push(other);
    }
    let all: u32 = boxes.iter().flatten().map(|c| c.1).sum::<u32>().max(1);
    boxes
        .iter()
        .map(|colours| {
            let weight: u32 = colours.iter().map(|c| c.1).sum();
            let mean = |channel: usize| colours.iter().map(|c| (c.0[channel] as f32 * 17.0 + 8.0) * c.1 as f32).sum::<f32>() / weight.max(1) as f32 / 255.0;
            ([mean(0), mean(1), mean(2)], weight as f32 / all as f32)
        })
        .collect()
}

/// A colour's hue (degrees), saturation and lightness (0 to 1).
pub fn hsl([r, g, b]: [f32; 3]) -> (f32, f32, f32) {
    let (most, least) = (r.max(g).max(b), r.min(g).min(b));
    let lightness = (most + least) / 2.0;
    if most - least < 1e-6 {
        return (0.0, 0.0, lightness);
    }
    let span = most - least;
    let saturation = span / (1.0 - (2.0 * lightness - 1.0).abs()).max(1e-6);
    let hue = if most == r {
        60.0 * ((g - b) / span).rem_euclid(6.0)
    } else if most == g {
        60.0 * ((b - r) / span + 2.0)
    } else {
        60.0 * ((r - g) / span + 4.0)
    };
    (hue, saturation.min(1.0), lightness)
}

/// The colour of `hue` (degrees), `saturation` and `lightness`.
pub fn rgb(hue: f32, saturation: f32, lightness: f32) -> [f32; 3] {
    let chroma = (1.0 - (2.0 * lightness - 1.0).abs()) * saturation;
    let x = chroma * (1.0 - ((hue / 60.0).rem_euclid(2.0) - 1.0).abs());
    let (r, g, b) = match (hue.rem_euclid(360.0) / 60.0) as u32 {
        0 => (chroma, x, 0.0),
        1 => (x, chroma, 0.0),
        2 => (0.0, chroma, x),
        3 => (0.0, x, chroma),
        4 => (x, 0.0, chroma),
        _ => (chroma, 0.0, x),
    };
    let m = lightness - chroma / 2.0;
    [r + m, g + m, b + m]
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn finds_a_covers_colours() {
        // Most of it dark grey, a red part and a blue part: the vivid two, one the base and the other with it.
        let mut pixels = Vec::new();
        for i in 0..1000 {
            let bgr = if i < 600 { [40, 40, 40] } else if i < 850 { [30, 30, 220] } else { [220, 120, 30] };
            pixels.extend_from_slice(&[bgr[0], bgr[1], bgr[2], 255]);
        }
        let (base, accent) = colours(&pixels);
        let red = |rgb: [f32; 3]| rgb[0] > 0.7 && rgb[2] < 0.3;
        let blue = |rgb: [f32; 3]| rgb[2] > 0.7 && rgb[0] < 0.3;
        assert!((red(base) && blue(accent)) || (blue(base) && red(accent)), "{base:?} {accent:?}");
        let (h, s, l) = hsl([0.8, 0.2, 0.2]);
        let back = rgb(h, s, l);
        assert!((back[0] - 0.8).abs() < 1e-3 && (back[1] - 0.2).abs() < 1e-3);
    }

    /// Reads the cover of the music playing: `cargo test -- --ignored reads_the_cover --nocapture`.
    #[test]
    #[ignore]
    fn reads_the_cover() {
        unsafe {
            let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
        }
        let session = crate::media::session_of(crate::media::Follow::Music).expect("music playing");
        let title = session.TryGetMediaPropertiesAsync().unwrap().join().unwrap().Title().unwrap().to_string();
        let cover = read(&title).expect("a cover");
        println!("{title}");
        println!("base {:?}, accent {:?}, {} bytes", cover.base, cover.accent, cover.pixels.len());
    }
}
