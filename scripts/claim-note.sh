#!/bin/bash
#
# claim-note.sh - Claim a Miden note using a deterministic account
#
# Usage: ./claim-note.sh [--rebuild] <command> [args]
#
# Options:
#   --rebuild            Force rebuild of the binary before running
#
# Commands:
#   address              Print the derived claimer account address
#   claim <note-id>      Claim a note to the derived account
#   miden-to-eth <addr>  Convert Miden address to Eth format
#   eth-to-miden <addr>  Convert Eth address to Miden format
#
# The account is derived deterministically from CLAIMER_SEED.
# Same seed = same account, every time.

set -euo pipefail

# ============================================================================
# HARDCODED CONFIGURATION - Edit this seed phrase
# ============================================================================

# Seed phrase for deterministic account derivation
# IMPORTANT: Change this to your own secret phrase!
CLAIMER_SEED="my-test-claimer-seed-change-me"

# Miden node RPC endpoint (auto-detected from Docker if not set)
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
MIDEN_RPC_URL="${MIDEN_RPC_URL:-$(_auto_detect_rpc_url)}"

# Store path for miden client (keystore and SQLite store)
# If not set, the binary creates a unique temp folder per run
MIDEN_STORE_PATH="${MIDEN_STORE_PATH:-}"

# ============================================================================
# Address conversion utilities (for manual use)
# ============================================================================

# Convert Miden AccountId (15 bytes / 30 hex chars) to Ethereum address (20 bytes / 40 hex chars)
miden_to_eth() {
    local miden_addr="$1"
    miden_addr="${miden_addr#0x}"
    if [[ ${#miden_addr} -ne 30 ]]; then
        echo "ERROR: Miden address must be 30 hex chars (15 bytes), got ${#miden_addr}" >&2
        return 1
    fi
    echo "0x0000000000${miden_addr}"
}

# Convert Ethereum address (20 bytes / 40 hex chars) to Miden AccountId (15 bytes / 30 hex chars)
eth_to_miden() {
    local eth_addr="$1"
    eth_addr="${eth_addr#0x}"
    if [[ "${eth_addr:0:10}" != "0000000000" ]]; then
        echo "Warning: Eth address doesn't start with expected zeros" >&2
    fi
    echo "0x${eth_addr:10:30}"
}

# ============================================================================
# Script logic
# ============================================================================

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(cd "$SCRIPT_DIR/.." && pwd)"

print_usage() {
    echo "Usage: $0 [--rebuild] <command> [args]"
    echo ""
    echo "Options:"
    echo "  --rebuild                    Force rebuild of the binary"
    echo ""
    echo "Commands:"
    echo "  address                      Print the derived claimer account address"
    echo "  claim <note-id>              Claim a note to the derived account"
    echo "  miden-to-eth <miden-addr>    Convert Miden AccountId to Eth address"
    echo "  eth-to-miden <eth-addr>      Convert Eth address to Miden AccountId"
    echo ""
    echo "The account is derived deterministically from the seed phrase."
    echo "Edit CLAIMER_SEED in this script to change the account."
    echo ""
    echo "Current config:"
    echo "  MIDEN_RPC_URL:    $MIDEN_RPC_URL"
    echo "  MIDEN_STORE_PATH: ${MIDEN_STORE_PATH:-<unique temp folder per run>}"
}

# Parse --rebuild flag
REBUILD=false
if [[ $# -ge 1 && "$1" == "--rebuild" ]]; then
    REBUILD=true
    shift
fi

if [[ $# -lt 1 ]]; then
    print_usage
    exit 1
fi

COMMAND="$1"
shift

# Export environment variables for the Rust binary
export CLAIMER_SEED
export MIDEN_RPC_URL
# Only export MIDEN_STORE_PATH if explicitly set
if [[ -n "$MIDEN_STORE_PATH" ]]; then
    export MIDEN_STORE_PATH
fi

# The claim-note Rust binary (src/bin/claim_note.rs) was removed when the
# vibecoded proxy lib was supplanted by miden-agglayer. Claiming now happens in
# the miden-agglayer service; this shim keeps the pure address helpers working
# and redirects the binary-backed commands there.
binary_gone() {
    cat >&2 <<EOF
claim-note: the '$1' command was removed — the proxy is now miden-agglayer.

Claims are handled by the miden-agglayer service:
  - L1->L2 deposits are auto-claimed by the running service (no manual step).
  - L2->L1 exits: use the 'bridge-autoclaim' binary, or claim manually with
    'bridge-out-tool', from the miden-agglayer repo:
        docker exec <miden-agglayer-proxy-container> bridge-out-tool --help
  - Inspect notes on the node with:
        ./scripts/list-notes.sh
        ./scripts/list-unclaimed-notes.sh

The address-conversion helpers still work here:
  $0 miden-to-eth <addr>
  $0 eth-to-miden <addr>
EOF
    exit 2
}

case "$COMMAND" in
    address|addr|whoami)
        binary_gone "address"
        ;;

    claim)
        binary_gone "claim"
        ;;

    miden-to-eth|m2e)
        if [[ $# -lt 1 ]]; then
            echo "Usage: $0 miden-to-eth <miden-address>"
            exit 1
        fi
        miden_to_eth "$1"
        ;;

    eth-to-miden|e2m)
        if [[ $# -lt 1 ]]; then
            echo "Usage: $0 eth-to-miden <eth-address>"
            exit 1
        fi
        eth_to_miden "$1"
        ;;

    *)
        echo "Unknown command: $COMMAND"
        print_usage
        exit 1
        ;;
esac
