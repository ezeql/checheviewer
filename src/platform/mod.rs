//! Operating system integration.

#[cfg(target_os = "macos")]
mod macos_open;

/// Sets up listening for file-open requests from the system. Called before
/// the window is created.
pub fn install_open_handler() {
    #[cfg(target_os = "macos")]
    {
        macos_open::install(None);
        // AppKit overwrites the handler while starting up, so it is also set
        // again as soon as the application finishes launching.
        macos_open::install_on_launch();
    }
}

/// Registers the egui context so the UI can be woken up when the system
/// sends a file.
pub fn set_repaint_context(_ctx: &egui::Context) {
    #[cfg(target_os = "macos")]
    macos_open::set_repaint_context(_ctx.clone());
}

/// Returns a file the system has asked to open since the last call.
pub fn take_requested_path() -> Option<std::path::PathBuf> {
    #[cfg(target_os = "macos")]
    {
        macos_open::take_pending()
    }
    #[cfg(not(target_os = "macos"))]
    {
        None
    }
}
