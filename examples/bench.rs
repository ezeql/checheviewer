//! Mide lo que la app tiene que hacer al abrir una carpeta grande.
//!
//! ```sh
//! cargo run --release --example bench -- ~/Pictures
//! ```
//!
//! Sirve para comprobar dos cosas antes de culpar a la interfaz: cuánto tarda
//! el escaneo del directorio y a qué ritmo salen las miniaturas.

use std::path::PathBuf;
use std::time::Instant;

use checheviewer::decode;
use checheviewer::loader::{Kind, Loader, Outcome, Priority};
use checheviewer::scan;

fn main() {
    let Some(dir) = std::env::args_os().nth(1).map(PathBuf::from) else {
        eprintln!("uso: bench <carpeta>");
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
    println!("escaneo: {} imágenes en {:?}", entries.len(), start.elapsed());

    if entries.is_empty() {
        return;
    }

    // Miniaturas: lo que hay que producir para llenar la grilla.
    let loader = Loader::new(|| {});
    let start = Instant::now();
    for entry in &entries {
        loader.request(&entry.path, Kind::Thumbnail, Priority::Prefetch);
    }

    let (mut ok, mut failed, mut first) = (0usize, 0usize, None);
    while ok + failed < entries.len() {
        for result in loader.poll() {
            match result.outcome {
                Outcome::Image(_) => {
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
        "miniaturas: {ok} ok, {failed} con error en {elapsed:?} ({:.0}/s)",
        ok as f64 / elapsed.as_secs_f64()
    );
    if let Some(first) = first {
        println!("primera miniatura a los {first:?}");
    }

    // Imagen completa: lo que se espera al entrar al visor.
    let start = Instant::now();
    match decode::decode_full(&entries[0].path) {
        Ok(image) => println!(
            "primera imagen completa: {}×{} en {:?}",
            image.width,
            image.height,
            start.elapsed()
        ),
        Err(err) => println!("primera imagen completa: error ({err:#})"),
    }
}
