#!/usr/bin/env bash
# Installs the pinned actionlint 1.7.12 into target/tools/actionlint-1.7.12/actionlint (gitignored) and
# prints its version. The release tarball's sha256 is pinned per host. An actionlint on PATH is never
# used, so local and CI lint with the same rules.
set -euo pipefail
version=1.7.12
case "$(uname -s) $(uname -m)" in
  "Darwin arm64") os=darwin_arm64; sha=aba9ced2dee8d27fecca3dc7feb1a7f9a52caefa1eb46f3271ea66b6e0e6953f ;;
  "Linux x86_64") os=linux_amd64; sha=8aca8db96f1b94770f1b0d72b6dddcb1ebb8123cb3712530b08cc387b349a3d8 ;;
  *) echo "fetch-actionlint: no pinned actionlint $version build for $(uname -s) $(uname -m)" >&2; exit 1 ;;
esac
root="$(cd "$(dirname "$0")/.." && pwd)"
dest="$root/target/tools/actionlint-$version"
if [ ! -x "$dest/actionlint" ]; then
  mkdir -p "$dest"
  tgz="$dest/actionlint.tar.gz"
  curl -fL --max-time 120 "https://github.com/rhysd/actionlint/releases/download/v$version/actionlint_${version}_$os.tar.gz" -o "$tgz"
  got="$(shasum -a 256 "$tgz" | cut -d' ' -f1)"
  if [ "$got" != "$sha" ]; then
    echo "fetch-actionlint: $tgz sha256 $got != pinned $sha" >&2
    rm -f "$tgz"
    exit 1
  fi
  tar -xzf "$tgz" -C "$dest" actionlint
  rm -f "$tgz"
fi
"$dest/actionlint" -version | head -1
