#!/usr/bin/env bash
#
# list-unclaimed-notes.sh - List unclaimed P2ID notes from the Miden node
#
# Usage: TOPOLOGY=compose|kurtosis ./scripts/list-unclaimed-notes.sh [miden-node-container]
#
# Queries the miden-node's SQLite store for notes that have assets but have not
# been consumed (i.e., unclaimed P2ID notes from bridge deposits). Works against
# both local stacks via scripts/lib/topology.sh; pass a container override as $1.

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib/topology.sh"

# Resolve miden-node container (explicit $1 wins over the topology default)
NODE_CONTAINER="${1:-$MIDEN_NODE_CONTAINER}"
NODE_CID=$(topology_cid "$NODE_CONTAINER")
if [[ -z "$NODE_CID" ]]; then
    echo "Error: miden-node container '$NODE_CONTAINER' not found (TOPOLOGY=$TOPOLOGY)"
    exit 1
fi

DB_PATH=$(MIDEN_NODE_CONTAINER="$NODE_CONTAINER" topology_node_db)
if [[ -z "$DB_PATH" ]]; then
    echo "Error: could not locate miden-store.sqlite3 in $NODE_CONTAINER under $MIDEN_NODE_DATA_DIR"
    exit 1
fi

# Create temp dir for DB copy
TMPDIR=$(mktemp -d)
trap "rm -rf $TMPDIR" EXIT

# Copy SQLite DB with WAL (required for fresh data)
docker cp "$NODE_CID:$DB_PATH" "$TMPDIR/miden-store.sqlite3" 2>/dev/null
docker cp "$NODE_CID:${DB_PATH}-wal" "$TMPDIR/miden-store.sqlite3-wal" 2>/dev/null || true
docker cp "$NODE_CID:${DB_PATH}-shm" "$TMPDIR/miden-store.sqlite3-shm" 2>/dev/null || true

DB="$TMPDIR/miden-store.sqlite3"

total=$(sqlite3 "$DB" "SELECT count(*) FROM notes;")
unclaimed=$(sqlite3 "$DB" "SELECT count(*) FROM notes WHERE consumed_at IS NULL AND length(assets) > 1;")

echo "Miden Node: $NODE_CONTAINER"
echo "Total notes: $total"
echo "Unclaimed P2ID notes: $unclaimed"
echo ""

if [[ "$unclaimed" -eq 0 ]]; then
    echo "No unclaimed P2ID notes found."
    exit 0
fi

# Print header
printf "%-68s  %6s  %8s  %s\n" "Note ID" "Block" "Amount" "Recipient (inputs hex)"
printf "%-68s  %6s  %8s  %s\n" "$(printf '%0.s-' {1..66})" "------" "--------" "$(printf '%0.s-' {1..40})"

# Query unclaimed notes with assets
sqlite3 "$DB" "SELECT hex(note_id), committed_at, hex(assets), hex(inputs) FROM notes WHERE consumed_at IS NULL AND length(assets) > 1 ORDER BY committed_at;" | while IFS='|' read -r note_id block assets inputs; do
    # Decode amount from assets blob
    # Format: count(1 byte) + per asset: faucet_id(15 bytes) + amount(8 bytes LE)
    # Skip count (2 hex chars) + faucet_id (30 hex chars) = 32 hex chars, then 16 hex chars for amount
    amount_hex="${assets:32:16}"
    # Convert little-endian hex to integer
    amount=0
    if [[ -n "$amount_hex" ]]; then
        # Reverse byte order (LE to BE)
        be_hex=""
        for (( i=${#amount_hex}-2; i>=0; i-=2 )); do
            be_hex="${be_hex}${amount_hex:$i:2}"
        done
        amount=$((16#$be_hex))
    fi

    # Extract recipient from inputs
    # Format: count(1 byte) + felts (8 bytes each, LE)
    # Remove count byte (2 hex chars)
    recipient_hex="${inputs:2}"

    printf "0x%-66s  %6s  %8s  %s\n" "$note_id" "$block" "$amount" "$recipient_hex"
done

echo ""
echo "Assets are denominated in Miden bridge units (faucet: 0xE23C6282BAC7EC206A41E4AA36CC77)"
