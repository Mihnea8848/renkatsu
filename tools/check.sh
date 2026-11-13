#!/bin/sh
# Copyright (c) 2026 Mihnea Gîrbăcică (Mihnea8848)
# SPDX-License-Identifier: GPL-3.0-or-later
#
# Full local check. CI has no game folder, so the corpus tests and the
# contact sheet only run here. Needs renkatsu.toml or RENKATSU_GAME_DIR.
set -eu

cd "$(dirname "$0")/.."

echo "== format"
cargo fmt --all -- --check

echo "== lint"
cargo clippy --workspace --all-targets --locked -- -D warnings

echo "== tests (with the game folder: every card, bundle and mod)"
cargo test --workspace --locked --release

if cargo run -q --release -p rk-cli -- index >/dev/null 2>&1; then
    echo "== contact sheet"
    mkdir -p renders
    cargo run -q --release -p rk-cli -- compare -o renders/compare.png | tail -n 1
else
    echo "== no game folder configured: corpus tests were skipped, no contact sheet"
fi
