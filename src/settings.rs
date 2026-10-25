use serde::{Deserialize, Serialize};
use std::path::PathBuf;

#[derive(Clone, Serialize, Deserialize, Debug)]
pub struct Place {
    pub name: String,
    pub region: String,
    pub lat: f64,
    pub lon: f64,
}

/// Order matches the "Dashboard tiles" switches in the UI.
pub const FLAG_NAMES: [&str; 14] = [
    "Temperature",
    "Feels like",
    "Wind",
    "Precipitation",
    "Clouds",
    "Humidity",
    "Air quality",
    "UV index",
    "Visibility",
    "Pressure",
    "Sun",
    "Moon phase",
    "Hourly chart",
    "Daily forecast",
];
pub const REFRESH_MINUTES: [u64; 4] = [5, 15, 30, 60];
/// How long a cached response counts as fresh. 0 turns the cache off.
pub const CACHE_MINUTES: [u64; 5] = [0, 15, 30, 60, 120];

#[derive(Clone, Serialize, Deserialize)]
#[serde(default)]
pub struct Settings {
    pub theme: i32,
    pub accent: i32,
    pub u_temp: i32,
    pub u_wind: i32,
    pub u_press: i32,
    pub u_dist: i32,
    pub refresh: i32,
    pub cache: i32,
    pub anim: bool,
    /// Show a tray icon and keep running when the window is closed.
    pub tray: bool,
    pub tiles: Vec<bool>,
    pub places: Vec<Place>,
    pub selected: usize,
    /// Window size in physical pixels at last close.
    pub win: Option<[u32; 2]>,
}

impl Default for Settings {
    fn default() -> Self {
        Settings {
            theme: 0,
            accent: 0,
            u_temp: 0,
            u_wind: 0,
            u_press: 0,
            u_dist: 0,
            refresh: 1,
            cache: 2,
            anim: true,
            tray: true,
            tiles: vec![true; FLAG_NAMES.len()],
            places: vec![
                Place { name: "London".into(), region: "England, United Kingdom".into(), lat: 51.5085, lon: -0.1257 },
                Place { name: "New York".into(), region: "New York, United States".into(), lat: 40.7143, lon: -74.006 },
                Place { name: "Tokyo".into(), region: "Tokyo, Japan".into(), lat: 35.6895, lon: 139.6917 },
            ],
            selected: 0,
            win: None,
        }
    }
}

fn path() -> Option<PathBuf> {
    let base = std::env::var_os("APPDATA")?;
    Some(PathBuf::from(base).join("Skylight").join("settings.json"))
}

impl Settings {
    pub fn load() -> Settings {
        let mut s: Settings =
            path().and_then(|p| std::fs::read_to_string(p).ok()).and_then(|t| serde_json::from_str(&t).ok()).unwrap_or_default();
        s.tiles.resize(FLAG_NAMES.len(), true);
        if s.places.is_empty() {
            s.places = Settings::default().places;
        }
        if s.selected >= s.places.len() {
            s.selected = 0;
        }
        s
    }

    pub fn save(&self) {
        if let Some(p) = path() {
            if let Some(dir) = p.parent() {
                let _ = std::fs::create_dir_all(dir);
            }
            if let Ok(t) = serde_json::to_string_pretty(self) {
                let _ = std::fs::write(p, t);
            }
        }
    }
}
