#!/usr/bin/env bash
#
# check-l2-deposits.sh — Check bridge DB for L2->L1 deposits and their claim status
#
# Usage: ./scripts/check-l2-deposits.sh [--all]
#
# Shows deposits with dest_net=0 (L2->L1 withdrawals) from the bridge-service DB.
# Also shows L1->L2 summary for context.
#
# Options:
#   --all   Also show all L1->L2 deposits

set -euo pipefail

SHOW_ALL=false
[[ "${1:-}" == "--all" ]] && SHOW_ALL=true

PG=$(docker ps --filter 'name=postgres-001' --format '{{.Names}}' | grep -v agglayer | head -1)
[[ -z "$PG" ]] && { echo "ERROR: postgres-001 not found"; exit 1; }

echo "=== Bridge DB: L2 -> L1 Deposits (dest_net=0) ==="
echo ""
docker exec "$PG" psql -U master_user -d bridge_db -c \
    "SELECT deposit_cnt, dest_net, ready_for_claim, amount, network_id
     FROM sync.deposit WHERE dest_net = 0 ORDER BY deposit_cnt;" 2>&1

L2_TOTAL=$(docker exec "$PG" psql -t -A -U master_user -d bridge_db -c \
    "SELECT COUNT(*) FROM sync.deposit WHERE dest_net = 0;")
L2_READY=$(docker exec "$PG" psql -t -A -U master_user -d bridge_db -c \
    "SELECT COUNT(*) FROM sync.deposit WHERE dest_net = 0 AND ready_for_claim;")

echo ""
echo "=== Bridge DB: L1 -> L2 Summary ==="
L1_TOTAL=$(docker exec "$PG" psql -t -A -U master_user -d bridge_db -c \
    "SELECT COUNT(*) FROM sync.deposit WHERE dest_net = 1;")
L1_READY=$(docker exec "$PG" psql -t -A -U master_user -d bridge_db -c \
    "SELECT COUNT(*) FROM sync.deposit WHERE dest_net = 1 AND ready_for_claim;")
echo "L1->L2 deposits: $L1_READY/$L1_TOTAL ready_for_claim"

if $SHOW_ALL; then
    echo ""
    docker exec "$PG" psql -U master_user -d bridge_db -c \
        "SELECT deposit_cnt, dest_net, ready_for_claim, amount
         FROM sync.deposit WHERE dest_net = 1 ORDER BY deposit_cnt;" 2>&1
fi

echo ""
echo "=== Cross-Check ==="
echo "L2->L1: $L2_READY/$L2_TOTAL ready_for_claim"
echo "L1->L2: $L1_READY/$L1_TOTAL ready_for_claim"

# Check proxy BridgeEvents
PROXY_RPC="${PROXY_RPC:-}"
if [[ -z "$PROXY_RPC" ]]; then
    PROXY_RPC=$(kurtosis port print miden-cdk miden-proxy-001 rpc 2>/dev/null || echo "")
fi
if [[ -n "$PROXY_RPC" ]]; then
    BRIDGE_EVENTS=$(curl -s "$PROXY_RPC" -X POST -H 'Content-Type: application/json' \
        -d '{"jsonrpc":"2.0","method":"eth_getLogs","params":[{"fromBlock":"0x0","toBlock":"latest","topics":["0x501781209a1f8899323b96b4ef08b168df93e0a90c673d1e4cce39366cb62f9b"]}],"id":1}' \
        | jq '.result | length')
    echo "Proxy BridgeEvents:   $BRIDGE_EVENTS"
fi

echo ""
if [[ "$L2_TOTAL" = "$L2_READY" ]] && [[ "$L2_TOTAL" -gt 0 ]]; then
    echo "All L2->L1 deposits are ready for claim on L1."
else
    NOT_READY=$((L2_TOTAL - L2_READY))
    echo "$NOT_READY L2->L1 deposit(s) not yet ready_for_claim."
    echo "This may be because:"
    echo "  - B2AGG note not yet consumed by bridge account (~30s)"
    echo "  - BridgeEvent not yet synced by bridge-service"
    echo "  - Certificate not yet settled on AggLayer"
fi
