#!/usr/bin/env bash
#
# Stress test: send N deposits and track the full pipeline
#
set -euo pipefail

# Configuration
NUM_DEPOSITS="${1:-150}"
AMOUNT_WEI="1000000000000000"  # 0.001 ETH each
PRIVATE_KEY="0x12d7de8621a77640c9241b2595ba78ce443d05e94090365ab3bb5e19df82c625"
FROM_ADDRESS="0xE34aaF64b29273B7D567FCFc40544c014EEe9970"
DEST_NETWORK=2

# Auto-detect ports
L1_PORT=$(docker port $(docker ps --filter "name=el-1-geth" -q) 8545 | cut -d: -f2)
L1_RPC="http://127.0.0.1:$L1_PORT"
_proxy=$(docker ps --format '{{.Names}}' | grep -E 'miden-proxy' | head -1)
BRIDGE_ADDRESS=$(docker exec "$_proxy" printenv BRIDGE_ADDRESS 2>/dev/null)

echo "========================================="
echo " Stress Test: $NUM_DEPOSITS deposits"
echo "========================================="
echo "L1 RPC:   $L1_RPC"
echo "Bridge:   $BRIDGE_ADDRESS"
echo "Amount:   0.001 ETH per deposit"
echo ""

# Generate unique destination addresses (deterministic from index)
# Each address: 0x00000000 + keccak256("stress_test_<i>")[0:16] + 00
generate_dest_address() {
    local idx=$1
    # Use cast to compute keccak, take first 30 hex chars for the Miden part
    local hash=$(cast keccak "stress_test_deposit_${idx}" 2>/dev/null | cut -c3-32)
    echo "0x00000000${hash}00"
}

# Encode bridgeAsset calldata
encode_calldata() {
    local dest=$1
    cast calldata "bridgeAsset(uint32,address,uint256,address,bool,bytes)" \
        "$DEST_NETWORK" "$dest" "$AMOUNT_WEI" \
        "0x0000000000000000000000000000000000000000" true "0x"
}

# ============================================================================
# Phase 1: Send deposits
# ============================================================================
echo "=== Phase 1: Sending $NUM_DEPOSITS deposits ==="
START_TIME=$(date +%s)

# Get starting nonce
NONCE=$(cast nonce "$FROM_ADDRESS" --rpc-url "$L1_RPC")
echo "Starting nonce: $NONCE"

SENT=0
FAILED=0
TX_HASHES=()

for i in $(seq 1 "$NUM_DEPOSITS"); do
    DEST=$(generate_dest_address "$i")
    CALLDATA=$(encode_calldata "$DEST")

    # Send with explicit nonce for parallelism
    TX_HASH=$(cast send "$BRIDGE_ADDRESS" "$CALLDATA" \
        --value "$AMOUNT_WEI" \
        --private-key "$PRIVATE_KEY" \
        --rpc-url "$L1_RPC" \
        --gas-limit 300000 \
        --nonce "$((NONCE + i - 1))" \
        --json 2>/dev/null | jq -r '.transactionHash // empty') || true

    if [[ -n "$TX_HASH" ]]; then
        TX_HASHES+=("$TX_HASH")
        SENT=$((SENT + 1))
    else
        FAILED=$((FAILED + 1))
    fi

    # Progress every 10
    if (( i % 10 == 0 )); then
        echo "  Sent: $SENT/$i (failed: $FAILED)"
    fi
done

SEND_TIME=$(( $(date +%s) - START_TIME ))
echo ""
echo "Phase 1 complete: $SENT sent, $FAILED failed in ${SEND_TIME}s"
echo ""

# ============================================================================
# Phase 2: Wait for L1 confirmations
# ============================================================================
echo "=== Phase 2: Waiting for L1 confirmations ==="
sleep 15  # Wait for blocks to be mined

CONFIRMED=0
for tx in "${TX_HASHES[@]}"; do
    STATUS=$(cast receipt "$tx" --rpc-url "$L1_RPC" --json 2>/dev/null | jq -r '.status // "0x0"') || true
    if [[ "$STATUS" == "0x1" ]]; then
        CONFIRMED=$((CONFIRMED + 1))
    fi
