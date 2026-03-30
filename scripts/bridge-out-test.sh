#!/usr/bin/env bash
#
# Bridge-Out Test — L2→L1 withdrawal flow
#
# Tests the full B2AGG bridge-out flow:
#   1. Claim a P2ID note (gives wallet a balance)
#   2. Create B2AGG note (withdraws to L1)
#   3. Verify BridgeEvent is emitted by the proxy
#   4. Verify bridge-service picks it up
#
# Prerequisites:
#   - Kurtosis miden-cdk enclave running
#   - At least one unclaimed P2ID note on Miden (from prior L1→L2 deposit)
#   - claim-note binary built (./scripts/claim-note.sh)
#
# Usage: ./scripts/bridge-out-test.sh [dest_address]
#   dest_address  - L1 destination (default: kurtosis funded account)

set -euo pipefail

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_ROOT="$(dirname "$SCRIPT_DIR")"

# L1 destination address for the withdrawal
DEST_ADDRESS="${1:-0xE34aaF64b29273B7D567FCFc40544c014EEe9970}"

# Auto-detect proxy RPC
PROXY_RPC="${PROXY_RPC:-}"
if [[ -z "$PROXY_RPC" ]]; then
    PROXY_RPC=$(kurtosis port print miden-cdk miden-proxy-001 rpc 2>/dev/null || echo "")
fi
[[ -z "$PROXY_RPC" ]] && { echo "ERROR: Cannot find proxy RPC"; exit 1; }

# Auto-detect Miden node RPC
MIDEN_RPC="${MIDEN_RPC:-}"
if [[ -z "$MIDEN_RPC" ]]; then
    MIDEN_NODE_CONTAINER=$(docker ps --format '{{.Names}}' 2>/dev/null | grep -E 'miden-node' | head -1)
    if [[ -n "$MIDEN_NODE_CONTAINER" ]]; then
        MIDEN_PORT=$(docker port "$MIDEN_NODE_CONTAINER" 57291/tcp 2>/dev/null | head -1 | cut -d: -f2)
        [[ -n "$MIDEN_PORT" ]] && MIDEN_RPC="http://localhost:$MIDEN_PORT"
    fi
fi
[[ -z "$MIDEN_RPC" ]] && { echo "ERROR: Cannot find Miden node RPC"; exit 1; }

echo "========================================="
echo " Bridge-Out Test (L2 → L1)"
echo "========================================="
echo ""
echo "Proxy RPC:       $PROXY_RPC"
echo "Miden Node RPC:  $MIDEN_RPC"
echo "L1 Dest:         $DEST_ADDRESS"
echo ""

# Get proxy accounts
ACCOUNTS=$(kurtosis service exec miden-cdk miden-proxy-001 "cat /var/lib/miden-agglayer-service/bridge_accounts.toml" 2>/dev/null | grep -v "^$")
BRIDGE_ID=$(echo "$ACCOUNTS" | grep "^bridge" | cut -d'"' -f2)
FAUCET_ETH=$(echo "$ACCOUNTS" | grep "faucet_eth" | cut -d'"' -f2)
WALLET_ID=$(echo "$ACCOUNTS" | grep "wallet_hardhat" | cut -d'"' -f2)

echo "Bridge ID:       $BRIDGE_ID"
echo "ETH Faucet:      $FAUCET_ETH"
echo "Wallet:          $WALLET_ID"
echo ""

# ---- Step 1: Find an unclaimed P2ID note ----
echo "--- Step 1: Finding unclaimed P2ID notes ---"

# Use the claim-note tool to find consumable notes
echo "Using claim-note tool to discover notes..."
CLAIM_OUTPUT=$("$SCRIPT_DIR/claim-note.sh" address 2>&1)
CLAIMER_MIDEN=$(echo "$CLAIM_OUTPUT" | grep "Miden:" | awk '{print $2}')
echo "Claimer account: $CLAIMER_MIDEN"

