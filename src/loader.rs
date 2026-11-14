use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use crossbeam_channel::{unbounded, Receiver, Sender};

use crate::decode::{self, DecodedImage};

/// Cuántas imágenes decodificar en paralelo. Más de cuatro no acelera nada
/// perceptible y multiplica el pico de memoria (cada worker puede tener una
/// imagen entera en RAM).
const MAX_WORKERS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    /// La imagen que el usuario está mirando ahora.
    Visible,
    /// Un vecino que probablemente pida enseguida.
    Prefetch,
}

/// Qué versión de la imagen se pide.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Resolución completa, para el visor.
    Full,
    /// Miniatura para la grilla.
    Thumbnail,
}

pub enum Outcome {
    Image(Box<DecodedImage>),
    Error(String),
    /// El pedido quedó obsoleto antes de empezar a decodificarse.
    ///
    /// Se avisa igual para que quien pidió pueda sacarlo de su lista de
    /// pendientes; si todavía lo necesita, lo vuelve a pedir.
    Cancelled,
}

pub struct LoadResult {
    pub path: PathBuf,
    pub kind: Kind,
    pub outcome: Outcome,
}

struct Job {
    path: PathBuf,
    kind: Kind,
    epoch: u64,
}

#[derive(Default)]
struct Queue {
    visible: VecDeque<Job>,
    prefetch: VecDeque<Job>,
    shutdown: bool,
}

impl Queue {
    fn pop(&mut self) -> Option<Job> {
        self.visible.pop_front().or_else(|| self.prefetch.pop_front())
    }
}

struct Shared {
    queue: Mutex<Queue>,
    ready: Condvar,
    /// Se incrementa en cada cambio de imagen. Un job encolado con un epoch
    /// viejo ya no le sirve a nadie y se descarta sin decodificar: sin esto,
    /// mantener la flecha apretada encola cientos de decodificaciones que
    /// terminan tapando a la que el usuario está esperando.
    epoch: AtomicU64,
    /// Las miniaturas llevan su propia cuenta, que solo avanza al cambiar de
    /// carpeta.
    ///
    /// Si compartieran el epoch de las imágenes completas, mantener la flecha
    /// apretada en la grilla las cancelaría todas en cada frame y la grilla no
    /// terminaría de dibujarse nunca.
    thumbnail_epoch: AtomicU64,
}

impl Shared {
    fn epoch_for(&self, kind: Kind) -> &AtomicU64 {
        match kind {
            Kind::Full => &self.epoch,
            Kind::Thumbnail => &self.thumbnail_epoch,
        }
    }
}

/// Pool de decodificación en background.
///
/// El hilo de UI nunca decodifica: pide por `request` y recoge por `poll`.
pub struct Loader {
    shared: Arc<Shared>,
    results: Receiver<LoadResult>,
    workers: Vec<JoinHandle<()>>,
}

impl Loader {
    /// Arranca los workers. `wake` se llama cada vez que hay un resultado
    /// nuevo, para sacar a la UI de su espera (típicamente
    /// `Context::request_repaint`).
    pub fn new(wake: impl Fn() + Send + Sync + 'static) -> Self {
        let shared = Arc::new(Shared {
            queue: Mutex::new(Queue::default()),
            ready: Condvar::new(),
            epoch: AtomicU64::new(0),
            thumbnail_epoch: AtomicU64::new(0),
        });
        let (tx, results) = unbounded();
        let wake = Arc::new(wake);

        let count = std::thread::available_parallelism()
            .map(|n| n.get().saturating_sub(1).clamp(1, MAX_WORKERS))
            .unwrap_or(1);

        let workers = (0..count)
            .map(|i| {
                let shared = Arc::clone(&shared);
                let tx = tx.clone();
                let wake = Arc::clone(&wake);
                std::thread::Builder::new()
                    .name(format!("decode-{i}"))
                    .spawn(move || worker(&shared, &tx, &*wake))
                    .expect("no se pudo crear el hilo de decodificación")
            })
            .collect();

        Self { shared, results, workers }
    }

    /// Marca como obsoletos los pedidos de imagen completa. Se llama al
    /// cambiar de imagen.
    pub fn invalidate(&self) {
        self.shared.epoch.fetch_add(1, Ordering::SeqCst);
    }

