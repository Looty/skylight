//! Turns raw weather + settings into the plain values the Slint UI shows.
//! Unit conversion lives here, so changing units never needs a network call.
use crate::api::*;
use crate::settings::*;
use crate::{AppWindow, DayData, FlagData, HitData, HourData, MarkData, PlaceData, State, Theme, TileData};
use slint::{Color, ComponentHandle, ModelRc, VecModel};

const GOOD: i32 = 0;
const WARN: i32 = 1;
const BAD: i32 = 2;
const INFO: i32 = 3;
const TONES: [(u8, u8, u8); 4] = [(0x3f, 0xb9, 0x7f), (0xee, 0x8f, 0x3a), (0xe8, 0x50, 0x5f), (0x4d, 0x9b, 0xff)];

fn rgb(c: (u8, u8, u8)) -> Color {
    Color::from_rgb_u8(c.0, c.1, c.2)
}
fn model<T: Clone + 'static>(v: Vec<T>) -> ModelRc<T> {
    ModelRc::new(VecModel::from(v))
}
fn r0(x: f64) -> String {
    format!("{}", x.round() as i64)
}

// ---------- calendar and moon ----------
fn days_from_civil(y: i64, m: i64, d: i64) -> i64 {
    let y = if m <= 2 { y - 1 } else { y };
    let era = (if y >= 0 { y } else { y - 399 }) / 400;
    let yoe = y - era * 400;
    let doy = (153 * (if m > 2 { m - 3 } else { m + 9 }) + 2) / 5 + d - 1;
    let doe = yoe * 365 + yoe / 4 - yoe / 100 + doy;
    era * 146097 + doe - 719468
}
fn civil_from_days(z: i64) -> (i64, i64, i64) {
    let z = z + 719468;
    let era = (if z >= 0 { z } else { z - 146096 }) / 146097;
    let doe = z - era * 146097;
    let yoe = (doe - doe / 1460 + doe / 36524 - doe / 146096) / 365;
    let y = yoe + era * 400;
    let doy = doe - (365 * yoe + yoe / 4 - yoe / 100);
    let mp = (5 * doy + 2) / 153;
    let d = doy - (153 * mp + 2) / 5 + 1;
    let m = if mp < 10 { mp + 3 } else { mp - 9 };
    (if m <= 2 { y + 1 } else { y }, m, d)
}
fn ymd(s: &str) -> (i64, i64, i64) {
    let n = |a: usize, b: usize| s.get(a..b).and_then(|x| x.parse().ok()).unwrap_or(0);
    (n(0, 4), n(5, 7), n(8, 10))
}
fn hm(s: &str) -> (i64, i64) {
    let n = |a: usize, b: usize| s.get(a..b).and_then(|x| x.parse().ok()).unwrap_or(0);
    (n(11, 13), n(14, 16))
}
fn mins(s: &str) -> f64 {
    let (h, m) = hm(s);
    (h * 60 + m) as f64
}
fn weekday(date: &str) -> &'static str {
    let (y, m, d) = ymd(date);
    ["Sun", "Mon", "Tue", "Wed", "Thu", "Fri", "Sat"][(days_from_civil(y, m, d) + 4).rem_euclid(7) as usize]
}
const MONTHS: [&str; 12] = ["Jan", "Feb", "Mar", "Apr", "May", "Jun", "Jul", "Aug", "Sep", "Oct", "Nov", "Dec"];

const SYNODIC: f64 = 29.530588853;
fn moon_age() -> f64 {
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0) / 86400.0;
    let reference = days_from_civil(2000, 1, 6) as f64 + (18.0 * 60.0 + 14.0) / 1440.0;
    (now - reference).rem_euclid(SYNODIC)
}
fn next_full_moon(age: f64) -> String {
    let until = if age < SYNODIC / 2.0 { SYNODIC / 2.0 - age } else { SYNODIC * 1.5 - age };
    let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH).map(|d| d.as_secs_f64()).unwrap_or(0.0) / 86400.0;
    let (_, m, d) = civil_from_days((now + until).floor() as i64);
    format!("{} {}", d, MONTHS[(m - 1).clamp(0, 11) as usize])
}

