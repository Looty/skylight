#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

mod api;
mod cache;
mod settings;
mod system;
mod view;

use api::{PlaceNow, Weather};
use settings::{Place, Settings, CACHE_MINUTES, REFRESH_MINUTES, ZOOM_MAX, ZOOM_MIN, ZOOM_STEP};
use slint::{ComponentHandle, Timer, TimerMode};
use std::cell::{Cell, RefCell};
use std::rc::Rc;
use std::sync::{LazyLock, Mutex};
use std::time::Duration;

slint::include_modules!();

pub struct State {
    pub s: Settings,
    pub weather: Option<Weather>,
    pub place_now: Vec<Option<PlaceNow>>,
    pub hits: Vec<Place>,
    pub searching: bool,
    pub status: String,
    /// True when the weather on screen came from the cache and is over a minute old.
    pub cached: bool,
    /// Height / width of the hourly chart box, reported by the UI so the path matches it.
    pub aspect: f64,
}

static STATE: LazyLock<Mutex<State>> = LazyLock::new(|| {
    Mutex::new(State {
        s: Settings::load(),
        weather: None,
        place_now: vec![],
        hits: vec![],
        searching: false,
        status: "Loading…".into(),
        cached: false,
        aspect: 1.0 / 6.0,
    })
});

fn with_state<R>(f: impl FnOnce(&mut State) -> R) -> R {
    f(&mut STATE.lock().unwrap())
}

thread_local! {
    /// The tray icon lives on the UI thread; kept here so `redraw` can update its tooltip.
    static TRAY: RefCell<Option<AppTray>> = const { RefCell::new(None) };
    /// The window's own (DPI) scale factor, and the zoom we last applied on top of it.
    static NATIVE_SCALE: Cell<f32> = const { Cell::new(0.0) };
    static APPLIED_ZOOM: Cell<f32> = const { Cell::new(1.0) };
}

/// Scale the whole interface by the saved zoom. Safe to call often: it only acts when the
/// window's scale factor differs from native * zoom (first call, or the OS changed it, e.g. on
/// a monitor change).
fn apply_zoom(ui: &AppWindow) {
    let zoom = with_state(|st| st.s.zoom) as f32 / 100.0;
    let cur = ui.window().scale_factor();
    let close = |a: f32, b: f32| (a - b).abs() < 0.001;
    if NATIVE_SCALE.get() <= 0.0 || !close(cur, NATIVE_SCALE.get() * APPLIED_ZOOM.get()) {
        // First call, or the OS changed the scale factor under us: `cur` is the native value.
        NATIVE_SCALE.set(cur);
        APPLIED_ZOOM.set(1.0);
    }
    let target = NATIVE_SCALE.get() * zoom;
    if !close(cur, target) {
        // Slint keeps the logical size on a scale change, so tell it the OS window's real
        // (unchanged) physical size in the new logical units, or the renderer's buffer no longer
        // matches the window.
        let size = ui.window().size();
        ui.window().dispatch_event(slint::platform::WindowEvent::ScaleFactorChanged { scale_factor: target });
        ui.window().dispatch_event(slint::platform::WindowEvent::Resized {
            size: slint::LogicalSize::new(size.width as f32 / target, size.height as f32 / target),
        });
    }
    APPLIED_ZOOM.set(zoom);
}

fn redraw(ui: &AppWindow) {
    {
        let mut st = STATE.lock().unwrap();
        if let Some(w) = st.weather.as_mut() {
            w.roll();
        }
        view::apply(ui, &st);
    }
    TRAY.with(|t| {
        if let Some(t) = t.borrow().as_ref() {
            t.set_tip(ui.get_tray_tip());
        }
    });
}

/// Fresh cache entry, else network (then cached), else any stale entry when offline.
/// Returns the raw JSON, its age in seconds, and the network error if it fell back.
fn load(
    key: &str,
    ttl: u64,
    force: bool,
    net: impl FnOnce() -> Result<serde_json::Value, String>,
) -> Result<(serde_json::Value, u64, Option<String>), String> {
    if !force && ttl > 0 {
        if let Some((v, age)) = cache::get(key, Some(ttl)) {
            return Ok((v, age, None));
        }
    }
    match net() {
        Ok(v) => {
            cache::put(key, v.clone());
            Ok((v, 0, None))
        }
        Err(e) => match cache::get(key, None) {
            Some((v, age)) => Ok((v, age, Some(e))),
            None => Err(e),
        },
    }
}

