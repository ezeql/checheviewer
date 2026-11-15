//! Decoding with ImageIO, the macOS image engine.
//!
//! It provides two things the `image` crate can't:
//!
//! 1. **Apple formats**: HEIC/HEIF (iPhone photos) and camera RAW, without
//!    depending on Homebrew libraries.
//! 2. **Nearly free thumbnails**: `CGImageSourceCreateThumbnailAtIndex` uses
//!    the thumbnail already embedded in the EXIF instead of decoding the whole
//!    photo. It's the difference between a grid that appears instantly and
//!    one that takes twenty seconds.
//!
//! All of the app's `unsafe` lives here; the rest of the code sees a normal
//! function that returns a `Result`.

use std::ffi::c_void;
use std::path::Path;

use anyhow::{anyhow, bail, Context, Result};
use objc2_core_foundation::{
    kCFBooleanTrue, CFDictionary, CFNumber, CFNumberType, CFRetained, CFString, CFType, CGPoint,
    CGRect, CGSize, CFURL,
};
use objc2_core_graphics::{
    CGBitmapContextCreate, CGColorSpace, CGContext, CGImage, CGImageAlphaInfo,
};
use objc2_image_io::{
    kCGImagePropertyOrientation, kCGImagePropertyPixelHeight, kCGImagePropertyPixelWidth,
    kCGImageSourceCreateThumbnailFromImageAlways, kCGImageSourceCreateThumbnailFromImageIfAbsent,
    kCGImageSourceCreateThumbnailWithTransform, kCGImageSourceThumbnailMaxPixelSize, CGImageSource,
};

use super::{DecodedImage, Quality};

/// Decodes `path` to premultiplied RGBA, capped at `max_side` pixels per side
/// and with EXIF orientation already applied.
pub fn decode(path: &Path, max_side: u32, quality: Quality) -> Result<DecodedImage> {
    let url = file_url(path)?;

    // SAFETY: `url` is a valid CFURL and the options are `None`.
    let source = unsafe { CGImageSource::with_url(&url, None) }
        .ok_or_else(|| anyhow!("ImageIO could not open {}", path.display()))?;

    let (original_width, original_height) = source_pixel_size(&source);

    let options = thumbnail_options(max_side, quality);
    // SAFETY: `options` has keys and values of the types ImageIO expects (see
    // `thumbnail_options`).
    let image = unsafe { source.thumbnail_at_index(0, Some(options.as_opaque())) }
        .ok_or_else(|| anyhow!("ImageIO could not decode {}", path.display()))?;

    let (mut pixels, mut width, mut height) =
        to_rgba(&image).with_context(|| format!("could not convert to RGBA {}", path.display()))?;

    // Many cameras embed a 160×120 (4:3) thumbnail with black bars for a 3:2
    // photo: half the photos in a real test folder. Dropping the bars keeps it
    // from showing up squashed or letterboxed.
    if let (Quality::Thumbnail, Some(w), Some(h)) = (quality, original_width, original_height) {
        (pixels, width, height) = strip_black_bars(pixels, width, height, w as f64 / h as f64);
    }

    Ok(DecodedImage {
        width,
        height,
        pixels,
        // If the properties couldn't be read, the best we have is the size of
        // what we actually decoded.
        original_width: original_width.unwrap_or(width),
        original_height: original_height.unwrap_or(height),
        premultiplied: true,
    })
}

/// Crops an RGBA thumbnail around its center to `aspect` (width / height),
/// but only when it is off by more than 2 % and the bands to drop are black:
/// a letterbox. Anything else is returned untouched, so metadata that
/// disagrees with the pixels can never cut real content.
fn strip_black_bars(pixels: Vec<u8>, w: u32, h: u32, aspect: f64) -> (Vec<u8>, u32, u32) {
    let (wf, hf) = (w as f64, h as f64);
    if w == 0 || h == 0 || ((wf / hf) / aspect - 1.0).abs() <= 0.02 {
        return (pixels, w, h);
    }
    let (cw, ch) = if wf / hf > aspect {
        (((hf * aspect).round() as u32).min(w), h)
    } else {
        (w, ((wf / aspect).round() as u32).min(h))
    };
    let (x, y) = ((w - cw) / 2, (h - ch) / 2);

    // Mean brightness of what would be cut: JPEG ringing lifts the rows next
    // to the photo a little, so this is an average, not a per-pixel test.
    let (mut sum, mut count) = (0u64, 0u64);
    for py in 0..h {
        for px in 0..w {
            if (x..x + cw).contains(&px) && (y..y + ch).contains(&py) {
                continue;
            }
            let i = ((py * w + px) * 4) as usize;
            sum += pixels[i..i + 3].iter().map(|&c| c as u64).sum::<u64>();
            count += 3;
        }
    }
    if count == 0 || sum / count > 16 {
        return (pixels, w, h);
    }

    let mut cropped = Vec::with_capacity((cw * ch * 4) as usize);
    for row in y..y + ch {
        let start = ((row * w + x) * 4) as usize;
        cropped.extend_from_slice(&pixels[start..start + (cw * 4) as usize]);
    }
    (cropped, cw, ch)
}