    /// Marca todo como obsoleto, miniaturas incluidas. Se llama al cambiar de
    /// carpeta.
    pub fn invalidate_all(&self) {
        self.shared.epoch.fetch_add(1, Ordering::SeqCst);
        self.shared.thumbnail_epoch.fetch_add(1, Ordering::SeqCst);
    }

    /// Encola una imagen para decodificar.
    ///
    /// Los pedidos `Visible` se atienden antes que cualquier `Prefetch`, sin
    /// importar el orden de llegada.
    pub fn request(&self, path: &Path, kind: Kind, priority: Priority) {
        let job = Job {
            path: path.to_path_buf(),
            kind,
            epoch: self.shared.epoch_for(kind).load(Ordering::SeqCst),
        };
        {
            let mut queue = self.shared.queue.lock().expect("cola de decodificación envenenada");
            match priority {
                Priority::Visible => queue.visible.push_back(job),
                Priority::Prefetch => queue.prefetch.push_back(job),
            }
        }
        self.shared.ready.notify_one();
    }

    /// Devuelve los resultados listos. No bloquea.
    pub fn poll(&self) -> impl Iterator<Item = LoadResult> + '_ {
        self.results.try_iter()
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        {
            let mut queue = self.shared.queue.lock().expect("cola de decodificación envenenada");
            queue.shutdown = true;
            queue.visible.clear();
            queue.prefetch.clear();
        }
        self.shared.ready.notify_all();
        for worker in self.workers.drain(..) {
            let _ = worker.join();
        }
    }
}

