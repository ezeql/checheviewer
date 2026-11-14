#!/bin/sh
# Generates bundle/AppIcon.icns. Requires ImageMagick (brew install imagemagick).
#
# The icon is a stylized photo (white frame, mountain and sun) on a gradient
# background, with macOS-style rounded corners.
set -eu

cd "$(dirname "$0")"
WORK=$(mktemp -d)
trap 'rm -rf "$WORK"' EXIT

S=1024
R=225 # corner radius

# Background: gradient clipped by a rounded-corner mask.
magick -size ${S}x${S} gradient:'#5b8def-#8b5cf6' "$WORK/gradient.png"
magick -size ${S}x${S} xc:black -fill white \
	-draw "roundrectangle 0,0 $((S - 1)),$((S - 1)) $R,$R" -alpha off "$WORK/mask.png"
magick "$WORK/gradient.png" "$WORK/mask.png" \
	-alpha off -compose CopyOpacity -composite "$WORK/base.png"

# The "photo": white frame with a mountain and a sun.
magick -size ${S}x${S} xc:none \
	-fill white -draw "roundrectangle 205,255 819,769 28,28" \
	-fill '#5b8def' -draw "polygon 255,719 470,420 640,719" \
	-fill '#7c5cf0' -draw "polygon 520,719 660,530 769,719" \
	-fill '#ffd166' -draw "circle 655,375 655,325" \
	"$WORK/photo.png"

magick "$WORK/base.png" "$WORK/photo.png" -compose over -composite "$WORK/icon.png"

# Every size iconutil asks for.
ICONSET="$WORK/AppIcon.iconset"
mkdir -p "$ICONSET"
for size in 16 32 128 256 512; do
	magick "$WORK/icon.png" -resize ${size}x${size} "$ICONSET/icon_${size}x${size}.png"
	magick "$WORK/icon.png" -resize $((size * 2))x$((size * 2)) "$ICONSET/icon_${size}x${size}@2x.png"
done

iconutil -c icns "$ICONSET" -o AppIcon.icns
echo "Generated $(pwd)/AppIcon.icns"
