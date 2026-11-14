use std::path::PathBuf;

/// An image in the current directory.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    /// File name, cached so it isn't re-derived every frame.
    pub file_name: String,
}

impl Entry {
    pub fn new(path: PathBuf) -> Self {
        let file_name =
            path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Self { path, file_name }
    }
}

/// Which of the two views is active.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// Grid of thumbnails for the directory.
    #[default]
    Grid,
    /// A single full-screen image, navigable with the arrow keys.
    Viewer,
}
