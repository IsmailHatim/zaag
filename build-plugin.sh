#!/bin/sh
# Compiles the zellij plugin into dist/ (needs cargo and `rustup target add wasm32-wasip1`).
# The paths of the machine it is built on are kept out of the binary.
set -eu
here=$(cd "$(dirname "$0")" && pwd)
cargo_home="${CARGO_HOME:-$HOME/.cargo}"
cd "$here/plugin"
RUSTFLAGS="--remap-path-prefix=$cargo_home=/cargo --remap-path-prefix=$here=/zaag --remap-path-prefix=$HOME=/home" \
  cargo build --release --target wasm32-wasip1
mkdir -p "$here/dist"
cp target/wasm32-wasip1/release/zaag-sidebar.wasm "$here/dist/zaag-sidebar.wasm"
echo "built $here/dist/zaag-sidebar.wasm"
