#!/usr/bin/env bash
# Rebuild the PB_kit showcase in the live Blender session and take a viewport shot.
# Usage: blender/kit/live.sh [shot.png]. Holds /tmp/parble-blender.lock (shared with other agents).
set -euo pipefail
here="$(cd "$(dirname "$0")" && pwd)"
bl="$HOME/Repos/mux-xr/.local/tools/bl.py"
lock=/tmp/parble-blender.lock
got=0
for _ in $(seq 150); do if mkdir "$lock" 2>/dev/null; then got=1; break; fi; sleep 2; done
[ "$got" = 1 ] || { echo "lock busy for 5 min"; exit 1; }
trap 'rmdir "$lock"' EXIT
code="$(mktemp -t pbk-live).py"
sed "s#__KIT_DIR__#$here#" "$here/live_showcase.py" > "$code"
python3 "$bl" exec "$code"
rm -f "$code"
if [ $# -ge 1 ]; then python3 "$bl" shot "$1" 1200; fi
