//! The sound the PC is playing, as what moves with the music: how loud it is,
//! how much of it there is at each pitch (a spectrum, in bands from the bass
//! up), and a pulse at each beat. Heard as it leaves for the speakers
//! (Windows' loopback of the device sound plays from), and only while
//! something that shows it is up: each moment's sound is measured and let
//! go, nothing kept or sent.

use std::sync::atomic::{AtomicBool, AtomicU64, Ordering};
use std::sync::Mutex;
use std::time::{Duration, Instant};

use windows::Win32::Media::Audio::{
    eConsole, eRender, IAudioCaptureClient, IAudioClient, IMMDeviceEnumerator, MMDeviceEnumerator, AUDCLNT_BUFFERFLAGS_SILENT, AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, WAVEFORMATEX,
    WAVEFORMATEXTENSIBLE,
};
use windows::Win32::System::Com::{CoCreateInstance, CoTaskMemFree, CLSCTX_ALL};

/// How many bands the spectrum is in, and the pitches it spans (Hz).
pub const BANDS: usize = 24;
const LOWEST: f32 = 40.0;
const HIGHEST: f32 = 16_000.0;
/// How many samples each spectrum is taken over (at 48 kHz, 43 ms).
const WINDOW: usize = 2048;
/// How quiet is silence, and how loud is full, as the bands and the level show them (dB of full scale).
const FLOOR: f32 = -72.0;
const CEILING: f32 = -22.0;

/// What is heard now: each 0 to 1.
#[derive(Clone, Copy, Debug)]
pub struct Heard {
    pub level: f32,
    pub bands: [f32; BANDS],
    pub beat: f32,
}

impl Default for Heard {
    fn default() -> Self {
        Heard { level: 0.0, bands: [0.0; BANDS], beat: 0.0 }
    }
}

static HEARD: Mutex<Heard> = Mutex::new(Heard { level: 0.0, bands: [0.0; BANDS], beat: 0.0 });
/// When it was last asked for: listened while that was a moment ago.
static ASKED: AtomicU64 = AtomicU64::new(0);
static LISTENING: AtomicBool = AtomicBool::new(false);
/// How long it listens on after it was last asked.
const KEPT_ON: Duration = Duration::from_secs(2);

fn epoch() -> &'static Instant {
    static EPOCH: std::sync::OnceLock<Instant> = std::sync::OnceLock::new();
    EPOCH.get_or_init(Instant::now)
}

/// What is heard now; listening starts as it is first asked for, and stops a little after it no longer is.
pub fn heard() -> Heard {
    ASKED.store(epoch().elapsed().as_millis() as u64, Ordering::Relaxed);
    if !LISTENING.swap(true, Ordering::AcqRel) {
        std::thread::Builder::new().name("glance: sound heard".into()).spawn(|| {
            listen();
            *HEARD.lock().unwrap() = Heard::default();
            LISTENING.store(false, Ordering::Release);
        }).expect("a thread started");
    }
    *HEARD.lock().unwrap()
}

fn wanted() -> bool {
    epoch().elapsed().as_millis() as u64 - ASKED.load(Ordering::Relaxed) < KEPT_ON.as_millis() as u64
}

/// Listens while it is wanted: to the device sound plays from, again when that changes or goes.
fn listen() {
    unsafe {
        let _ = windows::Win32::System::Com::CoInitializeEx(None, windows::Win32::System::Com::COINIT_MULTITHREADED);
    }
    let mut analysis = Analysis::new();
    while wanted() {
        // (Ended by a device taken away or changed: listened to afresh after a moment.)
        let _ = listen_to_device(&mut analysis);
        decay_to_silence(&mut analysis);
        std::thread::sleep(Duration::from_millis(500));
    }
}

/// The sound of the default device for playing, till it is not wanted or the device fails.
fn listen_to_device(analysis: &mut Analysis) -> windows::core::Result<()> {
    unsafe {
        let devices: IMMDeviceEnumerator = CoCreateInstance(&MMDeviceEnumerator, None, CLSCTX_ALL)?;
        let device = devices.GetDefaultAudioEndpoint(eRender, eConsole)?;
        let client: IAudioClient = device.Activate(CLSCTX_ALL, None)?;
        let format = client.GetMixFormat()?;
        let (channels, rate, float, bits) = described(format);
        // Its own shared format, a fifth of a second of buffer.
        let begun = client.Initialize(AUDCLNT_SHAREMODE_SHARED, AUDCLNT_STREAMFLAGS_LOOPBACK, 2_000_000, 0, format, None);
        CoTaskMemFree(Some(format as *const _));
        begun?;
        let capture: IAudioCaptureClient = client.GetService()?;
        client.Start()?;
        analysis.rate = rate as f32;
        let mut last_quiet = Instant::now();
        let outcome = (|| -> windows::core::Result<()> {
            while wanted() {
                std::thread::sleep(Duration::from_millis(10));
                let mut took = false;
                while capture.GetNextPacketSize()? > 0 {
                    let (mut data, mut frames, mut flags) = (std::ptr::null_mut(), 0u32, 0u32);
                    capture.GetBuffer(&mut data, &mut frames, &mut flags, None, None)?;
                    let silent = flags & AUDCLNT_BUFFERFLAGS_SILENT.0 as u32 != 0;
                    for frame in 0..frames as usize {
                        let sample = if silent || data.is_null() { 0.0 } else { mono(data, frame, channels, float, bits) };
                        analysis.push(sample);
                    }
                    capture.ReleaseBuffer(frames)?;
                    took = true;
                }
                // Nothing comes while nothing plays: what was heard dies away.
                if took {
                    last_quiet = Instant::now();
                    analysis.measure();
                } else if last_quiet.elapsed() > Duration::from_millis(40) {
                    decay_to_silence(analysis);
                }
            }
            Ok(())
        })();
        let _ = client.Stop();
        outcome
    }
}

