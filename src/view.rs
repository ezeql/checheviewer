//! How the image is framed inside the window: fit, zoom, pan and rotation.
//!
//! This is pure geometry, with no egui beyond `Rect`/`Vec2`, so it can be
//! tested without opening a window.

use egui::{Rect, Vec2};

/// How much the zoom grows or shrinks per `+` / `-` step.
const ZOOM_STEP: f32 = 1.25;
/// Zoom limits. The lower bound lets a huge photo fit entirely; the upper one
/// lets you inspect a single pixel without zooming off to infinity.
const MIN_ZOOM: f32 = 0.02;
const MAX_ZOOM: f32 = 32.0;

/// Rotation in multiples of 90°, for the view only: the file is untouched.
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

    /// Does the image end up on its side?
    pub fn swaps_axes(self) -> bool {
        matches!(self, Rotation::Quarter | Rotation::ThreeQuarters)
    }

    pub fn degrees(self) -> u32 {
        self.index() as u32 * 90
    }

    /// Size of the image once rotated.
    pub fn apply_to_size(self, size: Vec2) -> Vec2 {
        if self.swaps_axes() {
            Vec2::new(size.y, size.x)
        } else {
            size
        }
    }
}

/// Current framing of the viewer.
#[derive(Debug, Clone, Copy, Default)]
pub struct View {
    /// Explicit zoom. `None` means "fit to window", which is the initial
    /// state and the one we return to when the image changes.
    zoom: Option<f32>,
    /// Offset from the center of the area, in screen pixels.
    offset: Vec2,
    pub rotation: Rotation,
}

impl View {
    /// Returns to automatic fit. Called when the image changes: inheriting
    /// the previous photo's zoom is more disorienting than helpful.
    pub fn reset(&mut self) {
        *self = Self::default();
    }

    /// Scale at which the image is drawn inside `available`.
    pub fn scale(&self, image_size: Vec2, available: Rect) -> f32 {
        match self.zoom {
            Some(zoom) => zoom,
            None => fit_scale(self.rotation.apply_to_size(image_size), available),
        }
    }

    /// Is it in "fit to window" mode?
    pub fn is_fit(&self) -> bool {
        self.zoom.is_none()
    }

    pub fn fit(&mut self) {
        self.zoom = None;
        self.offset = Vec2::ZERO;
    }

    /// 1:1 zoom: one image pixel per screen pixel.
    pub fn actual_size(&mut self) {
        self.zoom = Some(1.0);
        self.offset = Vec2::ZERO;
    }

    /// One zoom step in or out, centered on the middle of the area.
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

    /// Zoom that keeps the point under the cursor fixed, which is what you
    /// expect when zooming in on a detail.
    pub fn zoom_at(&mut self, factor: f32, cursor: egui::Pos2, image_size: Vec2, available: Rect) {
        let old_scale = self.scale(image_size, available);
        let new_scale = (old_scale * factor).clamp(MIN_ZOOM, MAX_ZOOM);
        if (new_scale - old_scale).abs() < f32::EPSILON {
            return;
        }

        // The image center moves so that the point under the cursor keeps
        // its position on screen.
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

    /// Rectangle in which the image must be drawn.
    pub fn image_rect(&self, image_size: Vec2, available: Rect) -> Rect {
        let scale = self.scale(image_size, available);
        let size = self.rotation.apply_to_size(image_size) * scale;
        Rect::from_center_size(available.center() + self.offset, size)
    }

    /// Can the image be moved, or does it fit entirely in the window?
    pub fn can_pan(&self, image_size: Vec2, available: Rect) -> bool {
        let limit = self.pan_limit(image_size, available);
        limit.x > 0.0 || limit.y > 0.0
    }

    /// Prevents dragging the image out of the window: at most, one of its
    /// edges can reach the edge of the area.
    fn clamp_offset(&mut self, image_size: Vec2, available: Rect) {
        let limit = self.pan_limit(image_size, available);
        self.offset.x = self.offset.x.clamp(-limit.x, limit.x);
        self.offset.y = self.offset.y.clamp(-limit.y, limit.y);
    }

    fn pan_limit(&self, image_size: Vec2, available: Rect) -> Vec2 {
        let scale = self.scale(image_size, available);
        let size = self.rotation.apply_to_size(image_size) * scale;
        // On an axis where the image fits completely there is nothing to move.
        Vec2::new(
            ((size.x - available.width()) / 2.0).max(0.0),
            ((size.y - available.height()) / 2.0).max(0.0),
        )
    }
}

/// Scale that makes the image fit in the area without distorting it.
///
/// Only shrinks: an image smaller than the window is shown at actual size
/// rather than being enlarged and looking pixelated.
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
    fn fit_shrinks_large_images() {
        let view = View::default();
        let rect = view.image_rect(egui::vec2(2000.0, 1000.0), area(1000.0, 1000.0));
        assert_eq!(rect.size(), egui::vec2(1000.0, 500.0));
    }

