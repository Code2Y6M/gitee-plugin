#!/usr/bin/env bash
# Build the Gitee plugin for OpenIDEA.
# Requires the wasm32-wasip2 target; installed automatically if missing.
set -euo pipefail

cd "$(dirname "$0")"

if ! rustup target list --installed | grep -q '^wasm32-wasip2$'; then
    echo "Installing wasm32-wasip2 target..."
    rustup target add wasm32-wasip2
fi

echo "Building plugin (release, wasm32-wasip2)..."
cargo build --release --target wasm32-wasip2

mkdir -p dist
cp "target/wasm32-wasip2/release/openidea_plugin_gitee.wasm" dist/plugin.wasm

echo "Built dist/plugin.wasm"
echo "Run 'openidea-plugin check' to validate, or 'openidea-plugin link' to load it in the IDE."
