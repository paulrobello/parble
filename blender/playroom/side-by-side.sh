#!/usr/bin/env bash
# P1-T5 owner review sheet: each look-dev reference next to the matching parble shot.
# Usage: blender/playroom/side-by-side.sh <rug.png> <blocks.png> <out.png>
set -euo pipefail
root="$(cd "$(dirname "$0")/../.." && pwd)"
rug="$1" blocks="$2" out="$3"
tile() { magick "$1" -resize 960x540^ -gravity center -extent 960x540 \
  -gravity north -font /System/Library/Fonts/Supplemental/Arial.ttf -fill white -undercolor '#000000a0' -pointsize 22 -annotate +0+8 " $2 " "$3"; }
tmp="$(mktemp -d)"
tile "$root/docs/reference/ref-07-playroom-rug-lantern.webp" "ref-07" "$tmp/a.png"
tile "$rug" "parble --scenario playroom --view rug --quality high" "$tmp/b.png"
tile "$root/docs/reference/ref-08-playroom-blocks.webp" "ref-08" "$tmp/c.png"
tile "$blocks" "parble --scenario playroom --view blocks --quality high" "$tmp/d.png"
magick \( "$tmp/a.png" "$tmp/b.png" +append \) \( "$tmp/c.png" "$tmp/d.png" +append \) -append "$out"
rm -rf "$tmp"
echo "wrote $out"
