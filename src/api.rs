use crate::settings::Place;
use serde_json::{json, Value};
use std::time::Duration;

pub struct Current {
    pub temp: f64,
    pub feels: f64,
    pub humidity: f64,
    pub code: i32,
    pub is_day: bool,
    pub cloud: f64,
    pub pressure: f64,
    pub wind: f64,
    pub gust: f64,
    pub dir: f64,
    pub precip: f64,
}

pub struct Hour {
    pub time: String,
    pub temp: f64,
    pub code: i32,
    pub is_day: bool,
    pub prob: f64,
    pub vis: f64,
    pub dew: f64,
    pub uv: f64,
    pub pressure: f64,
}

pub struct Day {
    pub date: String,
    pub code: i32,
    pub hi: f64,
    pub lo: f64,
    pub sunrise: String,
    pub sunset: String,
    pub uv_max: f64,
}

pub struct Weather {
    pub cur: Current,
    /// Local time at the location, "YYYY-MM-DDTHH:MM".
    pub time: String,
    pub hours: Vec<Hour>,
    /// Index into `hours` for the current hour.
    pub idx: usize,
    pub days: Vec<Day>,
    pub today: usize,
    pub aqi: Option<f64>,
    pub pm25: Option<f64>,
}

#[derive(Clone, Copy)]
pub struct PlaceNow {
    pub temp: f64,
    pub code: i32,
    pub is_day: bool,
}

fn get(url: &str, query: &[(&str, &str)]) -> Result<Value, String> {
    let mut req = ureq::get(url).timeout(Duration::from_secs(12)).set("User-Agent", concat!("Skylight/", env!("CARGO_PKG_VERSION")));
    for (k, v) in query {
        req = req.query(k, v);
    }
    req.call().map_err(|e| e.to_string())?.into_json::<Value>().map_err(|e| e.to_string())
}

fn f(v: &Value) -> f64 {
    v.as_f64().unwrap_or(f64::NAN)
}
fn nums(v: &Value) -> Vec<f64> {
    v.as_array().map(|a| a.iter().map(f).collect()).unwrap_or_default()
}
fn strs(v: &Value) -> Vec<String> {
    v.as_array().map(|a| a.iter().map(|x| x.as_str().unwrap_or("").to_string()).collect()).unwrap_or_default()
}
fn or0(x: f64) -> f64 {
    if x.is_nan() {
        0.0
    } else {
        x
    }
}

/// Raw JSON from the network, kept as-is so it can be cached on disk and parsed later.
pub fn fetch_weather_raw(p: &Place) -> Result<Value, String> {
    let lat = p.lat.to_string();
    let lon = p.lon.to_string();
    let v = get(
        "https://api.open-meteo.com/v1/forecast",
        &[
            ("latitude", &lat),
            ("longitude", &lon),
            ("current", "temperature_2m,relative_humidity_2m,apparent_temperature,is_day,precipitation,weather_code,cloud_cover,pressure_msl,wind_speed_10m,wind_direction_10m,wind_gusts_10m"),
            ("hourly", "temperature_2m,weather_code,is_day,precipitation_probability,visibility,dew_point_2m,uv_index,pressure_msl"),
            ("daily", "weather_code,temperature_2m_max,temperature_2m_min,sunrise,sunset,uv_index_max"),
            ("timezone", "auto"),
            ("past_days", "1"),
            ("forecast_days", "5"),
            ("wind_speed_unit", "kmh"),
        ],
    )?;
    // Air quality is optional: some places have no coverage.
    let a = get(
        "https://air-quality-api.open-meteo.com/v1/air-quality",
        &[("latitude", &lat), ("longitude", &lon), ("current", "us_aqi,pm2_5")],
    )
    .unwrap_or(Value::Null);
    Ok(json!({ "f": v, "a": a }))
}

