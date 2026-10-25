//! Windows integration: start with Windows, and a single running instance.
use windows::core::{w, PCWSTR};
use windows::Win32::Foundation::{GetLastError, ERROR_ALREADY_EXISTS, HANDLE};
use windows::Win32::System::Registry::{
    RegCloseKey, RegDeleteValueW, RegOpenKeyExW, RegQueryValueExW, RegSetValueExW, HKEY, HKEY_CURRENT_USER, KEY_READ, KEY_WRITE, REG_SZ,
};
use windows::Win32::System::Threading::{CreateEventW, CreateMutexW, SetEvent, WaitForSingleObject, INFINITE};

const RUN_KEY: PCWSTR = w!("Software\\Microsoft\\Windows\\CurrentVersion\\Run");
const RUN_VALUE: PCWSTR = w!("Skylight");

fn wide(s: &str) -> Vec<u16> {
    s.encode_utf16().chain(std::iter::once(0)).collect()
}

pub fn autostart_enabled() -> bool {
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, KEY_READ, &mut key).is_err() {
            return false;
        }
        let found = RegQueryValueExW(key, RUN_VALUE, None, None, None, None).is_ok();
        let _ = RegCloseKey(key);
        found
    }
}

/// Add or remove the Run entry. With `hidden`, Windows starts the app with `--minimized`
/// so it goes straight to the tray.
pub fn set_autostart(on: bool, hidden: bool) {
    unsafe {
        let mut key = HKEY::default();
        if RegOpenKeyExW(HKEY_CURRENT_USER, RUN_KEY, None, KEY_READ | KEY_WRITE, &mut key).is_err() {
            return;
        }
        if on {
            if let Ok(exe) = std::env::current_exe() {
                let value = wide(&format!("\"{}\"{}", exe.display(), if hidden { " --minimized" } else { "" }));
                let bytes = std::slice::from_raw_parts(value.as_ptr() as *const u8, value.len() * 2);
                let _ = RegSetValueExW(key, RUN_VALUE, None, REG_SZ, Some(bytes));
            }
        } else {
            let _ = RegDeleteValueW(key, RUN_VALUE);
        }
        let _ = RegCloseKey(key);
    }
}

/// Keep the registered path current if the exe was moved or rebuilt elsewhere.
pub fn refresh_autostart(hidden: bool) {
    if autostart_enabled() {
        set_autostart(true, hidden);
    }
}

/// Handle of the "show yourself" event, as a plain integer so it can move to a thread.
pub struct ShowEvent(isize);

/// Returns `None` when Skylight is already running; in that case the running copy is told
/// to show its window. The mutex handle is intentionally never closed: it lives as long as
/// the process and releases when the process exits.
pub fn claim_single_instance() -> Option<ShowEvent> {
    unsafe {
        let event = CreateEventW(None, false, false, w!("Local\\Skylight.Show")).ok()?;
        let mutex = CreateMutexW(None, true, w!("Local\\Skylight.Instance"));
        let already = GetLastError() == ERROR_ALREADY_EXISTS;
        if mutex.is_err() {
            return Some(ShowEvent(event.0 as isize));
        }
        if already {
            let _ = SetEvent(event);
            return None;
        }
        Some(ShowEvent(event.0 as isize))
    }
}

/// Run `on_show` on a background thread each time another launch asks this one to show.
pub fn listen_for_show(event: ShowEvent, on_show: impl Fn() + Send + 'static) {
    std::thread::spawn(move || {
        let handle = HANDLE(event.0 as *mut _);
        loop {
            unsafe {
                WaitForSingleObject(handle, INFINITE);
            }
            on_show();
        }
    });
}
