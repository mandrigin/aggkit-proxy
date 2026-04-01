#!/usr/bin/env bash
#
# check-bridge-events.sh — Check BridgeEvent synthetic logs on the proxy via eth_getLogs
#
# Usage: ./scripts/check-bridge-events.sh
#
# Shows all BridgeEvents (L2->L1 withdrawals) emitted by the proxy.
# Also shows GER events and ClaimEvents for context.

set -euo pipefail

PROXY_RPC="${PROXY_RPC:-}"
if [[ -z "$PROXY_RPC" ]]; then
    PROXY_RPC=$(kurtosis port print miden-cdk miden-proxy-001 rpc 2>/dev/null || echo "")
fi
[[ -z "$PROXY_RPC" ]] && { echo "ERROR: Cannot find proxy RPC"; exit 1; }

# Topics
BRIDGE_EVENT_TOPIC="0x501781209a1f8899323b96b4ef08b168df93e0a90c673d1e4cce39366cb62f9b"
CLAIM_EVENT_TOPIC="0x1df3f2a973a00d6635911755c260704e95e8a5876997546798770f76396fda4d"
GER_EVENT_TOPIC="0x65d3bf36615f1f02a134d12dfa9ea6b1d4a52386e825973cd27ddb70895c2319"

LATEST=$(curl -s "$PROXY_RPC" -X POST -H 'Content-Type: application/json' \
    -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' | jq -r '.result')

echo "=== Proxy Synthetic Events (eth_getLogs) ==="
echo "Proxy RPC: $PROXY_RPC"
echo "Latest block: $LATEST ($(printf '%d' "$LATEST"))"
echo ""

# BridgeEvents
BRIDGE_EVENTS=$(curl -s "$PROXY_RPC" -X POST -H 'Content-Type: application/json' \
    -d "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getLogs\",\"params\":[{\"fromBlock\":\"0x0\",\"toBlock\":\"$LATEST\",\"topics\":[\"$BRIDGE_EVENT_TOPIC\"]}],\"id\":2}")
BRIDGE_COUNT=$(echo "$BRIDGE_EVENTS" | jq '.result | length')
echo "BridgeEvents (L2->L1): $BRIDGE_COUNT"
if [[ "$BRIDGE_COUNT" -gt 0 ]]; then
    echo "$BRIDGE_EVENTS" | jq -r '.result[] | "  block=\(.blockNumber) tx=\(.transactionHash[0:18])..."'
fi
echo ""

# ClaimEvents
CLAIM_COUNT=$(curl -s "$PROXY_RPC" -X POST -H 'Content-Type: application/json' \
    -d "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getLogs\",\"params\":[{\"fromBlock\":\"0x0\",\"toBlock\":\"$LATEST\",\"topics\":[\"$CLAIM_EVENT_TOPIC\"]}],\"id\":3}" \
    | jq '.result | length')
echo "ClaimEvents (L1->L2):  $CLAIM_COUNT"

# GER events
GER_COUNT=$(curl -s "$PROXY_RPC" -X POST -H 'Content-Type: application/json' \
    -d "{\"jsonrpc\":\"2.0\",\"method\":\"eth_getLogs\",\"params\":[{\"fromBlock\":\"0x0\",\"toBlock\":\"$LATEST\",\"topics\":[\"$GER_EVENT_TOPIC\"]}],\"id\":4}" \
    | jq '.result | length')
echo "GER events:            $GER_COUNT"

echo ""
echo "Total synthetic logs:  $((BRIDGE_COUNT + CLAIM_COUNT + GER_COUNT))"

# Proxy DB cross-check
AGPG=$(docker ps --filter "name=miden-agglayer-postgres" --format "{{.Names}}" | head -1)
if [[ -n "$AGPG" ]]; then
    echo ""
    echo "=== Proxy DB: bridge_out_processed ==="
    docker exec "$AGPG" psql -U agglayer -d agglayer_store -c \
        "SELECT deposit_count, created_at FROM bridge_out_processed ORDER BY deposit_count;" 2>&1
fi