done
echo "Confirmed: $CONFIRMED/$SENT"
echo ""

# ============================================================================
# Phase 3: Wait for bridge to sync deposits
# ============================================================================
echo "=== Phase 3: Waiting for bridge to sync deposits ==="

# We need to wait for:
# 1. L1 sync to pick up all deposits
# 2. GER injection for each deposit batch
# 3. L2 sync to see GER events
# 4. Deposits marked ready_for_claim

EXPECTED=$((SENT + 2))  # +2 for the deposits already in the system

for attempt in $(seq 1 60); do
    TOTAL_DEPS=$(docker exec $(docker ps --filter 'name=postgres' -q) psql -U bridge_user -d bridge_db -t -c \
        "SELECT count(*) FROM sync.deposit WHERE dest_net = 2;" 2>/dev/null | tr -d ' ')
    READY_DEPS=$(docker exec $(docker ps --filter 'name=postgres' -q) psql -U bridge_user -d bridge_db -t -c \
        "SELECT count(*) FROM sync.deposit WHERE dest_net = 2 AND ready_for_claim = true;" 2>/dev/null | tr -d ' ')
    NOT_READY=$((TOTAL_DEPS - READY_DEPS))

    echo "  [${attempt}0s] Deposits: $TOTAL_DEPS total, $READY_DEPS ready, $NOT_READY pending"

    if [[ "$READY_DEPS" -ge "$EXPECTED" ]]; then
        echo "✓ All deposits ready!"
        break
    fi

    # Check for bridge errors
    ERRORS=$(docker logs --since 10s $(docker ps --filter "name=zkevm-bridge-service" -q) 2>&1 | grep -c "ERROR" || true)
    if [[ "$ERRORS" -gt 0 ]]; then
        echo "    (bridge errors: $ERRORS in last 10s)"
    fi

    sleep 10
done

BRIDGE_TIME=$(( $(date +%s) - START_TIME - SEND_TIME ))
echo ""
echo "Phase 3 complete: bridge sync took ~${BRIDGE_TIME}s"
echo ""

# ============================================================================
# Phase 4: Check notes on Miden node
# ============================================================================
echo "=== Phase 4: Checking Miden notes ==="

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT
CONTAINER=$(docker ps --filter "name=miden-node" --format '{{.ID}}' | head -1)
docker cp "$CONTAINER:/app/data/miden-store.sqlite3" "$TMPDIR/miden-store.sqlite3"
docker cp "$CONTAINER:/app/data/miden-store.sqlite3-wal" "$TMPDIR/miden-store.sqlite3-wal" 2>/dev/null || true
docker cp "$CONTAINER:/app/data/miden-store.sqlite3-shm" "$TMPDIR/miden-store.sqlite3-shm" 2>/dev/null || true

TOTAL_NOTES=$(sqlite3 "$TMPDIR/miden-store.sqlite3" "SELECT count(*) FROM notes;")
CONSUMED_NOTES=$(sqlite3 "$TMPDIR/miden-store.sqlite3" "SELECT count(*) FROM notes WHERE consumed_at IS NOT NULL;")
UNCONSUMED_NOTES=$(sqlite3 "$TMPDIR/miden-store.sqlite3" "SELECT count(*) FROM notes WHERE consumed_at IS NULL;")
CLAIM_NOTES=$(sqlite3 "$TMPDIR/miden-store.sqlite3" "SELECT count(*) FROM notes WHERE tag = 0;")
P2ID_NOTES=$(sqlite3 "$TMPDIR/miden-store.sqlite3" "SELECT count(*) FROM notes WHERE tag != 0;")

echo "Total notes:     $TOTAL_NOTES"
echo "Consumed:        $CONSUMED_NOTES"
echo "Unconsumed:      $UNCONSUMED_NOTES"
echo "CLAIM notes:     $CLAIM_NOTES"
echo "P2ID notes:      $P2ID_NOTES"
echo ""

