use std::path::Path;

use anyhow::{Context, Result};
use image::{imageops::FilterType, DynamicImage, ImageDecoder, ImageReader};

use super::{DecodedImage, MAX_TEXTURE_SIDE};

/// Decodes with the `image` crate (pure Rust), applying EXIF orientation.
pub fn decode(path: &Path) -> Result<DecodedImage> {
    decode_scaled(path, MAX_TEXTURE_SIDE)
}

/// Same as `decode`, but capping the result at `max_side` pixels per side.
pub fn decode_scaled(path: &Path, max_side: u32) -> Result<DecodedImage> {
    let reader = ImageReader::open(path)
        .with_context(|| format!("could not open {}", path.display()))?
        .with_guessed_format()
        .with_context(|| format!("could not determine the format of {}", path.display()))?;

    let mut decoder = reader
        .into_decoder()
        .with_context(|| format!("unsupported format in {}", path.display()))?;

    // `image` 0.25 doesn't apply orientation on its own: without this step,
    // portrait photos taken with a phone show up sideways. It must be read
    // before the decoder is consumed.
    let orientation = decoder.orientation().unwrap_or(image::metadata::Orientation::NoTransforms);

    let mut img = DynamicImage::from_decoder(decoder)
        .with_context(|| format!("could not decode {}", path.display()))?;
    img.apply_orientation(orientation);

    Ok(to_decoded(img, max_side))
}

/// Converts to RGBA8, downscaling if it exceeds the requested limit.
fn to_decoded(img: DynamicImage, max_side: u32) -> DecodedImage {
    let (original_width, original_height) = (img.width(), img.height());

    let img = if original_width > max_side || original_height > max_side {
        // `resize` preserves the aspect ratio and fits within the given box.
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
