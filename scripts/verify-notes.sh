#!/bin/bash
# Verify available notes on the Miden node
#
# This script queries the miden node to list and verify which notes are currently
# available. Useful for debugging and verifying node state during development.
#
# Usage:
#   ./scripts/verify-notes.sh                    # List all notes
#   ./scripts/verify-notes.sh --filter committed # List only committed notes
#   ./scripts/verify-notes.sh --show <note_id>   # Show details for a specific note
#   ./scripts/verify-notes.sh --init             # Initialize miden-client for localhost
#   ./scripts/verify-notes.sh --sync-only        # Just sync without listing
#
# Environment variables:
#   MIDEN_RPC_URL  - Miden node RPC endpoint (default: http://localhost:57291)
#   MIDEN_LOCAL    - If set, use local .miden directory instead of global

set -e

# Configuration
MIDEN_RPC_URL="${MIDEN_RPC_URL:-http://localhost:57291}"
MIDEN_CLIENT="${MIDEN_CLIENT:-miden-client}"

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

# Check if miden-client is available
check_miden_client() {
    if ! command -v "$MIDEN_CLIENT" &> /dev/null; then
        error "miden-client not found in PATH"
        echo "Install miden-client or set MIDEN_CLIENT environment variable"
        exit 1
    fi
    success "miden-client found: $(which $MIDEN_CLIENT)"
}

# Initialize miden-client for localhost
init_client() {
    local init_args="--network $MIDEN_RPC_URL"

    if [[ -n "$MIDEN_LOCAL" ]]; then
        init_args="$init_args --local"
        info "Initializing miden-client with local config..."
    else
        info "Initializing miden-client with global config..."
    fi

    if $MIDEN_CLIENT init $init_args; then
        success "miden-client initialized for $MIDEN_RPC_URL"
    else
        error "Failed to initialize miden-client"
        exit 1
    fi
}

# Sync with the Miden node
sync_client() {
    info "Syncing with Miden node at $MIDEN_RPC_URL..."

    if $MIDEN_CLIENT sync 2>&1; then
        success "Sync completed"
    else
        warn "Sync may have failed - client might not be initialized"
        echo "Run: $0 --init"
        return 1
    fi
}

# Get client info
show_info() {
    info "Client state summary:"
    $MIDEN_CLIENT info 2>&1 || warn "Could not get client info"
}

# List notes with optional filter
list_notes() {
    local filter="${1:-all}"

    info "Listing notes (filter: $filter)..."
    echo ""

    if ! $MIDEN_CLIENT notes --list "$filter" 2>&1; then
        warn "Failed to list notes - try running: $0 --init && $0 --sync-only"
        return 1
    fi
}

# Show details for a specific note
show_note() {
    local note_id="$1"

    if [[ -z "$note_id" ]]; then
        error "Note ID required for --show"
        exit 1
    fi

    info "Showing note details for: $note_id"
    echo ""

    $MIDEN_CLIENT notes --show "$note_id" --with-code 2>&1 || {
        warn "Failed to show note - it may not exist or client may need sync"
        return 1
    }
}

# Show summary of all note types
show_summary() {
    info "Note Summary:"
    echo ""

    echo "=== Expected Notes ==="
    $MIDEN_CLIENT notes --list expected 2>&1 || echo "(none or error)"
    echo ""

    echo "=== Committed Notes ==="
    $MIDEN_CLIENT notes --list committed 2>&1 || echo "(none or error)"
    echo ""

    echo "=== Consumable Notes ==="
    $MIDEN_CLIENT notes --list consumable 2>&1 || echo "(none or error)"
    echo ""

    echo "=== Consumed Notes ==="
    $MIDEN_CLIENT notes --list consumed 2>&1 || echo "(none or error)"
    echo ""

    echo "=== Processing Notes ==="
    $MIDEN_CLIENT notes --list processing 2>&1 || echo "(none or error)"
}

# Print usage
usage() {
    cat << EOF
Usage: $0 [OPTIONS]

Verify available notes on the Miden node.

Options:
  --init              Initialize miden-client for localhost node
  --sync-only         Sync with node without listing notes
  --filter <type>     List notes with filter (all|expected|committed|consumed|processing|consumable)
  --show <note_id>    Show details for a specific note
  --summary           Show summary of all note types
  --info              Show client state info
  --help              Show this help message

Environment Variables:
  MIDEN_RPC_URL       Miden node RPC endpoint (default: http://localhost:57291)
  MIDEN_LOCAL         If set, use local .miden directory
  MIDEN_CLIENT        Path to miden-client binary (default: miden-client)

Examples:
  $0                          # Sync and list all notes
  $0 --init                   # Initialize client for localhost
  $0 --filter consumable      # List only consumable notes
  $0 --show 0x1234...         # Show specific note details
  $0 --summary                # Show counts of all note types
EOF
}

# Main
main() {
    local do_init=false
    local sync_only=false
    local filter=""
    local show_id=""
    local show_summary_flag=false
    local show_info_flag=false

    # Parse arguments
    while [[ $# -gt 0 ]]; do
        case $1 in
            --init)
                do_init=true
                shift
                ;;
            --sync-only)
                sync_only=true
                shift
                ;;
            --filter)
                filter="$2"
                shift 2
                ;;
            --show)
                show_id="$2"
                shift 2
                ;;
            --summary)
                show_summary_flag=true
                shift
                ;;
            --info)
                show_info_flag=true
                shift
                ;;
            --help|-h)
                usage
                exit 0
                ;;
            *)
                error "Unknown option: $1"
                usage
                exit 1
                ;;
        esac
    done

    echo "========================================"
    echo "  Miden Node Note Verification Tool"
    echo "========================================"
    echo ""

    # Check miden-client exists
    check_miden_client
    echo ""

    # Initialize if requested
    if $do_init; then
        init_client
        echo ""
    fi

    # Show info if requested
    if $show_info_flag; then
        show_info
        echo ""
    fi

    # Try to sync
    if ! sync_client; then
        if ! $do_init; then
            echo ""
            warn "Try running: $0 --init"
        fi
        exit 1
    fi
    echo ""

    # Exit early if sync-only
    if $sync_only; then
        success "Sync complete. Run without --sync-only to list notes."
        exit 0
    fi

    # Show specific note
    if [[ -n "$show_id" ]]; then
        show_note "$show_id"
        exit 0
    fi

    # Show summary
    if $show_summary_flag; then
        show_summary
        exit 0
    fi

    # List notes (default: all)
    list_notes "${filter:-all}"
}

main "$@"
