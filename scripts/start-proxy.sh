#!/bin/bash
# Start the Miden RPC proxy locally
#
# This script builds and runs the proxy service.
# Make sure the miden-node is running first (see start-miden-node.sh).

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_DIR"

# Default configuration
export MIDEN_RPC_URL="${MIDEN_RPC_URL:-http://localhost:57291}"
export RUST_LOG="${RUST_LOG:-info,miden_rpc_proxy=debug}"

echo "Building proxy..."
cargo build --release

echo ""
echo "Starting Miden RPC Proxy..."
echo "  MIDEN_RPC_URL: $MIDEN_RPC_URL"
echo "  RUST_LOG: $RUST_LOG"
echo "  Proxy endpoint: http://localhost:8546"
echo ""

cargo run --release "$@"
