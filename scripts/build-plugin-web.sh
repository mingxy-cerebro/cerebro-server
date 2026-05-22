#!/bin/bash
set -e

SCRIPT_DIR="$(cd "$(dirname "$0")" && pwd)"
ROOT_DIR="$(dirname "$SCRIPT_DIR")"
WEB_SRC="$ROOT_DIR/omem-web"

echo "Building omem-web..."
cd "$WEB_SRC" && npm run build && cd "$ROOT_DIR"

rm -rf "$ROOT_DIR/plugins/opencode/web/"

mkdir -p "$ROOT_DIR/plugins/opencode/web/"
cp -r "$WEB_SRC/dist/"* "$ROOT_DIR/plugins/opencode/web/"

if [ ! -f "$ROOT_DIR/plugins/opencode/web/index.html" ]; then
  echo "Error: index.html not found in build output"
  exit 1
fi

echo "Build complete. Output in plugins/opencode/web/"
ls -la "$ROOT_DIR/plugins/opencode/web/"