/// The device's channels, rate, and whether its samples are floats, and of how many bits.
unsafe fn described(format: *const WAVEFORMATEX) -> (usize, u32, bool, u16) {
    unsafe {
        let base = *format;
        // WAVE_FORMAT_IEEE_FLOAT, or an extensible one whose kind is float (KSDATAFORMAT_SUBTYPE_IEEE_FLOAT's first field).
        let float = base.wFormatTag == 3 || (base.wFormatTag == 0xFFFE && (*(format as *const WAVEFORMATEXTENSIBLE)).SubFormat.data1 == 3);
        (base.nChannels.max(1) as usize, base.nSamplesPerSec, float, base.wBitsPerSample)
    }
}

/// Frame `frame` of the buffer at `data`, its channels made one (-1 to 1).
unsafe fn mono(data: *const u8, frame: usize, channels: usize, float: bool, bits: u16) -> f32 {
    unsafe {
        let mut sum = 0.0;
        for channel in 0..channels {
            let at = frame * channels + channel;
            sum += match (float, bits) {
                (true, 32) => *(data as *const f32).add(at),
                (false, 16) => *(data as *const i16).add(at) as f32 / 32768.0,
                (false, 32) => *(data as *const i32).add(at) as f32 / 2_147_483_648.0,
                (false, 24) => {
                    let byte = data.add(at * 3);
                    let value = (*byte as i32) | (*byte.add(1) as i32) << 8 | (*byte.add(2) as i8 as i32) << 16;
                    value as f32 / 8_388_608.0
                }
                _ => 0.0,
            };
        }
        sum / channels as f32
    }
}

/// What turns samples into what is heard: the latest of them, and what has been heard, smoothed.
struct Analysis {
    rate: f32,
    samples: Vec<f32>,
    written: usize,
    heard: Heard,
    /// The bass's rise from one look to the next (its spectral flux) on average lately, and how much it varies
    /// (for a beat: well above both), the bass bins' last magnitudes, and when the last beat was.
    flux_mean: f32,
    flux_variance: f32,
    bass_before: Vec<f32>,
    last_beat: Instant,
    measured: Instant,
}

impl Analysis {
    fn new() -> Self {
        Analysis { rate: 48_000.0, samples: vec![0.0; WINDOW], written: 0, heard: Heard::default(), flux_mean: 0.0, flux_variance: 0.0, bass_before: Vec::new(), last_beat: Instant::now(), measured: Instant::now() }
    }

    fn push(&mut self, sample: f32) {
        self.samples[self.written % WINDOW] = sample;
        self.written += 1;
    }

