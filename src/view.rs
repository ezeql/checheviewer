//! Cómo se encuadra la imagen dentro de la ventana: ajuste, zoom, paneo y
//! rotación.
//!
//! Es geometría pura, sin egui de por medio más allá de `Rect`/`Vec2`, así que
//! se puede testear sin abrir una ventana.

use egui::{Rect, Vec2};

/// Cuánto se agranda o achica por cada paso de `+` / `-`.
const ZOOM_STEP: f32 = 1.25;
/// Límites de zoom. Abajo, para que una foto enorme entre entera; arriba, para
/// poder mirar un píxel de cerca sin que se vaya al infinito.
const MIN_ZOOM: f32 = 0.02;
const MAX_ZOOM: f32 = 32.0;

/// Rotación en múltiplos de 90°, solo para la vista: no toca el archivo.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum Rotation {
    #[default]
    None,
    Quarter,
    Half,
    ThreeQuarters,
}

impl Rotation {
    fn index(self) -> u8 {
        match self {
            Rotation::None => 0,
            Rotation::Quarter => 1,
            Rotation::Half => 2,
            Rotation::ThreeQuarters => 3,
        }
    }

    fn from_index(index: u8) -> Self {
        match index % 4 {
            0 => Rotation::None,
            1 => Rotation::Quarter,
            2 => Rotation::Half,
            _ => Rotation::ThreeQuarters,
        }
    }

    pub fn rotate_right(self) -> Self {
        Self::from_index(self.index() + 1)
    }

    pub fn rotate_left(self) -> Self {
        Self::from_index(self.index() + 3)
    }

    /// ¿La imagen queda de costado?
    pub fn swaps_axes(self) -> bool {
        matches!(self, Rotation::Quarter | Rotation::ThreeQuarters)
    }

    pub fn degrees(self) -> u32 {
        self.index() as u32 * 90
    }

    /// Tamaño de la imagen una vez rotada.
    pub fn apply_to_size(self, size: Vec2) -> Vec2 {
        if self.swaps_axes() {
            Vec2::new(size.y, size.x)
        } else {
            size
        }
    }
}

/// Encuadre actual del visor.
#[derive(Debug, Clone, Copy, Default)]
pub struct View {
    /// Zoom explícito. `None` significa "ajustar a la ventana", que es el
    /// estado inicial y al que se vuelve al cambiar de imagen.
    zoom: Option<f32>,
    /// Desplazamiento respecto del centro del área, en píxeles de pantalla.
    offset: Vec2,
    pub rotation: Rotation,
}

impl View {
    /// Vuelve al ajuste automático. Se llama al cambiar de imagen: heredar el
    /// zoom de la foto anterior desorienta más de lo que ayuda.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Escala con la que se dibuja la imagen dentro de `available`.
    pub fn scale(&self, image_size: Vec2, available: Rect) -> f32 {
        match self.zoom {
            Some(zoom) => zoom,
            None => fit_scale(self.rotation.apply_to_size(image_size), available),
        }
    }

    /// ¿Está en modo "ajustar a la ventana"?
    pub fn is_fit(&self) -> bool {
        self.zoom.is_none()
    }

    pub fn fit(&mut self) {
        self.zoom = None;
        self.offset = Vec2::ZERO;
    }

    /// Zoom 1:1, un píxel de la imagen por píxel de pantalla.
    pub fn actual_size(&mut self) {
        self.zoom = Some(1.0);
        self.offset = Vec2::ZERO;
    }

    /// Un paso de zoom hacia adentro o hacia afuera, centrado en el medio del
    /// área.
    pub fn zoom_by(&mut self, factor: f32, image_size: Vec2, available: Rect) {
        let current = self.scale(image_size, available);
        self.zoom = Some((current * factor).clamp(MIN_ZOOM, MAX_ZOOM));
        self.clamp_offset(image_size, available);
    }

    pub fn zoom_in(&mut self, image_size: Vec2, available: Rect) {
        self.zoom_by(ZOOM_STEP, image_size, available);
    }

    pub fn zoom_out(&mut self, image_size: Vec2, available: Rect) {
        self.zoom_by(1.0 / ZOOM_STEP, image_size, available);
    }

