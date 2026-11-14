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

/// Cuántas imágenes precargar hacia adelante y hacia atrás.
///
/// Adelante pesa más porque casi todo el mundo avanza: al mirar la número 5
/// ya están pedidas la 6 y la 7, y la flecha se siente instantánea.
const PREFETCH_AHEAD: usize = 2;
const PREFETCH_BEHIND: usize = 1;

/// Una imagen ya subida a la GPU y lista para dibujar.
pub struct LoadedImage {
    pub texture: TextureHandle,
    /// Tamaño de la textura, que puede ser menor al del archivo si hubo que
    /// reducirla para entrar en la GPU.
    pub size: egui::Vec2,
    /// Tamaño real del archivo, que es lo que se le muestra al usuario.
    pub original_size: [u32; 2],
    /// Bytes de píxeles, para el presupuesto del cache.
    pub bytes: usize,
}

pub struct App {
    /// Navegación: carpeta, lista de imágenes e índice actual.
    pub session: Session,
    /// La imagen a dibujar en este frame, si ya está lista.
    pub current: Option<Arc<LoadedImage>>,
    /// Error de la imagen actual, mostrado en lugar de ella.
    pub error: Option<String>,
    /// Error al abrir una carpeta o un archivo. Va aparte de `error` porque
    /// ese se recalcula en cada frame según la imagen actual.
    pub open_error: Option<String>,

    /// Encuadre del visor: ajuste, zoom, paneo y rotación.
    pub view: View,
    /// Área en la que se dibujó la imagen en el último frame. El zoom por
    /// teclado la necesita para saber respecto de qué ajustar.
    viewport: Rect,
    /// Cuántas columnas dibujó la grilla en el último frame, para que ↑ y ↓
    /// sepan cuánto moverse.
    pub grid_columns: usize,
    /// La selección cambió por teclado y la grilla tiene que traerla a la
    /// vista.
    pub scroll_to_selection: bool,

    loader: Loader,
    cache: ByteCache<Arc<LoadedImage>>,
    thumbnails: ByteCache<Arc<LoadedImage>>,
    /// Pedidos en vuelo, para no encolar dos veces lo mismo.
    pending: HashSet<(PathBuf, Kind)>,
    /// Archivos que fallaron, para no reintentarlos en cada frame.
    failed: HashMap<PathBuf, String>,
    /// Último índice para el que se programaron cargas.
    scheduled_for: Option<usize>,
    /// Qué archivo se está mostrando, para resetear el encuadre al cambiar.
    shown_path: Option<PathBuf>,
    fullscreen: bool,
}

