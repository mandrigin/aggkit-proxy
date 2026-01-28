#!/bin/bash
# Start the Miden RPC proxy locally
#
# This script builds and runs the proxy service.
# Make sure the miden-node is running first (see start-miden-node.sh).

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_DIR"

# Auto-detect RPC URL from Docker (Kurtosis maps random host ports)
if [[ -z "${MIDEN_RPC_URL:-}" ]]; then
    _node=$(docker ps --format '{{.Names}}' 2>/dev/null | grep -E '^miden-node' | head -1)
    if [[ -n "$_node" ]]; then
        _port=$(docker port "$_node" 57291/tcp 2>/dev/null | head -1 | cut -d: -f2)
        if [[ -n "$_port" ]]; then
            export MIDEN_RPC_URL="http://localhost:$_port"
        fi
    fi
fi
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
