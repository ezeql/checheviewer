//! Decodificación con ImageIO, el motor de imágenes de macOS.
//!
//! Aporta dos cosas que el crate `image` no puede dar:
//!
//! 1. **Formatos de Apple**: HEIC/HEIF (las fotos del iPhone) y los RAW de
//!    cámara, sin depender de bibliotecas de Homebrew.
//! 2. **Miniaturas casi gratis**: `CGImageSourceCreateThumbnailAtIndex` usa la
//!    miniatura que ya viene embebida en el EXIF en vez de decodificar la foto
//!    entera. Es la diferencia entre una grilla que aparece al instante y una
//!    que tarda veinte segundos.
//!
//! Todo el `unsafe` de la app vive acá; el resto del código ve una función
//! normal que devuelve un `Result`.

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

/// Decodifica `path` a RGBA premultiplicado, acotado a `max_side` píxeles por
/// lado y con la orientación EXIF ya aplicada.
pub fn decode(path: &Path, max_side: u32, quality: Quality) -> Result<DecodedImage> {
    let url = file_url(path)?;

    // SAFETY: `url` es un CFURL válido y las opciones son `None`.
    let source = unsafe { CGImageSource::with_url(&url, None) }
        .ok_or_else(|| anyhow!("ImageIO no pudo abrir {}", path.display()))?;

    let (original_width, original_height) = source_pixel_size(&source);

    let options = thumbnail_options(max_side, quality);
    // SAFETY: `options` tiene claves y valores de los tipos que espera
    // ImageIO (ver `thumbnail_options`).
    let image = unsafe { source.thumbnail_at_index(0, Some(options.as_opaque())) }
        .ok_or_else(|| anyhow!("ImageIO no pudo decodificar {}", path.display()))?;

    let (pixels, width, height) = to_rgba(&image)
        .with_context(|| format!("no se pudo convertir a RGBA {}", path.display()))?;

    Ok(DecodedImage {
        width,
        height,
        pixels,
        // Si no se pudieron leer las propiedades, lo mejor que tenemos es el
        // tamaño de lo que efectivamente decodificamos.
        original_width: original_width.unwrap_or(width),
        original_height: original_height.unwrap_or(height),
        premultiplied: true,
    })
}

/// CFURL a partir de una ruta del sistema de archivos.
fn file_url(path: &Path) -> Result<CFRetained<CFURL>> {
    use std::os::unix::ffi::OsStrExt;
    let bytes = path.as_os_str().as_bytes();

    // SAFETY: `bytes` apunta a un buffer válido de la longitud que se pasa, y
    // sigue vivo durante la llamada.
    unsafe {
        CFURL::from_file_system_representation(None, bytes.as_ptr(), bytes.len() as isize, false)
    }
    .ok_or_else(|| anyhow!("ruta no representable como URL: {}", path.display()))
}

/// Opciones para `thumbnail_at_index`.
///
/// `CreateThumbnailWithTransform` es lo que aplica la orientación EXIF: sin
/// eso, las fotos verticales salen acostadas.
fn thumbnail_options(
    max_side: u32,
    quality: Quality,
) -> CFRetained<CFDictionary<CFString, CFType>> {
    let source_key = match quality {
        // SAFETY: constantes estáticas del framework.
        Quality::Thumbnail => unsafe { kCGImageSourceCreateThumbnailFromImageIfAbsent },
        Quality::Full => unsafe { kCGImageSourceCreateThumbnailFromImageAlways },
    };
    let transform_key = unsafe { kCGImageSourceCreateThumbnailWithTransform };
    let max_size_key = unsafe { kCGImageSourceThumbnailMaxPixelSize };

    let yes = unsafe { kCFBooleanTrue }.expect("kCFBooleanTrue siempre existe");
    let max_size = CFNumber::new_i32(max_side as i32);

    let keys: [&CFString; 3] = [source_key, transform_key, max_size_key];
    let values: [&CFType; 3] = [
        // Todos los tipos de CoreFoundation son CFType.
        yes.as_ref(),
        yes.as_ref(),
        max_size.as_ref(),
    ];
    CFDictionary::from_slices(&keys, &values)
}

/// Dimensiones reales del archivo, leídas de los metadatos sin decodificar los
/// píxeles. Devuelve `(None, None)` si el formato no las declara.
fn source_pixel_size(source: &CGImageSource) -> (Option<u32>, Option<u32>) {
    // SAFETY: índice 0 siempre existe si la fuente se abrió, y no pasamos
    // opciones.
    let Some(properties) = (unsafe { source.properties_at_index(0, None) }) else {
        return (None, None);
    };

    let width = dict_u32(&properties, unsafe { kCGImagePropertyPixelWidth });
    let height = dict_u32(&properties, unsafe { kCGImagePropertyPixelHeight });
    let orientation = dict_u32(&properties, unsafe { kCGImagePropertyOrientation }).unwrap_or(1);

    // Las orientaciones 5 a 8 implican un giro de 90°: el ancho y el alto que
    // ve el usuario están al revés de los del sensor.
    if orientation >= 5 {
        (height, width)
    } else {
        (width, height)
    }
}