# Count unique senders (bridge vs faucet)
BRIDGE_SENT=$(sqlite3 "$TMPDIR/miden-store.sqlite3" "
    SELECT count(*) FROM notes WHERE hex(sender) LIKE '21DEED%' OR hex(sender) LIKE '67AE2C%';
")
FAUCET_SENT=$(sqlite3 "$TMPDIR/miden-store.sqlite3" "
    SELECT count(*) FROM notes WHERE hex(sender) NOT LIKE '21DEED%' AND hex(sender) NOT LIKE '67AE2C%';
")
echo "Notes from bridge account: $BRIDGE_SENT"
echo "Notes from faucet:         $FAUCET_SENT"
echo ""

# ============================================================================
# Phase 5: Summary Report
# ============================================================================
TOTAL_TIME=$(( $(date +%s) - START_TIME ))

echo "========================================="
echo " STRESS TEST REPORT"
echo "========================================="
echo ""
echo "Configuration:"
echo "  Deposits requested:   $NUM_DEPOSITS"
echo "  Amount per deposit:   0.001 ETH"
echo "  Total amount:         $(echo "scale=3; $NUM_DEPOSITS * 0.001" | bc) ETH"
echo ""
echo "L1 Transactions:"
echo "  Sent:                 $SENT"
echo "  Failed to send:       $FAILED"
echo "  Confirmed:            $CONFIRMED"
echo "  Send time:            ${SEND_TIME}s"
echo ""
echo "Bridge Sync:"
FINAL_TOTAL=$(docker exec $(docker ps --filter 'name=postgres' -q) psql -U bridge_user -d bridge_db -t -c \
    "SELECT count(*) FROM sync.deposit WHERE dest_net = 2;" 2>/dev/null | tr -d ' ')
FINAL_READY=$(docker exec $(docker ps --filter 'name=postgres' -q) psql -U bridge_user -d bridge_db -t -c \
    "SELECT count(*) FROM sync.deposit WHERE dest_net = 2 AND ready_for_claim = true;" 2>/dev/null | tr -d ' ')
echo "  Deposits detected:    $FINAL_TOTAL"
echo "  Ready for claim:      $FINAL_READY"
echo "  Not ready:            $((FINAL_TOTAL - FINAL_READY))"
echo "  Sync time:            ~${BRIDGE_TIME}s"
echo ""
echo "Miden Notes:"
echo "  Total notes:          $TOTAL_NOTES"
echo "  CLAIM notes:          $CLAIM_NOTES"
echo "  P2ID notes:           $P2ID_NOTES"
echo "  Consumed:             $CONSUMED_NOTES"
echo "  Unconsumed:           $UNCONSUMED_NOTES"
echo ""
echo "Pipeline Health:"
BRIDGE_ERRORS=$(docker logs $(docker ps --filter "name=zkevm-bridge-service" -q) 2>&1 | grep -c "ERROR" || true)
BRIDGE_REORGS=$(docker logs $(docker ps --filter "name=zkevm-bridge-service" -q) 2>&1 | grep -c "REORG" || true)
PROXY_ERRORS=$(docker logs $(docker ps --filter "name=miden-proxy" -q) 2>&1 | grep -c "ERROR" || true)
echo "  Bridge errors:        $BRIDGE_ERRORS"
echo "  Bridge reorgs:        $BRIDGE_REORGS"
echo "  Proxy errors:         $PROXY_ERRORS"
echo ""
echo "Timing:"
echo "  Total elapsed:        ${TOTAL_TIME}s"
echo ""

# Verdict
if [[ "$CONFIRMED" -eq "$SENT" ]] && [[ "$FINAL_READY" -ge "$EXPECTED" ]]; then
    echo "RESULT: ✓ PASS"
elif [[ "$CONFIRMED" -eq "$SENT" ]]; then
    echo "RESULT: ⚠ PARTIAL — deposits confirmed but not all ready_for_claim yet"
else
    echo "RESULT: ✗ FAIL — some deposits not confirmed"
fi
echo "========================================="