/// CFURL built from a filesystem path.
fn file_url(path: &Path) -> Result<CFRetained<CFURL>> {
    use std::os::unix::ffi::OsStrExt;
    let bytes = path.as_os_str().as_bytes();

    // SAFETY: `bytes` points to a valid buffer of the length passed, and
    // stays alive for the duration of the call.
    unsafe {
        CFURL::from_file_system_representation(None, bytes.as_ptr(), bytes.len() as isize, false)
    }
    .ok_or_else(|| anyhow!("path not representable as a URL: {}", path.display()))
}

/// Options for `thumbnail_at_index`.
///
/// `CreateThumbnailWithTransform` is what applies EXIF orientation: without
/// it, portrait photos come out sideways.
fn thumbnail_options(
    max_side: u32,
    quality: Quality,
) -> CFRetained<CFDictionary<CFString, CFType>> {
    let source_key = match quality {
        // SAFETY: static constants from the framework.
        Quality::Thumbnail => unsafe { kCGImageSourceCreateThumbnailFromImageIfAbsent },
        Quality::Full => unsafe { kCGImageSourceCreateThumbnailFromImageAlways },
    };
    let transform_key = unsafe { kCGImageSourceCreateThumbnailWithTransform };
    let max_size_key = unsafe { kCGImageSourceThumbnailMaxPixelSize };

    let yes = unsafe { kCFBooleanTrue }.expect("kCFBooleanTrue always exists");
    let max_size = CFNumber::new_i32(max_side as i32);

    let keys: [&CFString; 3] = [source_key, transform_key, max_size_key];
    let values: [&CFType; 3] = [
        // All CoreFoundation types are CFType.
        yes.as_ref(),
        yes.as_ref(),
        max_size.as_ref(),
    ];
    CFDictionary::from_slices(&keys, &values)
}

/// Real dimensions of the file, read from the metadata without decoding the
/// pixels. Returns `(None, None)` if the format doesn't declare them.
fn source_pixel_size(source: &CGImageSource) -> (Option<u32>, Option<u32>) {
    // SAFETY: index 0 always exists if the source opened, and we pass no
    // options.
    let Some(properties) = (unsafe { source.properties_at_index(0, None) }) else {
        return (None, None);
    };

    let width = dict_u32(&properties, unsafe { kCGImagePropertyPixelWidth });
    let height = dict_u32(&properties, unsafe { kCGImagePropertyPixelHeight });
    let orientation = dict_u32(&properties, unsafe { kCGImagePropertyOrientation }).unwrap_or(1);

    // Orientations 5 to 8 imply a 90° rotation: the width and height the user
    // sees are swapped relative to the sensor's.
    if orientation >= 5 {
        (height, width)
    } else {
        (width, height)
    }
}

fn dict_u32(dict: &CFDictionary, key: &CFString) -> Option<u32> {
    // SAFETY: `key` is a valid CFString; the returned value is borrowed from
    // the dictionary, which is still alive.
    let value = unsafe { dict.value(key as *const CFString as *const c_void) };
    if value.is_null() {
        return None;
    }

    // SAFETY: the keys we query are numeric according to the ImageIO
    // documentation.
    let number = unsafe { &*(value as *const CFNumber) };
    let mut out: i64 = 0;
    let ok = unsafe { number.value(CFNumberType::SInt64Type, &mut out as *mut i64 as *mut c_void) };
    if ok && out > 0 {
        u32::try_from(out).ok()
    } else {
        None
    }
}

