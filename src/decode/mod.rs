#[cfg(target_os = "macos")]
mod imageio;
mod rust_image;

use std::path::Path;

use anyhow::Result;

/// Per-side limit when uploading a texture to the GPU. A 100 MP photo as an
/// RGBA texture is ~400 MB of VRAM; at 8192 px per side the worst case is
/// 256 MB, and no current display shows more detail than that.
pub const MAX_TEXTURE_SIDE: u32 = 8192;

/// Maximum side length of the grid thumbnails.
pub const THUMBNAIL_SIDE: u32 = 256;

/// Formats the `image` crate can't open but ImageIO can: Apple's and camera
/// RAW. They go straight to the native path instead of failing first.
const NATIVE_ONLY_EXTENSIONS: &[&str] =
    &["heic", "heif", "cr2", "cr3", "nef", "arw", "dng", "orf", "rw2", "raf", "srw"];

/// How to obtain the image.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    /// For the grid: the thumbnail embedded in the file is accepted, which is
    /// orders of magnitude faster to read.
    Thumbnail,
    /// For the viewer: the real image is decoded.
    Full,
}

/// Image already decoded to RGBA8, ready to upload to the GPU.
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    /// Original dimensions of the file, before any downscaling. This is what
    /// is shown to the user.
    pub original_width: u32,
    pub original_height: u32,
    /// Whether the color is multiplied by alpha. ImageIO returns it
    /// premultiplied; the `image` crate doesn't.
    pub premultiplied: bool,
}

impl DecodedImage {
    /// Converts to egui's pixel format. Runs on the decode worker: doing it on
    /// the UI thread cost 8-12 ms per 12 MP photo, most of a frame.
    ///
    /// Works in place and hands the buffer over without copying: with several
    /// workers converting at once, a second full-size buffer each pushed the
    /// memory peak up by ~120 MB. ImageIO's output is already premultiplied;
    /// the `image` crate's straight alpha is premultiplied here, since mixing
    /// the two up tints translucent areas black.
    pub fn into_color_image(mut self) -> egui::ColorImage {
        let size = [self.width as usize, self.height as usize];
        if !self.premultiplied {
            for p in self.pixels.as_chunks_mut::<4>().0 {
                *p = egui::Color32::from_rgba_unmultiplied(p[0], p[1], p[2], p[3]).to_array();
            }
        }
        match bytemuck::allocation::try_cast_vec(self.pixels) {
            Ok(pixels) => egui::ColorImage::new(size, pixels),
            // A capacity that isn't a whole number of pixels: copy instead.
            Err((_, pixels)) => egui::ColorImage::from_rgba_premultiplied(size, &pixels),
        }
    }
}

/// Decodes an image at full resolution, capped at `MAX_TEXTURE_SIDE`.
pub fn decode_full(path: &Path) -> Result<DecodedImage> {
    if needs_native(path) {
        return native_decode(path, MAX_TEXTURE_SIDE, Quality::Full);
    }

    match rust_image::decode(path) {
        Ok(image) => Ok(image),
        // ImageIO opens quite a bit more than the `image` crate does; if the
        // pure Rust path fails, it's worth trying the system one before
        // giving the user an error.
        Err(err) => native_decode(path, MAX_TEXTURE_SIDE, Quality::Full).map_err(|_| err),
    }
}

/// Decodes a thumbnail for the grid.
///
/// On macOS it goes through ImageIO, which returns the thumbnail already
/// embedded in the file instead of decoding the whole photo.
pub fn decode_thumbnail(path: &Path) -> Result<DecodedImage> {
    match native_decode(path, THUMBNAIL_SIDE, Quality::Thumbnail) {
        Ok(image) => Ok(image),
        Err(err) => rust_image::decode_scaled(path, THUMBNAIL_SIDE).map_err(|_| err),
    }
}

fn needs_native(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| NATIVE_ONLY_EXTENSIONS.contains(&e.to_ascii_lowercase().as_str()))
        .unwrap_or(false)
}

#[cfg(target_os = "macos")]
fn native_decode(path: &Path, max_side: u32, quality: Quality) -> Result<DecodedImage> {
    imageio::decode(path, max_side, quality)
}

#[cfg(not(target_os = "macos"))]
fn native_decode(path: &Path, max_side: u32, _quality: Quality) -> Result<DecodedImage> {
    rust_image::decode_scaled(path, max_side)
}

#[cfg(test)]
mod tests {
    use super::*;

    fn pixel(premultiplied: bool, rgba: [u8; 4]) -> [u8; 4] {
        let image = DecodedImage {
            width: 1,
            height: 1,
            pixels: rgba.to_vec(),
            original_width: 1,
            original_height: 1,
            premultiplied,
        };
        image.into_color_image().pixels[0].to_array()
    }

    #[test]
    fn premultiplied_pixels_pass_through_and_straight_alpha_is_premultiplied() {
        assert_eq!(pixel(true, [10, 20, 30, 128]), [10, 20, 30, 128]);
        let [r, _, _, a] = pixel(false, [200, 200, 200, 128]);
        assert!(r < 200 && a == 128, "straight alpha must be premultiplied, got {r}");
    }

    #[test]
    fn apple_formats_use_the_native_decoder() {
        assert!(needs_native(Path::new("photo.heic")));
        assert!(needs_native(Path::new("photo.HEIC")));
        assert!(needs_native(Path::new("capture.dng")));
        assert!(!needs_native(Path::new("photo.jpg")));
        assert!(!needs_native(Path::new("photo.png")));
        assert!(!needs_native(Path::new("no_extension")));
    }
}