pub fn parse_weather(raw: &Value) -> Result<Weather, String> {
    let v = &raw["f"];
    let c = &v["current"];
    let cur = Current {
        temp: or0(f(&c["temperature_2m"])),
        feels: or0(f(&c["apparent_temperature"])),
        humidity: or0(f(&c["relative_humidity_2m"])),
        code: c["weather_code"].as_i64().unwrap_or(0) as i32,
        is_day: c["is_day"].as_i64().unwrap_or(1) == 1,
        cloud: or0(f(&c["cloud_cover"])),
        pressure: or0(f(&c["pressure_msl"])),
        wind: or0(f(&c["wind_speed_10m"])),
        gust: or0(f(&c["wind_gusts_10m"])),
        dir: or0(f(&c["wind_direction_10m"])),
        precip: or0(f(&c["precipitation"])),
    };
    let time = c["time"].as_str().ok_or("no time in response")?.to_string();

    let h = &v["hourly"];
    let (ht, tt, hc, hd, hp, hv, hdw, huv, hpr) = (
        strs(&h["time"]),
        nums(&h["temperature_2m"]),
        nums(&h["weather_code"]),
        nums(&h["is_day"]),
        nums(&h["precipitation_probability"]),
        nums(&h["visibility"]),
        nums(&h["dew_point_2m"]),
        nums(&h["uv_index"]),
        nums(&h["pressure_msl"]),
    );
    let at = |a: &Vec<f64>, i: usize| a.get(i).copied().unwrap_or(f64::NAN);
    let hours: Vec<Hour> = (0..ht.len())
        .map(|i| Hour {
            time: ht[i].clone(),
            temp: or0(at(&tt, i)),
            code: or0(at(&hc, i)) as i32,
            is_day: at(&hd, i) != 0.0,
            prob: or0(at(&hp, i)),
            vis: at(&hv, i),
            dew: or0(at(&hdw, i)),
            uv: or0(at(&huv, i)),
            pressure: or0(at(&hpr, i)),
        })
        .collect();
    let key = &time[..13];
    let idx = hours.iter().position(|x| x.time.starts_with(key)).ok_or("current hour not found")?;
    if idx < 3 || idx + 20 >= hours.len() {
        return Err("forecast window too short".into());
    }

    let d = &v["daily"];
    let (dt, dc, dh, dl, dsr, dss, duv) = (
        strs(&d["time"]),
        nums(&d["weather_code"]),
        nums(&d["temperature_2m_max"]),
        nums(&d["temperature_2m_min"]),
        strs(&d["sunrise"]),
        strs(&d["sunset"]),
        nums(&d["uv_index_max"]),
    );
    let days: Vec<Day> = (0..dt.len())
        .map(|i| Day {
            date: dt[i].clone(),
            code: or0(at(&dc, i)) as i32,
            hi: or0(at(&dh, i)),
            lo: or0(at(&dl, i)),
            sunrise: dsr.get(i).cloned().unwrap_or_default(),
            sunset: dss.get(i).cloned().unwrap_or_default(),
            uv_max: or0(at(&duv, i)),
        })
        .collect();
    let today = days.iter().position(|x| time.starts_with(&x.date)).unwrap_or(0);
    if days.len() < today + 5 {
        return Err("daily forecast too short".into());
    }

    let (x, y) = (f(&raw["a"]["current"]["us_aqi"]), f(&raw["a"]["current"]["pm2_5"]));
    let (aqi, pm25) = ((!x.is_nan()).then_some(x), (!y.is_nan()).then_some(y));

    Ok(Weather { cur, time, hours, idx, days, today, aqi, pm25 })
}

/// Current conditions for every saved place in one request.
pub fn fetch_places_raw(places: &[Place]) -> Result<Value, String> {
    let lat = places.iter().map(|p| p.lat.to_string()).collect::<Vec<_>>().join(",");
    let lon = places.iter().map(|p| p.lon.to_string()).collect::<Vec<_>>().join(",");
    get(
        "https://api.open-meteo.com/v1/forecast",
        &[("latitude", &lat), ("longitude", &lon), ("current", "temperature_2m,weather_code,is_day"), ("timezone", "auto")],
    )
}

pub fn parse_places(raw: &Value, count: usize) -> Vec<Option<PlaceNow>> {
    let parse = |v: &Value| {
        let c = &v["current"];
        let t = f(&c["temperature_2m"]);
        (!t.is_nan()).then(|| PlaceNow {
            temp: t,
            code: c["weather_code"].as_i64().unwrap_or(0) as i32,
            is_day: c["is_day"].as_i64().unwrap_or(1) == 1,
        })
    };
    let mut out: Vec<Option<PlaceNow>> = match raw {
        Value::Array(a) => a.iter().map(parse).collect(),
        Value::Null => vec![],
        v => vec![parse(v)],
    };
    out.resize(count, None);
    out
}

pub fn geocode(q: &str) -> Result<Vec<Place>, String> {
    let v = get("https://geocoding-api.open-meteo.com/v1/search", &[("name", q), ("count", "6"), ("language", "en"), ("format", "json")])?;
    Ok(v["results"]
        .as_array()
        .map(|a| {
            a.iter()
                .filter_map(|r| {
                    let region = [r["admin1"].as_str(), r["country"].as_str()].iter().flatten().copied().collect::<Vec<_>>().join(", ");
                    Some(Place {
                        name: r["name"].as_str()?.to_string(),
                        region,
                        lat: r["latitude"].as_f64()?,
                        lon: r["longitude"].as_f64()?,
                    })
                })
                .collect()
        })
        .unwrap_or_default())
}