/// Fetch on a worker thread; the UI thread only draws.
/// `force` skips the cache (manual refresh button).
fn fetch(ui: slint::Weak<AppWindow>, force: bool) {
    std::thread::spawn(move || {
        let (place, places, ttl) =
            with_state(|st| (st.s.places[st.s.selected].clone(), st.s.places.clone(), CACHE_MINUTES[st.s.cache.clamp(0, 4) as usize] * 60));
        let coords = |p: &Place| format!("{},{}", p.lat, p.lon);
        let wkey = format!("w:{}", coords(&place));
        let pkey = format!("p:{}", places.iter().map(coords).collect::<Vec<_>>().join(";"));
        let w = load(&wkey, ttl, force, || api::fetch_weather_raw(&place))
            .and_then(|(raw, age, err)| api::parse_weather(&raw).map(|w| (w, age, err)));
        let now = load(&pkey, ttl, force, || api::fetch_places_raw(&places))
            .map(|(raw, _, _)| api::parse_places(&raw, places.len()))
            .unwrap_or_else(|_| vec![None; places.len()]);
        with_state(|st| {
            match w {
                Ok((mut w, age, err)) => {
                    // Cached data was current `age` seconds ago, not now.
                    w.base = std::time::Instant::now().checked_sub(Duration::from_secs(age)).unwrap_or(w.base);
                    st.weather = Some(w);
                    st.cached = age > 60;
                    st.status = match err {
                        Some(_) => format!("Offline · showing data from {} min ago", age / 60),
                        None => String::new(),
                    };
                }
                Err(e) => st.status = format!("Offline: {}", e.chars().take(60).collect::<String>()),
            }
            st.place_now = now;
        });
        let _ = ui.upgrade_in_event_loop(|ui| redraw(&ui));
    });
}

fn start_timer(timer: &Timer, ui: slint::Weak<AppWindow>) {
    let mins = with_state(|st| REFRESH_MINUTES[st.s.refresh.clamp(0, 3) as usize]);
    timer.start(TimerMode::Repeated, Duration::from_secs(mins * 60), move || {
        // Skip while hidden or minimised; showing the window again refreshes.
        if ui.upgrade().is_some_and(|u| u.window().is_visible() && !u.window().is_minimized()) {
            fetch(ui.clone(), false);
        }
    });
}

