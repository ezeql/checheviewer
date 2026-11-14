# CheCheViewer

Visor de imágenes para macOS con foco en el teclado. Está inspirado en
[IrfanView](https://www.irfanview.com/): la idea es la misma, ver imágenes
rápido, sin esperas ni vueltas. Se abre una carpeta, se ven todas sus imágenes
en una grilla y, al entrar en una, las flechas ← → recorren el directorio.

Escrito en Rust con [egui](https://github.com/emilk/egui) sobre wgpu (Metal), y
apoyado en ImageIO de macOS para los formatos de Apple.

## Uso

```sh
make run                    # abre la ventana vacía
make run ARGS=~/Pictures    # abre una carpeta en la grilla
make run ARGS=~/foto.jpg    # abre una imagen en el visor, ubicada en su carpeta
```

Para tenerla como aplicación de verdad:

```sh
make bundle     # arma target/CheCheViewer.app
make install    # la copia a /Applications y la registra en el sistema
```

Una vez instalada aparece en el menú **Abrir con** del Finder. Para que sea la
aplicación por omisión de un tipo de archivo: Cmd+I sobre una imagen → *Abrir
con* → CheCheViewer → *Cambiar todos*.

## Atajos

| Tecla | Acción |
|---|---|
| ← → | imagen anterior / siguiente |
| ↑ ↓ | fila anterior / siguiente en la grilla |
| Espacio / Retroceso | siguiente / anterior en el visor |
| Inicio / Fin | primera / última |
| Enter | abrir la imagen seleccionada |
| Esc | volver a la grilla |
| F | pantalla completa |
| + − | acercar / alejar |
| 0 | zoom 100 % |
| ⌘0 | ajustar a la ventana |
| L R | rotar a izquierda / derecha (solo la vista) |
| ⌘O | abrir carpeta |
| ⌘Q | salir |

Con el mouse: arrastrar mueve la imagen, pellizcar o ⌘+rueda hace zoom, la
rueda sola desplaza y el doble clic alterna entre ajustar y tamaño real.
También se puede arrastrar una carpeta o una imagen sobre la ventana.

## Formatos

PNG, JPEG, GIF, WebP, TIFF, BMP, ICO, AVIF y compañía por el crate `image`;
HEIC/HEIF y los RAW de cámara (CR2, CR3, NEF, ARW, DNG, ORF, RW2, RAF, SRW) por
ImageIO, el mismo motor que usa Vista Previa. Sin dependencias de Homebrew.

La orientación EXIF se aplica siempre: las fotos verticales del celular se ven
verticales.

## Desarrollo

```sh
make check      # formato, clippy y tests
make test
cargo run --release --example bench -- ~/Pictures   # medir escaneo y miniaturas
```

Estructura:

| Archivo | Qué hace |
|---|---|
| `src/session.rs` | Navegación: carpeta, lista ordenada de imágenes e índice actual |
| `src/scan.rs` | Lectura del directorio y orden natural (`img2` antes que `img10`) |
| `src/loader.rs` | Pool de decodificación en background, con prioridades y cancelación |
| `src/cache.rs` | Cache LRU acotado por bytes, no por cantidad |
| `src/decode/` | Decodificación: crate `image` e ImageIO |
| `src/view.rs` | Encuadre del visor: ajuste, zoom, paneo, rotación |
| `src/ui/` | Grilla, visor y barra superior |
| `src/platform/` | Apertura de archivos desde el Finder (Apple Events) |

Tres decisiones que sostienen el resto:

1. **El hilo de interfaz nunca decodifica.** Todo pasa por `loader.rs`; la
   interfaz solo sube texturas y dibuja.
2. **Los pedidos obsoletos se cancelan.** Cada cambio de imagen invalida lo
   encolado; sin eso, mantener → apretada encola cientos de decodificaciones
   que terminan tapando a la que se está esperando.
3. **Las miniaturas salen de ImageIO**, que usa la miniatura ya embebida en el
   archivo en vez de decodificar la foto entera.

Para diagnosticar aperturas desde el Finder, donde no hay terminal a la vista:

```sh
open -a /Applications/CheCheViewer.app foto.jpg --env CHECHEVIEWER_DEBUG=1 --stderr /tmp/cv.log
```
