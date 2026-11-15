//! Measures what the app has to do when opening a large folder.
//!
//! ```sh
//! cargo run --release --example bench -- ~/Pictures
//! ```
//!
//! Use it to check two things before blaming the UI: how long the directory
//! scan takes and how fast thumbnails come out.

use std::path::PathBuf;
use std::time::Instant;

use checheviewer::decode;
use checheviewer::loader::{Kind, Loader, Outcome, Priority};
use checheviewer::scan;

fn main() {
    let Some(dir) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("usage: bench <folder>");
        std::process::exit(2);
    };

    let start = Instant::now();
    let entries = match scan::scan_dir(&dir) {
        Ok(entries) => entries,
        Err(err) => {
            eprintln!("{err:#}");
            std::process::exit(1);
        }
    };
    println!("scan: {} images in {:?}", entries.len(), start.elapsed());

    if entries.is_empty() {
        return;
    }

    // Thumbnails: what has to be produced to fill the grid.
    let loader = Loader::new(|| {});
    let start = Instant::now();
    for entry in &entries {
        loader.request(&entry.path, Kind::Thumbnail, Priority::Prefetch);
    }

    let (mut ok, mut failed, mut first) = (0usize, 0usize, None);
    while ok + failed < entries.len() {
        for result in loader.poll() {
            match result.outcome {
                Outcome::Image { .. } => {
                    ok += 1;
                    if first.is_none() {
                        first = Some(start.elapsed());
                    }
                }
                Outcome::Error(_) => failed += 1,
                Outcome::Cancelled => failed += 1,
            }
        }
        std::thread::sleep(std::time::Duration::from_millis(1));
    }

    let elapsed = start.elapsed();
    println!(
        "thumbnails: {ok} ok, {failed} failed in {elapsed:?} ({:.0}/s)",
        ok as f64 / elapsed.as_secs_f64()
    );
    if let Some(first) = first {
        println!("first thumbnail after {first:?}");
    }

    // Full image: what you wait for when entering the viewer.
    let start = Instant::now();
    match decode::decode_full(&entries[0].path) {
        Ok(image) => {
            println!("first full image: {}×{} in {:?}", image.width, image.height, start.elapsed())
        }
        Err(err) => println!("first full image: error ({err:#})"),
    }
}
