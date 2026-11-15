use std::path::{Path, PathBuf};

use anyhow::{Context, Result};

use crate::model::{Entry, ViewMode};
use crate::scan;

/// Navigation state: which folder is open, which images it has and which one
/// we're on.
///
/// It knows nothing about egui or textures, so it can be tested without
/// opening a window. `App` adds image loading and drawing on top of it.
#[derive(Default)]
pub struct Session {
    pub dir: Option<PathBuf>,
    /// Source of truth: the directory's images, already sorted.
    pub entries: Vec<Entry>,
    /// Index of the current image. Always valid as long as `entries` is not
    /// empty.
    index: usize,
    pub mode: ViewMode,
    /// Where to go when the folder has no images: the parent, named "..", and
    /// then the subfolders. Empty when there are images.
    pub folders: Vec<Entry>,
    /// Selected row in `folders`.
    pub folder_index: usize,
}

impl Session {
    /// Opens a file or a directory.
    ///
    /// With a file: scans the directory that contains it, positions on it and
    /// enters the viewer — the "Open With" case from Finder.
    /// With a directory: scans it and shows the grid.
    pub fn open_path(&mut self, path: &Path) -> Result<()> {
        // Without this, `photo.jpg` has `""` as its parent and `read_dir("")`
        // fails. `absolute` doesn't resolve symlinks: an alias to a photo still
        // opens the alias's folder, not the original's.
        let path = &std::path::absolute(path)
            .with_context(|| format!("invalid path: {}", path.display()))?;
        let (dir, target) = if path.is_dir() {
            (path.to_path_buf(), None)
        } else {
            let parent = path.parent().ok_or_else(|| {
                anyhow::anyhow!("path has no parent directory: {}", path.display())
            })?;
            (parent.to_path_buf(), Some(path.to_path_buf()))
        };

        let entries = scan::scan_dir(&dir)?;
        self.index =
            target.as_ref().and_then(|t| entries.iter().position(|e| &e.path == t)).unwrap_or(0);
        // With nothing to show, the grid is where the folder browser lives.
        self.mode =
            if target.is_some() && !entries.is_empty() { ViewMode::Viewer } else { ViewMode::Grid };

        self.folders = Vec::new();
        if entries.is_empty() {
            if let Some(parent) = dir.parent() {
                self.folders.push(Entry { path: parent.to_path_buf(), file_name: "..".into() });
            }
            self.folders.extend(scan::scan_subdirs(&dir).unwrap_or_default());
        }
        // Going up selects the folder we came from, as file managers do, so
        // going back down is a single Enter.
        let came_from = self.dir.take();
        self.folder_index =
            self.folders.iter().position(|f| Some(&f.path) == came_from.as_ref()).unwrap_or(0);

        self.entries = entries;
        self.dir = Some(dir);
        Ok(())
    }

    /// Drops the current image after its file was deleted. The index stays
    /// put, so it lands on the next image, or on the previous one if it was
    /// the last.
    pub fn remove_current(&mut self) {
        if self.index < self.entries.len() {
            self.entries.remove(self.index);
        }
        self.index = self.index.min(self.entries.len().saturating_sub(1));
    }

