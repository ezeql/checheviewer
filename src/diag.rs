//! Traza de diagnóstico opcional.
//!
//! Cuando la app la abre el Finder no hay terminal a la vista y cuesta saber
//! qué archivo recibió. Con `CHECHEVIEWER_DEBUG=1` escribe a stderr lo que va
//! abriendo:
//!
//! ```sh
//! open -a CheCheViewer.app foto.jpg --env CHECHEVIEWER_DEBUG=1 --stderr /tmp/cv.log
//! ```

use std::sync::OnceLock;

static ENABLED: OnceLock<bool> = OnceLock::new();

pub fn enabled() -> bool {
    *ENABLED.get_or_init(|| std::env::var_os("CHECHEVIEWER_DEBUG").is_some())
}

/// Escribe una línea de diagnóstico si está habilitado.
#[macro_export]
macro_rules! trace {
    ($($arg:tt)*) => {
        if $crate::diag::enabled() {
            eprintln!("checheviewer: {}", format_args!($($arg)*));
        }
    };
}
