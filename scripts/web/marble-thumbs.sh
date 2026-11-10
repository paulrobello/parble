#!/usr/bin/env bash
# Marble picker thumbnails (P5-T3): one native `closeup` shot per marble type on High with DoF off,
# cropped square around the resting marble and scaled to 160 px WebP in web/ui/thumbs/marbles.
# Needs a built native binary (cargo build -p parble-app), ImageMagick and cwebp. Run from the repo root.
set -euo pipefail
BIN=target/debug/parble
SHOTS=target/shots/marbles
OUT=web/ui/thumbs/marbles
mkdir -p "$SHOTS" "$OUT"
for m in cats-eye glazed steel swirl onionskin opal glow; do
  "$BIN" --scenario closeup --marble "$m" --quality high --dof 0 \
    --screenshot "$SHOTS/$m.png" --screenshot-after 4 --exit-after 5 > "$SHOTS/$m.log" 2>&1
  grep -q "ok=true" "$SHOTS/$m.log" || { echo "$m: capture not lit" >&2; exit 1; }
  magick "$SHOTS/$m.png" -crop 204x204+538+260 +repage -resize 160x160 "$SHOTS/$m-crop.png"
  cwebp -quiet -q 86 -m 6 "$SHOTS/$m-crop.png" -o "$OUT/$m.webp"
  echo "$m ok"
done
