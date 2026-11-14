# CheCheViewer

A keyboard-driven image viewer for macOS. It is inspired by
[IrfanView](https://www.irfanview.com/) and shares its goal: view images fast,
with no waiting around. Open a folder, see all of its images in a grid, and
once you enter one, the ← → arrows walk through the folder.

Written in Rust with [egui](https://github.com/emilk/egui) on wgpu (Metal),
backed by macOS ImageIO for Apple's formats.

## Install

### Homebrew

```sh
brew tap ezeql/checheviewer https://github.com/ezeql/checheviewer
brew install checheviewer
```

Homebrew builds it from source (it fetches Rust for the build), so there is
no unsigned-app warning from Gatekeeper. It installs the `checheviewer`
command and the app bundle. To have it listed in Finder's **Open With** menu,
launch it once:

```sh
open "$(brew --prefix)/opt/checheviewer/CheCheViewer.app"
```

### From source

Requires [Rust](https://rustup.rs).

```sh
git clone https://github.com/ezeql/checheviewer
cd checheviewer
make install    # builds CheCheViewer.app and copies it to /Applications
```

`make` alone lists every target. `make uninstall` removes the app.

To make it the default app for a file type: Cmd+I on an image → *Open with* →
CheCheViewer → *Change All*.

## Usage

```sh
checheviewer                # empty window
checheviewer ~/Pictures     # open a folder in the grid
checheviewer photo.jpg      # open an image in the viewer, inside its folder
```

You can also drag a folder or an image onto the window.

## Shortcuts

| Key | Action |
|---|---|
| ← → | previous / next image |
| ↑ ↓ | previous / next row in the grid |
| Space / Backspace | next / previous in the viewer |
| Home / End | first / last |
| Enter | open the selected image |
| Esc | back to the grid |
| F | full screen |
| + − | zoom in / out |
| 0 | zoom 100 % |
| ⌘0 | fit to window |
| L R | rotate left / right (view only) |
| ⌘O | open folder |
| ⌘Q | quit |

With the mouse: drag to pan, pinch or ⌘+wheel to zoom, the wheel alone
scrolls, and double-click toggles between fit and actual size.

## Formats

PNG, JPEG, GIF, WebP, TIFF, BMP, ICO and friends through the `image` crate;
HEIC/HEIF, AVIF and camera RAW (CR2, CR3, NEF, ARW, DNG, ORF, RW2, RAF, SRW)
through ImageIO, the same engine Preview uses. No Homebrew libraries needed at
runtime.

EXIF orientation is always applied: portrait phone photos show up upright.

## Development

```sh
make run                    # debug build, empty window
make run ARGS=~/Pictures    # debug build, open a folder
make check                  # format, clippy and tests
cargo run --release --example bench -- ~/Pictures   # time scanning and thumbnails
```

Layout:

| File | What it does |
|---|---|
| `src/session.rs` | Navigation: folder, sorted image list and current index |
| `src/scan.rs` | Directory listing and natural sort (`img2` before `img10`) |
| `src/loader.rs` | Background decode pool, with priorities and cancellation |
| `src/cache.rs` | LRU cache bounded by bytes, not by count |
| `src/decode/` | Decoding: the `image` crate and ImageIO |
| `src/view.rs` | Viewer framing: fit, zoom, pan, rotation |
| `src/ui/` | Grid, viewer and top bar |
| `src/platform/` | Opening files from Finder (Apple Events) |

Three decisions hold up the rest:

1. **The UI thread never decodes.** Everything goes through `loader.rs`; the
   UI only uploads textures and draws.
2. **Stale requests get cancelled.** Every image change invalidates what is
   queued; without that, holding → down queues hundreds of decodes that end
   up blocking the one you are waiting for.
3. **Thumbnails come from ImageIO**, which uses the thumbnail already embedded
   in the file instead of decoding the whole photo.

To debug opening from Finder, where there is no terminal in sight:

```sh
open -a /Applications/CheCheViewer.app photo.jpg --env CHECHEVIEWER_DEBUG=1 --stderr /tmp/cv.log
```

## License

[MIT](LICENSE)
