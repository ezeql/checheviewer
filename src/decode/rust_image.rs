use std::path::Path;

use anyhow::{Context, Result};
use image::{imageops::FilterType, DynamicImage, ImageDecoder, ImageReader};

use super::{DecodedImage, MAX_TEXTURE_SIDE};

/// Decodifica con el crate `image` (Rust puro), aplicando la orientación EXIF.
pub fn decode(path: &Path) -> Result<DecodedImage> {
    decode_scaled(path, MAX_TEXTURE_SIDE)
}

/// Igual que `decode`, pero acotando el resultado a `max_side` píxeles por
/// lado.
pub fn decode_scaled(path: &Path, max_side: u32) -> Result<DecodedImage> {
    let reader = ImageReader::open(path)
        .with_context(|| format!("no se pudo abrir {}", path.display()))?
        .with_guessed_format()
        .with_context(|| format!("no se pudo determinar el formato de {}", path.display()))?;

    let mut decoder = reader
        .into_decoder()
        .with_context(|| format!("formato no soportado en {}", path.display()))?;

    // `image` 0.25 no aplica la orientación por su cuenta: sin este paso las
    // fotos verticales sacadas con el celular se ven acostadas. Hay que leerla
    // antes de consumir el decoder.
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);

    let mut img = DynamicImage::from_decoder(decoder)
        .with_context(|| format!("no se pudo decodificar {}", path.display()))?;
    img.apply_orientation(orientation);

    Ok(to_decoded(img, max_side))
}

/// Convierte a RGBA8 reduciendo si excede el límite pedido.
fn to_decoded(img: DynamicImage, max_side: u32) -> DecodedImage {
    let (original_width, original_height) = (img.width(), img.height());

    let img = if original_width > max_side || original_height > max_side {
        // `resize` preserva el aspecto y encaja dentro de la caja dada.
        img.resize(max_side, max_side, FilterType::Triangle)
    } else {
        img
    };

    let rgba = img.into_rgba8();
    DecodedImage {
        width: rgba.width(),
        height: rgba.height(),
        pixels: rgba.into_raw(),
        original_width,
        original_height,
        premultiplied: false,
    }
}