    /// Zoom manteniendo quieto el punto que está bajo el cursor, que es lo que
    /// uno espera al acercarse a un detalle.
    pub fn zoom_at(&mut self, factor: f32, cursor: egui::Pos2, image_size: Vec2, available: Rect) {
        let old_scale = self.scale(image_size, available);
        let new_scale = (old_scale * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        if (new_scale - old_scale).abs() < f32::EPSILON {
            return;
        }

        // El centro de la imagen se mueve de modo que el punto bajo el cursor
        // conserve su posición en pantalla.
        let center = available.center() + self.offset;
        let ratio = new_scale / old_scale;
        let new_center = cursor - (cursor - center) * ratio;

        self.zoom = Some(new_scale);
        self.offset = new_center - available.center();
        self.clamp_offset(image_size, available);
    }

    pub fn pan(&mut self, delta: Vec2, image_size: Vec2, available: Rect) {
        self.offset += delta;
        self.clamp_offset(image_size, available);
    }

    pub fn rotate_left(&mut self) {
        self.rotation = self.rotation.rotate_left();
        self.offset = Vec2::ZERO;
    }

    pub fn rotate_right(&mut self) {
        self.rotation = self.rotation.rotate_right();
        self.offset = Vec2::ZERO;
    }

    /// Rectángulo en el que hay que dibujar la imagen.
    pub fn image_rect(&self, image_size: Vec2, available: Rect) -> Rect {
        let scale = self.scale(image_size, available);
        let size = self.rotation.apply_to_size(image_size) * scale;
        Rect::from_center_size(available.center() + self.offset, size)
    }

    /// ¿Se puede mover la imagen, o entra entera en la ventana?
    pub fn can_pan(&self, image_size: Vec2, available: Rect) -> bool {
        let limit = self.pan_limit(image_size, available);
        limit.x > 0.0 || limit.y > 0.0
    }

    /// Impide arrastrar la imagen fuera de la ventana: como mucho, se llega a
    /// que uno de sus bordes toque el borde del área.
    fn clamp_offset(&mut self, image_size: Vec2, available: Rect) {
        let limit = self.pan_limit(image_size, available);
        self.offset.x = self.offset.x.clamp(-limit.x, limit.x);
        self.offset.y = self.offset.y.clamp(-limit.y, limit.y);
    }

    fn pan_limit(&self, image_size: Vec2, available: Rect) -> Vec2 {
        let scale = self.scale(image_size, available);
        let size = self.rotation.apply_to_size(image_size) * scale;
        // En el eje donde la imagen entra completa no hay nada que mover.
        Vec2::new(
            ((size.x - available.width()) / 2.0).max(0.0),
            ((size.y - available.height()) / 2.0).max(0.0),
        )
    }
}

/// Escala que hace entrar la imagen en el área sin deformarla.
///
/// Solo reduce: una imagen más chica que la ventana se muestra a tamaño real
/// en lugar de ampliarse y verse pixelada.
fn fit_scale(image_size: Vec2, available: Rect) -> f32 {
    if image_size.x <= 0.0 || image_size.y <= 0.0 {
        return 1.0;
    }
    (available.width() / image_size.x).min(available.height() / image_size.y).min(1.0)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn area(w: f32, h: f32) -> Rect {
        Rect::from_min_size(egui::pos2(0.0, 0.0), egui::vec2(w, h))
    }

    #[test]
    fn el_ajuste_reduce_las_imagenes_grandes() {
        let view = View::default();
        let rect = view.image_rect(egui::vec2(2000.0, 1000.0), area(1000.0, 1000.0));
        assert_eq!(rect.size(), egui::vec2(1000.0, 500.0));
    }

    #[test]
    fn el_ajuste_no_amplia_las_imagenes_chicas() {
        let view = View::default();
        let rect = view.image_rect(egui::vec2(100.0, 50.0), area(1000.0, 1000.0));
        assert_eq!(rect.size(), egui::vec2(100.0, 50.0));
    }

    #[test]
    fn la_imagen_queda_centrada_sin_desplazamiento() {
        let view = View::default();
        let rect = view.image_rect(egui::vec2(100.0, 50.0), area(1000.0, 800.0));
        assert_eq!(rect.center(), egui::pos2(500.0, 400.0));
    }

    #[test]
    fn el_zoom_al_100_muestra_la_imagen_a_tamano_real() {
        let mut view = View::default();
        view.actual_size();
        let rect = view.image_rect(egui::vec2(4000.0, 3000.0), area(800.0, 600.0));
        assert_eq!(rect.size(), egui::vec2(4000.0, 3000.0));
    }

    #[test]
    fn volver_a_ajustar_deshace_el_zoom() {
        let mut view = View::default();
        view.actual_size();
        assert!(!view.is_fit());
        view.fit();
        assert!(view.is_fit());
        let rect = view.image_rect(egui::vec2(2000.0, 1000.0), area(1000.0, 1000.0));
        assert_eq!(rect.size(), egui::vec2(1000.0, 500.0));
    }

    #[test]
    fn el_zoom_respeta_sus_limites() {
        let image = egui::vec2(100.0, 100.0);
        let available = area(1000.0, 1000.0);

        let mut view = View::default();
        for _ in 0..100 {
            view.zoom_in(image, available);
        }
        assert!(view.scale(image, available) <= MAX_ZOOM);

        for _ in 0..300 {
            view.zoom_out(image, available);
        }
        assert!(view.scale(image, available) >= MIN_ZOOM);
    }

    #[test]
    fn el_zoom_con_el_cursor_deja_quieto_el_punto_apuntado() {
        let image = egui::vec2(1000.0, 1000.0);
        let available = area(500.0, 500.0);
        let cursor = egui::pos2(100.0, 100.0);

        let mut view = View::default();
        let before = view.image_rect(image, available);
        // Posición del cursor relativa a la imagen, en fracción de su tamaño.
        let t = (cursor - before.min) / before.size();

        view.zoom_at(2.0, cursor, image, available);

        let after = view.image_rect(image, available);
        let same_point = after.min + t * after.size();
        assert!(
            (same_point - cursor).length() < 1.0,
            "el punto bajo el cursor se movió: {same_point:?} en vez de {cursor:?}"
        );
    }

    #[test]
    fn no_se_puede_arrastrar_una_imagen_que_entra_entera() {
        let image = egui::vec2(100.0, 100.0);
        let available = area(1000.0, 1000.0);

        let mut view = View::default();
        assert!(!view.can_pan(image, available));
        view.pan(egui::vec2(500.0, 500.0), image, available);
        let rect = view.image_rect(image, available);
        assert_eq!(rect.center(), available.center(), "debería seguir centrada");
    }

    #[test]
    fn el_arrastre_no_deja_escapar_la_imagen_de_la_ventana() {
        let image = egui::vec2(2000.0, 2000.0);
        let available = area(500.0, 500.0);

        let mut view = View::default();
        view.actual_size();
        assert!(view.can_pan(image, available));
        view.pan(egui::vec2(10_000.0, 10_000.0), image, available);

        let rect = view.image_rect(image, available);
        assert!(
            rect.min.x <= available.min.x && rect.min.y <= available.min.y,
            "la imagen se corrió más allá del borde: {rect:?}"
        );
    }

    #[test]
    fn rotar_intercambia_ancho_y_alto() {
        let mut view = View::default();
        view.rotate_right();
        assert_eq!(view.rotation, Rotation::Quarter);
        assert_eq!(view.rotation.degrees(), 90);

        let rect = view.image_rect(egui::vec2(400.0, 200.0), area(1000.0, 1000.0));
        assert_eq!(rect.size(), egui::vec2(200.0, 400.0));
    }

    #[test]
    fn cuatro_rotaciones_vuelven_al_origen() {
        let mut view = View::default();
        for _ in 0..4 {
            view.rotate_right();
        }
        assert_eq!(view.rotation, Rotation::None);

        for _ in 0..4 {
            view.rotate_left();
        }
        assert_eq!(view.rotation, Rotation::None);
    }

    #[test]
    fn rotar_a_izquierda_y_derecha_se_cancelan() {
        let mut view = View::default();
        view.rotate_right();
        view.rotate_left();
        assert_eq!(view.rotation, Rotation::None);
    }

    #[test]
    fn una_imagen_apaisada_rotada_se_ajusta_al_alto_de_la_ventana() {
        let mut view = View::default();
        view.rotate_right();
        // 4000×2000 rotada es 2000×4000: tiene que entrar en una ventana de
        // 1000×1000 usando el alto como límite.
        let rect = view.image_rect(egui::vec2(4000.0, 2000.0), area(1000.0, 1000.0));
        assert_eq!(rect.size(), egui::vec2(500.0, 1000.0));
    }

    #[test]
    fn reset_vuelve_al_estado_inicial() {
        let mut view = View::default();
        view.actual_size();
        view.rotate_right();
        view.pan(egui::vec2(50.0, 50.0), egui::vec2(4000.0, 4000.0), area(500.0, 500.0));

        view.reset();
        assert!(view.is_fit());
        assert_eq!(view.rotation, Rotation::None);
        assert_eq!(view.offset, Vec2::ZERO);
    }
}
