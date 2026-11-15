use std::collections::VecDeque;
use std::path::{Path, PathBuf};
use std::sync::atomic::{AtomicU64, Ordering};
use std::sync::{Arc, Condvar, Mutex};
use std::thread::JoinHandle;

use crossbeam_channel::{unbounded, Receiver, Sender};

use crate::decode;

/// How many images to decode in parallel. More than four speeds up nothing
/// noticeable and multiplies peak memory (each worker may hold a whole image
/// in RAM).
const MAX_WORKERS: usize = 4;

#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum Priority {
    /// The image the user is looking at right now.
    Visible,
    /// A neighbor the user will probably ask for soon.
    Prefetch,
}

/// Which version of the image is requested.
#[derive(Debug, Clone, Copy, PartialEq, Eq, Hash)]
pub enum Kind {
    /// Full resolution, for the viewer.
    Full,
    /// Thumbnail for the grid.
    Thumbnail,
}

pub enum Outcome {
    /// Already in egui's format, so the UI thread only has to upload it.
    Image {
        color: egui::ColorImage,
        original_size: [u32; 2],
    },
    Error(String),
    /// The request became stale before decoding started.
    ///
    /// It is still reported so the requester can remove it from its pending
    /// list; if it still needs it, it requests it again.
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
    /// Incremented on every image change. A job queued with an old epoch is
    /// no longer useful to anyone and is dropped without decoding: without
    /// this, holding the arrow key queues hundreds of decodes that end up
    /// burying the one the user is waiting for.
    epoch: AtomicU64,
    /// Thumbnails keep their own counter, which only advances when the folder
    /// changes.
    ///
    /// If they shared the full-image epoch, holding the arrow key in the grid
    /// would cancel them all every frame and the grid would never finish
    /// drawing.
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

/// Background decoding pool.
///
/// The UI thread never decodes: it asks via `request` and collects via
/// `poll`.
pub struct Loader {
    shared: Arc<Shared>,
    results: Receiver<LoadResult>,
    workers: Vec<JoinHandle<()>>,
}

impl Loader {
    /// Starts the workers. `wake` is called whenever there is a new result,
    /// to rouse the UI from its wait (typically `Context::request_repaint`).
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
                    .expect("could not create the decoding thread")
            })
            .collect();

        Self { shared, results, workers }
    }

    /// Marks full-image requests as stale. Called when the image changes.
    pub fn invalidate(&self) {
        self.shared.epoch.fetch_add(1, Ordering::SeqCst);
    }

    /// Marks everything as stale, thumbnails included. Called when the
    /// folder changes.
    pub fn invalidate_all(&self) {
        self.shared.epoch.fetch_add(1, Ordering::SeqCst);
        self.shared.thumbnail_epoch.fetch_add(1, Ordering::SeqCst);
    }

    /// Queues an image for decoding.
    ///
    /// `Visible` requests are served before any `Prefetch`, regardless of
    /// arrival order.
    pub fn request(&self, path: &Path, kind: Kind, priority: Priority) {
        let job = Job {
            path: path.to_path_buf(),
            kind,
            epoch: self.shared.epoch_for(kind).load(Ordering::SeqCst),
        };
        {
            let mut queue = self.shared.queue.lock().expect("decoding queue poisoned");
            match priority {
                // Newest first: after scrubbing through a folder, what was
                // asked for the image you stopped on goes ahead of the
                // thumbnails of everything you passed, which nothing cancels.
                Priority::Visible => queue.visible.push_front(job),
                Priority::Prefetch => queue.prefetch.push_back(job),
            }
        }
        self.shared.ready.notify_one();
    }

    /// Returns the ready results. Doesn't block.
    pub fn poll(&self) -> impl Iterator<Item = LoadResult> + '_ {
        self.results.try_iter()
    }
}

impl Drop for Loader {
    fn drop(&mut self) {
        {
            let mut queue = self.shared.queue.lock().expect("decoding queue poisoned");
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
            let mut queue = shared.queue.lock().expect("decoding queue poisoned");
            loop {
                if queue.shutdown {
                    return;
                }
                if let Some(job) = queue.pop() {
                    break job;
                }
                queue = shared.ready.wait(queue).expect("decoding queue poisoned");
            }
        };

        // The check goes here, with the lock already released and right before
        // spending CPU time: if the user already moved on, this work is
        // useless.
        let outcome = if job.epoch < shared.epoch_for(job.kind).load(Ordering::SeqCst) {
            Outcome::Cancelled
        } else {
            let decoded = match job.kind {
                Kind::Full => decode::decode_full(&job.path),
                Kind::Thumbnail => decode::decode_thumbnail(&job.path),
            };
            match decoded {
                Ok(image) => Outcome::Image {
                    original_size: [image.original_width, image.original_height],
                    color: image.into_color_image(),
                },
                Err(err) => Outcome::Error(format!("{err:#}")),
            }
        };

        if tx.send(LoadResult { path: job.path, kind: job.kind, outcome }).is_err() {
            return; // nobody is listening: the app closed
        }
        wake();
    }
}

