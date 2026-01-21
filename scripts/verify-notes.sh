#!/bin/bash
# Verify available notes on the Miden node
#
# This script wraps the verify-notes Rust binary to query the miden node
# and list/verify which notes are currently available.
#
# Usage:
#   ./scripts/verify-notes.sh                    # List all notes
#   ./scripts/verify-notes.sh --filter consumable # List only consumable notes
#   ./scripts/verify-notes.sh --show <note_id>   # Show details for a specific note
#   ./scripts/verify-notes.sh --sync-only        # Just sync without listing
#   ./scripts/verify-notes.sh --summary          # Show summary of all note types
#   ./scripts/verify-notes.sh --info             # Show client info
#
# Environment variables:
#   MIDEN_RPC_URL    - Miden node RPC endpoint (default: http://localhost:57291)
#   MIDEN_STORE_PATH - Path to store client state (default: ~/.miden-verify-notes)
#   BUILD_RELEASE    - If set, build in release mode

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

# Configuration
MIDEN_RPC_URL="${MIDEN_RPC_URL:-http://localhost:57291}"
BINARY_NAME="verify-notes"

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Print colored output
info() { echo -e "${BLUE}[INFO]${NC} $1"; }
success() { echo -e "${GREEN}[OK]${NC} $1"; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
error() { echo -e "${RED}[ERROR]${NC} $1"; }

# Determine build mode
if [[ -n "$BUILD_RELEASE" ]]; then
    BUILD_MODE="release"
    BUILD_FLAGS="--release"
    BINARY_PATH="$PROJECT_ROOT/target/release/$BINARY_NAME"
else
    BUILD_MODE="debug"
    BUILD_FLAGS=""
    BINARY_PATH="$PROJECT_ROOT/target/debug/$BINARY_NAME"
fi

# Build the binary if needed
build_binary() {
    local needs_build=false

    if [[ ! -f "$BINARY_PATH" ]]; then
        needs_build=true
        info "Binary not found, building..."
    elif [[ "$PROJECT_ROOT/src/bin/verify_notes.rs" -nt "$BINARY_PATH" ]] || \
         [[ "$PROJECT_ROOT/Cargo.toml" -nt "$BINARY_PATH" ]]; then
        needs_build=true
        info "Source files changed, rebuilding..."
    fi

    if $needs_build; then
        info "Building verify-notes ($BUILD_MODE mode)..."
        (cd "$PROJECT_ROOT" && cargo build --bin "$BINARY_NAME" $BUILD_FLAGS)
        if [[ $? -eq 0 ]]; then
            success "Build complete: $BINARY_PATH"
        else
            error "Build failed"
            exit 1
        fi
    fi
}

# Print usage
usage() {
    cat << EOF
Usage: $0 [OPTIONS]

Verify available notes on the Miden node.

This script builds and runs the verify-notes Rust binary, which directly
queries the Miden node RPC to list and inspect notes.

Options:
  --filter <type>     Filter notes by status (all|consumable|committed|consumed|processing|expected)
  --show <note_id>    Show details for a specific note
  --sync-only         Sync with node without listing notes
  --summary           Show summary of all note types
  --info              Show client state info
  --account <id>      Filter notes for a specific account
  --help              Show this help message

Environment Variables:
  MIDEN_RPC_URL       Miden node RPC endpoint (default: http://localhost:57291)
  MIDEN_STORE_PATH    Path to store client state (default: ~/.miden-verify-notes)
  BUILD_RELEASE       If set, build and run in release mode

Examples:
  $0                          # Sync and list all notes
  $0 --filter consumable      # List only consumable notes
  $0 --show 0x1234...         # Show specific note details
  $0 --summary                # Show counts of all note types
  $0 --info                   # Show client info
EOF
}

# Check for help flag
for arg in "$@"; do
    if [[ "$arg" == "--help" ]] || [[ "$arg" == "-h" ]]; then
        usage
        exit 0
    fi
done

# Main
main() {
    # Build the binary if needed
    build_binary

    # Build the command with environment variables
    local cmd=("$BINARY_PATH")

    # Add RPC URL
    cmd+=(--rpc-url "$MIDEN_RPC_URL")

    # Add store path if set
    if [[ -n "$MIDEN_STORE_PATH" ]]; then
        cmd+=(--store-path "$MIDEN_STORE_PATH")
    fi

    # Pass through all arguments
    cmd+=("$@")

    # Run the binary
    exec "${cmd[@]}"
}

main "$@"
