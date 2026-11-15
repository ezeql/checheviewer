use crate::app::App;
use crate::model::ViewMode;

/// Top bar: navigation, file name, position in the folder and framing
/// state.
pub fn show(app: &mut App, ui: &mut egui::Ui) {
    egui::Panel::top("topbar").show(ui, |ui| {
        ui.horizontal(|ui| {
            if ui.button("Open…").on_hover_text("⌘O").clicked() {
                app.pick_folder();
            }

            ui.separator();

            if ui.add_enabled(!app.session.is_first(), egui::Button::new("◀")).clicked() {
                app.session.step(-1);
            }
            if ui.add_enabled(!app.session.is_last(), egui::Button::new("▶")).clicked() {
                app.session.step(1);
            }

            ui.separator();

            match app.session.current() {
                Some(entry) => {
                    ui.label(egui::RichText::new(&entry.file_name).strong());
                    ui.label(
                        egui::RichText::new(format!(
                            "{} / {}",
                            app.session.index() + 1,
                            app.session.len()
                        ))
                        .weak(),
                    );
                }
                None if app.session.dir.is_some() => {
                    ui.label(egui::RichText::new("No images").weak());
                }
                None => {
                    ui.label(egui::RichText::new("No folder open").weak());
                }
            }

            if let Some(loaded) = &app.current {
                let [w, h] = loaded.original_size;
                ui.label(egui::RichText::new(format!("{w}×{h}")).weak());
            }

            if let Some(err) = &app.action_error {
                ui.colored_label(ui.visuals().error_fg_color, err);
            }

            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                let label = match app.session.mode {
                    ViewMode::Grid => "Viewer",
                    ViewMode::Viewer => "Grid",
                };
                if ui.button(label).clicked() {
                    app.session.mode = match app.session.mode {
                        ViewMode::Grid => ViewMode::Viewer,
                        ViewMode::Viewer => ViewMode::Grid,
                    };
                }

                if app.session.mode == ViewMode::Viewer {
                    show_view_controls(app, ui);
                }
            });
        });
    });
}

/// Framing controls, viewer only.
fn show_view_controls(app: &mut App, ui: &mut egui::Ui) {
    ui.separator();

    if ui.add_enabled(!app.view.is_fit(), egui::Button::new("Fit")).clicked() {
        app.view.fit();
    }

    if let Some(loaded) = app.current.clone() {
        let percent = app.view.scale(loaded.logical_size(), app.viewport()) * 100.0;
        ui.label(egui::RichText::new(format!("{percent:.0} %")).weak());
    }

    if app.view.rotation.degrees() != 0 {
        ui.label(egui::RichText::new(format!("↻ {}°", app.view.rotation.degrees())).weak());
    }
}
