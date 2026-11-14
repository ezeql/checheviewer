//! Tests de decodificación sobre archivos reales generados al vuelo.

use std::io::Cursor;
use std::path::PathBuf;

use image::{ImageFormat, RgbImage};

/// Directorio temporal propio del test, borrado al terminar.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("checheviewer-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("no se pudo crear el directorio temporal");
        Self(dir)
    }

    fn join(&self, name: &str) -> PathBuf {
        self.0.join(name)
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = std::fs::remove_dir_all(&self.0);
    }
}

/// Segmento APP1/EXIF mínimo con un solo tag: Orientation (0x0112).
fn exif_app1(orientation: u16) -> Vec<u8> {
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II"); // little-endian
    tiff.extend_from_slice(&42u16.to_le_bytes());
    tiff.extend_from_slice(&8u32.to_le_bytes()); // offset del IFD0
    tiff.extend_from_slice(&1u16.to_le_bytes()); // una entrada
    tiff.extend_from_slice(&0x0112u16.to_le_bytes()); // tag Orientation
    tiff.extend_from_slice(&3u16.to_le_bytes()); // tipo SHORT
    tiff.extend_from_slice(&1u32.to_le_bytes()); // count
    tiff.extend_from_slice(&orientation.to_le_bytes());
    tiff.extend_from_slice(&0u16.to_le_bytes()); // relleno del valor
    tiff.extend_from_slice(&0u32.to_le_bytes()); // sin IFD siguiente

    let mut payload = b"Exif\x00\x00".to_vec();
    payload.extend_from_slice(&tiff);

    let mut app1 = vec![0xFF, 0xE1];
    app1.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    app1.extend_from_slice(&payload);
    app1
}

/// Escribe un JPEG apaisado de `w`×`h` con la orientación EXIF indicada.
fn write_jpeg_with_orientation(path: &std::path::Path, w: u32, h: u32, orientation: u16) {
    let img = RgbImage::from_fn(w, h, |x, _| image::Rgb([(x % 256) as u8, 0, 0]));
    let mut jpeg = Vec::new();
    img.write_to(&mut Cursor::new(&mut jpeg), ImageFormat::Jpeg).expect("no se pudo codificar");

    assert_eq!(&jpeg[..2], &[0xFF, 0xD8], "el encoder no produjo un SOI");
    let mut out = jpeg[..2].to_vec();
    out.extend_from_slice(&exif_app1(orientation)); // el APP1 va justo después del SOI
    out.extend_from_slice(&jpeg[2..]);
    std::fs::write(path, out).expect("no se pudo escribir el JPEG");
}

/// Una foto vertical sacada con el celular llega apaisada con un tag EXIF que
/// dice cómo rotarla. Si no se aplica, se ve acostada.
#[test]
fn aplica_la_orientacion_exif() {
    let dir = TempDir::new("orientacion");
    let path = dir.join("vertical.jpg");
    write_jpeg_with_orientation(&path, 90, 60, 6); // 6 = rotar 90° en sentido horario

    let decoded = checheviewer::decode::decode_full(&path).expect("debería decodificar");
    assert_eq!(
        (decoded.width, decoded.height),
        (60, 90),
        "la orientación EXIF no se aplicó: la imagen quedó acostada"
    );
}

#[test]
fn sin_tag_exif_conserva_las_dimensiones() {
    let dir = TempDir::new("sin-exif");
    let path = dir.join("apaisada.jpg");
    let img = RgbImage::from_pixel(90, 60, image::Rgb([10, 20, 30]));
    img.save(&path).expect("no se pudo guardar");

    let decoded = checheviewer::decode::decode_full(&path).expect("debería decodificar");
    assert_eq!((decoded.width, decoded.height), (90, 60));
}

/// Un archivo ilegible tiene que devolver un error, no entrar en pánico: la
/// carpeta del usuario puede tener cualquier cosa.
#[test]
fn un_archivo_corrupto_da_error_en_vez_de_panic() {
    let dir = TempDir::new("corrupto");
    let path = dir.join("roto.jpg");
    std::fs::write(&path, vec![0x42; 4096]).expect("no se pudo escribir");

    assert!(checheviewer::decode::decode_full(&path).is_err());
}

/// Un JPEG con extensión .png tiene que abrirse igual: se detecta por
/// contenido, no por nombre.
#[test]
fn detecta_el_formato_por_contenido_y_no_por_extension() {
    let dir = TempDir::new("extension-mentirosa");
    let path = dir.join("mentiroso.png");
    let img = RgbImage::from_pixel(40, 30, image::Rgb([1, 2, 3]));
    let mut jpeg = Vec::new();
    img.write_to(&mut Cursor::new(&mut jpeg), ImageFormat::Jpeg).expect("no se pudo codificar");
    std::fs::write(&path, jpeg).expect("no se pudo escribir");

    let decoded = checheviewer::decode::decode_full(&path).expect("debería decodificar");
    assert_eq!((decoded.width, decoded.height), (40, 30));
}
