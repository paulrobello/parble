#!/usr/bin/env bash
# P1-T5 runtime derivatives: KTX2 (RGBA8, zstd, full mip chain) from the sources in
# assets-source/playroom. Needs the pinned KTX-Software (make fetch-tools) and ImageMagick.
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
# shellcheck disable=SC1091
source "$root/target/tools/ktx-env.sh"
src="$root/assets-source/playroom"
out="$root/assets/playroom"
stage="$root/target/playroom-stage"
mkdir -p "$out" "$stage"

# Rug: base colour 512 (sRGB), normal 256 (linear, renormalised by the mip filter).
magick "$src/rug/rug.base_color.png" -resize 512x512 -alpha set "PNG32:$stage/rug.base_color.png"
magick "$src/rug/rug.normal.png" -resize 256x256 -alpha set "PNG32:$stage/rug.normal.png"
ktx create --format R8G8B8A8_SRGB --assign-tf srgb --generate-mipmap --zstd 19 \
  "$stage/rug.base_color.png" "$out/rug.base_color.ktx2"
ktx create --format R8G8B8A8_UNORM --assign-tf linear --generate-mipmap --normal-mode --zstd 19 \
  "$stage/rug.normal.png" "$out/rug.normal.ktx2"

# Backdrop plate: out-of-focus already; pre-blurred a little more and downscaled to 1024x576.
magick "$src/backdrop/backdrop-01.png" -resize 1024x576! -blur 0x3 -alpha set \
  "PNG32:$stage/backdrop.png"
ktx create --format R8G8B8A8_SRGB --assign-tf srgb --generate-mipmap --zstd 19 \
  "$stage/backdrop.png" "$out/backdrop.ktx2"

for f in "$out"/*.ktx2; do ktx validate "$f" >/dev/null; done
ls -l "$out"
