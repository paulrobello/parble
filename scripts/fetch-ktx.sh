#!/usr/bin/env bash
# Installs the pinned KTX-Software 4.4.2 tools into target/tools/ktx-4.4.2 (gitignored) and writes
# target/tools/ktx-env.sh. macOS arm64 only: material builds are local, not CI.
# KTX_PKG=<path> reuses an already-downloaded pkg; its sha256 is verified like a fresh download.
set -euo pipefail
version=4.4.2
pkg_sha256=500bd8f9d63358c3f3a0d83b724c8574436a72c37dc0e4bad90ec1ca38032c3c
host="${PARBLE_HOST:-$(uname -s) $(uname -m)}"
if [ "$host" != "Darwin arm64" ]; then
  echo "fetch-ktx: KTX-Software $version is pinned to its macOS arm64 build and this host is $host." >&2
  exit 1
fi
root="$(cd "$(dirname "$0")/.." && pwd)"
tools="$root/target/tools"
dest="$tools/ktx-$version"
bin="$dest/KTX-Software-$version-Darwin-arm64-tools.pkg/Payload/usr/local/bin"
lib="$dest/KTX-Software-$version-Darwin-arm64-library.pkg/Payload/usr/local/lib"
if [ ! -x "$bin/ktx" ]; then
  mkdir -p "$tools"
  pkg="$tools/KTX-Software-$version-Darwin-arm64.pkg"
  if [ -n "${KTX_PKG:-}" ]; then
    cp "$KTX_PKG" "$pkg"
  else
    curl -fL --max-time 300 "https://github.com/KhronosGroup/KTX-Software/releases/download/v$version/KTX-Software-$version-Darwin-arm64.pkg" -o "$pkg"
  fi
  got="$(shasum -a 256 "$pkg" | cut -d' ' -f1)"
  if [ "$got" != "$pkg_sha256" ]; then
    echo "fetch-ktx: $pkg sha256 $got != pinned $pkg_sha256" >&2
    rm -f "$pkg"
    exit 1
  fi
  rm -rf "$dest"
  pkgutil --expand-full "$pkg" "$dest"
fi
printf 'export PATH="%s:$PATH"\nexport DYLD_LIBRARY_PATH="%s${DYLD_LIBRARY_PATH:+:$DYLD_LIBRARY_PATH}"\n' "$bin" "$lib" > "$tools/ktx-env.sh"
. "$tools/ktx-env.sh"
ktx --version
