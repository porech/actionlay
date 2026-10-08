//! Measurement preferences are independent of the interface language.
use actionlay_layout::model::Units;
use actionlay_telemetry::units::UnitSystem;
use std::sync::{
    Mutex,
    atomic::{AtomicBool, Ordering},
};
use std::time::Duration;
#[cfg(not(target_arch = "wasm32"))]
use std::time::Instant;
#[cfg(target_arch = "wasm32")]
use web_time::Instant;
static IMPERIAL: AtomicBool = AtomicBool::new(false);
static LAST: Mutex<Option<(Option<Units>, Instant)>> = Mutex::new(None);
pub fn current() -> UnitSystem {
    if IMPERIAL.load(Ordering::Relaxed) {
        UnitSystem::Imperial
    } else {
        UnitSystem::Metric
    }
}
/// Refresh system defaults without resolving them into the saved preference.
pub fn configure(choice: Option<Units>) -> bool {
    let mut last = LAST.lock().unwrap_or_else(|e| e.into_inner());
    if last
        .as_ref()
        .is_some_and(|(old, at)| *old == choice && at.elapsed() < Duration::from_secs(1))
    {
        return false;
    }
    *last = Some((choice, Instant::now()));
    let imperial = match choice.unwrap_or_default() {
        Units::Metric => false,
        Units::Imperial => true,
        Units::Default => system_imperial(),
    };
    IMPERIAL.swap(imperial, Ordering::Relaxed) != imperial
}
#[cfg(target_os = "macos")]
fn system_imperial() -> bool {
    use std::ffi::{c_char, c_void};
    #[link(name = "CoreFoundation", kind = "framework")]
    unsafe extern "C" {
        fn CFLocaleCopyCurrent() -> *const c_void;
        fn CFLocaleGetValue(locale: *const c_void, key: *const c_void) -> *const c_void;
        fn CFStringGetCString(
            string: *const c_void,
            buffer: *mut c_char,
            size: isize,
            encoding: u32,
        ) -> bool;
        fn CFRelease(object: *const c_void);
        static kCFLocaleMeasurementSystem: *const c_void;
    }
    // Copy/Create ownership belongs here; GetValue's borrowed string belongs to the locale.
    unsafe {
        let locale = CFLocaleCopyCurrent();
        if locale.is_null() {
            return false;
        }
        let value = CFLocaleGetValue(locale, kCFLocaleMeasurementSystem);
        let mut bytes = [0i8; 64];
        let ok = !value.is_null() && CFStringGetCString(value, bytes.as_mut_ptr(), 64, 0x08000100);
        let imperial = ok && std::ffi::CStr::from_ptr(bytes.as_ptr()).to_bytes() == b"U.S.";
        CFRelease(locale);
        imperial
    }
}
#[cfg(target_os = "windows")]
fn system_imperial() -> bool {
    #[link(name = "kernel32")]
    unsafe extern "system" {
        fn GetLocaleInfoEx(name: *const u16, kind: u32, data: *mut u16, size: i32) -> i32;
    }
    let mut data = [0u16; 4];
    unsafe {
        GetLocaleInfoEx(std::ptr::null(), 0x0000000d, data.as_mut_ptr(), 4) > 0
            && data[0] == b'1' as u16
    }
}
#[cfg(not(any(target_os = "macos", target_os = "windows", target_arch = "wasm32")))]
fn system_imperial() -> bool {
    if let Ok(result) = std::process::Command::new("locale")
        .args(["-k", "LC_MEASUREMENT"])
        .output()
        && result.status.success()
        && let Some(value) = String::from_utf8_lossy(&result.stdout)
            .lines()
            .find_map(|line| line.strip_prefix("measurement="))
    {
        return value.trim_matches('"') == "2";
    }
    // Minimal installations may omit locale(1). LC_MEASUREMENT takes precedence over LANG.
    let locale = ["LC_ALL", "LC_MEASUREMENT", "LANG"]
        .iter()
        .find_map(|key| std::env::var(key).ok().filter(|v| !v.is_empty()))
        .unwrap_or_default();
    matches!(
        locale
            .split(['_', '-'])
            .nth(1)
            .unwrap_or("")
            .split(['.', '@'])
            .next(),
        Some("US" | "LR" | "MM")
    )
}

#[cfg(target_arch = "wasm32")]
fn system_imperial() -> bool {
    false
}