// ---------- unit helpers ----------
fn deg(c: f64, s: &Settings) -> f64 {
    if s.u_temp == 1 {
        c * 9.0 / 5.0 + 32.0
    } else {
        c
    }
}
fn ft(c: f64, s: &Settings) -> String {
    format!("{}°", deg(c, s).round() as i64)
}
fn wind_unit(s: &Settings) -> &'static str {
    ["km/h", "m/s", "mph", "kn"][s.u_wind.clamp(0, 3) as usize]
}
fn wind_val(k: f64, s: &Settings) -> String {
    match s.u_wind {
        1 => format!("{:.1}", k / 3.6),
        2 => r0(k / 1.609344),
        3 => r0(k / 1.852),
        _ => r0(k),
    }
}
fn press_unit(s: &Settings) -> &'static str {
    ["hPa", "inHg", "mmHg"][s.u_press.clamp(0, 2) as usize]
}
fn press_val(p: f64, s: &Settings) -> String {
    match s.u_press {
        1 => format!("{:.2}", p * 0.029530),
        2 => r0(p * 0.750062),
        _ => r0(p),
    }
}
fn dist_unit(s: &Settings) -> &'static str {
    if s.u_dist == 1 {
        "mi"
    } else {
        "km"
    }
}
fn dist_val(m: f64, s: &Settings) -> String {
    if s.u_dist == 1 {
        format!("{:.0}", m / 1609.344)
    } else {
        format!("{:.0}", m / 1000.0)
    }
}
fn beaufort(kmh: f64) -> usize {
    [1.0, 5.0, 11.0, 19.0, 28.0, 38.0, 49.0, 61.0, 74.0, 88.0, 102.0, 117.0].iter().filter(|&&t| kmh >= t).count()
}
fn compass16(d: f64) -> &'static str {
    const N: [&str; 16] = ["N", "NNE", "NE", "ENE", "E", "ESE", "SE", "SSE", "S", "SSW", "SW", "WSW", "W", "WNW", "NW", "NNW"];
    N[((d / 22.5).round() as usize) % 16]
}

/// WMO weather code -> (icon id, label). Icons: 0 sun, 1 moon, 2 partly, 3 night cloud, 4 cloud, 5 rain.
fn wmo(code: i32, day: bool) -> (i32, &'static str) {
    match code {
        0 => (if day { 0 } else { 1 }, "Clear"),
        1 => (if day { 0 } else { 1 }, "Mainly clear"),
        2 => (if day { 2 } else { 3 }, "Partly cloudy"),
        3 => (4, "Overcast"),
        45 | 48 => (4, "Fog"),
        51..=57 => (5, "Drizzle"),
        61..=67 => (5, "Rain"),
        71..=77 => (4, "Snow"),
        80..=82 => (5, "Showers"),
        85 | 86 => (4, "Snow showers"),
        95..=99 => (5, "Thunderstorm"),
        _ => (4, "Cloudy"),
    }
}

// ---------- path builders (100x100 tile viewbox) ----------
fn ring(pct: f64) -> (String, String) {
    let pt = |deg: f64| {
        let a = deg.to_radians();
        (50.0 + 40.0 * a.cos(), 50.0 + 40.0 * a.sin())
    };
    let (sx, sy) = pt(135.0);
    let (ex, ey) = pt(405.0);
    let track = format!("M{:.1} {:.1} A40 40 0 1 1 {:.1} {:.1}", sx, sy, ex, ey);
    let pct = pct.clamp(0.0, 1.0);
    if pct < 0.01 {
        return (track, String::new());
    }
    let sweep = 270.0 * pct;
    let (ax, ay) = pt(135.0 + sweep);
    let arc = format!("M{:.1} {:.1} A40 40 0 {} 1 {:.1} {:.1}", sx, sy, if sweep > 180.0 { 1 } else { 0 }, ax, ay);
    (track, arc)
}
fn circle_path(cx: f64, cy: f64, r: f64) -> String {
    format!("M{:.1} {:.1} A{r} {r} 0 0 1 {:.1} {:.1} A{r} {r} 0 0 1 {:.1} {:.1} Z", cx, cy - r, cx, cy + r, cx, cy - r, r = r)
}
fn dot_path(x: f64, y: f64) -> String {
    format!("M{:.1} {:.1} a6 6 0 1 0 12 0 a6 6 0 1 0 -12 0 Z", x - 6.0, y)
}