    #[test]
    fn fit_does_not_enlarge_small_images() {
        let view = View::default();
        let rect = view.image_rect(egui::vec2(100.0, 50.0), area(1000.0, 1000.0));
        assert_eq!(rect.size(), egui::vec2(100.0, 50.0));
    }

    #[test]
    fn image_is_centered_without_offset() {
        let view = View::default();
        let rect = view.image_rect(egui::vec2(100.0, 50.0), area(1000.0, 800.0));
        assert_eq!(rect.center(), egui::pos2(500.0, 400.0));
    }

    #[test]
    fn zoom_100_shows_image_at_actual_size() {
        let mut view = View::default();
        view.actual_size();
        let rect = view.image_rect(egui::vec2(4000.0, 3000.0), area(800.0, 600.0));
        assert_eq!(rect.size(), egui::vec2(4000.0, 3000.0));
    }

    #[test]
    fn fitting_again_undoes_zoom() {
        let mut view = View::default();
        view.actual_size();
        assert!(!view.is_fit());
        view.fit();
        assert!(view.is_fit());
        let rect = view.image_rect(egui::vec2(2000.0, 1000.0), area(1000.0, 1000.0));
        assert_eq!(rect.size(), egui::vec2(1000.0, 500.0));
    }

    #[test]
    fn zoom_respects_its_limits() {
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
    fn cursor_zoom_keeps_pointed_spot_fixed() {
        let image = egui::vec2(1000.0, 1000.0);
        let available = area(500.0, 500.0);
        let cursor = egui::pos2(100.0, 100.0);

        let mut view = View::default();
        let before = view.image_rect(image, available);
        // Cursor position relative to the image, as a fraction of its size.
        let t = (cursor - before.min) / before.size();

        view.zoom_at(2.0, cursor, image, available);

        let after = view.image_rect(image, available);
        let same_point = after.min + t * after.size();
        assert!(
            (same_point - cursor).length() < 1.0,
            "the point under the cursor moved: {same_point:?} instead of {cursor:?}"
        );
    }

    #[test]
    fn an_image_that_fits_cannot_be_dragged() {
        let image = egui::vec2(100.0, 100.0);
        let available = area(1000.0, 1000.0);

        let mut view = View::default();
        assert!(!view.can_pan(image, available));
        view.pan(egui::vec2(500.0, 500.0), image, available);
        let rect = view.image_rect(image, available);
        assert_eq!(rect.center(), available.center(), "should stay centered");
    }

    #[test]
    fn dragging_does_not_let_image_escape_the_window() {
        let image = egui::vec2(2000.0, 2000.0);
        let available = area(500.0, 500.0);

        let mut view = View::default();
        view.actual_size();
        assert!(view.can_pan(image, available));
        view.pan(egui::vec2(10_000.0, 10_000.0), image, available);

        let rect = view.image_rect(image, available);
        assert!(
            rect.min.x <= available.min.x && rect.min.y <= available.min.y,
            "the image moved past the edge: {rect:?}"
        );
    }

    #[test]
    fn rotating_swaps_width_and_height() {
        let mut view = View::default();
        view.rotate_right();
        assert_eq!(view.rotation, Rotation::Quarter);
        assert_eq!(view.rotation.degrees(), 90);

        let rect = view.image_rect(egui::vec2(400.0, 200.0), area(1000.0, 1000.0));
        assert_eq!(rect.size(), egui::vec2(200.0, 400.0));
    }

    #[test]
    fn four_rotations_return_to_start() {
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
    fn rotating_left_and_right_cancel_out() {
        let mut view = View::default();
        view.rotate_right();
        view.rotate_left();
        assert_eq!(view.rotation, Rotation::None);
    }

    #[test]
    fn rotated_landscape_image_fits_to_window_height() {
        let mut view = View::default();
        view.rotate_right();
        // 4000×2000 rotated is 2000×4000: it must fit in a 1000×1000 window
        // using the height as the limit.
        let rect = view.image_rect(egui::vec2(4000.0, 2000.0), area(1000.0, 1000.0));
        assert_eq!(rect.size(), egui::vec2(500.0, 1000.0));
    }

    #[test]
    fn reset_returns_to_initial_state() {
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
