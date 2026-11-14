use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::model::{Entry, ViewMode};
use crate::scan;

/// El estado de navegación: qué carpeta está abierta, qué imágenes tiene y en
/// cuál estamos parados.
///
/// No sabe nada de egui ni de texturas, así que se puede testear sin abrir una
/// ventana. `App` le agrega encima la carga de imágenes y el dibujado.
#[derive(Default)]
pub struct Session {
    pub dir: Option<PathBuf>,
    /// Fuente de verdad: las imágenes del directorio, ya ordenadas.
    pub entries: Vec<Entry>,
    /// Índice de la imagen actual. Siempre válido mientras `entries` no esté
    /// vacío.
    index: usize,
    pub mode: ViewMode,
}

impl Session {
    /// Abre un archivo o un directorio.
    ///
    /// Con un archivo: escanea el directorio que lo contiene, se posiciona en
    /// él y entra al visor — es el caso de "abrir con" desde Finder.
    /// Con un directorio: lo escanea y muestra la grilla.
    pub fn open_path(&mut self, path: &Path) -> Result<()> {
        // Sin esto, `foto.jpg` tiene como padre `""` y `read_dir("")` falla.
        // `absolute` no resuelve symlinks: un alias a una foto sigue abriendo
        // la carpeta del alias, no la del original.
        let path = &std::path::absolute(path)
            .with_context(|| format!("ruta inválida: {}", path.display()))?;
        let (dir, target) = if path.is_dir() {
            (path.to_path_buf(), None)
        } else {
            let parent = path
                .parent()
                .ok_or_else(|| anyhow::anyhow!("ruta sin directorio padre: {}", path.display()))?;
            (parent.to_path_buf(), Some(path.to_path_buf()))
        };

        let entries = scan::scan_dir(&dir)?;
        self.index =
            target.as_ref().and_then(|t| entries.iter().position(|e| &e.path == t)).unwrap_or(0);
        self.mode = if target.is_some() { ViewMode::Viewer } else { ViewMode::Grid };
        self.entries = entries;
        self.dir = Some(dir);
        Ok(())
    }

    pub fn index(&self) -> usize {
        self.index
    }

    pub fn len(&self) -> usize {
        self.entries.len()
    }

    pub fn is_empty(&self) -> bool {
        self.entries.is_empty()
    }

    pub fn current(&self) -> Option<&Entry> {
        self.entries.get(self.index)
    }

    pub fn is_first(&self) -> bool {
        self.index == 0
    }

    pub fn is_last(&self) -> bool {
        self.index + 1 >= self.entries.len()
    }