/// Draws the `CGImage` into an RGBA8 bitmap of our own.
///
/// Going through a bitmap context instead of reading the `CGImage`'s raw bytes
/// normalizes in one go every format it may hold (BGRA, 16 bits per
/// component, YUV from a HEIC, odd color spaces).
fn to_rgba(image: &CGImage) -> Result<(Vec<u8>, u32, u32)> {
    let width = CGImage::width(Some(image));
    let height = CGImage::height(Some(image));
    if width == 0 || height == 0 {
        bail!("the decoded image has zero size");
    }

    let bytes_per_row = width.checked_mul(4).ok_or_else(|| anyhow!("image too wide: {width}"))?;
    let len = bytes_per_row
        .checked_mul(height)
        .ok_or_else(|| anyhow!("image too large: {width}×{height}"))?;

    let color_space = CGColorSpace::new_device_rgb()
        .ok_or_else(|| anyhow!("could not create the RGB color space"))?;
    let mut pixels = vec![0u8; len];

    // SAFETY: the buffer has exactly `bytes_per_row * height` bytes and
    // outlives the context, which is destroyed at the end of this function.
    let context = unsafe {
        CGBitmapContextCreate(
            pixels.as_mut_ptr() as *mut c_void,
            width,
            height,
            8,
            bytes_per_row,
            Some(&color_space),
            CGImageAlphaInfo::PremultipliedLast.0,
        )
    }
    .ok_or_else(|| anyhow!("could not create the bitmap context"))?;

    let rect = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(width as f64, height as f64));
    CGContext::draw_image(Some(&context), rect, Some(image));
    // The context is released here; from this point on `pixels` is ours.
    drop(context);

    Ok((pixels, width as u32, height as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// The system wallpaper is a real HEIC and is always present on macOS: it
    /// works as a test case without having to version a binary.
    const SYSTEM_HEIC: &str = "/System/Library/Wallpapers/.default/DefaultAerial.heic";

    #[test]
    fn opens_a_system_heic() {
        let path = Path::new(SYSTEM_HEIC);
        if !path.exists() {
            eprintln!("skipped: {SYSTEM_HEIC} does not exist on this macOS version");
            return;
        }

        let image = decode(path, 512, Quality::Full).expect("should open the HEIC");
        assert!(image.width > 0 && image.height > 0);
        assert!(image.width <= 512 && image.height <= 512, "did not respect the size limit");
        assert_eq!(image.pixels.len(), image.width as usize * image.height as usize * 4);
        assert!(
            image.original_width >= image.width,
            "the original size should be the file's, not the downscaled copy's"
        );
    }

    #[test]
    fn thumbnail_is_smaller_than_the_limit() {
        let path = Path::new(SYSTEM_HEIC);
        if !path.exists() {
            return;
        }

        let thumb = decode(path, 256, Quality::Thumbnail).expect("should generate the thumbnail");
        assert!(thumb.width <= 256 && thumb.height <= 256);
    }

    #[test]
    fn a_non_image_file_errors() {
        let path = std::env::temp_dir().join("checheviewer-imageio-garbage.jpg");
        std::fs::write(&path, vec![0x00; 256]).unwrap();

        let result = decode(&path, 256, Quality::Full);
        let _ = std::fs::remove_file(&path);
        assert!(result.is_err(), "a garbage file should not decode");
    }

    /// A 6×6 image whose top and bottom rows are bars, for a 3:2 photo.
    fn letterboxed(bar: u8) -> Vec<u8> {
        (0..6)
            .flat_map(|row| {
                let v = if row == 0 || row == 5 { bar } else { 150 };
                [v, v, v, 255].repeat(6)
            })
            .collect()
    }

    #[test]
    fn black_bars_are_cropped_away() {
        let (pixels, w, h) = strip_black_bars(letterboxed(0), 6, 6, 1.5);
        assert_eq!((w, h), (6, 4));
        assert!(pixels.chunks(4).all(|p| p[0] == 150), "only the photo is left");
    }

    #[test]
    fn bands_with_content_are_never_cropped() {
        let (_, w, h) = strip_black_bars(letterboxed(90), 6, 6, 1.5);
        assert_eq!((w, h), (6, 6), "the shape mismatch alone is not enough");
    }

    #[test]
    fn a_missing_file_errors() {
        assert!(decode(Path::new("/does/not/exist.heic"), 256, Quality::Full).is_err());
    }
}
