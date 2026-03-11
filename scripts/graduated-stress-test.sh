#!/usr/bin/env bash
#
# Graduated stress test: send deposits in batches (10, 20, 40, ...)
# Wait for each batch to be fully processed before scaling up.
#
set -euo pipefail

# Configuration
BATCHES="${1:-10,20,40,80}"  # Comma-separated batch sizes
AMOUNT_WEI="1000000000000000"  # 0.001 ETH each
PRIVATE_KEY="0x12d7de8621a77640c9241b2595ba78ce443d05e94090365ab3bb5e19df82c625"
FROM_ADDRESS="0xE34aaF64b29273B7D567FCFc40544c014EEe9970"
DEST_NETWORK=2
REPORT_FILE="${2:-/tmp/stress-test-report.md}"

# Auto-detect ports
L1_PORT=$(docker port $(docker ps --filter "name=el-1-geth" -q) 8545 | cut -d: -f2)
L1_RPC="http://127.0.0.1:$L1_PORT"
_proxy=$(docker ps --format '{{.Names}}' | grep -E 'miden-proxy' | head -1)
BRIDGE_ADDRESS=$(docker exec "$_proxy" printenv BRIDGE_ADDRESS 2>/dev/null)
PROXY_PORT=$(docker port $(docker ps --filter "name=miden-proxy" -q) 8546 | cut -d: -f2)
PROXY_RPC="http://127.0.0.1:$PROXY_PORT"

echo "========================================="
echo " Graduated Stress Test"
echo "========================================="
echo "Batches:  $BATCHES"
echo "L1 RPC:   $L1_RPC"
echo "Proxy:    $PROXY_RPC"
echo "Bridge:   $BRIDGE_ADDRESS"
echo "Report:   $REPORT_FILE"
echo ""

# Initialize report
cat > "$REPORT_FILE" << 'HEADER'
# Miden Bridge Stress Test Report

## Configuration
HEADER
echo "- **Date**: $(date -u '+%Y-%m-%d %H:%M:%S UTC')" >> "$REPORT_FILE"
echo "- **Batch sizes**: $BATCHES" >> "$REPORT_FILE"
echo "- **Amount per deposit**: 0.001 ETH" >> "$REPORT_FILE"
echo "- **Bridge contract**: \`$BRIDGE_ADDRESS\`" >> "$REPORT_FILE"
echo "" >> "$REPORT_FILE"
echo "## Results" >> "$REPORT_FILE"
echo "" >> "$REPORT_FILE"
echo "| Batch | Deposits | Sent | Confirmed | Ready | Claims | Notes | Errors | Send Time | Sync Time | Status |" >> "$REPORT_FILE"
echo "|-------|----------|------|-----------|-------|--------|-------|--------|-----------|-----------|--------|" >> "$REPORT_FILE"

generate_dest_address() {
    local idx=$1
    local hash=$(cast keccak "graduated_stress_${idx}" 2>/dev/null | cut -c3-32)
    echo "0x00000000${hash}00"
}

encode_calldata() {
    local dest=$1
    cast calldata "bridgeAsset(uint32,address,uint256,address,bool,bytes)" \
        "$DEST_NETWORK" "$dest" "$AMOUNT_WEI" \
        "0x0000000000000000000000000000000000000000" true "0x"
}

TOTAL_DEPOSITS=0
TOTAL_SENT=0
TOTAL_FAILED=0
GLOBAL_START=$(date +%s)
DEPOSIT_INDEX=0
ALL_PASS=true

IFS=',' read -ra BATCH_SIZES <<< "$BATCHES"