    /// Mueve el índice `delta` posiciones, cortando en los extremos en lugar
    /// de dar la vuelta: al llegar al final de la carpeta uno quiere saberlo,
    /// no volver silenciosamente al principio.
    ///
    /// Devuelve `true` si el índice cambió.
    pub fn step(&mut self, delta: isize) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        let last = (self.entries.len() - 1) as isize;
        let next = (self.index as isize + delta).clamp(0, last) as usize;
        self.set_index(next)
    }

    /// Salta a un índice concreto. Devuelve `true` si el índice cambió.
    pub fn set_index(&mut self, index: usize) -> bool {
        if index >= self.entries.len() || index == self.index {
            return false;
        }
        self.index = index;
        true
    }

    pub fn go_first(&mut self) -> bool {
        self.set_index(0)
    }

    pub fn go_last(&mut self) -> bool {
        self.set_index(self.entries.len().saturating_sub(1))
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    /// Directorio temporal con imágenes vacías, borrado al terminar el test.
    struct TempDir(PathBuf);

    impl TempDir {
        fn with_files(name: &str, files: &[&str]) -> Self {
            let dir = std::env::temp_dir().join(format!("checheviewer-session-{name}"));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            for f in files {
                std::fs::write(dir.join(f), b"").unwrap();
            }
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    fn names(session: &Session) -> Vec<&str> {
        session.entries.iter().map(|e| e.file_name.as_str()).collect()
    }

    #[test]
    fn abrir_un_directorio_lista_solo_las_imagenes_y_muestra_la_grilla() {
        let dir = TempDir::with_files(
            "listado",
            &["b.jpg", "a.png", "notas.txt", ".oculta.png", "sin_extension"],
        );
        let mut session = Session::default();
        session.open_path(&dir.0).unwrap();

        assert_eq!(names(&session), ["a.png", "b.jpg"]);
        assert_eq!(session.index(), 0);
        assert_eq!(session.mode, ViewMode::Grid);
    }

    #[test]
    fn abrir_un_archivo_se_posiciona_en_el_y_entra_al_visor() {
        let dir = TempDir::with_files("posicion", &["a.png", "b.png", "c.png"]);
        let mut session = Session::default();
        session.open_path(&dir.0.join("c.png")).unwrap();

        assert_eq!(session.index(), 2);
        assert_eq!(session.current().unwrap().file_name, "c.png");
        assert_eq!(session.mode, ViewMode::Viewer);
    }

    #[test]
    fn las_flechas_recorren_el_directorio_y_frenan_en_los_extremos() {
        let dir = TempDir::with_files("navegacion", &["a.png", "b.png", "c.png"]);
        let mut session = Session::default();
        session.open_path(&dir.0).unwrap();

        assert!(session.step(1));
        assert_eq!(session.index(), 1);
        assert!(session.step(1));
        assert_eq!(session.index(), 2);

        // En el final no da la vuelta ni se pasa de rango.
        assert!(!session.step(1), "no debería moverse más allá de la última");
        assert_eq!(session.index(), 2);

        assert!(session.step(-1));
        assert_eq!(session.index(), 1);
        session.step(-1);
        assert!(!session.step(-1), "no debería moverse antes de la primera");
        assert_eq!(session.index(), 0);
    }

    #[test]
    fn home_y_end_van_a_los_extremos() {
        let dir = TempDir::with_files("extremos", &["a.png", "b.png", "c.png"]);
        let mut session = Session::default();
        session.open_path(&dir.0).unwrap();

        assert!(session.go_last());
        assert_eq!(session.index(), 2);
        assert!(session.is_last());
        assert!(session.go_first());
        assert_eq!(session.index(), 0);
        assert!(session.is_first());
    }

    #[test]
    fn una_carpeta_sin_imagenes_no_rompe_la_navegacion() {
        let dir = TempDir::with_files("vacia", &["notas.txt"]);
        let mut session = Session::default();
        session.open_path(&dir.0).unwrap();

        assert!(session.is_empty());
        assert!(session.current().is_none());
        assert!(!session.step(1));
        assert!(!session.go_last());
        assert_eq!(session.index(), 0);
    }

    #[test]
    fn abrir_otra_carpeta_reinicia_la_posicion() {
        let first = TempDir::with_files("primera", &["a.png", "b.png", "c.png"]);
        let second = TempDir::with_files("segunda", &["x.png"]);

        let mut session = Session::default();
        session.open_path(&first.0).unwrap();
        session.go_last();
        assert_eq!(session.index(), 2);

        session.open_path(&second.0).unwrap();
        assert_eq!(session.index(), 0, "el índice viejo no debe sobrevivir al cambio de carpeta");
        assert_eq!(names(&session), ["x.png"]);
    }

    /// `checheviewer foto.jpg` desde la terminal: la ruta no tiene carpeta y
    /// su padre es `""`. `cargo test` corre con el directorio del crate como
    /// directorio actual, así que el archivo se crea ahí y se borra al salir.
    #[test]
    fn una_ruta_relativa_sin_carpeta_abre_el_directorio_actual() {
        struct Borrar(&'static str);
        impl Drop for Borrar {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(self.0);
            }
        }
        let name = "checheviewer-test-ruta-relativa.png";
        std::fs::write(name, b"").unwrap();
        let _borrar = Borrar(name);

        let mut session = Session::default();
        session.open_path(Path::new(name)).expect("una ruta relativa debería abrirse");
        assert_eq!(session.current().unwrap().file_name, name);
        assert_eq!(session.mode, ViewMode::Viewer);
    }

    #[test]
    fn un_directorio_inexistente_da_error_sin_tocar_el_estado() {
        let dir = TempDir::with_files("intacto", &["a.png"]);
        let mut session = Session::default();
        session.open_path(&dir.0).unwrap();

        assert!(session.open_path(Path::new("/no/existe/esta/carpeta")).is_err());
        assert_eq!(names(&session), ["a.png"], "el estado previo debe seguir intacto");
    }
}