#[allow(clippy::too_many_arguments)]
fn tile(
    title: &str,
    value: String,
    unit: String,
    status: String,
    tone: i32,
    caption: String,
    shapes: (String, String, String),
    stroke: f32,
    color: Color,
) -> TileData {
    TileData {
        title: title.into(),
        value: value.into(),
        unit: unit.into(),
        status: status.into(),
        tone,
        caption: caption.into(),
        track: shapes.0.into(),
        arc: shapes.1.into(),
        shape: shapes.2.into(),
        stroke,
        color,
    }
}

struct Tiles {
    ov: Vec<TileData>,
    air: Vec<TileData>,
    sky: Vec<TileData>,
}

fn build_tiles(w: &Weather, s: &Settings) -> Tiles {
    let on = |i: usize| s.tiles.get(i).copied().unwrap_or(true);
    let c = &w.cur;
    let day = &w.days[w.today];
    let hr = &w.hours[w.idx];
    let tone = |t: i32| rgb(TONES[t as usize]);
    let mut t = Tiles { ov: vec![], air: vec![], sky: vec![] };

    if on(0) {
        let ahead = w.hours[w.idx + 3].temp - c.temp;
        let (st, tn) = if ahead > 1.5 {
            ("Warming", WARN)
        } else if ahead < -1.5 {
            ("Cooling", INFO)
        } else {
            ("Steady", INFO)
        };
        t.ov.push(tile(
            "Temperature",
            format!("{}", deg(c.temp, s).round() as i64),
            format!("°{}", if s.u_temp == 1 { "F" } else { "C" }),
            st.into(),
            tn,
            format!("Today {} to {}.", ft(day.lo, s), ft(day.hi, s)),
            {
                let (a, b) = ring((c.temp + 10.0) / 50.0);
                (a, b, String::new())
            },
            8.0,
            Color::from_rgb_u8(0xf0, 0xa4, 0x5d),
        ));
    }
    if on(1) {
        let d = c.feels - c.temp;
        let (st, tn) = if d >= 3.0 {
            ("Feels warmer", WARN)
        } else if d <= -3.0 {
            ("Feels cooler", INFO)
        } else {
            ("Matches the air", GOOD)
        };
        t.ov.push(tile(
            "Feels like",
            format!("{}", deg(c.feels, s).round() as i64),
            format!("°{}", if s.u_temp == 1 { "F" } else { "C" }),
            st.into(),
            tn,
            format!("Humidity is {}%.", r0(c.humidity)),
            {
                let (a, b) = ring((c.feels + 10.0) / 50.0);
                (a, b, String::new())
            },
            8.0,
            Color::from_rgb_u8(0xff, 0x7a, 0x9c),
        ));
    }
    if on(3) {
        let p = w.hours[w.idx..w.idx + 24].iter().map(|h| h.prob).fold(0.0, f64::max);
        let (st, tn) = if p < 20.0 {
            ("No rain expected", GOOD)
        } else if p < 60.0 {
            ("Rain possible", WARN)
        } else {
            ("Rain likely", INFO)
        };
        t.ov.push(tile(
            "Precipitation",
            r0(p),
            "% chance, next 24 h".into(),
            st.into(),
            tn,
            format!("{} mm right now.", c.precip),
            {
                let (a, b) = ring(p / 100.0);
                (a, b, String::new())
            },
            8.0,
            tone(INFO),
        ));
    }
    if on(4) {
        let (st, tn) = if c.cloud < 30.0 {
            ("Sunny", WARN)
        } else if c.cloud < 70.0 {
            ("Partly cloudy", INFO)
        } else {
            ("Cloudy", INFO)
        };
        t.ov.push(tile(
            "Clouds",
            r0(c.cloud),
            "% cover".into(),
            st.into(),
            tn,
            "Cloud cover at the moment.".into(),
            {
                let (a, b) = ring(c.cloud / 100.0);
                (a, b, String::new())
            },
            8.0,
            tone(INFO),
        ));
    }
    if on(5) {
        let (st, tn) = if c.humidity > 65.0 {
            ("Humid", INFO)
        } else if c.humidity < 35.0 {
            ("Dry", WARN)
        } else {
            ("Comfortable", GOOD)
        };
        t.ov.push(tile(
            "Humidity",
            r0(c.humidity),
            format!("% · dew point {}", ft(hr.dew, s)),
            st.into(),
            tn,
            "Higher humidity makes warm air feel heavier.".into(),
            {
                let (a, b) = ring(c.humidity / 100.0);
                (a, b, String::new())
            },
            8.0,
            tone(INFO),
        ));
    }

    if on(2) {
        let b = beaufort(c.wind);
        let th = (c.dir + 180.0).to_radians();
        let pts = [(0.0, -26.0), (10.0, 26.0), (0.0, 17.0), (-10.0, 26.0)];
        let arrow = pts
            .iter()
            .enumerate()
            .map(|(i, (x, y))| {
                let (rx, ry) = (50.0 + x * th.cos() - y * th.sin(), 50.0 + x * th.sin() + y * th.cos());
                format!("{}{:.1} {:.1}", if i == 0 { "M" } else { " L" }, rx, ry)
            })
            .collect::<String>()
            + " Z";
        t.air.push(tile(
            "Wind",
            wind_val(c.wind, s),
            format!("{} · gusts {}", wind_unit(s), wind_val(c.gust, s)),
            format!("Beaufort {}", b),
            if b >= 6 { WARN } else { GOOD },
            format!("From {} ({}°).", compass16(c.dir), r0(c.dir)),
            (circle_path(50.0, 50.0, 42.0), String::new(), arrow),
            5.0,
            tone(GOOD),
        ));
    }
    if on(9) {
        let d = c.pressure - w.hours[w.idx.saturating_sub(3)].pressure;
        let (st, cap) = if d > 1.0 {
            ("Rising", "Up over the last 3 hours.")
        } else if d < -1.0 {
            ("Falling", "Down over the last 3 hours.")
        } else {
            ("Steady", "Little change in 3 hours.")
        };
        t.air.push(tile(
            "Pressure",
            press_val(c.pressure, s),
            press_unit(s).into(),
            st.into(),
            INFO,
            cap.into(),
            {
                let (a, b) = ring((c.pressure - 980.0) / 60.0);
                (a, b, String::new())
            },
            8.0,
            tone(INFO),
        ));
    }
    if on(6) {
        match w.aqi {
            Some(a) => {
                let (st, tn, col) = if a <= 50.0 {
                    ("Good", GOOD, (0x47, 0xc9, 0x8a))
                } else if a <= 100.0 {
                    ("Moderate", WARN, (0xf2, 0xb7, 0x05))
                } else if a <= 150.0 {
                    ("Unhealthy for some", WARN, (0xff, 0x8a, 0x3d))
                } else {
                    ("Unhealthy", BAD, (0xef, 0x4b, 0x4b))
                };
                t.air.push(tile(
                    "Air quality",
                    r0(a),
                    "US AQI".into(),
                    st.into(),
                    tn,
                    w.pm25.map(|p| format!("PM2.5 at {:.1} µg/m³.", p)).unwrap_or_default(),
                    {
                        let (x, y) = ring(a / 200.0);
                        (x, y, String::new())
                    },
                    8.0,
                    rgb(col),
                ));
            }
            None => t.air.push(tile(
                "Air quality",
                "–".into(),
                "no data".into(),
                "Unavailable".into(),
                INFO,
                "No air-quality coverage here.".into(),
                {
                    let (x, y) = ring(0.0);
                    (x, y, String::new())
                },
                8.0,
                tone(INFO),
            )),
        }
    }
    if on(7) {
        let u = hr.uv;
        let (st, tn, col) = if u < 3.0 {
            ("Low", GOOD, (0x5f, 0xcf, 0x6a))
        } else if u < 6.0 {
            ("Moderate", WARN, (0xf2, 0xd2, 0x0f))
        } else if u < 8.0 {
            ("High", WARN, (0xff, 0x9f, 0x1c))
        } else if u < 11.0 {
            ("Very high", BAD, (0xef, 0x4b, 0x4b))
        } else {
            ("Extreme", BAD, (0x9b, 0x4f, 0xd0))
        };
        t.air.push(tile(
            "UV index",
            format!("{:.0}", u),
            format!("peak today {:.0}", day.uv_max),
            st.into(),
            tn,
            "Use sun protection around midday.".into(),
            {
                let (x, y) = ring(u / 11.0);
                (x, y, String::new())
            },
            8.0,
            rgb(col),
        ));
    }
    if on(8) {
        let km = hr.vis / 1000.0;
        let (st, tn) = if km >= 10.0 {
            ("Excellent", GOOD)
        } else if km >= 5.0 {
            ("Fair", WARN)
        } else {
            ("Poor", BAD)
        };
        t.air.push(tile(
            "Visibility",
            dist_val(hr.vis, s),
            dist_unit(s).into(),
            st.into(),
            tn,
            "How far you can see right now.".into(),
            {
                let (x, y) = ring(km / 20.0);
                (x, y, String::new())
            },
            8.0,
            tone(GOOD),
        ));
    }

    if on(10) {
        let (a, b) = (mins(&day.sunrise), mins(&day.sunset));
        let now = mins(&w.time);
        let u = ((now - a) / (b - a)).clamp(0.0, 1.0);
        let up = now >= a && now <= b;
        let (px, py) = (50.0 - 40.0 * (std::f64::consts::PI * u).cos(), 70.0 - 40.0 * (std::f64::consts::PI * u).sin());
        let arc = if u > 0.0 { format!("M10 70 A40 40 0 0 1 {:.1} {:.1}", px, py) } else { String::new() };
        let len = (b - a) as i64;
        t.sky.push(tile(
            "Sun",
            format!("{}h {}m", len / 60, len % 60),
            "of daylight".into(),
            if up { "Sun is up" } else { "Sun is down" }.into(),
            INFO,
            format!("Sunrise {} · sunset {}", day.sunrise.get(11..16).unwrap_or("--"), day.sunset.get(11..16).unwrap_or("--")),
            ("M10 70 A40 40 0 0 1 90 70".into(), arc, dot_path(px, py)),
            5.0,
            Color::from_rgb_u8(0xff, 0x9f, 0x1c),
        ));
    }
    if on(11) {
        let age = moon_age();
        let p = age / SYNODIC;
        let f = (1.0 - (2.0 * std::f64::consts::PI * p).cos()) / 2.0;
        let rx = 40.0 * (2.0 * f - 1.0).abs();
        let gibbous = f > 0.5;
        let lit = if p < 0.5 {
            format!("M50 10 A40 40 0 0 1 50 90 A{:.1} 40 0 0 {} 50 10 Z", rx, if gibbous { 1 } else { 0 })
        } else {
            format!("M50 10 A40 40 0 0 0 50 90 A{:.1} 40 0 0 {} 50 10 Z", rx, if gibbous { 0 } else { 1 })
        };
        let name = match p {
            x if x < 0.03 || x > 0.97 => "New moon",
            x if x < 0.22 => "Waxing crescent",
            x if x < 0.28 => "First quarter",
            x if x < 0.47 => "Waxing gibbous",
            x if x < 0.53 => "Full moon",
            x if x < 0.72 => "Waning gibbous",
            x if x < 0.78 => "Last quarter",
            _ => "Waning crescent",
        };
        let nf = next_full_moon(age);
        t.sky.push(tile(
            "Moon phase",
            r0(f * 100.0),
            "% lit".into(),
            name.into(),
            INFO,
            format!("Next full moon {}.", nf),
            (circle_path(50.0, 50.0, 40.0), String::new(), lit),
            1.5,
            Color::from_rgb_u8(0xff, 0xcf, 0x5a),
        ));
    }
    t
}