impl App {
    pub fn new(cc: &eframe::CreationContext<'_>, start: Option<PathBuf>) -> Self {
        // Los workers despiertan a la UI cuando terminan de decodificar; sin
        // esto la imagen recién cargada no aparecería hasta el próximo evento
        // de mouse o teclado.
        let ctx = cc.egui_ctx.clone();
        let loader = Loader::new(move || ctx.request_repaint());
        // Para que un archivo abierto desde el Finder despierte a la interfaz
        // aunque esté quieta esperando eventos.
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
                    "abierto {} → {} imágenes, en la {}",
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
                crate::trace!("no se pudo abrir {}: {err:#}", path.display());
                self.open_error = Some(format!("{err:#}"));
            }
        }
    }

    fn apply(&mut self, action: Action, ctx: &Context) {
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

    /// Abre el diálogo nativo para elegir una carpeta.
    ///
    /// Es modal y bloquea el hilo de UI mientras está abierto, que es
    /// exactamente lo que hace cualquier app de macOS con ⌘O.
    pub fn pick_folder(&mut self) {
        let start = self.session.dir.clone();
        let mut dialog = rfd::FileDialog::new().set_title("Abrir carpeta de imágenes");
        if let Some(dir) = start {
            dialog = dialog.set_directory(dir);
        }
        if let Some(folder) = dialog.pick_folder() {
            self.open_path(&folder);
        }
    }

    /// Mueve la selección y, si cambió, pide que la grilla la traiga a la
    /// vista.
    fn move_by(&mut self, delta: isize) {
        self.scroll_to_selection |= self.session.step(delta);
    }

    /// Tamaño de la imagen cargada, o algo razonable si todavía no hay
    /// ninguna.
    fn image_size(&self) -> egui::Vec2 {
        self.current.as_ref().map(|img| img.size).unwrap_or(egui::vec2(1.0, 1.0))
    }

    /// Recoge lo que terminaron los workers y lo sube a la GPU.
    ///
    /// Subir texturas es lo único de la carga que tiene que pasar sí o sí en
    /// el hilo de UI.
    fn collect_results(&mut self, ctx: &Context) {
        for result in self.loader.poll().collect::<Vec<_>>() {
            self.pending.remove(&(result.path.clone(), result.kind));
            match result.outcome {
                Outcome::Image(image) => {
                    let name = format!("{}#{:?}", result.path.display(), result.kind);
                    let size = [image.width as usize, image.height as usize];
                    // ImageIO devuelve el color ya multiplicado por el alfa;
                    // el crate `image`, no. Confundirlos tiñe de negro las
                    // zonas semitransparentes.
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
                // Quedó obsoleto: si todavía hace falta, se vuelve a pedir en
                // el próximo frame.
                Outcome::Cancelled => {}
            }
        }
    }

    /// Pide la imagen actual y precarga sus vecinas.
    fn schedule_loads(&mut self) {
        let index = self.session.index();
        if self.session.is_empty() {
            return;
        }

        // Al moverse cambia qué es urgente: lo encolado deja de servir.
        if self.scheduled_for != Some(index) {
            self.loader.invalidate();
            self.scheduled_for = Some(index);
        }

        // Se pide en cada frame aunque nada haya cambiado: un pedido que vuelve
        // `Cancelled` sale de `pending` y hay que reencolarlo. `request` ya
        // descarta lo que está en cache o en vuelo, así que repetirlo es gratis.
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
            return; // ya estaba pedido
        }
        self.loader.request(&path, Kind::Full, priority);
    }

    /// Devuelve la miniatura de `path`, pidiéndola si todavía no está.
    ///
    /// La llama la grilla para cada celda visible: pedir solo lo que se ve es
    /// lo que hace que una carpeta de miles de fotos abra al instante.
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

    /// ¿Falló la carga de este archivo?
    pub fn has_failed(&self, path: &Path) -> bool {
        self.failed.contains_key(path)
    }

    /// Deja en `current` / `error` lo que corresponde dibujar este frame.
    fn refresh_current(&mut self) {
        let Some(entry) = self.session.current() else {
            self.current = None;
            self.error = None;
            return;
        };
        let path = entry.path.clone();

        // Cada imagen empieza ajustada a la ventana: heredar el zoom y la
        // rotación de la anterior desorienta más de lo que ayuda.
        if self.shown_path.as_ref() != Some(&path) {
            self.shown_path = Some(path.clone());
            self.view.reset();
        }

        self.error = self.failed.get(&path).cloned();
        // `get` además marca la entrada como recién usada, que es justo lo que
        // queremos: la imagen visible nunca debería ser la primera candidata a
        // desalojo.
        self.current = self.cache.get(&path).cloned();
    }

    /// Registra el área donde se dibuja la imagen, para que el zoom por
    /// teclado sepa respecto de qué encuadre trabajar.
    pub fn set_viewport(&mut self, rect: Rect) {
        self.viewport = rect;
    }

    pub fn viewport(&self) -> Rect {
        self.viewport
    }

    /// ¿Se está esperando la imagen actual?
    pub fn is_loading(&self) -> bool {
        self.session
            .current()
            .is_some_and(|entry| self.pending.contains(&(entry.path.clone(), Kind::Full)))
    }

    /// Memoria ocupada por las imágenes en cache.
    pub fn cached_bytes(&self) -> usize {
        self.cache.bytes()
    }
}

impl eframe::App for App {
    fn ui(&mut self, ui: &mut egui::Ui, _frame: &mut eframe::Frame) {
        let ctx = ui.ctx().clone();

        // El input se consume antes de dibujar, para que los widgets no se
        // queden con las flechas.
        for action in ctx.input_mut(|i| input::poll(i, self.session.mode)) {
            self.apply(action, &ctx);
        }

        // Doble clic en una imagen desde el Finder, o "Abrir con".
        if let Some(path) = platform::take_requested_path() {
            self.open_path(&path);
        }

        // Arrastrar y soltar una carpeta o una imagen sobre la ventana.
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
