//! A one-screen welcome with the essential shortcuts, shown at launch until
//! the person ticks "Skip this next time".

use std::path::{Path, PathBuf};

/// The welcome card's state while it is open.
#[derive(Default)]
pub struct Onboarding {
    skip_next_time: bool,
}

impl Onboarding {
    /// `Some` unless the person already asked to skip it.
    pub fn at_launch() -> Option<Self> {
        marker().filter(|m| should_show(m)).map(|_| Self::default())
    }
}

/// Draws the card. Returns `false` once it was dismissed: Start, Enter, Esc or
/// a click outside.
pub fn show(onboarding: &mut Onboarding, ctx: &egui::Context) -> bool {
    let mut start = false;
    let response = egui::Modal::new(egui::Id::new("onboarding")).show(ctx, |ui| {
        ui.set_max_width(380.0);
        ui.heading("Welcome to CheCheViewer");
        ui.label("A fast, keyboard-first image viewer. Open a folder with ⌘O or drop it on the window, then:");
        ui.add_space(8.0);

        egui::Grid::new("onboarding-keys").num_columns(2).spacing([16.0, 4.0]).show(ui, |ui| {
            for (keys, what) in [
                ("← →", "previous / next image"),
                ("↑ ↓", "move through the grid"),
                ("Enter / Esc", "open an image / back to the grid"),
                ("+ − 0", "zoom in, out, 100 %"),
                ("F", "full screen"),
                ("Delete", "move the image to the Trash"),
                ("⌘Delete", "delete it for good"),
            ] {
                ui.label(egui::RichText::new(keys).strong());
                ui.label(what);
                ui.end_row();
            }
        });

        ui.add_space(12.0);
        ui.horizontal(|ui| {
            ui.checkbox(&mut onboarding.skip_next_time, "Skip this next time");
            ui.with_layout(egui::Layout::right_to_left(egui::Align::Center), |ui| {
                start = ui.button("Start").clicked();
            });
        });
        start |= ui.input(|i| i.key_pressed(egui::Key::Enter));
    });

    if !(start || response.should_close()) {
        return true;
    }
    if onboarding.skip_next_time {
        if let Some(marker) = marker() {
            remember_skip(&marker);
        }
    }
    false
}

/// `~/Library/Application Support/CheCheViewer/skip-onboarding`: its presence
/// is the whole setting.
fn marker() -> Option<PathBuf> {
    let home = std::env::var_os("HOME")?;
    Some(PathBuf::from(home).join("Library/Application Support/CheCheViewer/skip-onboarding"))
}

fn should_show(marker: &Path) -> bool {
    !marker.exists()
}

/// Best effort: if it can't be written, the card simply shows up again.
fn remember_skip(marker: &Path) {
    if let Some(dir) = marker.parent() {
        let _ = std::fs::create_dir_all(dir);
    }
    let _ = std::fs::write(marker, b"");
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn it_shows_until_skip_is_remembered() {
        let dir = std::env::temp_dir().join("checheviewer-onboarding");
        let _ = std::fs::remove_dir_all(&dir);
        let marker = dir.join("Support/skip-onboarding");

        assert!(should_show(&marker));
        remember_skip(&marker);
        assert!(!should_show(&marker), "after skipping it must not show again");
        let _ = std::fs::remove_dir_all(&dir);
    }
}