#[cfg(test)]
mod tests {
    use super::*;
    use std::sync::atomic::AtomicUsize;
    use std::time::{Duration, Instant};

    /// Waits up to `timeout` for `n` results to accumulate.
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
    fn decodes_in_background_and_wakes() {
        let dir = TempDir::new("basic");
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
        assert!(matches!(results[0].outcome, Outcome::Image { .. }));
        assert!(wakes.load(Ordering::SeqCst) >= 1, "should have woken the UI");
    }

    /// If thumbnails shared the full-image epoch, holding the arrow key
    /// would cancel them every frame and the grid would never finish
    /// drawing.
    #[test]
    fn moving_between_images_does_not_cancel_thumbnails() {
        let dir = TempDir::new("epoch-thumbnails");
        let path = write_png(&dir.0, "a.png", 32);

        let loader = Loader::new(|| {});
        loader.request(&path, Kind::Thumbnail, Priority::Prefetch);
        loader.invalidate(); // the user moved to another image

        let results = collect_results(&loader, 1, Duration::from_secs(5));
        assert_eq!(results.len(), 1);
        assert!(
            matches!(results[0].outcome, Outcome::Image { .. }),
            "the thumbnail should not be cancelled by changing image"
        );
    }

    #[test]
    fn changing_folder_does_cancel_thumbnails() {
        let dir = TempDir::new("epoch-folder");
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
    fn unreadable_file_returns_error_and_does_not_kill_worker() {
        let dir = TempDir::new("error");
        let broken = dir.0.join("broken.png");
        std::fs::write(&broken, vec![0u8; 128]).unwrap();
        let good = write_png(&dir.0, "good.png", 16);

        let loader = Loader::new(|| {});
        loader.request(&broken, Kind::Full, Priority::Visible);
        loader.request(&good, Kind::Full, Priority::Visible);

        let results = collect_results(&loader, 2, Duration::from_secs(5));
        assert_eq!(results.len(), 2);
        assert!(results.iter().any(|r| matches!(r.outcome, Outcome::Error(_))));
        assert!(
            results.iter().any(|r| matches!(r.outcome, Outcome::Image { .. })),
            "the worker must stay alive after a broken file"
        );
    }

    #[test]
    fn stale_requests_are_cancelled_without_decoding() {
        let dir = TempDir::new("cancellation");
        let path = write_png(&dir.0, "a.png", 16);

        let loader = Loader::new(|| {});
        // Queueing and marking stale before the workers get to it is a race;
        // what we do guarantee is that a job whose epoch went stale never
        // produces an image.
        for _ in 0..50 {
            loader.request(&path, Kind::Full, Priority::Prefetch);
        }
        loader.invalidate();

        let results = collect_results(&loader, 50, Duration::from_secs(5));
        assert_eq!(results.len(), 50, "every request must report something");
        assert!(
            results.iter().any(|r| matches!(r.outcome, Outcome::Cancelled)),
            "most stale requests should be cancelled"
        );
    }

    #[test]
    fn every_request_gets_a_response() {
        let dir = TempDir::new("responses");
        let paths: Vec<_> = (0..8).map(|i| write_png(&dir.0, &format!("img{i}.png"), 8)).collect();

        let loader = Loader::new(|| {});
        for path in &paths {
            loader.request(path, Kind::Full, Priority::Prefetch);
        }

        let results = collect_results(&loader, paths.len(), Duration::from_secs(10));
        assert_eq!(results.len(), paths.len());
    }

    #[test]
    fn closing_the_loader_does_not_hang_with_pending_requests() {
        let dir = TempDir::new("shutdown");
        let path = write_png(&dir.0, "a.png", 64);

        let loader = Loader::new(|| {});
        for _ in 0..200 {
            loader.request(&path, Kind::Full, Priority::Prefetch);
        }
        drop(loader); // must not hang waiting for the queue to drain
    }
}