fn worker(shared: &Shared, tx: &Sender<LoadResult>, wake: &(dyn Fn() + Send + Sync)) {
    loop {
        let job = {
            let mut queue = shared.queue.lock().expect("cola de decodificación envenenada");
            loop {
                if queue.shutdown {
                    return;
                }
                if let Some(job) = queue.pop() {
                    break job;
                }
                queue = shared.ready.wait(queue).expect("cola de decodificación envenenada");
            }
        };

        // El chequeo va acá, con el lock ya soltado y justo antes de gastar
        // tiempo de CPU: si el usuario ya se movió, este trabajo no sirve.
        let outcome = if job.epoch < shared.epoch_for(job.kind).load(Ordering::SeqCst) {
            Outcome::Cancelled
        } else {
            let decoded = match job.kind {
                Kind::Full => decode::decode_full(&job.path),
                Kind::Thumbnail => decode::decode_thumbnail(&job.path),
            };
            match decoded {
                Ok(image) => Outcome::Image(Box::new(image)),
                Err(err) => Outcome::Error(format!("{err:#}")),
            }
        };

        if tx.send(LoadResult { path: job.path, kind: job.kind, outcome }).is_err() {
            return; // nadie está escuchando: la app se cerró
        }
        wake();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::{Duration, Instant};

    /// Espera hasta `timeout` a que se acumulen `n` resultados.
    fn collect_results(loader: &Loader, n: usize, timeout: Duration) -> Vec<LoadResult> {
        let deadline = Instant::now() + timeout;
        let mut out = Vec::new();
        while out.len() < n && Instant::now() < deadline {
            out.extend(loader.poll());
            std::thread::sleep(Duration::from_millis(5));
        }
        out
    }

    fn write_png(dir: &Path, name: &str, size: u32) -> PathBuf {
        let path = dir.join(name);
        image::RgbImage::from_pixel(size, size, image::Rgb([7, 7, 7])).save(&path).unwrap();
        path
    }

    struct TempDir(PathBuf);

    impl TempDir {
        fn new(name: &str) -> Self {
            let dir = std::env::temp_dir().join(format!("checheviewer-loader-{name}"));
            let _ = std::fs::remove_dir_all(&dir);
            std::fs::create_dir_all(&dir).unwrap();
            Self(dir)
        }
    }

    impl Drop for TempDir {
        fn drop(&mut self) {
            let _ = std::fs::remove_dir_all(&self.0);
        }
    }

    #[test]
    fn decodifica_en_background_y_avisa() {
        let dir = TempDir::new("basico");
        let path = write_png(&dir.0, "a.png", 16);

        let wakes = Arc::new(AtomicUsize::new(0));
        let counter = Arc::clone(&wakes);
        let loader = Loader::new(move || {
            counter.fetch_add(1, Ordering::SeqCst);
        });

        loader.request(&path, Kind::Full, Priority::Visible);
        let results = collect_results(&loader, 1, Duration::from_secs(5));

        assert_eq!(results.len(), 1);
        assert_eq!(results[0].path, path);
        assert_eq!(results[0].kind, Kind::Full);
        assert!(matches!(results[0].outcome, Outcome::Image(_)));
        assert!(wakes.load(Ordering::SeqCst) >= 1, "debería haber despertado a la UI");
    }

    /// Si las miniaturas compartieran el epoch de las imágenes completas,
    /// mantener la flecha apretada las cancelaría en cada frame y la grilla
    /// nunca terminaría de dibujarse.
    #[test]
    fn moverse_entre_imagenes_no_cancela_las_miniaturas() {
        let dir = TempDir::new("epoch-miniaturas");
        let path = write_png(&dir.0, "a.png", 32);

        let loader = Loader::new(|| {});
        loader.request(&path, Kind::Thumbnail, Priority::Prefetch);
        loader.invalidate(); // el usuario se movió a otra imagen

        let results = collect_results(&loader, 1, Duration::from_secs(5));
        assert_eq!(results.len(), 1);
        assert!(
            matches!(results[0].outcome, Outcome::Image(_)),
            "la miniatura no debería cancelarse por cambiar de imagen"
        );
    }

    #[test]
    fn cambiar_de_carpeta_si_cancela_las_miniaturas() {
        let dir = TempDir::new("epoch-carpeta");
        let path = write_png(&dir.0, "a.png", 32);

        let loader = Loader::new(|| {});
        for _ in 0..30 {
            loader.request(&path, Kind::Thumbnail, Priority::Prefetch);
        }
        loader.invalidate_all();

        let results = collect_results(&loader, 30, Duration::from_secs(5));
        assert!(results.iter().any(|r| matches!(r.outcome, Outcome::Cancelled)));
    }

    #[test]
    fn un_archivo_ilegible_devuelve_error_y_no_mata_al_worker() {
        let dir = TempDir::new("error");
        let roto = dir.0.join("roto.png");
        std::fs::write(&roto, vec![0u8; 128]).unwrap();
        let bueno = write_png(&dir.0, "bueno.png", 16);

        let loader = Loader::new(|| {});
        loader.request(&roto, Kind::Full, Priority::Visible);
        loader.request(&bueno, Kind::Full, Priority::Visible);

        let results = collect_results(&loader, 2, Duration::from_secs(5));
        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|r| matches!(r.outcome, Outcome::Error(_))));
        assert!(
            results.iter().any(|r| matches!(r.outcome, Outcome::Image(_))),
            "el worker debe seguir vivo después de un archivo roto"
        );
    }

    #[test]
    fn los_pedidos_obsoletos_se_cancelan_sin_decodificar() {
        let dir = TempDir::new("cancelacion");
        let path = write_png(&dir.0, "a.png", 16);

        let loader = Loader::new(|| {});
        // Encolar y marcar obsoleto antes de que los workers alcancen a
        // tomarlo es una carrera; lo que sí garantizamos es que un job cuyo
        // epoch quedó viejo nunca produce una imagen.
        for _ in 0..50 {
            loader.request(&path, Kind::Full, Priority::Prefetch);
        }
        loader.invalidate();

        let results = collect_results(&loader, 50, Duration::from_secs(5));
        assert_eq!(results.len(), 50, "todo pedido tiene que reportar algo");
        assert!(
            results.iter().any(|r| matches!(r.outcome, Outcome::Cancelled)),
            "la mayoría de los pedidos obsoletos debería cancelarse"
        );
    }

    #[test]
    fn siempre_llega_una_respuesta_por_pedido() {
        let dir = TempDir::new("respuestas");
        let paths: Vec<_> = (0..8).map(|i| write_png(&dir.0, &format!("img{i}.png"), 8)).collect();

        let loader = Loader::new(|| {});
        for path in &paths {
            loader.request(path, Kind::Full, Priority::Prefetch);
        }

        let results = collect_results(&loader, paths.len(), Duration::from_secs(10));
        assert_eq!(results.len(), paths.len());
    }

    #[test]
    fn cerrar_el_loader_no_cuelga_aunque_queden_pedidos() {
        let dir = TempDir::new("cierre");
        let path = write_png(&dir.0, "a.png", 64);

        let loader = Loader::new(|| {});
        for _ in 0..200 {
            loader.request(&path, Kind::Full, Priority::Prefetch);
        }
        drop(loader); // no debe colgarse esperando a vaciar la cola
    }
}
