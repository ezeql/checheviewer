use std::collections::{HashMap, HashSet};
use std::path::{Path, PathBuf};
use std::sync::Arc;

use egui::{ColorImage, Context, Rect, TextureHandle, TextureOptions};

use crate::cache::{ByteCache, DEFAULT_BUDGET, THUMBNAIL_BUDGET};
use crate::input::{self, Action};
use crate::loader::{Kind, Loader, Outcome, Priority};
use crate::model::ViewMode;
use crate::platform;
use crate::session::Session;
use crate::ui;
use crate::view::View;

/// How many images to prefetch ahead and behind.
///
/// Ahead weighs more because almost everyone moves forward: while looking at
/// number 5, 6 and 7 are already requested, so the arrow key feels instant.
const PREFETCH_AHEAD: usize = 2;
const PREFETCH_BEHIND: usize = 1;

/// An image already uploaded to the GPU and ready to draw.
pub struct LoadedImage {
    pub texture: TextureHandle,
    /// Size of the texture, which may be smaller than the file's if it had
    /// to be downscaled to fit on the GPU.
    pub size: egui::Vec2,
    /// Actual size of the file, which is what is shown to the user.
    pub original_size: [u32; 2],
    /// Pixel bytes, for the cache budget.
    pub bytes: usize,
}

pub struct App {
    /// Navigation: folder, image list and current index.
    pub session: Session,
    /// The image to draw this frame, if it is ready.
    pub current: Option<Arc<LoadedImage>>,
    /// Error for the current image, shown in its place.
    pub error: Option<String>,
    /// Error opening a folder or file. Kept apart from `error` because that
    /// one is recomputed every frame from the current image.
    pub open_error: Option<String>,

    /// Viewer framing: fit, zoom, pan and rotation.
    pub view: View,
    /// Area where the image was drawn in the last frame. Keyboard zoom needs
    /// it to know what to adjust against.
    viewport: Rect,
    /// How many columns the grid drew in the last frame, so ↑ and ↓ know how
    /// far to move.
    pub grid_columns: usize,
    /// The selection changed via keyboard and the grid must scroll it into
    /// view.
    pub scroll_to_selection: bool,

