//! Decoding tests on real files generated on the fly.

use std::io::Cursor;
use std::path::PathBuf;

use image::{ImageFormat, RgbImage};

/// A temporary directory owned by the test, removed when it ends.
struct TempDir(PathBuf);

impl TempDir {
    fn new(name: &str) -> Self {
        let dir = std::env::temp_dir().join(format!("checheviewer-test-{name}"));
        let _ = std::fs::remove_dir_all(&dir);
        std::fs::create_dir_all(&dir).expect("could not create the temporary directory");
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

/// Minimal APP1/EXIF segment with a single tag: Orientation (0x0112).
fn exif_app1(orientation: u16) -> Vec<u8> {
    let mut tiff = Vec::new();
    tiff.extend_from_slice(b"II"); // little-endian
    tiff.extend_from_slice(&42u16.to_le_bytes());
    tiff.extend_from_slice(&8u32.to_le_bytes()); // IFD0 offset
    tiff.extend_from_slice(&1u16.to_le_bytes()); // one entry
    tiff.extend_from_slice(&0x0112u16.to_le_bytes()); // Orientation tag
    tiff.extend_from_slice(&3u16.to_le_bytes()); // SHORT type
    tiff.extend_from_slice(&1u32.to_le_bytes()); // count
    tiff.extend_from_slice(&orientation.to_le_bytes());
    tiff.extend_from_slice(&0u16.to_le_bytes()); // value padding
    tiff.extend_from_slice(&0u32.to_le_bytes()); // no next IFD

    let mut payload = b"Exif\x00\x00".to_vec();
    payload.extend_from_slice(&tiff);

    let mut app1 = vec![0xFF, 0xE1];
    app1.extend_from_slice(&((payload.len() + 2) as u16).to_be_bytes());
    app1.extend_from_slice(&payload);
    app1
}

/// Writes a `w`×`h` landscape JPEG with the given EXIF orientation.
fn write_jpeg_with_orientation(path: &std::path::Path, w: u32, h: u32, orientation: u16) {
    let img = RgbImage::from_fn(w, h, |x, _| image::Rgb([(x % 256) as u8, 0, 0]));
    let mut jpeg = Vec::new();
    img.write_to(&mut Cursor::new(&mut jpeg), ImageFormat::Jpeg).expect("could not encode");

    assert_eq!(&jpeg[..2], &[0xFF, 0xD8], "the encoder did not produce an SOI");
    let mut out = jpeg[..2].to_vec();
    out.extend_from_slice(&exif_app1(orientation)); // APP1 goes right after the SOI
    out.extend_from_slice(&jpeg[2..]);
    std::fs::write(path, out).expect("could not write the JPEG");
}

/// A portrait phone photo arrives as landscape with an EXIF tag that says how
/// to rotate it. If it is not applied, the photo shows up lying on its side.
#[test]
fn applies_exif_orientation() {
    let dir = TempDir::new("orientation");
    let path = dir.join("portrait.jpg");
    write_jpeg_with_orientation(&path, 90, 60, 6); // 6 = rotate 90° clockwise

    let decoded = checheviewer::decode::decode_full(&path).expect("should decode");
    assert_eq!(
        (decoded.width, decoded.height),
        (60, 90),
        "EXIF orientation was not applied: the image is lying on its side"
    );
}

#[test]
fn without_exif_tag_keeps_dimensions() {
    let dir = TempDir::new("no-exif");
    let path = dir.join("landscape.jpg");
    let img = RgbImage::from_pixel(90, 60, image::Rgb([10, 20, 30]));
    img.save(&path).expect("could not save");

    let decoded = checheviewer::decode::decode_full(&path).expect("should decode");
    assert_eq!((decoded.width, decoded.height), (90, 60));
}

/// An unreadable file has to return an error, not panic: the user's folder
/// can contain anything.
#[test]
fn corrupt_file_errors_instead_of_panicking() {
    let dir = TempDir::new("corrupt");
    let path = dir.join("broken.jpg");
    std::fs::write(&path, vec![0x42; 4096]).expect("could not write");

    assert!(checheviewer::decode::decode_full(&path).is_err());
}

/// A JPEG with a .png extension still has to open: the format is detected by
/// content, not by name.
#[test]
fn detects_format_by_content_not_extension() {
    let dir = TempDir::new("lying-extension");
    let path = dir.join("liar.png");
    let img = RgbImage::from_pixel(40, 30, image::Rgb([1, 2, 3]));
    let mut jpeg = Vec::new();
    img.write_to(&mut Cursor::new(&mut jpeg), ImageFormat::Jpeg).expect("could not encode");
    std::fs::write(&path, jpeg).expect("could not write");

    let decoded = checheviewer::decode::decode_full(&path).expect("should decode");
    assert_eq!((decoded.width, decoded.height), (40, 30));
}
