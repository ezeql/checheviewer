use egui::{text::LayoutJob, Align2, Color32, CornerRadius, FontId, Rect, Sense, Vec2};

use crate::app::App;
use crate::model::ViewMode;

/// Side of the thumbnail within the cell.
const THUMB_SIDE: f32 = 150.0;
/// Height reserved for the file name, below the thumbnail.
const LABEL_HEIGHT: f32 = 18.0;
/// Spacing between cells.
const GAP: f32 = 10.0;

const CELL: Vec2 = Vec2::new(THUMB_SIDE, THUMB_SIDE + LABEL_HEIGHT);

/// Thumbnail grid for the directory.
///
/// Only the visible rows are drawn (and requested): a folder with thousands
/// of photos opens as fast as one with ten.
pub fn show(app: &mut App, ui: &mut egui::Ui) {
    egui::CentralPanel::default().show(ui, |ui| {
        if app.session.is_empty() {
            folder_browser(app, ui);
            return;
        }

        let total = app.session.len();
        let columns = columns_that_fit(ui.available_width());
        let rows = total.div_ceil(columns);
        let row_height = CELL.y + GAP;
        app.grid_columns = columns;

        let mut scroll = egui::ScrollArea::vertical().auto_shrink([false, false]);

        // When the selection was moved with the keyboard it has to be brought
        // into view. `scroll_to_rect` on the cell is not enough: if it fell
        // outside the visible range, that cell never gets drawn.
        if app.scroll_to_selection {
            app.scroll_to_selection = false;
            let row = app.session.index() / columns;
            // `show_rows` adds `item_spacing.y` to each row: without it, the
            // error grows 3 px per row and in large folders the selection
            // ends up off screen.
            let pitch = row_height + ui.spacing().item_spacing.y;
            let target = row as f32 * pitch - (ui.available_height() - row_height) / 2.0;
            scroll = scroll.vertical_scroll_offset(target.max(0.0));
        }

        let mut clicked: Option<usize> = None;
        let mut opened: Option<usize> = None;

        scroll.show_rows(ui, row_height, rows, |ui, row_range| {
            for row in row_range {
                ui.horizontal(|ui| {
                    for column in 0..columns {
                        let index = row * columns + column;
                        if index >= total {
                            break;
                        }
                        match cell(app, ui, index) {
                            Some(Interaction::Clicked) => clicked = Some(index),
                            Some(Interaction::Opened) => opened = Some(index),
                            None => {}
                        }
                    }
                });
                ui.add_space(GAP);
            }
        });

        if let Some(index) = clicked {
            app.session.set_index(index);
        }
        if let Some(index) = opened {
            app.session.set_index(index);
            app.session.mode = ViewMode::Viewer;
        }
    });
}

enum Interaction {
    Clicked,
    Opened,
}

/// What the grid shows when the folder has no images: where to go next.
/// Click selects and double-click opens, as in the grid.
fn folder_browser(app: &mut App, ui: &mut egui::Ui) {
    let Some(dir) = &app.session.dir else {
        ui.centered_and_justified(|ui| {
            ui.label("Drop a folder here to start");
        });
        return;
    };
    ui.label(egui::RichText::new(dir.display().to_string()).strong());
    ui.label(
        egui::RichText::new("No images here. ↑ ↓ to pick a folder, Enter to open it, ← to go up.")
            .weak(),
    );
    ui.separator();

    let scroll_to_selection = std::mem::take(&mut app.scroll_to_selection);
    let (mut clicked, mut opened) = (None, None);
    egui::ScrollArea::vertical().auto_shrink([false, false]).show(ui, |ui| {
        for (index, folder) in app.session.folders.iter().enumerate() {
            let selected = index == app.session.folder_index;
            let icon = if folder.file_name == ".." { "⬆" } else { "🗀" };
            let response = ui.selectable_label(selected, format!("{icon} {}", folder.file_name));
            if selected && scroll_to_selection {
                response.scroll_to_me(None);
            }
            if response.double_clicked() {
                opened = Some(folder.path.clone());
            } else if response.clicked() {
                clicked = Some(index);
            }
        }
    });

    if let Some(index) = clicked {
        app.session.folder_index = index;
    }
    if let Some(path) = opened {
        app.open_path(&path);
    }
}

