//! What the helper keeps: the port it listens on, the key shared with the
//! paired phone, and how the lights are when they are on. In the user's own
//! folder; the helper runs as the user.

use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The port the helper listens on unless another is kept.
const PORT: u16 = 47631;
/// How many LEDs a header for addressable strips is taken to have while
/// OpenRGB has none for it: enough for a few fans in a chain.
const STRIP_LEDS: u32 = 120;

/// What the lights do while they are on.
#[derive(Clone, Copy, Debug, Default, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Effect {
    /// One colour, steady.
    #[default]
    Steady,
    /// That colour, swelling and fading.
    Breathing,
    /// Every colour in turn (the colour chosen is not used).
    Rainbow,
    /// A colour for how warm the PC is, from blue through to red (the
    /// colour chosen, while Glance is not there to say how warm).
    Heat,
}

/// How the lights are when they are on: their colour (RRGGBB), what they
/// do, and how bright they are (percent of the colour's own brightness).
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(default)]
pub struct Light {
    pub color: String,
    pub effect: Effect,
    pub brightness: u8,
}

impl Default for Light {
    fn default() -> Self {
        Light { color: "FFFFFF".into(), effect: Effect::Steady, brightness: 100 }
    }
}

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Config {
    pub port: u16,
    /// This PC, as the phone names it.
    pub name: String,
    /// The key shared with the paired phone (hex, 32 bytes): made at first run.
    pub key: String,
    /// Where OpenRGB is, if it is not where its installer puts it.
    pub openrgb: Option<PathBuf>,
    pub strip_leds: u32,
    pub light: Light,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            port: PORT,
            name: std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into()),
            key: String::new(),
            openrgb: None,
            strip_leds: STRIP_LEDS,
            light: Light::default(),
        }
    }
}

fn path() -> PathBuf {
    let roaming = std::env::var_os("APPDATA").map(PathBuf::from).unwrap_or_default();
    roaming.join("dev.weiyi.glance.remote").join("config.json")
}

impl Config {
    /// The configuration kept, or a new one, with a key made for it: kept
    /// at once, so that the key is the same from then on.
    pub fn load() -> std::io::Result<Config> {
        let path = path();
        let mut config: Config = std::fs::read_to_string(&path).ok().and_then(|text| serde_json::from_str(&text).ok()).unwrap_or_default();
        if crate::sign::key(&config.key).is_none() {
            let mut key = [0u8; 32];
            getrandom::getrandom(&mut key).map_err(|error| std::io::Error::other(error.to_string()))?;
            config.key = crate::sign::hex(&key);
            config.save()?;
        }
        Ok(config)
    }

    pub fn save(&self) -> std::io::Result<()> {
        let path = path();
        std::fs::create_dir_all(path.parent().unwrap())?;
        std::fs::write(path, serde_json::to_string_pretty(self).unwrap())
    }

}
