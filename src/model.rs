use std::path::PathBuf;

/// Una imagen del directorio actual.
#[derive(Debug, Clone, PartialEq, Eq)]
pub struct Entry {
    pub path: PathBuf,
    /// Nombre del archivo, cacheado para no re-derivarlo en cada frame.
    pub file_name: String,
}

impl Entry {
    pub fn new(path: PathBuf) -> Self {
        let file_name =
            path.file_name().map(|n| n.to_string_lossy().into_owned()).unwrap_or_default();
        Self { path, file_name }
    }
}

/// Cuál de las dos vistas está activa.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Default)]
pub enum ViewMode {
    /// Grilla de miniaturas del directorio.
    #[default]
    Grid,
    /// Una sola imagen a pantalla completa, navegable con las flechas.
    Viewer,
}
