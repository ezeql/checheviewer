#!/bin/sh
# Genera bundle/AppIcon.icns. Requiere ImageMagick (brew install imagemagick).
#
# El ícono es una foto estilizada — marco blanco, montaña y sol — sobre un
# fondo con degradado, con las esquinas redondeadas al estilo de macOS.
set -eu

cd "$(dirname "$0")"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

S=1024
R=225 # radio de las esquinas

# Fondo: degradado recortado con una máscara de esquinas redondeadas.
magick -size ${S}x${S} gradient:'#5b8def-#8b5cf6' "$WORK/gradient.png"
magick -size ${S}x${S} xc:black -fill white \
	-draw "roundrectangle 0,0 $((S - 1)),$((S - 1)) $R,$R" -alpha off "$WORK/mask.png"
magick "$WORK/gradient.png" "$WORK/mask.png" \
	-alpha off -compose CopyOpacity -composite "$WORK/base.png"

# La "foto": marco blanco con montaña y sol.
magick -size ${S}x${S} xc:none \
	-fill white -draw "roundrectangle 205,255 819,769 28,28" \
	-fill '#5b8def' -draw "polygon 255,719 470,420 640,719" \
	-fill '#7c5cf0' -draw "polygon 520,719 660,530 769,719" \
	-fill '#ffd166' -draw "circle 655,375 655,325" \
	"$WORK/photo.png"

magick "$WORK/base.png" "$WORK/photo.png" -compose over -composite "$WORK/icon.png"

# Todos los tamaños que pide iconutil.
ICONSET="$WORK/AppIcon.iconset"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
	magick "$WORK/icon.png" -resize ${size}x${size} "$ICONSET/icon_${size}x${size}.png"
	magick "$WORK/icon.png" -resize $((size * 2))x$((size * 2)) "$ICONSET/icon_${size}x${size}@2x.png"
done

iconutil -c icns "$ICONSET" -o AppIcon.icns
echo "Generado $(pwd)/AppIcon.icns"
