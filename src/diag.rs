//! Optional diagnostic trace.
//!
//! When Finder launches the app there is no terminal in sight and it is hard
//! to tell which file it received. With `CHECHEVIEWER_DEBUG=1` it writes what
//! it is opening to stderr:
//!
//! ```sh
//! open -a CheCheViewer.app foto.jpg --env CHECHEVIEWER_DEBUG=1 --stderr /tmp/cv.log
//! ```

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