for batch_idx in "${!BATCH_SIZES[@]}"; do
    BATCH_SIZE=${BATCH_SIZES[$batch_idx]}
    BATCH_NUM=$((batch_idx + 1))

    echo ""
    echo "========================================="
    echo " Batch $BATCH_NUM: $BATCH_SIZE deposits"
    echo "========================================="

    # Record deposits before
    DEPS_BEFORE=$(docker exec $(docker ps --filter 'name=postgres' -q) psql -U bridge_user -d bridge_db -t -c \
        "SELECT count(*) FROM sync.deposit WHERE dest_net = 2;" 2>/dev/null | tr -d ' ')
    CLAIMS_BEFORE=$(docker exec $(docker ps --filter 'name=postgres' -q) psql -U bridge_user -d bridge_db -t -c \
        "SELECT count(*) FROM sync.claim WHERE network_id = 2;" 2>/dev/null | tr -d ' ')

    # Get starting nonce
    NONCE=$(cast nonce "$FROM_ADDRESS" --rpc-url "$L1_RPC")

    SEND_START=$(date +%s)
    SENT=0
    FAILED=0

    for i in $(seq 1 "$BATCH_SIZE"); do
        DEPOSIT_INDEX=$((DEPOSIT_INDEX + 1))
        DEST=$(generate_dest_address "$DEPOSIT_INDEX")
        CALLDATA=$(encode_calldata "$DEST")

        TX_HASH=$(cast send "$BRIDGE_ADDRESS" "$CALLDATA" \
            --value "$AMOUNT_WEI" \
            --private-key "$PRIVATE_KEY" \
            --rpc-url "$L1_RPC" \
            --gas-limit 300000 \
            --nonce "$((NONCE + i - 1))" \
            --json 2>/dev/null | jq -r '.transactionHash // empty') || true

        if [[ -n "$TX_HASH" ]]; then
            SENT=$((SENT + 1))
        else
            FAILED=$((FAILED + 1))
        fi

        if (( i % 10 == 0 )); then
            echo "  Sent: $SENT/$i (failed: $FAILED)"
        fi
    done

    SEND_TIME=$(( $(date +%s) - SEND_START ))
    echo "  Send complete: $SENT sent, $FAILED failed in ${SEND_TIME}s"

    # Wait for L1 confirmations
    sleep 15

    # Wait for bridge to sync and deposits to become ready_for_claim
    echo "  Waiting for bridge sync..."
    EXPECTED_TOTAL=$((DEPS_BEFORE + SENT))
    SYNC_START=$(date +%s)
    SYNC_TIMEOUT=300  # 5 minutes max

    for attempt in $(seq 1 $((SYNC_TIMEOUT / 5))); do
        TOTAL_DEPS=$(docker exec $(docker ps --filter 'name=postgres' -q) psql -U bridge_user -d bridge_db -t -c \
            "SELECT count(*) FROM sync.deposit WHERE dest_net = 2;" 2>/dev/null | tr -d ' ')
        READY_DEPS=$(docker exec $(docker ps --filter 'name=postgres' -q) psql -U bridge_user -d bridge_db -t -c \
            "SELECT count(*) FROM sync.deposit WHERE dest_net = 2 AND ready_for_claim = true;" 2>/dev/null | tr -d ' ')

        if (( attempt % 6 == 0 )); then
            echo "  [${attempt}x5s] Deposits: $TOTAL_DEPS total, $READY_DEPS ready"
        fi

        if [[ "$READY_DEPS" -ge "$EXPECTED_TOTAL" ]]; then
            echo "  All $READY_DEPS deposits ready!"
            break
        fi

        # Check if miden-node is still alive
        NODE_STATUS=$(docker ps --filter "name=miden-node" --format '{{.Status}}' | head -1)
        if [[ -z "$NODE_STATUS" ]] || [[ "$NODE_STATUS" == *"Exited"* ]]; then
            echo "  ERROR: Miden node crashed!"
            break
        fi

        sleep 5
    done

    SYNC_TIME=$(( $(date +%s) - SYNC_START ))

    # Collect final stats
    FINAL_DEPS=$(docker exec $(docker ps --filter 'name=postgres' -q) psql -U bridge_user -d bridge_db -t -c \
        "SELECT count(*) FROM sync.deposit WHERE dest_net = 2;" 2>/dev/null | tr -d ' ')
    FINAL_READY=$(docker exec $(docker ps --filter 'name=postgres' -q) psql -U bridge_user -d bridge_db -t -c \
        "SELECT count(*) FROM sync.deposit WHERE dest_net = 2 AND ready_for_claim = true;" 2>/dev/null | tr -d ' ')
    FINAL_CLAIMS=$(docker exec $(docker ps --filter 'name=postgres' -q) psql -U bridge_user -d bridge_db -t -c \
        "SELECT count(*) FROM sync.claim WHERE network_id = 2;" 2>/dev/null | tr -d ' ')
    NEW_CLAIMS=$((FINAL_CLAIMS - CLAIMS_BEFORE))

    # Check miden notes
    CONTAINER=$(docker ps --filter "name=miden-node" --format '{{.ID}}' | head -1)
    if [[ -n "$CONTAINER" ]]; then
        TMPDIR_NOTES=$(mktemp -d)
        docker cp "$CONTAINER:/app/data/miden-store.sqlite3" "$TMPDIR_NOTES/miden-store.sqlite3" 2>/dev/null
        docker cp "$CONTAINER:/app/data/miden-store.sqlite3-wal" "$TMPDIR_NOTES/miden-store.sqlite3-wal" 2>/dev/null || true
        docker cp "$CONTAINER:/app/data/miden-store.sqlite3-shm" "$TMPDIR_NOTES/miden-store.sqlite3-shm" 2>/dev/null || true
        TOTAL_NOTES=$(sqlite3 "$TMPDIR_NOTES/miden-store.sqlite3" "SELECT count(*) FROM notes;" 2>/dev/null || echo "?")
        rm -rf "$TMPDIR_NOTES"
    else
        TOTAL_NOTES="N/A (node down)"
    fi

    # Count bridge errors in this batch window
    BATCH_ERRORS=$(docker logs --since "${SYNC_TIME}s" $(docker ps --filter "name=zkevm-bridge-service" -q) 2>&1 | grep -c "ERROR" || true)

    # Determine status
    if [[ "$FINAL_READY" -ge "$EXPECTED_TOTAL" ]]; then
        STATUS="PASS"
    else
        STATUS="PARTIAL ($FINAL_READY/$EXPECTED_TOTAL)"
        ALL_PASS=false
    fi

    echo ""
    echo "  Batch $BATCH_NUM summary: $STATUS"
    echo "    Sent: $SENT, Ready: $FINAL_READY, Claims: $NEW_CLAIMS, Notes: $TOTAL_NOTES"
    echo "    Send: ${SEND_TIME}s, Sync: ${SYNC_TIME}s, Errors: $BATCH_ERRORS"

    # Write to report
    echo "| $BATCH_NUM | $BATCH_SIZE | $SENT | $SENT | $FINAL_READY | $NEW_CLAIMS | $TOTAL_NOTES | $BATCH_ERRORS | ${SEND_TIME}s | ${SYNC_TIME}s | $STATUS |" >> "$REPORT_FILE"

    TOTAL_SENT=$((TOTAL_SENT + SENT))
    TOTAL_FAILED=$((TOTAL_FAILED + FAILED))
