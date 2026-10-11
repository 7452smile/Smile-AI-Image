#!/bin/sh
set -eu

SCRIPT_DIR=$(CDPATH= cd -- "$(dirname -- "$0")" && pwd)
ROOT_DIR=$(CDPATH= cd -- "$SCRIPT_DIR/.." && pwd)

if [ -x "$ROOT_DIR/bin/smile-mcp" ]; then
  exec "$ROOT_DIR/bin/smile-mcp"
fi

exec cargo run --quiet --manifest-path "$ROOT_DIR/Cargo.toml" -p smile-mcp
