use std::path::Path;

use anyhow::{Context, Result};

use crate::model::Entry;

/// Extensions we know how to open. The macOS ones (HEIC/RAW) are resolved via
/// ImageIO; the rest via the `image` crate. See `decode::decode_full`.
pub const SUPPORTED_EXTENSIONS: &[&str] = &[
    // crate `image`
    "png", "jpg", "jpeg", "jpe", "jfif", "gif", "webp", "tif", "tiff", "bmp", "ico", "avif", "pnm",
    "pbm", "pgm", "ppm", "tga", "dds", "ff", "qoi", // ImageIO (macOS)
    "heic", "heif", "cr2", "cr3", "nef", "arw", "dng", "orf", "rw2", "raf", "srw",
];

/// Does the path have an extension we know how to open?
pub fn is_supported(path: &Path) -> bool {
    path.extension()
        .and_then(|e| e.to_str())
        .map(|e| {
            let e = e.to_ascii_lowercase();
            SUPPORTED_EXTENSIONS.contains(&e.as_str())
        })
        .unwrap_or(false)
}

/// Lists the images in a directory (non-recursive), naturally sorted:
/// `img2.jpg` comes before `img10.jpg`, as in Finder.
///
/// Ignores hidden files, subdirectories and anything without a known
/// extension. Unreadable entries are silently skipped instead of aborting the
/// whole scan.
pub fn scan_dir(dir: &Path) -> Result<Vec<Entry>> {
    list(dir, |p| p.is_file() && is_supported(p))
}

/// Lists the subfolders of a directory, skipping hidden ones, in the same
/// natural order. They feed the folder browser shown when a folder has no
/// images.
pub fn scan_subdirs(dir: &Path) -> Result<Vec<Entry>> {
    list(dir, Path::is_dir)
}

fn list(dir: &Path, keep: impl Fn(&Path) -> bool) -> Result<Vec<Entry>> {
    let read = std::fs::read_dir(dir)
        .with_context(|| format!("could not read directory {}", dir.display()))?;

    let mut entries: Vec<Entry> = read
        .filter_map(|e| e.ok())
        .map(|e| e.path())
        .filter(|p| !is_hidden(p) && keep(p))
        .map(Entry::new)
        .collect();

    sort_natural(&mut entries);
    Ok(entries)
}

fn is_hidden(path: &Path) -> bool {
    path.file_name().and_then(|n| n.to_str()).map(|n| n.starts_with('.')).unwrap_or(false)
}

/// Natural, case-insensitive order, with the full name as a tie-breaker so the
/// order is stable for names that differ only in case.
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
    fn sorts_numbers_naturally() {
        let mut e = entries_from(&["img10.jpg", "img2.jpg", "img1.jpg"]);
        sort_natural(&mut e);
        assert_eq!(names(&e), ["img1.jpg", "img2.jpg", "img10.jpg"]);
    }

    #[test]
    fn sorts_ignoring_case() {
        let mut e = entries_from(&["Zebra.png", "apple.png", "Banana.png"]);
        sort_natural(&mut e);
        assert_eq!(names(&e), ["apple.png", "Banana.png", "Zebra.png"]);
    }

    #[test]
    fn order_is_stable_for_names_differing_only_in_case() {
        let mut e = entries_from(&["FOO.png", "foo.png"]);
        sort_natural(&mut e);
        assert_eq!(names(&e), ["FOO.png", "foo.png"]);
    }

    #[test]
    fn recognizes_supported_extensions() {
        assert!(is_supported(Path::new("a.jpg")));
        assert!(is_supported(Path::new("a.JPEG")));
        assert!(is_supported(Path::new("a.HEIC")));
        assert!(!is_supported(Path::new("a.txt")));
        assert!(!is_supported(Path::new("a")));
        assert!(!is_supported(Path::new("jpg")));
    }

    #[test]
    fn detects_hidden_files() {
        assert!(is_hidden(Path::new("/x/.DS_Store")));
        assert!(is_hidden(Path::new("/x/._photo.jpg")));
        assert!(!is_hidden(Path::new("/x/photo.jpg")));
    }
}