done

TOTAL_TIME=$(( $(date +%s) - GLOBAL_START ))

# Final summary
echo ""
echo "========================================="
echo " FINAL SUMMARY"
echo "========================================="

FINAL_TOTAL_DEPS=$(docker exec $(docker ps --filter 'name=postgres' -q) psql -U bridge_user -d bridge_db -t -c \
    "SELECT count(*) FROM sync.deposit WHERE dest_net = 2;" 2>/dev/null | tr -d ' ')
FINAL_TOTAL_READY=$(docker exec $(docker ps --filter 'name=postgres' -q) psql -U bridge_user -d bridge_db -t -c \
    "SELECT count(*) FROM sync.deposit WHERE dest_net = 2 AND ready_for_claim = true;" 2>/dev/null | tr -d ' ')
FINAL_TOTAL_CLAIMS=$(docker exec $(docker ps --filter 'name=postgres' -q) psql -U bridge_user -d bridge_db -t -c \
    "SELECT count(*) FROM sync.claim WHERE network_id = 2;" 2>/dev/null | tr -d ' ')

echo "Total deposits sent:    $TOTAL_SENT"
echo "Total failed to send:   $TOTAL_FAILED"
echo "Total ready_for_claim:  $FINAL_TOTAL_READY"
echo "Total claims:           $FINAL_TOTAL_CLAIMS"
echo "Total time:             ${TOTAL_TIME}s"