    /// Moves the selection in `folders`, stopping at the ends. Returns `true`
    /// if it changed.
    pub fn step_folder(&mut self, delta: isize) -> bool {
        let last = self.folders.len().saturating_sub(1) as isize;
        let next = (self.folder_index as isize).saturating_add(delta).clamp(0, last) as usize;
        let changed = next != self.folder_index;
        self.folder_index = next;
        changed
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

    /// Moves the index `delta` positions, stopping at the ends instead of
    /// wrapping around: on reaching the end of the folder you want to know it,
    /// not silently return to the start.
    ///
    /// Returns `true` if the index changed.
    pub fn step(&mut self, delta: isize) -> bool {
        if self.entries.is_empty() {
            return false;
        }
        let last = (self.entries.len() - 1) as isize;
        let next = (self.index as isize + delta).clamp(0, last) as usize;
        self.set_index(next)
    }

    /// Jumps to a specific index. Returns `true` if the index changed.
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

    /// Temporary directory with empty images, deleted when the test ends.
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
    fn opening_a_directory_lists_only_images_and_shows_the_grid() {
        let dir = TempDir::with_files(
            "listing",
            &["b.jpg", "a.png", "notes.txt", ".hidden.png", "no_extension"],
        );
        let mut session = Session::default();
        session.open_path(&dir.0).unwrap();

        assert_eq!(names(&session), ["a.png", "b.jpg"]);
        assert_eq!(session.index(), 0);
        assert_eq!(session.mode, ViewMode::Grid);
    }

    #[test]
    fn opening_a_file_positions_on_it_and_enters_the_viewer() {
        let dir = TempDir::with_files("position", &["a.png", "b.png", "c.png"]);
        let mut session = Session::default();
        session.open_path(&dir.0.join("c.png")).unwrap();

        assert_eq!(session.index(), 2);
        assert_eq!(session.current().unwrap().file_name, "c.png");
        assert_eq!(session.mode, ViewMode::Viewer);
    }

    #[test]
    fn arrows_walk_the_directory_and_stop_at_the_ends() {
        let dir = TempDir::with_files("navigation", &["a.png", "b.png", "c.png"]);
        let mut session = Session::default();
        session.open_path(&dir.0).unwrap();

        assert!(session.step(1));
        assert_eq!(session.index(), 1);
        assert!(session.step(1));
        assert_eq!(session.index(), 2);

        // At the end it neither wraps around nor goes out of range.
        assert!(!session.step(1), "should not move past the last");
        assert_eq!(session.index(), 2);

        assert!(session.step(-1));
        assert_eq!(session.index(), 1);
        session.step(-1);
        assert!(!session.step(-1), "should not move before the first");
        assert_eq!(session.index(), 0);
    }

    #[test]
    fn home_and_end_go_to_the_ends() {
        let dir = TempDir::with_files("ends", &["a.png", "b.png", "c.png"]);
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
    fn a_folder_without_images_does_not_break_navigation() {
        let dir = TempDir::with_files("empty", &["notes.txt"]);
        let mut session = Session::default();
        session.open_path(&dir.0).unwrap();

        assert!(session.is_empty());
        assert!(session.current().is_none());
        assert!(!session.step(1));
        assert!(!session.go_last());
        assert_eq!(session.index(), 0);
    }

    #[test]
    fn opening_another_folder_resets_the_position() {
        let first = TempDir::with_files("first", &["a.png", "b.png", "c.png"]);
        let second = TempDir::with_files("second", &["x.png"]);

        let mut session = Session::default();
        session.open_path(&first.0).unwrap();
        session.go_last();
        assert_eq!(session.index(), 2);

        session.open_path(&second.0).unwrap();
        assert_eq!(session.index(), 0, "the old index must not survive a folder change");
        assert_eq!(names(&session), ["x.png"]);
    }

    /// `checheviewer photo.jpg` from the terminal: the path has no folder and
    /// its parent is `""`. `cargo test` runs with the crate directory as the
    /// current directory, so the file is created there and deleted on exit.
    #[test]
    fn a_relative_path_without_folder_opens_the_current_directory() {
        struct Remove(&'static str);
        impl Drop for Remove {
            fn drop(&mut self) {
                let _ = std::fs::remove_file(self.0);
            }
        }
        let name = "checheviewer-test-relative-path.png";
        std::fs::write(name, b"").unwrap();
        let _remove = Remove(name);

        let mut session = Session::default();
        session.open_path(Path::new(name)).expect("a relative path should open");
        assert_eq!(session.current().unwrap().file_name, name);
        assert_eq!(session.mode, ViewMode::Viewer);
    }

    #[test]
    fn a_folder_without_images_lists_the_parent_and_its_subfolders() {
        let dir = TempDir::with_files("browser", &["notes.txt"]);
        for sub in ["b10", "b2", ".hidden"] {
            std::fs::create_dir(dir.0.join(sub)).unwrap();
        }
        let mut session = Session::default();
        session.open_path(&dir.0.join("notes.txt")).unwrap();

        let folders: Vec<_> = session.folders.iter().map(|f| f.file_name.as_str()).collect();
        assert_eq!(folders, ["..", "b2", "b10"]);
        assert_eq!(session.mode, ViewMode::Grid, "the browser lives in the grid");

        std::fs::write(dir.0.join("a.png"), b"").unwrap();
        session.open_path(&dir.0).unwrap();
        assert!(session.folders.is_empty(), "with images there is nothing to browse");
    }

    #[test]
    fn going_up_selects_the_folder_we_came_from() {
        let dir = TempDir::with_files("came-from", &[]);
        for sub in ["a", "b", "c"] {
            std::fs::create_dir(dir.0.join(sub)).unwrap();
        }
        let mut session = Session::default();
        session.open_path(&dir.0.join("b")).unwrap();
        session.open_path(&dir.0).unwrap();

        assert_eq!(session.folders[session.folder_index].file_name, "b");
    }

    #[test]
    fn removing_the_current_image_moves_to_the_next_or_the_previous() {
        let dir = TempDir::with_files("remove", &["a.png", "b.png", "c.png"]);
        let mut session = Session::default();
        session.open_path(&dir.0.join("b.png")).unwrap();

        session.remove_current();
        assert_eq!(names(&session), ["a.png", "c.png"]);
        assert_eq!(session.current().unwrap().file_name, "c.png", "the next one");

        session.remove_current();
        assert_eq!(session.current().unwrap().file_name, "a.png", "it was the last: the previous");

        session.remove_current();
        assert!(session.is_empty() && session.current().is_none());
    }

    #[test]
    fn the_folder_selection_stops_at_the_ends() {
        let dir = TempDir::with_files("folder-ends", &[]);
        for sub in ["a", "b"] {
            std::fs::create_dir(dir.0.join(sub)).unwrap();
        }
        let mut session = Session::default();
        session.open_path(&dir.0).unwrap();

        assert!(session.step_folder(isize::MAX));
        assert_eq!(session.folder_index, 2, "\"..\", a, b: the last one");
        assert!(!session.step_folder(1));
        assert!(session.step_folder(isize::MIN));
        assert_eq!(session.folder_index, 0);
    }

    #[test]
    fn a_missing_directory_errors_without_touching_the_state() {
        let dir = TempDir::with_files("intact", &["a.png"]);
        let mut session = Session::default();
        session.open_path(&dir.0).unwrap();

        assert!(session.open_path(Path::new("/does/not/exist/this/folder")).is_err());
        assert_eq!(names(&session), ["a.png"], "the previous state must remain intact");
    }
}