    loader: Loader,
    cache: ByteCache<Arc<LoadedImage>>,
    thumbnails: ByteCache<Arc<LoadedImage>>,
    /// In-flight requests, to avoid queueing the same thing twice.
    pending: HashSet<(PathBuf, Kind)>,
    /// Files that failed, to avoid retrying them every frame.
    failed: HashMap<PathBuf, String>,
    /// Last index for which loads were scheduled.
    scheduled_for: Option<usize>,
    /// Which file is being shown, to reset the framing when it changes.
    shown_path: Option<PathBuf>,
    fullscreen: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, start: Option<PathBuf>) -> Self {
        // Workers wake the UI when they finish decoding; without this the
        // freshly loaded image wouldn't show up until the next mouse or
        // keyboard event.
        let ctx = cc.egui_ctx.clone();
        let loader = Loader::new(move || ctx.request_repaint());
        // So a file opened from Finder wakes the UI even if it is idle
        // waiting for events.
        platform::set_repaint_context(&cc.egui_ctx);

        let mut app = Self {
            session: Session::default(),
            current: None,
            error: None,
            open_error: None,
            view: View::default(),
            viewport: Rect::from_min_size(egui::Pos2::ZERO, egui::vec2(1280.0, 800.0)),
            grid_columns: 1,
            scroll_to_selection: false,
            loader,
            cache: ByteCache::new(DEFAULT_BUDGET),
            thumbnails: ByteCache::new(THUMBNAIL_BUDGET),
            pending: HashSet::new(),
            failed: HashMap::new(),
            scheduled_for: None,
            shown_path: None,
            fullscreen: false,
        };
        if let Some(path) = start {
            app.open_path(&path);
        }
        app
    }

    pub fn open_path(&mut self, path: &Path) {
        match self.session.open_path(path) {
            Ok(()) => {
                crate::trace!(
                    "opened {} → {} images, at #{}",
                    path.display(),
                    self.session.len(),
                    self.session.index() + 1
                );
                self.error = None;
                self.open_error = None;
                self.current = None;
                self.failed.clear();
                self.cache.clear();
                self.thumbnails.clear();
                self.scheduled_for = None;
                self.scroll_to_selection = true;
                self.loader.invalidate_all();
            }
            Err(err) => {
                crate::trace!("could not open {}: {err:#}", path.display());
                self.open_error = Some(format!("{err:#}"));
            }
        }
    }

    fn apply(&mut self, action: Action, ctx: &Context) {
        if self.session.is_empty() && self.session.mode == ViewMode::Grid && self.browse(action) {
            return;
        }
        match action {
            Action::NextImage => {
                self.move_by(1);
            }
            Action::PrevImage => {
                self.move_by(-1);
            }
            Action::MoveUp => {
                self.move_by(-(self.grid_columns as isize));
            }
            Action::MoveDown => {
                self.move_by(self.grid_columns as isize);
            }
            Action::FirstImage => {
                self.scroll_to_selection |= self.session.go_first();
            }
            Action::LastImage => {
                self.scroll_to_selection |= self.session.go_last();
            }
            Action::EnterViewer => {
                if !self.session.is_empty() {
                    self.session.mode = ViewMode::Viewer;
                }
            }
            Action::BackToGrid => self.session.mode = ViewMode::Grid,
            Action::ToggleFullscreen => {
                self.fullscreen = !self.fullscreen;
                ctx.send_viewport_cmd(egui::ViewportCommand::Fullscreen(self.fullscreen));
            }
            Action::Quit => ctx.send_viewport_cmd(egui::ViewportCommand::Close),
            Action::OpenFolder => self.pick_folder(),
            Action::ZoomIn => {
                let size = self.image_size();
                self.view.zoom_in(size, self.viewport);
            }
            Action::ZoomOut => {
                let size = self.image_size();
                self.view.zoom_out(size, self.viewport);
            }
            Action::ZoomActual => self.view.actual_size(),
            Action::ZoomFit => self.view.fit(),
            Action::RotateLeft => self.view.rotate_left(),
            Action::RotateRight => self.view.rotate_right(),
        }
    }

    /// Keys for the folder browser the grid shows when there are no images:
    /// ↑ ↓ pick, Enter or → opens, ← goes up. Returns whether it used the
    /// action.
    fn browse(&mut self, action: Action) -> bool {
        let delta = match action {
            Action::MoveUp => -1,
            Action::MoveDown => 1,
            Action::FirstImage => isize::MIN,
            Action::LastImage => isize::MAX,
            Action::EnterViewer | Action::NextImage => {
                let selected = self.session.folders.get(self.session.folder_index);
                if let Some(path) = selected.map(|f| f.path.clone()) {
                    self.open_path(&path);
                }
                return true;
            }
            Action::PrevImage => {
                let parent = self.session.dir.as_ref().and_then(|d| d.parent());
                if let Some(path) = parent.map(Path::to_path_buf) {
                    self.open_path(&path);
                }
                return true;
            }
            _ => return false,
        };
        self.scroll_to_selection |= self.session.step_folder(delta);
        true
    }

    /// Opens the native dialog to pick a folder.
    ///
    /// It is modal and blocks the UI thread while open, which is exactly what
    /// any macOS app does with ⌘O.
    pub fn pick_folder(&mut self) {
        let start = self.session.dir.clone();
        let mut dialog = rfd::FileDialog::new().set_title("Open image folder");
        if let Some(dir) = start {
            dialog = dialog.set_directory(dir);
        }
        if let Some(folder) = dialog.pick_folder() {
            self.open_path(&folder);
        }
    }

    /// Moves the selection and, if it changed, asks the grid to scroll it
    /// into view.
    fn move_by(&mut self, delta: isize) {
        self.scroll_to_selection |= self.session.step(delta);
    }

    /// Size of the loaded image, or something reasonable if there is none
    /// yet.
    fn image_size(&self) -> egui::Vec2 {
        self.current.as_ref().map(|img| img.size).unwrap_or(egui::vec2(1.0, 1.0))
    }

    /// Collects what the workers finished and uploads it to the GPU.
    ///
    /// Uploading textures is the only part of loading that must happen on the
    /// UI thread.
    fn collect_results(&mut self, ctx: &Context) {
        for result in self.loader.poll().collect::<Vec<_>>() {
            self.pending.remove(&(result.path.clone(), result.kind));
            match result.outcome {
                Outcome::Image(image) => {
                    let name = format!("{}#{:?}", result.path.display(), result.kind);
                    let size = [image.width as usize, image.height as usize];
                    // ImageIO returns color already multiplied by alpha; the
                    // `image` crate doesn't. Mixing them up tints
                    // semi-transparent areas black.
                    let color = if image.premultiplied {
                        ColorImage::from_rgba_premultiplied(size, &image.pixels)
                    } else {
                        ColorImage::from_rgba_unmultiplied(size, &image.pixels)
                    };
                    let bytes = image.byte_size();
                    let texture = ctx.load_texture(name, color, TextureOptions::LINEAR);
                    let loaded = Arc::new(LoadedImage {
                        texture,
                        size: egui::vec2(image.width as f32, image.height as f32),
                        original_size: [image.original_width, image.original_height],
                        bytes,
                    });
                    match result.kind {
                        Kind::Full => self.cache.insert(result.path, loaded, bytes),
                        Kind::Thumbnail => self.thumbnails.insert(result.path, loaded, bytes),
                    }
                }
                Outcome::Error(err) => {
                    self.failed.insert(result.path, err);
                }
                // Became stale: if still needed, it is requested again next
                // frame.
                Outcome::Cancelled => {}
            }
        }
    }

    /// Requests the current image and prefetches its neighbors.
    fn schedule_loads(&mut self) {
        let index = self.session.index();
        if self.session.is_empty() {
            return;
        }

        // Moving changes what is urgent: whatever was queued is no longer useful.
        if self.scheduled_for != Some(index) {
            self.loader.invalidate();
            self.scheduled_for = Some(index);
        }

        // Requested every frame even if nothing changed: a request that comes
        // back `Cancelled` leaves `pending` and must be requeued. `request`
        // already skips what is cached or in flight, so repeating is free.
        self.request(index, Priority::Visible);
        for offset in 1..=PREFETCH_AHEAD {
            self.request(index + offset, Priority::Prefetch);
        }
        for offset in 1..=PREFETCH_BEHIND {
            self.request(index.wrapping_sub(offset), Priority::Prefetch);
        }
    }

    fn request(&mut self, index: usize, priority: Priority) {
        let Some(entry) = self.session.entries.get(index) else {
            return;
        };
        let path = entry.path.clone();
        if self.cache.contains(&path) || self.failed.contains_key(&path) {
            return;
        }
        if !self.pending.insert((path.clone(), Kind::Full)) {
            return; // already requested
        }
        self.loader.request(&path, Kind::Full, priority);
    }

    /// Returns the thumbnail for `path`, requesting it if not there yet.
    ///
    /// The grid calls it for every visible cell: requesting only what is
    /// visible is what makes a folder of thousands of photos open instantly.
    pub fn thumbnail(&mut self, path: &Path) -> Option<Arc<LoadedImage>> {
        if let Some(thumb) = self.thumbnails.get(path) {
            return Some(Arc::clone(thumb));
        }
        if self.failed.contains_key(path) {
            return None;
        }
        if self.pending.insert((path.to_path_buf(), Kind::Thumbnail)) {
            self.loader.request(path, Kind::Thumbnail, Priority::Prefetch);
        }
        None
    }

    /// Did loading this file fail?
    pub fn has_failed(&self, path: &Path) -> bool {
        self.failed.contains_key(path)
    }

    /// Leaves in `current` / `error` what should be drawn this frame.
    fn refresh_current(&mut self) {
        let Some(entry) = self.session.current() else {
            self.current = None;
            self.error = None;
            return;
        };
        let path = entry.path.clone();

        // Each image starts fitted to the window: inheriting the previous
        // one's zoom and rotation is more disorienting than helpful.
        if self.shown_path.as_ref() != Some(&path) {
            self.shown_path = Some(path.clone());
            self.view.reset();
        }

        self.error = self.failed.get(&path).cloned();
        // `get` also marks the entry as most recently used, which is exactly
        // what we want: the visible image should never be the first eviction
        // candidate.
        self.current = self.cache.get(&path).cloned();
    }

    /// Records the area where the image is drawn, so keyboard zoom knows
    /// which framing to work against.
    pub fn set_viewport(&mut self, rect: Rect) {
        self.viewport = rect;
    }

    pub fn viewport(&self) -> Rect {
        self.viewport
    }

    /// Are we waiting for the current image?
    pub fn is_loading(&self) -> bool {
        self.session
            .current()
            .is_some_and(|entry| self.pending.contains(&(entry.path.clone(), Kind::Full)))
    }

    /// Memory used by the cached images.
    pub fn cached_bytes(&self) -> usize {
        self.cache.bytes()
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // Input is consumed before drawing, so widgets don't swallow the
        // arrow keys.
        for action in ctx.input_mut(|i| input::poll(i, self.session.mode)) {
            self.apply(action, &ctx);
        }

        // Double-click on an image from Finder, or "Open With".
        if let Some(path) = platform::take_requested_path() {
            self.open_path(&path);
        }

        // Drag and drop a folder or an image onto the window.
        let dropped = ctx.input(|i| i.raw.dropped_files.first().map(|f| f.path().to_path_buf()));
        if let Some(path) = dropped {
            self.open_path(&path);
        }

        self.collect_results(&ctx);
        self.schedule_loads();
        self.refresh_current();

        ui::topbar::show(self, ui);
        match self.session.mode {
            ViewMode::Grid => ui::grid::show(self, ui),
            ViewMode::Viewer => ui::viewer::show(self, ui),
        }
    }
}
