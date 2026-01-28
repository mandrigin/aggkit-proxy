#!/bin/bash
# Verify Notes - Shell wrapper for the Rust verify-notes binary
#
# Usage:
#   ./scripts/verify-notes.sh [options]
#
# Options:
#   --rpc-url URL      Miden node RPC URL (default: http://localhost:57291)
#   --store-path PATH  Client store path (default: /tmp/verify-notes-store)
#   --note-id ID       Query a specific note by ID from the node
#   --build            Force rebuild of the binary
#   --release          Build in release mode
#   --help             Show this help

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(cd "$SCRIPT_DIR/.." && pwd)"

# Auto-detect RPC URL from Docker if not set
_auto_detect_rpc_url() {
    local container
    container=$(docker ps --format '{{.Names}}' 2>/dev/null | grep -E '^miden-node' | head -1)
    if [[ -n "$container" ]]; then
        local port
        port=$(docker port "$container" 57291/tcp 2>/dev/null | head -1 | cut -d: -f2)
        if [[ -n "$port" ]]; then
            echo "http://localhost:$port"
            return
        fi
    fi
    echo "http://localhost:57291"
}

# Defaults
RPC_URL="${MIDEN_RPC_URL:-$(_auto_detect_rpc_url)}"
STORE_PATH="${MIDEN_STORE_PATH:-/tmp/verify-notes-store}"
BUILD_MODE="debug"
FORCE_BUILD=false
NOTE_ID=""
FRESH=""

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --rpc-url)
            RPC_URL="$2"
            shift 2
            ;;
        --store-path)
            STORE_PATH="$2"
            shift 2
            ;;
        --note-id|-n)
            NOTE_ID="$2"
            shift 2
            ;;
        --fresh)
            FRESH="--fresh"
            shift
            ;;
        --build)
            FORCE_BUILD=true
            shift
            ;;
        --release)
            BUILD_MODE="release"
            shift
            ;;
        --help)
            echo "Usage: $0 [options]"
            echo ""
            echo "Options:"
            echo "  --rpc-url URL      Miden node RPC URL (default: http://localhost:57291)"
            echo "  --store-path PATH  Client store path (default: /tmp/verify-notes-store)"
            echo "  --note-id ID       Query a specific note by ID from the node"
            echo "  --fresh            Clear local store before querying"
            echo "  --build            Force rebuild of the binary"
            echo "  --release          Build in release mode"
            echo "  --help             Show this help"
            echo ""
            echo "Environment variables:"
            echo "  MIDEN_RPC_URL      Same as --rpc-url"
            echo "  MIDEN_STORE_PATH   Same as --store-path"
            exit 0
            ;;
        *)
            echo "Unknown option: $1"
            exit 1
            ;;
    esac
done

cd "$PROJECT_DIR"

# Determine binary path
if [ "$BUILD_MODE" = "release" ]; then
    BINARY="$PROJECT_DIR/target/release/verify-notes"
    BUILD_FLAGS="--release"
else
    BINARY="$PROJECT_DIR/target/debug/verify-notes"
    BUILD_FLAGS=""
fi

# Build if needed
if [ "$FORCE_BUILD" = true ] || [ ! -f "$BINARY" ]; then
    echo "Building verify-notes ($BUILD_MODE)..."
    cargo build --bin verify-notes $BUILD_FLAGS
    echo ""
fi

# Run the binary
export MIDEN_RPC_URL="$RPC_URL"
export MIDEN_STORE_PATH="$STORE_PATH"

ARGS=""
if [ -n "$FRESH" ]; then
    ARGS="$ARGS --fresh"
fi
if [ -n "$NOTE_ID" ]; then
    ARGS="$ARGS --note-id $NOTE_ID"
fi

exec "$BINARY" $ARGS
