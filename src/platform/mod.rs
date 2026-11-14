//! Integración con el sistema operativo.

#[cfg(target_os = "macos")]
mod macos_open;

/// Prepara la escucha de aperturas de archivos del sistema. Se llama antes de
/// crear la ventana.
pub fn install_open_handler() {
    #[cfg(target_os = "macos")]
    {
        macos_open::install(None);
        // AppKit pisa el handler mientras arranca, así que además se lo vuelve
        // a poner en cuanto la aplicación termina de levantarse.
        macos_open::install_on_launch();
    }
}

/// Registra el contexto de egui para poder despertar la interfaz cuando el
/// sistema mande un archivo.
pub fn set_repaint_context(_ctx: &egui::Context) {
    #[cfg(target_os = "macos")]
    macos_open::set_repaint_context(_ctx.clone());
}

/// Devuelve un archivo que el sistema haya pedido abrir desde la última vez.
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
