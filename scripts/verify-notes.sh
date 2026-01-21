#!/bin/bash
# Verify available notes on the Miden node
#
# This script wraps the verify-notes Rust binary, building it if needed.
#
# Usage:
#   ./scripts/verify-notes.sh                     # List consumable notes
#   ./scripts/verify-notes.sh list --filter all   # List all notes
#   ./scripts/verify-notes.sh show <note_id>      # Show details for a specific note
#   ./scripts/verify-notes.sh sync                # Sync with the node
#   ./scripts/verify-notes.sh info                # Show client info
#   ./scripts/verify-notes.sh accounts            # List tracked accounts
#
# Environment variables:
#   MIDEN_RPC_URL    - Miden node RPC endpoint (default: http://localhost:57291)
#   MIDEN_STORE_PATH - Path to store directory (default: ~/.miden-verify)
#   RUST_LOG         - Set to enable verbose logging (e.g., RUST_LOG=debug)

set -e

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

info() { echo -e "${BLUE}[INFO]${NC} $1"; }
success() { echo -e "${GREEN}[OK]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; }

# Find project root (directory containing Cargo.toml)
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

# Binary name and paths
BINARY_NAME="verify-notes"
RELEASE_BINARY="$PROJECT_ROOT/target/release/$BINARY_NAME"
DEBUG_BINARY="$PROJECT_ROOT/target/debug/$BINARY_NAME"

# Check if we should use release build
USE_RELEASE="${USE_RELEASE:-1}"

# Determine which binary to use
if [[ "$USE_RELEASE" == "1" ]]; then
    BINARY="$RELEASE_BINARY"
    BUILD_FLAGS="--release"
else
    BINARY="$DEBUG_BINARY"
    BUILD_FLAGS=""
fi

# Build if necessary
build_if_needed() {
    local needs_build=false

    if [[ ! -f "$BINARY" ]]; then
        needs_build=true
        info "Binary not found, building..."
    else
        # Check if source is newer than binary
        local newest_source
        newest_source=$(find "$PROJECT_ROOT/src" -name "*.rs" -newer "$BINARY" 2>/dev/null | head -1)
        if [[ -n "$newest_source" ]]; then
            needs_build=true
            info "Source files changed, rebuilding..."
        fi
    fi

    if $needs_build; then
        info "Building $BINARY_NAME..."
        cd "$PROJECT_ROOT"
        if cargo build $BUILD_FLAGS --bin "$BINARY_NAME" 2>&1; then
            success "Build complete"
        else
            error "Build failed"
            exit 1
        fi
    fi
}

# Print usage
usage() {
    cat << EOF
Usage: $0 [COMMAND] [OPTIONS]

Verify available notes on the Miden node.

Commands:
  list              List notes (default)
    --filter <type>   Filter: consumable (default), all
    --account <id>    Filter by account ID (hex)
  show <note_id>    Show details for a specific note
  sync              Sync with the Miden node
  info              Show client info and sync status
  accounts          List all tracked accounts

Global Options:
  --rpc-url <url>   Miden node RPC endpoint
  --store-path <p>  Path to store directory
  --help            Show this help message

Environment Variables:
  MIDEN_RPC_URL     Miden node RPC endpoint (default: http://localhost:57291)
  MIDEN_STORE_PATH  Path to store directory (default: ~/.miden-verify)
  RUST_LOG          Enable verbose logging (e.g., RUST_LOG=debug)
  USE_RELEASE       Set to 0 for debug build (default: 1)

Examples:
  $0                              # List consumable notes
  $0 list --filter all            # List all available notes
  $0 show 0x1234...               # Show specific note details
  $0 sync                         # Sync with the node
  $0 accounts                     # List tracked accounts
EOF
}

# Main
main() {
    # Handle --help before building
    for arg in "$@"; do
        if [[ "$arg" == "--help" || "$arg" == "-h" ]]; then
            usage
            exit 0
        fi
    done

    echo "========================================"
    echo "  Miden Node Note Verification Tool"
    echo "========================================"
    echo ""

    # Build if needed
    build_if_needed
    echo ""

    # Run the binary with all arguments
    exec "$BINARY" "$@"
}

main "$@"
