// In release mode, don't open a console behind the window.
#![cfg_attr(not(debug_assertions), windows_subsystem = "windows")]

use std::path::PathBuf;

use checheviewer::app::App;
use checheviewer::platform;

fn main() -> eframe::Result<()> {
    checheviewer::diag::install_crash_log();

    // Before opening the window: if the user launches the app by double-
    // clicking an image, the system sends the file as soon as it starts, so
    // we must already be listening.
    platform::install_open_handler();

    let start = std::env::args_os().nth(1).map(PathBuf::from);
    checheviewer::trace!("starting; argument: {start:?}");

    let options = eframe::NativeOptions {
        viewport: egui::ViewportBuilder::default()
            .with_inner_size([1280.0, 800.0])
            .with_min_inner_size([480.0, 320.0])
            .with_title("CheCheViewer"),
        ..Default::default()
    };

    eframe::run_native("CheCheViewer", options, Box::new(|cc| Ok(Box::new(App::new(cc, start)))))
}