fn dict_u32(dict: &CFDictionary, key: &CFString) -> Option<u32> {
    // SAFETY: `key` es un CFString válido; el valor devuelto es prestado del
    // diccionario, que sigue vivo.
    let value = unsafe { dict.value(key as *const CFString as *const c_void) };
    if value.is_null() {
        return None;
    }

    // SAFETY: las claves que consultamos son numéricas según la
    // documentación de ImageIO.
    let number = unsafe { &*(value as *const CFNumber) };
    let mut out: i64 = 0;
    let ok = unsafe { number.value(CFNumberType::SInt64Type, &mut out as *mut i64 as *mut c_void) };
    if ok && out > 0 {
        u32::try_from(out).ok()
    } else {
        None
    }
}

/// Dibuja el `CGImage` en un bitmap RGBA8 propio.
///
/// Pasar por un contexto de bitmap en vez de leer los bytes crudos del
/// `CGImage` normaliza de una todos los formatos que puede tener adentro
/// (BGRA, 16 bits por componente, YUV de un HEIC, espacios de color raros).
fn to_rgba(image: &CGImage) -> Result<(Vec<u8>, u32, u32)> {
    let width = CGImage::width(Some(image));
    let height = CGImage::height(Some(image));
    if width == 0 || height == 0 {
        bail!("la imagen decodificada tiene tamaño cero");
    }

    let bytes_per_row =
        width.checked_mul(4).ok_or_else(|| anyhow!("imagen demasiado ancha: {width}"))?;
    let len = bytes_per_row
        .checked_mul(height)
        .ok_or_else(|| anyhow!("imagen demasiado grande: {width}×{height}"))?;

    let color_space =
        CGColorSpace::new_device_rgb().ok_or_else(|| anyhow!("no se pudo crear el espacio RGB"))?;
    let mut pixels = vec![0u8; len];

    // SAFETY: el buffer tiene exactamente `bytes_per_row * height` bytes y
    // sobrevive al contexto, que se destruye al final de esta función.
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
    .ok_or_else(|| anyhow!("no se pudo crear el contexto de bitmap"))?;

    let rect = CGRect::new(CGPoint::new(0.0, 0.0), CGSize::new(width as f64, height as f64));
    CGContext::draw_image(Some(&context), rect, Some(image));
    // El contexto se libera acá; a partir de este punto `pixels` es nuestro.
    drop(context);

    Ok((pixels, width as u32, height as u32))
}

#[cfg(test)]
mod tests {
    use super::*;

    /// El wallpaper del sistema es un HEIC real y siempre está presente en
    /// macOS: sirve como caso de prueba sin tener que versionar un binario.
    const SYSTEM_HEIC: &str = "/System/Library/Wallpapers/.default/DefaultAerial.heic";

    #[test]
    fn abre_un_heic_del_sistema() {
        let path = Path::new(SYSTEM_HEIC);
        if !path.exists() {
            eprintln!("saltado: {SYSTEM_HEIC} no existe en esta versión de macOS");
            return;
        }

        let image = decode(path, 512, Quality::Full).expect("debería abrir el HEIC");
        assert!(image.width > 0 && image.height > 0);
        assert!(image.width <= 512 && image.height <= 512, "no respetó el límite de tamaño");
        assert_eq!(image.pixels.len(), image.width as usize * image.height as usize * 4);
        assert!(
            image.original_width >= image.width,
            "el tamaño original debería ser el del archivo, no el de la copia reducida"
        );
    }

    #[test]
    fn la_miniatura_es_mas_chica_que_el_limite() {
        let path = Path::new(SYSTEM_HEIC);
        if !path.exists() {
            return;
        }

        let thumb = decode(path, 256, Quality::Thumbnail).expect("debería generar la miniatura");
        assert!(thumb.width <= 256 && thumb.height <= 256);
    }

    #[test]
    fn un_archivo_que_no_es_imagen_da_error() {
        let path = std::env::temp_dir().join("checheviewer-imageio-basura.jpg");
        std::fs::write(&path, vec![0x00; 256]).unwrap();

        let result = decode(&path, 256, Quality::Full);
        let _ = std::fs::remove_file(&path);
        assert!(result.is_err(), "un archivo basura no debería decodificar");
    }

    #[test]
    fn un_archivo_inexistente_da_error() {
        assert!(decode(Path::new("/no/existe.heic"), 256, Quality::Full).is_err());
    }
}
