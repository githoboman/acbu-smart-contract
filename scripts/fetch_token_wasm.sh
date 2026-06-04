#!/usr/bin/env bash
set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"
DEST="$PROJECT_ROOT/soroban_token_contract.wasm"

if [[ -f "$DEST" ]]; then
  echo "[OK] soroban_token_contract.wasm already present."
  exit 0
fi

echo "[INFO] Cloning soroban-examples v22.0.1 to build token contract..."
TMPDIR=$(mktemp -d)
git clone --depth 1 -b v22.0.1 https://github.com/stellar/soroban-examples "$TMPDIR/soroban-examples"

echo "[INFO] Building token contract..."
cd "$TMPDIR/soroban-examples/token"
cargo build --target wasm32-unknown-unknown --release

cp "$TMPDIR/soroban-examples/target/wasm32-unknown-unknown/release/soroban_token_contract.wasm" "$DEST"
rm -rf "$TMPDIR"

echo "[OK] soroban_token_contract.wasm ready at $DEST"