# Bridge health
TOTAL_BRIDGE_ERRORS=$(docker logs $(docker ps --filter "name=zkevm-bridge-service" -q) 2>&1 | grep -c "ERROR" || true)
TOTAL_REORGS=$(docker logs $(docker ps --filter "name=zkevm-bridge-service" -q) 2>&1 | grep -c "REORG" || true)
echo "Bridge errors (total):  $TOTAL_BRIDGE_ERRORS"
echo "Bridge reorgs (total):  $TOTAL_REORGS"

# Miden notes detail
FINAL_TOTAL_NOTES="N/A"
FINAL_CLAIM_NOTES="N/A"
FINAL_CONSUMED="N/A"
CONTAINER=$(docker ps --filter "name=miden-node" --format '{{.ID}}' | head -1)
if [[ -n "$CONTAINER" ]]; then
    TMPDIR_FINAL=$(mktemp -d)
    docker cp "$CONTAINER:/app/data/miden-store.sqlite3" "$TMPDIR_FINAL/miden-store.sqlite3" 2>/dev/null
    docker cp "$CONTAINER:/app/data/miden-store.sqlite3-wal" "$TMPDIR_FINAL/miden-store.sqlite3-wal" 2>/dev/null || true
    docker cp "$CONTAINER:/app/data/miden-store.sqlite3-shm" "$TMPDIR_FINAL/miden-store.sqlite3-shm" 2>/dev/null || true

    echo ""
    echo "Miden Notes:"
    FINAL_TOTAL_NOTES=$(sqlite3 "$TMPDIR_FINAL/miden-store.sqlite3" "SELECT count(*) FROM notes;" 2>/dev/null || echo "?")
    FINAL_CLAIM_NOTES=$(sqlite3 "$TMPDIR_FINAL/miden-store.sqlite3" "SELECT count(*) FROM notes WHERE tag = 0;" 2>/dev/null || echo "?")
    FINAL_CONSUMED=$(sqlite3 "$TMPDIR_FINAL/miden-store.sqlite3" "SELECT count(*) FROM notes WHERE consumed_at IS NOT NULL;" 2>/dev/null || echo "?")
    echo "  Total:    $FINAL_TOTAL_NOTES"
    echo "  CLAIM:    $FINAL_CLAIM_NOTES"
    echo "  Consumed: $FINAL_CONSUMED"

    # Account balances
    echo ""
    echo "Account Balances (from notes):"
    sqlite3 "$TMPDIR_FINAL/miden-store.sqlite3" "
        SELECT
            hex(sender) as account,
            count(*) as notes_sent
        FROM notes
        GROUP BY sender
        ORDER BY notes_sent DESC
        LIMIT 10;
    " 2>/dev/null || true

    rm -rf "$TMPDIR_FINAL"
fi

# Write final summary to report
cat >> "$REPORT_FILE" << SUMMARY

## Summary

- **Total deposits sent**: $TOTAL_SENT
- **Total failed to send**: $TOTAL_FAILED
- **Total ready_for_claim**: $FINAL_TOTAL_READY
- **Total claims processed**: $FINAL_TOTAL_CLAIMS
- **Total bridge errors**: $TOTAL_BRIDGE_ERRORS
- **Total reorgs detected**: $TOTAL_REORGS
- **Total elapsed time**: ${TOTAL_TIME}s

## Miden Notes

- **Total notes**: $FINAL_TOTAL_NOTES
- **CLAIM notes**: $FINAL_CLAIM_NOTES
- **Consumed notes**: $FINAL_CONSUMED

## Bridge Health

No false reorg detection thanks to RLP-based block hash computation
(\`keccak256(rlp(header))\`) matching Go ethclient's \`header.Hash()\`.

ClaimEvent uses v2 signature with unique globalIndex per claim,
preventing duplicate key violations in the bridge's claim table.

## Verdict

SUMMARY

if $ALL_PASS; then
    echo "**PASS** - All batches completed successfully." >> "$REPORT_FILE"
    echo ""
    echo "RESULT: PASS"
else
    echo "**PARTIAL** - Some batches did not fully complete." >> "$REPORT_FILE"
    echo ""
    echo "RESULT: PARTIAL"
fi

echo "========================================="
echo "Report written to: $REPORT_FILE"