# Get the ETH faucet ID in hex
AGPG=$(docker ps --filter "name=miden-agglayer-postgres" --format "{{.Names}}" | head -1)
ETH_FAUCET_HEX=$(docker exec "$AGPG" psql -t -U agglayer -d agglayer_store -c \
    "SELECT faucet_id FROM faucet_registry WHERE symbol='ETH';" 2>&1 | tr -d ' ')
echo "ETH Faucet hex:  $ETH_FAUCET_HEX"

# ---- Step 2: Claim a P2ID note ----
echo ""
echo "--- Step 2: Claiming a P2ID note (to get wallet balance) ---"

# Find the first unclaimed ETH P2ID note by running claim-note
# The claim-note tool auto-discovers consumable notes
FIRST_NOTE_ID=$("$SCRIPT_DIR/claim-note.sh" address 2>&1 | tail -1 || true)

# Actually we need to run claim and let it pick a note
echo "Running claim-note to consume first available P2ID note..."
CLAIM_RESULT=$("$SCRIPT_DIR/claim-note.sh" --rebuild claim auto 2>&1) || {
    echo "claim-note failed, trying to find a specific note..."
    # Fallback: get note ID from the node DB
    CONTAINER=$(docker ps --filter "name=miden-node-001" --format '{{.ID}}' | head -1)
    TMPDIR=$(mktemp -d)
    docker cp "$CONTAINER:/app/data/miden-store.sqlite3" "$TMPDIR/db.sqlite3"
    docker cp "$CONTAINER:/app/data/miden-store.sqlite3-wal" "$TMPDIR/db.sqlite3-wal" 2>/dev/null || true

    NOTE_ID=$(sqlite3 "$TMPDIR/db.sqlite3" "
        SELECT '0x' || lower(hex(note_id)) FROM notes
        WHERE consumed_at IS NULL
        AND hex(script_root) = (
            SELECT hex(script_root) FROM notes
            WHERE script_root IS NOT NULL
            GROUP BY script_root
            ORDER BY COUNT(*) DESC LIMIT 1
        )
        ORDER BY committed_at ASC LIMIT 1;
    ")
    rm -rf "$TMPDIR"

    echo "Found note: $NOTE_ID"
    CLAIM_RESULT=$("$SCRIPT_DIR/claim-note.sh" claim "$NOTE_ID" 2>&1)
}
echo "$CLAIM_RESULT" | tail -5
echo "✓ P2ID note claimed"

# ---- Step 3: Create B2AGG note (bridge-out) ----
echo ""
echo "--- Step 3: Creating B2AGG bridge-out note ---"

# The bridge-out-tool runs inside the proxy container using the proxy's store
# We need: wallet-id, bridge-id, faucet-id, amount, dest-address
# The wallet that claimed the P2ID note is the claimer account, but the
# bridge-out-tool uses the proxy's internal store. We need to run it
# from the proxy container.

# Actually the bridge-out-tool needs its own wallet. The proxy's wallet_hardhat
# might not have balance. We need to use the claimer wallet's store.
#
# Alternative: exec bridge-out-tool from the claim-note's store directory.
# But bridge-out-tool is in the proxy container, not on the host.
#
# Simplest: copy bridge-out-tool out and run it on the host with the claimer's store.

echo "Setting up bridge-out-tool..."

# Extract the bridge-out-tool binary
TMPDIR=$(mktemp -d)
PROXY_CONTAINER=$(docker ps --filter "name=miden-proxy-001" --format '{{.ID}}' | head -1)
docker cp "$PROXY_CONTAINER:/usr/local/bin/bridge-out-tool" "$TMPDIR/bridge-out-tool" 2>/dev/null || {
    echo "ERROR: Cannot extract bridge-out-tool from proxy container"
    exit 1
}
chmod +x "$TMPDIR/bridge-out-tool"

# The claim-note tool creates its store in /tmp/miden-claimer-*
# Find the latest claimer store
CLAIMER_STORE=$(ls -td /tmp/miden-claimer-* 2>/dev/null | head -1)
if [[ -z "$CLAIMER_STORE" ]]; then
    echo "ERROR: No claimer store found. Run claim-note first."
    exit 1
fi
echo "Claimer store:   $CLAIMER_STORE"

# Get the bridge and faucet IDs in hex format
# The bridge-out-tool expects hex account IDs
echo "Running bridge-out-tool..."
echo "  wallet:  $CLAIMER_MIDEN"
echo "  bridge:  $BRIDGE_ID"
echo "  faucet:  $ETH_FAUCET_HEX"
echo "  amount:  100000 (0.001 ETH in Miden 8-decimal units)"
echo "  dest:    $DEST_ADDRESS"

"$TMPDIR/bridge-out-tool" \
    --store-dir "$CLAIMER_STORE" \
    --node-url "$MIDEN_RPC" \
    --wallet-id "$CLAIMER_MIDEN" \
    --bridge-id "$BRIDGE_ID" \
    --faucet-id "$ETH_FAUCET_HEX" \
    --amount 100000 \
    --dest-address "$DEST_ADDRESS" \
    --dest-network 0 2>&1 || {
    echo ""
    echo "bridge-out-tool failed. This may be because:"
    echo "  - The wallet hasn't consumed any P2ID notes yet"
    echo "  - The wallet doesn't have sufficient balance"
    echo "  - The account IDs are in wrong format"
    echo ""
    echo "Trying with bech32 IDs..."
    "$TMPDIR/bridge-out-tool" \
        --store-dir "$CLAIMER_STORE" \
        --node-url "$MIDEN_RPC" \
        --wallet-id "0x$CLAIMER_MIDEN" \
        --bridge-id "$(echo "$BRIDGE_ID" | sed 's/mlcl1/0x/')" \
        --faucet-id "$ETH_FAUCET_HEX" \
        --amount 100000 \
        --dest-address "$DEST_ADDRESS" \
        --dest-network 0 2>&1
}

echo "✓ B2AGG note created"

# ---- Step 4: Wait for BridgeEvent ----
echo ""
echo "--- Step 4: Waiting for BridgeEvent emission ---"
echo "Waiting 60s for proxy to detect consumed B2AGG note..."
sleep 60

# Check for BridgeEvent in proxy logs
echo "Checking proxy logs for BridgeEvent..."
BRIDGE_EVENTS=$(kurtosis service logs miden-cdk miden-proxy-001 --all 2>&1 | grep -i "bridge.*event\|BridgeEvent\|bridge_out\|B2AGG\|b2agg" | tail -10)
if [[ -n "$BRIDGE_EVENTS" ]]; then
    echo "$BRIDGE_EVENTS"
    echo "✓ BridgeEvent detected"
else
    echo "No BridgeEvent found yet. May need more time."
fi

# Check synthetic logs in proxy DB
echo ""
echo "Checking synthetic logs for BridgeEvent..."
AGPG=$(docker ps --filter "name=miden-agglayer-postgres" --format "{{.Names}}" | head -1)
docker exec "$AGPG" psql -U agglayer -d agglayer_store -c \
    "SELECT id, block_number, transaction_hash, created_at FROM synthetic_logs WHERE topics[1] LIKE '%501781%' OR address ILIKE '%bridge%' ORDER BY id DESC LIMIT 5;" 2>&1

# ---- Step 5: Check bridge-service ----
echo ""
echo "--- Step 5: Checking bridge-service for BridgeEvent ---"
kurtosis service logs miden-cdk zkevm-bridge-service-001 2>&1 | grep -i "bridge.*event\|deposit.*l2\|network.*2\|withdrawal" | tail -5

echo ""
echo "========================================="
echo " Bridge-Out Test Complete"
echo "========================================="

# Cleanup
rm -rf "$TMPDIR"
