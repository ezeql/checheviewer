use egui::{text::LayoutJob, Align2, Color32, CornerRadius, FontId, Rect, Sense, Vec2};

use crate::app::App;
use crate::model::ViewMode;

/// Lado de la miniatura dentro de la celda.
const THUMB_SIDE: f32 = 150.0;
/// Alto reservado para el nombre del archivo, debajo de la miniatura.
const LABEL_HEIGHT: f32 = 18.0;
/// Separación entre celdas.
const GAP: f32 = 10.0;

const CELL: Vec2 = Vec2::new(THUMB_SIDE, THUMB_SIDE + LABEL_HEIGHT);

/// Grilla de miniaturas del directorio.
///
/// Solo se dibujan —y se piden— las filas visibles: una carpeta de miles de
/// fotos abre igual de rápido que una de diez.
pub fn show(app: &mut App, ui: &mut egui::Ui) {
    egui::CentralPanel::default().show(ui, |ui| {
        if app.session.is_empty() {
            ui.centered_and_justified(|ui| {
                ui.label("Arrastrá una carpeta acá para empezar");
            });
            return;
        }

        let total = app.session.len();
        let columns = columns_that_fit(ui.available_width());
        let rows = total.div_ceil(columns);
        let row_height = CELL.y + GAP;
        app.grid_columns = columns;

        let mut scroll = egui::ScrollArea::vertical().auto_shrink([false, false]);

        // Cuando la selección se movió con el teclado hay que traerla a la
        // vista. No alcanza con `scroll_to_rect` sobre la celda: si quedó
        // fuera del rango visible, esa celda no llega a dibujarse.
        if app.scroll_to_selection {
            app.scroll_to_selection = false;
            let row = app.session.index() / columns;
            // `show_rows` le suma `item_spacing.y` a cada fila: sin él, el
            // error crece 3 px por fila y en carpetas grandes la selección
            // queda fuera de pantalla.
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

/// Dibuja una celda y devuelve lo que hizo el usuario con ella.
fn cell(app: &mut App, ui: &mut egui::Ui, index: usize) -> Option<Interaction> {
    let (rect, response) = ui.allocate_exact_size(CELL, Sense::click());

    // Clonar corta el préstamo de `app.session` antes de pedirle la miniatura
    // a `app`, que necesita acceso mutable.
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

/// Nombre del archivo debajo de la miniatura, truncado con puntos suspensivos
/// si no entra.
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

/// Cuántas columnas entran en `width`.
fn columns_that_fit(width: f32) -> usize {
    (((width + GAP) / (CELL.x + GAP)).floor() as usize).max(1)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn calcula_las_columnas_segun_el_ancho() {
        // Una celda entra siempre, aunque la ventana sea más angosta que ella.
        assert_eq!(columns_that_fit(10.0), 1);
        assert_eq!(columns_that_fit(THUMB_SIDE), 1);
        // Dos celdas más el espacio entre ellas.
        assert_eq!(columns_that_fit(THUMB_SIDE * 2.0 + GAP), 2);
        assert_eq!(columns_that_fit(THUMB_SIDE * 2.0 + GAP - 1.0), 1);
    }

    #[test]
    fn el_ancho_de_una_ventana_tipica_da_varias_columnas() {
        assert!(columns_that_fit(1280.0) >= 7);
    }
}
