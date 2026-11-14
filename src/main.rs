// En modo release no abrimos una consola detrás de la ventana.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use checheviewer::app::App;
use checheviewer::platform;

fn main() -> eframe::Result<()> {
    // Antes de abrir la ventana: si al usuario le abrimos la app haciendo
    // doble clic en una imagen, el sistema manda el archivo apenas arranca y
    // hay que estar escuchando.
    platform::install_open_handler();

    let start = std::env::args_os().nth(1).map(PathBuf::from);
    checheviewer::trace!("iniciando; argumento: {start:?}");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([480.0, 320.0])
            .with_title("CheCheViewer"),
        ..Default::default()
    };

    eframe::run_native("CheCheViewer", options, Box::new(|cc| Ok(Box::new(App::new(cc, start)))))
}
