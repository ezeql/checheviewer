use egui::{epaint::Vertex, pos2, Color32, Mesh, Rect, Sense, Shape, TextureId, Vec2};

use crate::app::App;
use crate::view::Rotation;

/// Fondo oscuro fijo: el gris neutro del sistema falsea la percepción del
/// color de la foto.
const BACKDROP: Color32 = Color32::from_rgb(24, 24, 27);

/// Visor de una sola imagen.
pub fn show(app: &mut App, ui: &mut egui::Ui) {
    let frame = egui::Frame::NONE.fill(BACKDROP);
    egui::CentralPanel::default().frame(frame).show(ui, |ui| {
        let available = ui.available_rect_before_wrap();
        app.set_viewport(available);
        let response = ui.allocate_rect(available, Sense::click_and_drag());

        if let Some(err) = app.error.clone() {
            show_message(ui, available, &err, ui.visuals().error_fg_color);
            return;
        }

        let Some(loaded) = app.current.clone() else {
            if app.session.is_empty() {
                show_message(ui, available, "No hay imágenes en esta carpeta", Color32::GRAY);
            } else if app.is_loading() {
                // Un spinner en vez de un cartel: la decodificación es corta y
                // un texto que aparece y desaparece a cada rato molesta.
                ui.scope_builder(egui::UiBuilder::new().max_rect(available), |ui| {
                    ui.centered_and_justified(|ui| ui.spinner());
                });
            }
            return;
        };

        handle_pointer(app, ui, &response, loaded.size, available);

        let rect = app.view.image_rect(loaded.size, available);
        draw_image(ui, loaded.texture.id(), rect, app.view.rotation);

        if app.view.can_pan(loaded.size, available) {
            ui.ctx().set_cursor_icon(if response.dragged() {
                egui::CursorIcon::Grabbing
            } else {
                egui::CursorIcon::Grab
            });
        }
    });
}

/// Arrastre para mover, pellizco o ⌘+rueda para acercar, rueda para desplazar.
///
/// Es el comportamiento de Preview.app, que es lo que la mano ya tiene
/// aprendido en macOS.
fn handle_pointer(
    app: &mut App,
    ui: &egui::Ui,
    response: &egui::Response,
    image_size: Vec2,
    available: Rect,
) {
    if response.dragged() {
        app.view.pan(response.drag_delta(), image_size, available);
    }

    if !response.hovered() {
        return;
    }

    let cursor = ui.ctx().pointer_interact_pos().unwrap_or_else(|| available.center());
    let (pinch, scroll, zoom_modifier) =
        ui.input(|i| (i.zoom_delta(), i.smooth_scroll_delta, i.modifiers.command));

    if pinch != 1.0 {
        app.view.zoom_at(pinch, cursor, image_size, available);
    } else if scroll != Vec2::ZERO {
        if zoom_modifier {
            // Un desplazamiento hacia arriba acerca.
            let factor = (scroll.y * 0.005).exp();
            app.view.zoom_at(factor, cursor, image_size, available);
        } else {
            app.view.pan(scroll, image_size, available);
        }
    }

    if response.double_clicked() {
        // Alternar entre ajustar y tamaño real es el gesto más rápido para ir
        // a ver un detalle y volver.
        if app.view.is_fit() {
            app.view.actual_size();
        } else {
            app.view.fit();
        }
    }
}

/// Dibuja la textura rotando las coordenadas de textura.
///
/// `Painter::image` no sabe rotar, así que se arma un cuadrilátero a mano y se
/// giran las UV: la rotación sale gratis en la GPU y sin tocar los píxeles.
fn draw_image(ui: &egui::Ui, texture: TextureId, rect: Rect, rotation: Rotation) {
    let corners = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()];

    // Esquinas de la textura, en el mismo orden que `corners` pero corridas
    // según la rotación.
    let uv_corners = [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)];
    let shift = (4 - (rotation.degrees() / 90) as usize) % 4;

    let mut mesh = Mesh::with_texture(texture);
    for (i, position) in corners.iter().enumerate() {
        mesh.vertices.push(Vertex {
            pos: *position,
            uv: uv_corners[(i + shift) % 4],
            color: Color32::WHITE,
        });
    }
    mesh.add_triangle(0, 1, 2);
    mesh.add_triangle(0, 2, 3);

    ui.painter().add(Shape::mesh(mesh));
}

fn show_message(ui: &egui::Ui, area: Rect, text: &str, color: Color32) {
    ui.painter().text(
        area.center(),
        egui::Align2::CENTER_CENTER,
        text,
        egui::FontId::proportional(16.0),
        color,
    );
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Con la imagen derecha, cada esquina de la pantalla mira a la esquina
    /// correspondiente de la textura.
    #[test]
    fn sin_rotacion_las_uv_estan_en_su_lugar() {
        assert_eq!(uv_order(Rotation::None), [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]);
    }

    /// Rotada 90° a la derecha, la esquina superior izquierda de la pantalla
    /// tiene que mostrar la esquina inferior izquierda de la textura.
    #[test]
    fn rotar_90_gira_las_uv_un_cuarto_de_vuelta() {
        assert_eq!(uv_order(Rotation::Quarter), [(0.0, 1.0), (0.0, 0.0), (1.0, 0.0), (1.0, 1.0)]);
    }

    #[test]
    fn rotar_180_invierte_las_uv() {
        assert_eq!(uv_order(Rotation::Half), [(1.0, 1.0), (0.0, 1.0), (0.0, 0.0), (1.0, 0.0)]);
    }

    #[test]
    fn cuatro_cuartos_vuelven_al_principio() {
        assert_eq!(
            uv_order(Rotation::None),
            uv_order(Rotation::None.rotate_right().rotate_right().rotate_right().rotate_right())
        );
    }

    /// Réplica del cálculo de UV de `draw_image`, para poder verificarlo sin
    /// una GPU de por medio.
    fn uv_order(rotation: Rotation) -> [(f32, f32); 4] {
        let uv_corners: [egui::Pos2; 4] =
            [pos2(0.0, 0.0), pos2(1.0, 0.0), pos2(1.0, 1.0), pos2(0.0, 1.0)];
        let shift = (4 - (rotation.degrees() / 90) as usize) % 4;
        std::array::from_fn(|i| {
            let uv = uv_corners[(i + shift) % 4];
            (uv.x, uv.y)
        })
    }
}
