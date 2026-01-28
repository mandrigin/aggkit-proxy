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

# Miden node RPC endpoint
MIDEN_RPC_URL="${MIDEN_RPC_URL:-http://localhost:57291}"

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

BINARY="$PROJECT_ROOT/target/release/claim-note"

# Build if --rebuild flag or binary doesn't exist
maybe_build() {
    if [[ "$REBUILD" == "true" ]]; then
        echo "Rebuilding claim-note binary..."
        cargo build --release --bin claim-note --manifest-path "$PROJECT_ROOT/Cargo.toml" 2>&1 | grep -v "Compiling\|Downloading\|Downloaded" || true
    elif [[ ! -f "$BINARY" ]]; then
        echo "Binary not found, building..."
        cargo build --release --bin claim-note --manifest-path "$PROJECT_ROOT/Cargo.toml" 2>&1 | grep -v "Compiling\|Downloading\|Downloaded" || true
    fi
}

case "$COMMAND" in
    address|addr|whoami)
        maybe_build
        exec "$BINARY" derive-address
        ;;

    claim)
        if [[ $# -lt 1 ]]; then
            echo "Usage: $0 claim <note-id>"
            exit 1
        fi
        NOTE_ID="$1"

        maybe_build
        exec "$BINARY" claim "$NOTE_ID"
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
