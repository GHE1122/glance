//! What the helper keeps: the port it listens on, the key shared with the
//! paired phone, and how the lights are when they are on. In the user's own
//! folder; the helper runs as the user.

use std::collections::BTreeMap;
use std::path::PathBuf;

use serde::{Deserialize, Serialize};

/// The port the helper listens on unless another is kept.
const PORT: u16 = 47631;
/// How many LEDs a header for addressable strips is taken to have while
/// OpenRGB has none for it: enough for a few fans in a chain.
const STRIP_LEDS: u32 = 120;

/// How a lighting device is when the lights are on.
#[derive(Clone, Debug, PartialEq, Serialize, Deserialize)]
#[serde(rename_all = "lowercase")]
pub enum Lit {
    /// One of the device's own modes, by its name.
    Mode(String),
    /// All of it in one colour (RRGGBB).
    Color(String),
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
    /// Each lighting device, by its name as OpenRGB has it, as it is lit;
    /// one not named, in white.
    pub lights_on: BTreeMap<String, Lit>,
}

impl Default for Config {
    fn default() -> Self {
        Config {
            port: PORT,
            name: std::env::var("COMPUTERNAME").unwrap_or_else(|_| "PC".into()),
            key: String::new(),
            openrgb: None,
            strip_leds: STRIP_LEDS,
            lights_on: BTreeMap::new(),
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

    /// How the device named `name` is lit.
    pub fn lit(&self, name: &str) -> Lit {
        self.lights_on.get(name).cloned().unwrap_or(Lit::Color("FFFFFF".into()))
    }
}