fn main() -> Result<(), slint::PlatformError> {
    // A second launch asks the running copy to show itself, then exits.
    let Some(show_event) = system::claim_single_instance() else { return Ok(()) };
    let ui = AppWindow::new()?;
    let timer = Rc::new(Timer::default());

    // Settings changes: mutate state, persist, redraw. Only place changes refetch.
    macro_rules! change {
        ($ui:expr, |$st:ident| $body:block) => {{
            with_state(|$st| {
                $body;
                $st.s.save();
            });
            redraw(&$ui);
        }};
    }

    let w = ui.as_weak();
    ui.on_set_theme(move |v| {
        change!(w.unwrap(), |st| {
            st.s.theme = v;
        })
    });
    let w = ui.as_weak();
    ui.on_set_unit(move |k, v| {
        change!(w.unwrap(), |st| {
            match k {
                0 => st.s.u_temp = v,
                1 => st.s.u_wind = v,
                2 => st.s.u_press = v,
                _ => st.s.u_dist = v,
            }
        })
    });
    let w = ui.as_weak();
    ui.on_toggle_flag(move |i| {
        change!(w.unwrap(), |st| {
            let i = i as usize;
            st.s.tiles[i] = !st.s.tiles[i];
        })
    });
    let (w, t) = (ui.as_weak(), timer.clone());
    ui.on_set_refresh(move |v| {
        change!(w.unwrap(), |st| {
            st.s.refresh = v;
        });
        start_timer(&t, w.clone());
    });

    let w = ui.as_weak();
    ui.on_select_place(move |i| {
        change!(w.unwrap(), |st| {
            st.s.selected = i as usize;
            st.status = "Loading…".into();
        });
        fetch(w.clone(), false);
    });
    let w = ui.as_weak();
    ui.on_remove_place(move |i| {
        let i = i as usize;
        let removed = with_state(|st| {
            if st.s.places.len() <= 1 {
                return false;
            }
            st.s.places.remove(i);
            if i < st.s.selected {
                st.s.selected -= 1;
            } else if i == st.s.selected {
                st.s.selected = 0;
                st.status = "Loading…".into();
            }
            st.s.save();
            true
        });
        if removed {
            redraw(&w.unwrap());
            fetch(w.clone(), false);
        }
    });
    let w = ui.as_weak();
    ui.on_add_hit(move |i| {
        with_state(|st| {
            if let Some(h) = st.hits.get(i as usize).cloned() {
                let same = |p: &Place| (p.lat - h.lat).abs() < 0.01 && (p.lon - h.lon).abs() < 0.01;
                if !st.s.places.iter().any(same) {
                    st.s.places.push(h.clone());
                }
                st.s.selected = st.s.places.iter().position(same).unwrap_or(0);
                st.hits.clear();
                st.status = "Loading…".into();
                st.s.save();
            }
        });
        redraw(&w.unwrap());
        fetch(w.clone(), false);
    });
    let w = ui.as_weak();
    ui.on_search(move |q| {
        let q = q.to_string();
        if q.trim().len() < 2 {
            return;
        }
        with_state(|st| st.searching = true);
        redraw(&w.unwrap());
        let w = w.clone();
        std::thread::spawn(move || {
            let hits = api::geocode(q.trim()).unwrap_or_default();
            with_state(|st| {
                st.hits = hits;
                st.searching = false;
            });
            let _ = w.upgrade_in_event_loop(|ui| redraw(&ui));
        });
    });
    let w = ui.as_weak();
    ui.on_refresh(move || {
        with_state(|st| st.status = "Refreshing…".into());
        redraw(&w.unwrap());
        fetch(w.clone(), true);
    });
    let w = ui.as_weak();
    ui.on_chart_aspect(move |a| {
        let changed = with_state(|st| {
            let moved = (st.aspect - a as f64).abs() > 0.005;
            if moved {
                st.aspect = a as f64;
            }
            moved
        });
        if changed {
            redraw(&w.unwrap());
        }
    });
    let w = ui.as_weak();
    ui.on_set_cache(move |v| {
        change!(w.unwrap(), |st| {
            st.s.cache = v;
        })
    });
    let w = ui.as_weak();
    ui.on_zoom(move |d| {
        change!(w.unwrap(), |st| {
            st.s.zoom = if d == 0 { 100 } else { (st.s.zoom + d * ZOOM_STEP).clamp(ZOOM_MIN, ZOOM_MAX) };
        });
        apply_zoom(&w.unwrap());
    });
    let w = ui.as_weak();
    ui.on_set_anim(move |v| {
        change!(w.unwrap(), |st| {
            st.s.anim = v;
        })
    });

    // Remember the window size between runs.
    if let Some([w, h]) = with_state(|st| st.s.win) {
        ui.window().set_size(slint::PhysicalSize::new(w, h));
    }
    let wk = ui.as_weak();
    ui.window().on_close_requested(move || {
        if let Some(ui) = wk.upgrade() {
            let size = ui.window().size();
            // A minimised window reports a tiny size; don't save that.
            if size.width >= 400 && size.height >= 300 {
                with_state(|st| {
                    st.s.win = Some([size.width, size.height]);
                    st.s.save();
                });
            }
        }
        // With the tray on, closing hides the window. Otherwise it ends the program.
        if !with_state(|st| st.s.tray) {
            let _ = slint::quit_event_loop();
        }
        slint::CloseRequestResponse::HideWindow
    });

    // Dev aids for screenshots: SKYLIGHT_FONT=<family>, SKYLIGHT_VIEW=air | sky | settings0..settings4
    if let Ok(f) = std::env::var("SKYLIGHT_FONT") {
        ui.set_font_family(f.into());
    }
    if let Ok(v) = std::env::var("SKYLIGHT_VIEW") {
        match v.as_str() {
            "air" => ui.set_tab(1),
            "sky" => ui.set_tab(2),
            s if s.starts_with("settings") => {
                ui.set_settings_open(true);
                ui.set_nav(s.trim_start_matches("settings").parse().unwrap_or(0));
            }
            _ => {}
        }
    }

    // Icon animation: 4 frames a second, and no timer work at all when animations are off.
    let anim_timer = Timer::default();
    {
        let w = ui.as_weak();
        let mut frame: u32 = 0;
        anim_timer.start(TimerMode::Repeated, Duration::from_millis(250), move || {
            if !with_state(|st| st.s.anim) {
                return;
            }
            let Some(ui) = w.upgrade() else { return };
            if !ui.window().is_visible() || ui.window().is_minimized() {
                return;
            }
            frame = (frame + 1) % 8;
            let a = ui.global::<Anim>();
            // pulse: 0, .25, .5, .75, 1, .75, .5, .25 (2 s loop); fall: 0, .25, .5, .75 (1 s loop)
            a.set_pulse((4 - (frame as i32 - 4).abs()) as f32 / 4.0);
            a.set_fall((frame % 4) as f32 / 4.0);
        });
    }

    // Keep "now" on the current hour between fetches (redraw rolls it forward).
    let clock_timer = Timer::default();
    {
        let w = ui.as_weak();
        clock_timer.start(TimerMode::Repeated, Duration::from_secs(30), move || {
            let Some(ui) = w.upgrade() else { return };
            if ui.window().is_visible() && !ui.window().is_minimized() {
                apply_zoom(&ui);
                redraw(&ui);
            }
        });
    }

    // Tray icon, start-with-Windows and single-instance wiring.
    let tray = AppTray::new()?;
    {
        let w = ui.as_weak();
        tray.on_show_window(move || show_window(&w));
        let w = ui.as_weak();
        tray.on_refresh(move || {
            with_state(|st| st.status = "Refreshing…".into());
            redraw(&w.unwrap());
            fetch(w.clone(), true);
        });
        tray.on_quit(|| {
            let _ = slint::quit_event_loop();
        });
        if with_state(|st| st.s.tray) {
            tray.show()?;
        }
    }
    TRAY.with(|t| *t.borrow_mut() = Some(tray));
    let w = ui.as_weak();
    ui.on_set_autostart(move |on| {
        let hidden = with_state(|st| st.s.tray);
        system::set_autostart(on, hidden);
        redraw(&w.unwrap());
    });
    let w = ui.as_weak();
    ui.on_set_tray(move |on| {
        change!(w.unwrap(), |st| {
            st.s.tray = on;
        });
        TRAY.with(|t| {
            if let Some(t) = t.borrow().as_ref() {
                let _ = if on { t.show() } else { t.hide() };
            }
        });
        system::refresh_autostart(on);
    });
    system::refresh_autostart(with_state(|st| st.s.tray));
    let w = ui.as_weak();
    system::listen_for_show(show_event, move || {
        let w = w.clone();
        let _ = w.clone().upgrade_in_event_loop(move |_| show_window(&w));
    });

    redraw(&ui);
    fetch(ui.as_weak(), false);
    start_timer(&timer, ui.as_weak());
    // Started by Windows at sign-in: stay in the tray until the user opens the window.
    let hidden_start = std::env::args().any(|a| a == "--minimized") && with_state(|st| st.s.tray);
    if !hidden_start {
        ui.show()?;
        apply_zoom(&ui);
    }
    // The OS may report its own scale factor right after the window appears; apply ours on top.
    let zoom_timer = Timer::default();
    {
        let w = ui.as_weak();
        zoom_timer.start(TimerMode::SingleShot, Duration::from_millis(300), move || {
            if let Some(ui) = w.upgrade() {
                if ui.window().is_visible() {
                    apply_zoom(&ui);
                }
            }
        });
    }
    slint::run_event_loop_until_quit()
}

/// Bring the window back from the tray or the taskbar, and refresh if the data is old.
fn show_window(w: &slint::Weak<AppWindow>) {
    let Some(ui) = w.upgrade() else { return };
    let _ = ui.show();
    apply_zoom(&ui);
    ui.window().set_minimized(false);
    fetch(w.clone(), false);
}