/// `vb` is the chart path's viewbox height; its width is always 1200.
fn hour_cols(w: &Weather, s: &Settings, vb: f64) -> (Vec<HourData>, String, String, f32, Vec<MarkData>) {
    let i = w.idx as i64;
    // Now, then a constant 2 h step for 24 h.
    let idxs: Vec<i64> = (0..12).map(|k| i + 2 * k).collect();
    let cols: Vec<&Hour> = idxs.iter().map(|&k| &w.hours[k as usize]).collect();
    let mut hours: Vec<HourData> = cols
        .iter()
        .enumerate()
        .map(|(n, h)| HourData {
            label: if n == 0 { "Now".into() } else { h.time.get(11..16).unwrap_or("").into() },
            icon: wmo(h.code, h.is_day).0,
            temp: ft(h.temp, s).into(),
            precip: format!("{}%", r0(h.prob)).into(),
            now: n == 0,
            // chance of rain as 0..1; drives the bar height
            op: (h.prob.round() / 100.0).clamp(0.0, 1.0) as f32,
            y: 0.0,
            desc: wmo(h.code, h.is_day).1.into(),
        })
        .collect();
    let temps: Vec<f64> = cols.iter().map(|h| h.temp).collect();
    let mn = temps.iter().cloned().fold(f64::MAX, f64::min) - 4.0;
    let mx = temps.iter().cloned().fold(f64::MIN, f64::max) + 5.0;
    let ys: Vec<f64> = temps.iter().map(|t| vb * (1.0 - (t - mn) / (mx - mn))).collect();
    for (h, y) in hours.iter_mut().zip(&ys) {
        h.y = (*y / vb) as f32;
    }
    let mut pts: Vec<(f64, f64)> = vec![(0.0, ys[0])];
    pts.extend(ys.iter().enumerate().map(|(n, y)| ((n as f64 + 0.5) * 100.0, *y)));
    pts.push((1200.0, ys[11]));
    let mut line = format!("M{:.1} {:.1}", pts[0].0, pts[0].1);
    for n in 0..pts.len() - 1 {
        let a = pts[n.saturating_sub(1)];
        let b = pts[n];
        let c = pts[n + 1];
        let e = pts[(n + 2).min(pts.len() - 1)];
        line += &format!(
            " C{:.1} {:.1} {:.1} {:.1} {:.1} {:.1}",
            b.0 + (c.0 - a.0) / 6.0,
            b.1 + (c.1 - a.1) / 6.0,
            c.0 - (e.0 - b.0) / 6.0,
            c.1 - (e.1 - b.1) / 6.0,
            c.0,
            c.1
        );
    }
    let area = format!("{line} L1200 {vb:.1} L0 {vb:.1} Z");

    // sunrise/sunset markers, placed between hour columns by absolute time
    let (cur_h, cur_m) = hm(&w.time);
    let day_start = (i * 60) as f64 - (cur_h * 60) as f64;
    let _ = cur_m;
    let col_min: Vec<f64> = idxs.iter().map(|k| (*k * 60) as f64).collect();
    let place = |abs: f64| -> Option<f32> {
        for n in 0..11 {
            if abs >= col_min[n] && abs <= col_min[n + 1] {
                return Some(((n as f64 + 0.5 + (abs - col_min[n]) / (col_min[n + 1] - col_min[n])) / 12.0) as f32);
            }
        }
        None
    };
    let mut marks = vec![];
    for (txt, ev) in [("Sunrise", &w.days[w.today].sunrise), ("Sunset", &w.days[w.today].sunset)] {
        for add in [0.0, 1440.0] {
            if let Some(x) = place(day_start + mins(ev) + add) {
                marks.push(MarkData { x, text: format!("{} {}", txt, ev.get(11..16).unwrap_or("")).into() });
                break;
            }
        }
    }
    (hours, line, area, (ys[0] / vb) as f32, marks)
}

