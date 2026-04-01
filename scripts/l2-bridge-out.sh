#!/usr/bin/env bash
#
# l2-bridge-out.sh — Send a B2AGG bridge-out note (L2 -> L1)
#
# Usage: ./scripts/l2-bridge-out.sh [amount] [dest_address]
#
#   amount       - Miden token units (8-decimal). Default: 5000 (0.00005 ETH)
#   dest_address - L1 destination. Default: kurtosis funded account
#
# Prerequisites:
#   - Kurtosis miden-cdk running
#   - claim-note.sh used to claim at least one P2ID note (wallet has balance)
#   - MIDEN_STORE_PATH set to the claimer store (or auto-detected)
#
# Example:
#   MIDEN_STORE_PATH=/tmp/miden-claimer-e2e ./scripts/l2-bridge-out.sh 100000
#

set -euo pipefail

AMOUNT="${1:-5000}"
DEST_ADDRESS="${2:-0xE34aaF64b29273B7D567FCFc40544c014EEe9970}"

# Auto-detect store
STORE_PATH="${MIDEN_STORE_PATH:-}"
if [[ -z "$STORE_PATH" ]]; then
    STORE_PATH=$(ls -td /tmp/miden-claimer-* 2>/dev/null | head -1)
fi
if [[ -z "$STORE_PATH" || ! -f "$STORE_PATH/store.sqlite3" ]]; then
    echo "ERROR: No claimer store found."
    echo "Run: MIDEN_STORE_PATH=/tmp/my-store ./scripts/claim-note.sh claim <note-id>"
    echo "Then: MIDEN_STORE_PATH=/tmp/my-store ./scripts/l2-bridge-out.sh $AMOUNT"
    exit 1
fi

# Get proxy container + config
PROXY_CONTAINER=$(docker ps --filter "name=miden-proxy-001" --format '{{.ID}}' | head -1)
[[ -z "$PROXY_CONTAINER" ]] && { echo "ERROR: miden-proxy-001 not running"; exit 1; }

BRIDGE_ID=$(kurtosis service exec miden-cdk miden-proxy-001 \
    "cat /var/lib/miden-agglayer-service/bridge_accounts.toml" 2>/dev/null \
    | grep "^bridge" | cut -d'"' -f2)

AGPG=$(docker ps --filter "name=miden-agglayer-postgres" --format "{{.Names}}" | head -1)
ETH_FAUCET=$(docker exec "$AGPG" psql -t -A -U agglayer -d agglayer_store -c \
    "SELECT faucet_id FROM faucet_registry WHERE symbol='ETH';")

echo "=== L2 -> L1 Bridge-Out ==="
echo "Amount:      $AMOUNT (Miden 8-dec units)"
echo "Destination: $DEST_ADDRESS"
echo "Store:       $STORE_PATH"
echo "Bridge:      $BRIDGE_ID"
echo "ETH Faucet:  $ETH_FAUCET"
echo ""

# Clean input_notes to avoid re-consume attempts
python3 -c "
import sqlite3
conn = sqlite3.connect('$STORE_PATH/store.sqlite3')
conn.execute('DELETE FROM input_notes')
conn.commit()
conn.close()
"

# Copy store to proxy container
docker cp "$STORE_PATH/." "$PROXY_CONTAINER:/tmp/claimer-store/" 2>&1 > /dev/null

echo "Submitting B2AGG note..."
RESULT=$(docker exec "$PROXY_CONTAINER" bridge-out-tool \
    --store-dir /tmp/claimer-store \
    --node-url http://miden-node-001:57291 \
    --wallet-id 0xa417929a101b89100dda63bf4f6928 \
    --bridge-id "$BRIDGE_ID" \
    --faucet-id "$ETH_FAUCET" \
    --amount "$AMOUNT" \
    --dest-address "$DEST_ADDRESS" \
    --dest-network 0 2>&1)

echo "$RESULT"

if echo "$RESULT" | grep -q "done"; then
    # Save updated store back
    docker cp "$PROXY_CONTAINER:/tmp/claimer-store/store.sqlite3" "$STORE_PATH/store.sqlite3"
    echo ""
    echo "Bridge-out committed."
    echo ""
    echo "Waiting 15s for NTX builder to consume the B2AGG note..."
    echo "(This prevents the next bridge-out from hitting a poisoned block)"
    sleep 15
    echo "Ready for next bridge-out."
    echo ""
    echo "Monitor with:"
    echo "  ./scripts/check-bridge-events.sh"
    echo "  ./scripts/check-certificates.sh"
    echo "  ./scripts/check-l2-deposits.sh"
else
    echo ""
    echo "Bridge-out FAILED. Common causes:"
    echo "  - 'not committed': NTX builder poisoned the block, retry in 15s"
    echo "  - 'storage error': stale store, retry (sync catches up)"
    echo "  - 'RPC error': stale commitment, retry after a few seconds"
    echo "  - 'insufficient balance': claim more P2ID notes first"
fi