/// Draws a cell and returns what the user did with it.
fn cell(app: &mut App, ui: &mut egui::Ui, index: usize) -> Option<Interaction> {
    let (rect, response) = ui.allocate_exact_size(CELL, Sense::click());

    // Cloning ends the borrow of `app.session` before asking `app` for the
    // thumbnail, which needs mutable access.
    let entry = app.session.entries.get(index)?;
    let (path, name) = (entry.path.clone(), entry.file_name.clone());
    let selected = index == app.session.index();

    if !ui.is_rect_visible(rect) {
        return None;
    }

    let painter = ui.painter();
    if selected {
        painter.rect_filled(rect, CornerRadius::same(4), ui.visuals().selection.bg_fill);
    } else if response.hovered() {
        painter.rect_filled(rect, CornerRadius::same(4), ui.visuals().widgets.hovered.bg_fill);
    }

    let thumb_area = Rect::from_min_size(rect.min, Vec2::splat(THUMB_SIDE)).shrink(4.0);
    match app.thumbnail(&path) {
        Some(thumb) => {
            let scale = (thumb_area.width() / thumb.size.x)
                .min(thumb_area.height() / thumb.size.y)
                .min(1.0);
            let image_rect = Rect::from_center_size(thumb_area.center(), thumb.size * scale);
            ui.painter().image(
                thumb.texture.id(),
                image_rect,
                Rect::from_min_max(egui::pos2(0.0, 0.0), egui::pos2(1.0, 1.0)),
                Color32::WHITE,
            );
        }
        None => {
            let (symbol, color) = if app.has_failed(&path) {
                ("⚠", ui.visuals().error_fg_color)
            } else {
                ("…", ui.visuals().weak_text_color())
            };
            ui.painter().text(
                thumb_area.center(),
                Align2::CENTER_CENTER,
                symbol,
                FontId::proportional(20.0),
                color,
            );
        }
    }

    draw_label(ui, rect, &name, selected);

    if response.double_clicked() {
        Some(Interaction::Opened)
    } else if response.clicked() {
        Some(Interaction::Clicked)
    } else {
        None
    }
}

/// File name below the thumbnail, truncated with an ellipsis if it does not
/// fit.
fn draw_label(ui: &egui::Ui, cell_rect: Rect, name: &str, selected: bool) {
    let color = if selected { ui.visuals().strong_text_color() } else { ui.visuals().text_color() };

    let mut job = LayoutJob::single_section(
        name.to_owned(),
        egui::TextFormat { font_id: FontId::proportional(11.0), color, ..Default::default() },
    );
    job.wrap = egui::text::TextWrapping {
        max_width: cell_rect.width() - 6.0,
        max_rows: 1,
        break_anywhere: true,
        overflow_character: Some('…'),
    };

    let galley = ui.painter().layout_job(job);
    let position = egui::pos2(cell_rect.center().x, cell_rect.max.y - LABEL_HEIGHT / 2.0);
    let origin = position - galley.size() / 2.0;
    ui.painter().galley(origin, galley, color);
}

/// How many columns fit in `width`.
fn columns_that_fit(width: f32) -> usize {
    (((width + GAP) / (CELL.x + GAP)).floor() as usize).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn computes_columns_from_width() {
        // One cell always fits, even if the window is narrower than it.
        assert_eq!(columns_that_fit(10.0), 1);
        assert_eq!(columns_that_fit(THUMB_SIDE), 1);
        // Two cells plus the space between them.
        assert_eq!(columns_that_fit(THUMB_SIDE * 2.0 + GAP), 2);
        assert_eq!(columns_that_fit(THUMB_SIDE * 2.0 + GAP - 1.0), 1);
    }

    #[test]
    fn typical_window_width_gives_several_columns() {
        assert!(columns_that_fit(1280.0) >= 7);
    }
}
