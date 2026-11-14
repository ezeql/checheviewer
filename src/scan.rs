use std::path::Path;

use anyhow::{Context, Result};

use crate::model::Entry;

/// Extensiones que sabemos abrir. Las de macOS (HEIC/RAW) se resuelven vía
/// ImageIO; el resto vía el crate `image`. Ver `decode::decode_full`.
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    // crate `image`
    "png", "jpg", "jpeg", "jpe", "jfif", "gif", "webp", "tif", "tiff", "bmp", "ico", "avif", "pnm",
    "pbm", "pgm", "ppm", "tga", "dds", "ff", "qoi", // ImageIO (macOS)
    "heic", "heif", "cr2", "cr3", "nef", "arw", "dng", "orf", "rw2", "raf", "srw",
];

/// ¿La ruta tiene una extensión que sabemos abrir?
pub fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let e = e.to_ascii_lowercase();
            SUPPORTED_EXTENSIONS.contains(&e.as_str())
        })
        .unwrap_or(false)
}

/// Lista las imágenes de un directorio (no recursivo), ordenadas
/// naturalmente: `img2.jpg` va antes que `img10.jpg`, como en Finder.
///
/// Ignora archivos ocultos, subdirectorios y todo lo que no tenga una
/// extensión conocida. Las entradas ilegibles se saltean en silencio en lugar
/// de abortar el escaneo entero.
pub fn scan_dir(dir: &Path) -> Result<Vec<Entry>> {
    let read = std::fs::read_dir(dir)
        .with_context(|| format!("no se pudo leer el directorio {}", dir.display()))?;

    let mut entries: Vec<Entry> = read
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| p.is_file() && is_supported(p) && !is_hidden(p))
        .map(Entry::new)
        .collect();

    sort_natural(&mut entries);
    Ok(entries)
}

fn is_hidden(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with('.')).unwrap_or(false)
}

/// Orden natural e insensible a mayúsculas, con el nombre completo como
/// desempate para que el orden sea estable ante nombres que solo difieren en
/// mayúsculas.
fn sort_natural(entries: &mut [Entry]) {
    entries.sort_by(|a, b| {
        natord::compare_ignore_case(&a.file_name, &b.file_name)
            .then_with(|| a.file_name.cmp(&b.file_name))
    });
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::path::PathBuf;

    fn names(entries: &[Entry]) -> Vec<&str> {
        entries.iter().map(|e| e.file_name.as_str()).collect()
    }

    fn entries_from(names: &[&str]) -> Vec<Entry> {
        names.iter().map(|n| Entry::new(PathBuf::from(n))).collect()
    }

    #[test]
    fn ordena_numeros_naturalmente() {
        let mut e = entries_from(&["img10.jpg", "img2.jpg", "img1.jpg"]);
        sort_natural(&mut e);
        assert_eq!(names(&e), ["img1.jpg", "img2.jpg", "img10.jpg"]);
    }

    #[test]
    fn ordena_ignorando_mayusculas() {
        let mut e = entries_from(&["Zebra.png", "apple.png", "Banana.png"]);
        sort_natural(&mut e);
        assert_eq!(names(&e), ["apple.png", "Banana.png", "Zebra.png"]);
    }

    #[test]
    fn el_orden_es_estable_con_nombres_que_solo_difieren_en_mayusculas() {
        let mut e = entries_from(&["FOO.png", "foo.png"]);
        sort_natural(&mut e);
        assert_eq!(names(&e), ["FOO.png", "foo.png"]);
    }

    #[test]
    fn reconoce_extensiones_soportadas() {
        assert!(is_supported(Path::new("a.jpg")));
        assert!(is_supported(Path::new("a.JPEG")));
        assert!(is_supported(Path::new("a.HEIC")));
        assert!(!is_supported(Path::new("a.txt")));
        assert!(!is_supported(Path::new("a")));
        assert!(!is_supported(Path::new("jpg")));
    }

    #[test]
    fn detecta_archivos_ocultos() {
        assert!(is_hidden(Path::new("/x/.DS_Store")));
        assert!(is_hidden(Path::new("/x/._foto.jpg")));
        assert!(!is_hidden(Path::new("/x/foto.jpg")));
    }
}
