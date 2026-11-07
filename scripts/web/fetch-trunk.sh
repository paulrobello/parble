#!/usr/bin/env bash
# Installs the pinned Trunk 0.21.14 (plans/phase-0-foundations.md, hex-world CI) into
# target/tools/trunk-0.21.14/trunk (gitignored) and prints its version. The release tarball's sha256
# is pinned per host; the Linux x86_64 pin matches hex-world's CI. A trunk on PATH (0.22.x on the dev
# box) is never used by the Makefile, so local builds and CI build with the same bundler.
set -euo pipefail
version=0.21.14
case "$(uname -s) $(uname -m)" in
  "Darwin arm64") triple=aarch64-apple-darwin; sha=764e299dd50d89442a4e96a236349f57961984b701e74d3dbdb39cd1c9f5101e ;;
  "Linux x86_64") triple=x86_64-unknown-linux-gnu; sha=f2b4680cd239693a646a2795e4633c625328d7b2a044fbe749fa3a2fe9e7036b ;;
  *) echo "fetch-trunk: no pinned Trunk $version build for $(uname -s) $(uname -m)" >&2; exit 1 ;;
esac
root="$(cd "$(dirname "$0")/../.." && pwd)"
dest="$root/target/tools/trunk-$version"
if [ ! -x "$dest/trunk" ]; then
  mkdir -p "$dest"
  tgz="$dest/trunk.tar.gz"
  curl -fL --max-time 300 "https://github.com/trunk-rs/trunk/releases/download/v$version/trunk-$triple.tar.gz" -o "$tgz"
  got="$(shasum -a 256 "$tgz" | cut -d' ' -f1)"
  if [ "$got" != "$sha" ]; then
    echo "fetch-trunk: $tgz sha256 $got != pinned $sha" >&2
    rm -f "$tgz"
    exit 1
  fi
  tar -xzf "$tgz" -C "$dest"
  rm -f "$tgz"
fi
"$dest/trunk" --version