pub fn apply(ui: &AppWindow, st: &State) {
    let s = &st.s;
    ui.global::<Theme>().set_mode(s.theme);
    ui.set_theme_mode(s.theme);
    ui.set_u_temp(s.u_temp);
    ui.set_u_wind(s.u_wind);
    ui.set_u_press(s.u_press);
    ui.set_u_dist(s.u_dist);
    ui.set_refresh_index(s.refresh);
    ui.set_cache_index(s.cache);
    ui.set_tray_on(s.tray);
    ui.set_autostart(crate::system::autostart_enabled());
    ui.set_anim(s.anim);
    ui.set_zoom_pct(s.zoom);
    ui.set_show_hourly(s.tiles[12]);
    ui.set_show_daily(s.tiles[13]);
    ui.set_status_msg(st.status.clone().into());
    ui.set_searching(st.searching);
    ui.set_flags(model(FLAG_NAMES.iter().enumerate().map(|(i, n)| FlagData { name: (*n).into(), on: s.tiles[i] }).collect()));
    ui.set_hits(model(st.hits.iter().map(|h| HitData { name: h.name.clone().into(), region: h.region.clone().into() }).collect()));
    let place = &s.places[s.selected];
    ui.set_city_name(place.name.clone().into());
    ui.set_city_region(place.region.clone().into());
    ui.set_places(model(
        s.places
            .iter()
            .enumerate()
            .map(|(i, p)| {
                let now = st.place_now.get(i).copied().flatten();
                PlaceData {
                    name: p.name.clone().into(),
                    region: p.region.clone().into(),
                    temp: now.map(|n| ft(n.temp, s)).unwrap_or_else(|| "–".into()).into(),
                    icon: now.map(|n| wmo(n.code, n.is_day).0).unwrap_or(4),
                    selected: i == s.selected,
                }
            })
            .collect(),
    ));

    let Some(w) = &st.weather else { return };
    let c = &w.cur;
    let (icon, name) = wmo(c.code, c.is_day);
    let today = &w.days[w.today];
    ui.set_temp(format!("{}", deg(c.temp, s).round() as i64).into());
    ui.set_temp_unit(if s.u_temp == 1 { "F" } else { "C" }.into());
    ui.set_cond(name.into());
    ui.set_tray_tip(format!("{} {} · {}", place.name, ft(c.temp, s), name).into());
    ui.set_cond_icon(icon);
    ui.set_hi_lo(format!("High {} · Low {}", ft(today.hi, s), ft(today.lo, s)).into());
    ui.set_feels_text(format!("Feels like {}", ft(c.feels, s)).into());
    ui.set_updated(format!("Updated {}{}", w.time.get(11..16).unwrap_or(""), if st.cached { " · cached" } else { "" }).into());

    let days: Vec<&Day> = w.days.iter().take(6).collect();
    let mn = days.iter().map(|d| d.lo).fold(f64::MAX, f64::min) - 1.0;
    let mx = days.iter().map(|d| d.hi).fold(f64::MIN, f64::max) + 1.0;
    ui.set_days(model(
        days.iter()
            .enumerate()
            .map(|(i, d)| DayData {
                name: weekday(&d.date).into(),
                date: d.date.get(8..10).unwrap_or("").into(),
                icon: wmo(d.code, true).0,
                hi: ft(d.hi, s).into(),
                lo: ft(d.lo, s).into(),
                today: i == w.today,
                from: ((d.lo - mn) / (mx - mn)) as f32,
                to: ((d.hi - mn) / (mx - mn)) as f32,
            })
            .collect(),
    ));

    let vb = (st.aspect * 1200.0).clamp(120.0, 600.0);
    let (hours, line, area, now_y, marks) = hour_cols(w, s, vb);
    ui.set_chart_vb(vb as f32);
    ui.set_hours(model(hours));
    ui.set_chart_line(line.into());
    ui.set_chart_area(area.into());
    ui.set_now_y(now_y);
    ui.set_marks(model(marks));

    let t = build_tiles(w, s);
    ui.set_tiles_ov(model(t.ov));
    ui.set_tiles_air(model(t.air));
    ui.set_tiles_sky(model(t.sky));
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calendar_round_trip_and_weekday() {
        let d = days_from_civil(2026, 10, 25);
        assert_eq!(civil_from_days(d), (2026, 10, 25));
        assert_eq!(weekday("2026-10-25"), "Sun");
        assert_eq!(weekday("2000-01-06"), "Thu");
    }

    #[test]
    fn beaufort_and_compass() {
        assert_eq!(beaufort(0.0), 0);
        assert_eq!(beaufort(12.0), 3);
        assert_eq!(beaufort(120.0), 12);
        assert_eq!(compass16(0.0), "N");
        assert_eq!(compass16(343.0), "NNW");
        assert_eq!(compass16(359.0), "N");
    }

    #[test]
    fn unit_conversions() {
        let mut s = Settings::default();
        assert_eq!(wind_val(36.0, &s), "36");
        s.u_wind = 1;
        assert_eq!(wind_val(36.0, &s), "10.0");
        s.u_temp = 1;
        assert_eq!(ft(0.0, &s), "32°");
        s.u_press = 1;
        assert_eq!(press_val(1013.25, &s), "29.92");
    }

    #[test]
    fn ring_paths() {
        let (track, arc) = ring(0.0);
        assert!(!track.is_empty() && arc.is_empty());
        let (_, arc) = ring(1.0);
        assert!(arc.starts_with('M'));
    }

    #[test]
    fn time_parsing() {
        assert_eq!(hm("2026-10-25T10:44"), (10, 44));
        assert_eq!(mins("2026-10-25T05:51"), 351.0);
    }
}
