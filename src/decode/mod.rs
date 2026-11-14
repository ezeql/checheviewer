#[cfg(target_os = "macos")]
mod imageio;
mod rust_image;

use std::path::Path;

use anyhow::Result;

/// Límite por lado al subir una textura a la GPU. Una foto de 100 MP como
/// textura RGBA son ~400 MB de VRAM; a 8192 px de lado el costo máximo queda
/// en 256 MB y ninguna pantalla actual muestra más detalle que eso.
pub const MAX_TEXTURE_SIDE: u32 = 8192;

/// Lado máximo de las miniaturas de la grilla.
pub const THUMBNAIL_SIDE: u32 = 256;

/// Formatos que el crate `image` no abre y ImageIO sí: los de Apple y los RAW
/// de cámara. Van directo por la ruta nativa en lugar de fallar primero.
const NATIVE_ONLY_EXTENSIONS: &[&str] =
    &["heic", "heif", "cr2", "cr3", "nef", "arw", "dng", "orf", "rw2", "raf", "srw"];

/// Cómo obtener la imagen.
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Quality {
    /// Para la grilla: se acepta la miniatura embebida del archivo, que es
    /// órdenes de magnitud más rápida de leer.
    Thumbnail,
    /// Para el visor: se decodifica la imagen real.
    Full,
}

/// Imagen ya decodificada a RGBA8, lista para subir a la GPU.
pub struct DecodedImage {
    pub width: u32,
    pub height: u32,
    pub pixels: Vec<u8>,
    /// Dimensiones originales del archivo, antes de cualquier reducción. Es lo
    /// que se le muestra al usuario.
    pub original_width: u32,
    pub original_height: u32,
    /// Si el color viene multiplicado por el alfa. ImageIO devuelve
    /// premultiplicado; el crate `image`, no.
    pub premultiplied: bool,
}

impl DecodedImage {
    /// Bytes que ocupa en memoria; el cache LRU se acota con esto.
    pub fn byte_size(&self) -> usize {
        self.pixels.len()
    }
}

/// Decodifica una imagen a resolución completa, acotada por
/// `MAX_TEXTURE_SIDE`.
pub fn decode_full(path: &Path) -> Result<DecodedImage> {
    if needs_native(path) {
        return native_decode(path, MAX_TEXTURE_SIDE, Quality::Full);
    }

    match rust_image::decode(path) {
        Ok(image) => Ok(image),
        // ImageIO abre bastante más de lo que abre el crate `image`; si el
        // camino de Rust puro falla, vale la pena intentar con el del sistema
        // antes de darle un error al usuario.
        Err(err) => native_decode(path, MAX_TEXTURE_SIDE, Quality::Full).map_err(|_| err),
    }
}

/// Decodifica una miniatura para la grilla.
///
/// En macOS pasa por ImageIO, que devuelve la miniatura ya embebida en el
/// archivo en vez de decodificar la foto entera.
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

    #[test]
    fn los_formatos_de_apple_van_por_el_decodificador_nativo() {
        assert!(needs_native(Path::new("foto.heic")));
        assert!(needs_native(Path::new("foto.HEIC")));
        assert!(needs_native(Path::new("captura.dng")));
        assert!(!needs_native(Path::new("foto.jpg")));
        assert!(!needs_native(Path::new("foto.png")));
        assert!(!needs_native(Path::new("sin_extension")));
    }
}
