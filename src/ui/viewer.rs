use egui::{epaint::Vertex, pos2, Color32, Mesh, Rect, Sense, Shape, TextureId, Vec2};

use crate::app::App;
use crate::view::Rotation;

/// Fixed dark backdrop: the system's neutral gray skews the perception of
/// the photo's colors.
const BACKDROP: Color32 = Color32::from_rgb(24, 24, 27);

/// Single-image viewer.
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
                show_message(ui, available, "No images in this folder", Color32::GRAY);
            } else if app.is_loading() {
                // A spinner instead of a message: decoding is short and text
                // that keeps appearing and disappearing is annoying.
                ui.scope_builder(egui::UiBuilder::new().max_rect(available), |ui| {
                    ui.centered_and_justified(|ui| ui.spinner());
                });
            }
            return;
        };

        let size = loaded.logical_size();
        handle_pointer(app, ui, &response, size, available);

        let rect = app.view.image_rect(size, available);
        draw_image(ui, loaded.texture.id(), rect, app.view.rotation);

        // What's on screen may be the thumbnail standing in: say it's not the
        // final image yet.
        if app.is_loading() {
            let corner = available.right_top() + egui::vec2(-28.0, 8.0);
            ui.put(Rect::from_min_size(corner, Vec2::splat(20.0)), egui::Spinner::new());
        }

        if app.view.can_pan(size, available) {
            ui.ctx().set_cursor_icon(if response.dragged() {
                egui::CursorIcon::Grabbing
            } else {
                egui::CursorIcon::Grab
            });
        }
    });
}

/// Drag to move, pinch or ⌘+wheel to zoom, wheel to scroll.
///
/// This is Preview's behavior, which is what the hand has already learned on
/// macOS.
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
            // Scrolling up zooms in.
            let factor = (scroll.y * 0.005).exp();
            app.view.zoom_at(factor, cursor, image_size, available);
        } else {
            app.view.pan(scroll, image_size, available);
        }
    }

    if response.double_clicked() {
        // Toggling between fit and actual size is the quickest gesture for
        // checking a detail and coming back.
        if app.view.is_fit() {
            app.view.actual_size();
        } else {
            app.view.fit();
        }
    }
}

/// Draws the texture by rotating the texture coordinates.
///
/// `Painter::image` cannot rotate, so a quad is built by hand and the UVs are
/// rotated: the rotation is free on the GPU and never touches the pixels.
fn draw_image(ui: &egui::Ui, texture: TextureId, rect: Rect, rotation: Rotation) {
    let corners = [rect.left_top(), rect.right_top(), rect.right_bottom(), rect.left_bottom()];

    // Texture corners, in the same order as `corners` but shifted according
    // to the rotation.
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

    /// With the image upright, each screen corner maps to the corresponding
    /// texture corner.
    #[test]
    fn without_rotation_uvs_are_in_place() {
        assert_eq!(uv_order(Rotation::None), [(0.0, 0.0), (1.0, 0.0), (1.0, 1.0), (0.0, 1.0)]);
    }

    /// Rotated 90° right, the top-left screen corner must show the
    /// bottom-left texture corner.
    #[test]
    fn rotating_90_turns_uvs_a_quarter() {
        assert_eq!(uv_order(Rotation::Quarter), [(0.0, 1.0), (0.0, 0.0), (1.0, 0.0), (1.0, 1.0)]);
    }

    #[test]
    fn rotating_180_flips_uvs() {
        assert_eq!(uv_order(Rotation::Half), [(1.0, 1.0), (0.0, 1.0), (0.0, 0.0), (1.0, 0.0)]);
    }

    #[test]
    fn four_quarters_return_to_start() {
        assert_eq!(
            uv_order(Rotation::None),
            uv_order(Rotation::None.rotate_right().rotate_right().rotate_right().rotate_right())
        );
    }

    /// Replica of the UV computation in `draw_image`, so it can be verified
    /// without a GPU.
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