    /// The spectrum and the level of the latest samples, smoothed (quick to rise, slower to fall), and a beat
    /// where the bass leaps above its average; at most a few hundred times a second.
    fn measure(&mut self) {
        if self.measured.elapsed() < Duration::from_millis(8) {
            return;
        }
        let step = self.measured.elapsed().as_secs_f32().min(0.1);
        self.measured = Instant::now();
        // The window, oldest first, shaped (Hann) so its ends do not ring.
        let mut real: Vec<f32> = (0..WINDOW)
            .map(|i| {
                let sample = self.samples[(self.written + i) % WINDOW];
                let shape = 0.5 - 0.5 * (2.0 * std::f32::consts::PI * i as f32 / (WINDOW - 1) as f32).cos();
                sample * shape
            })
            .collect();
        let mut imaginary = vec![0.0; WINDOW];
        fft(&mut real, &mut imaginary);
        let decibels = |power: f32| ((10.0 * power.max(1e-12).log10() - FLOOR) / (CEILING - FLOOR)).clamp(0.0, 1.0);
        let bin_of = |hz: f32| ((hz / self.rate * WINDOW as f32) as usize).clamp(1, WINDOW / 2 - 1);
        let mut bands = [0.0; BANDS];
        for (band, value) in bands.iter_mut().enumerate() {
            // Each band an equal step of pitch (on a log scale) from the lowest to the highest.
            let (from, to) = (LOWEST * (HIGHEST / LOWEST).powf(band as f32 / BANDS as f32), LOWEST * (HIGHEST / LOWEST).powf((band + 1) as f32 / BANDS as f32));
            let (from, to) = (bin_of(from), bin_of(to).max(bin_of(from) + 1));
            let peak = (from..to).map(|bin| real[bin] * real[bin] + imaginary[bin] * imaginary[bin]).fold(0.0, f32::max);
            // (Magnitudes of a Hann-shaped window of this length, as a share of full scale.)
            *value = decibels(peak * 16.0 / (WINDOW * WINDOW) as f32);
        }
        let recent = &self.samples;
        let power = recent.iter().map(|s| s * s).sum::<f32>() / WINDOW as f32;
        let level = decibels(power * 2.0);
        // Rising at once, falling over about a fifth of a second.
        let fall = (step / 0.2).min(1.0);
        let follow = |was: f32, now: f32| if now > was { was + (now - was) * 0.7 } else { was + (now - was) * fall };
        for (kept, now) in self.heard.bands.iter_mut().zip(bands) {
            *kept = follow(*kept, now);
        }
        self.heard.level = follow(self.heard.level, level);
        // A beat: the bass (to 260 Hz) rising by much more than it lately rises, by its mean and spread over the
        // last second or so; no closer to the last than a beat in a fast song is.
        let bass: Vec<f32> = (1..bin_of(260.0) + 1).map(|bin| (real[bin] * real[bin] + imaginary[bin] * imaginary[bin]).sqrt()).collect();
        let flux = if self.bass_before.len() == bass.len() { bass.iter().zip(&self.bass_before).map(|(now, was)| (now - was).max(0.0)).sum::<f32>() } else { 0.0 };
        self.bass_before = bass;
        let keep = (step / 1.2).min(1.0);
        let spread = self.flux_variance.sqrt();
        let beat = flux > self.flux_mean + 1.8 * spread && flux > 1e-3 && self.last_beat.elapsed() >= Duration::from_millis(280);
        let off = flux - self.flux_mean;
        self.flux_mean += off * keep;
        self.flux_variance = (1.0 - keep) * (self.flux_variance + keep * off * off);
        self.heard.beat *= (1.0 - step / 0.25).max(0.0);
        if beat && self.heard.level > 0.05 {
            self.last_beat = Instant::now();
            self.heard.beat = 1.0;
        }
        *HEARD.lock().unwrap() = self.heard;
    }
}

/// What is heard dying away, nothing coming in.
fn decay_to_silence(analysis: &mut Analysis) {
    analysis.samples.iter_mut().for_each(|sample| *sample = 0.0);
    analysis.measure();
}

/// The discrete Fourier transform of `real` + i·`imaginary` (of a power of two's length), in place: radix 2,
/// decimation in time.
fn fft(real: &mut [f32], imaginary: &mut [f32]) {
    let n = real.len();
    let mut j = 0;
    for i in 1..n {
        let mut bit = n >> 1;
        while j & bit != 0 {
            j ^= bit;
            bit >>= 1;
        }
        j |= bit;
        if i < j {
            real.swap(i, j);
            imaginary.swap(i, j);
        }
    }
    let mut length = 2;
    while length <= n {
        let angle = -2.0 * std::f32::consts::PI / length as f32;
        let (turn_re, turn_im) = (angle.cos(), angle.sin());
        for start in (0..n).step_by(length) {
            let (mut w_re, mut w_im) = (1.0f32, 0.0f32);
            for k in 0..length / 2 {
                let (a, b) = (start + k, start + k + length / 2);
                let (b_re, b_im) = (real[b] * w_re - imaginary[b] * w_im, real[b] * w_im + imaginary[b] * w_re);
                real[b] = real[a] - b_re;
                imaginary[b] = imaginary[a] - b_im;
                real[a] += b_re;
                imaginary[a] += b_im;
                (w_re, w_im) = (w_re * turn_re - w_im * turn_im, w_re * turn_im + w_im * turn_re);
            }
        }
        length <<= 1;
    }
}

#[cfg(test)]
mod tests {
    #[test]
    fn fft_finds_a_tone() {
        let n = 256;
        let mut real: Vec<f32> = (0..n).map(|i| (2.0 * std::f32::consts::PI * 10.0 * i as f32 / n as f32).sin()).collect();
        let mut imaginary = vec![0.0; n];
        super::fft(&mut real, &mut imaginary);
        let power: Vec<f32> = (0..n / 2).map(|i| real[i] * real[i] + imaginary[i] * imaginary[i]).collect();
        let loudest = power.iter().enumerate().max_by(|a, b| a.1.total_cmp(b.1)).unwrap().0;
        assert_eq!(loudest, 10);
        assert!((power[10].sqrt() - n as f32 / 2.0).abs() < 0.01 * n as f32);
    }

    /// Listens for three seconds to what this PC plays and prints what it hears: `cargo test -- --ignored hears_the_pc --nocapture`.
    #[test]
    #[ignore]
    fn hears_the_pc() {
        for _ in 0..24 {
            let heard = super::heard();
            let bars: String = heard.bands.iter().map(|band| [' ', '▁', '▂', '▃', '▄', '▅', '▆', '▇', '█'][(band * 8.0).round() as usize]).collect();
            println!("level {:.2}  beat {:.2}  |{bars}|", heard.level, heard.beat);
            std::thread::sleep(std::time::Duration::from_millis(125));
        }
    }
}
