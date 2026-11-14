//! Diagnostics: an optional trace and a crash log.
//!
//! When Finder launches the app there is no terminal in sight and it is hard
//! to tell which file it received. With `CHECHEVIEWER_DEBUG=1` it writes what
//! it is opening to stderr:
//!
//! ```sh
//! open -a CheCheViewer.app foto.jpg --env CHECHEVIEWER_DEBUG=1 --stderr /tmp/cv.log
//! ```
//!
//! Crashes are always written to [`error_log_path`], for the same reason: a
//! panic or an AppKit exception otherwise vanishes with the process.

use std::io::Write;
use std::path::{Path, PathBuf};
use std::sync::OnceLock;

static ENABLED: OnceLock<bool> = OnceLock::new();

pub fn enabled() -> bool {
    *ENABLED.get_or_init(|| std::env::var_os("CHECHEVIEWER_DEBUG").is_some())
}

/// Writes a diagnostic line if enabled.
#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {
        if $crate::diag::enabled() {
            eprintln!("checheviewer: {}", format_args!($($arg)*));
        }
    };
}

/// `~/Library/Logs/CheCheViewer/error.log`, the standard macOS place for app
/// logs; Console.app lists it under "Log Reports".
pub fn error_log_path() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Logs/CheCheViewer/error.log"))
}

/// Sends Rust panics and uncaught Objective-C exceptions to the error log.
///
/// Call it first thing in `main`. Panics are logged even when they only kill
/// a decode worker and the app keeps running. Exceptions are what AppKit
/// raises from inside its event loop; the process still aborts afterwards,
/// but now leaves a trace.
pub fn install_crash_log() {
    let default_hook = std::panic::take_hook();
    std::panic::set_hook(Box::new(move |info| {
        let thread = std::thread::current();
        let backtrace = std::backtrace::Backtrace::force_capture();
        log_crash(&format!(
            "panic in thread '{}': {info}\n{backtrace}",
            thread.name().unwrap_or("?")
        ));
        default_hook(info);
    }));

    // SAFETY: the handler has the signature Foundation expects,
    // `void (*)(NSException *)`, and lives for the whole program.
    #[cfg(target_os = "macos")]
    unsafe {
        objc2_foundation::NSSetUncaughtExceptionHandler(
            on_uncaught_exception as *const () as *mut std::ffi::c_void,
        )
    };
}

#[cfg(target_os = "macos")]
extern "C" fn on_uncaught_exception(exception: *mut objc2_foundation::NSException) {
    // SAFETY: Foundation passes the exception being raised, alive during the
    // call.
    let Some(exception) = (unsafe { exception.as_ref() }) else {
        return;
    };
    let reason = exception.reason().map(|r| r.to_string()).unwrap_or_default();
    let stack: Vec<String> = exception.callStackSymbols().iter().map(|s| s.to_string()).collect();
    log_crash(&format!("uncaught exception {}: {reason}\n{}", exception.name(), stack.join("\n")));
}

fn log_crash(text: &str) {
    let Some(path) = error_log_path() else {
        return;
    };
    if append_entry(&path, &timestamp(), text).is_ok() {
        eprintln!("checheviewer: crash written to {}", path.display());
    }
}

fn timestamp() -> String {
    #[cfg(target_os = "macos")]
    return objc2_foundation::NSDate::now().description().to_string();

    #[cfg(not(target_os = "macos"))]
    {
        let now = std::time::SystemTime::now().duration_since(std::time::UNIX_EPOCH);
        format!("unix {}", now.map(|d| d.as_secs()).unwrap_or(0))
    }
}

/// Appends one entry to the log, creating its folder if needed.
fn append_entry(path: &Path, when: &str, text: &str) -> std::io::Result<()> {
    if let Some(dir) = path.parent() {
        std::fs::create_dir_all(dir)?;
    }
    let mut file = std::fs::OpenOptions::new().create(true).append(true).open(path)?;
    writeln!(file, "=== {when} · CheCheViewer {}\n{text}\n", env!("CARGO_PKG_VERSION"))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn entries_are_appended_and_the_folder_is_created() {
        let dir = std::env::temp_dir().join("checheviewer-diag-log");
        let _ = std::fs::remove_dir_all(&dir);
        let path = dir.join("Logs/error.log");

        append_entry(&path, "t1", "first crash").unwrap();
        append_entry(&path, "t2", "second crash").unwrap();

        let log = std::fs::read_to_string(&path).unwrap();
        let _ = std::fs::remove_dir_all(&dir);
        assert_eq!(log.matches("=== ").count(), 2);
        assert!(log.contains("first crash") && log.contains("second crash"));
    }
}
