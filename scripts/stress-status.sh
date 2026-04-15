#!/usr/bin/env bash
# stress-status.sh — one-line deposit/claim pipeline snapshot.
#
# Usage:
#   ./scripts/stress-status.sh         # single snapshot
#   watch -n 5 ./scripts/stress-status.sh
#
# Env:
#   DEST_NET   bridge dest_net to count (default: 1)
#   WINDOW     docker-logs --since window for rate/error counts (default: 1m)

set -uo pipefail

DEST_NET="${DEST_NET:-1}"
WINDOW="${WINDOW:-1m}"

BP=$(docker ps --format '{{.Names}}' | grep -E '^postgres-001--' | head -1)
PX=$(docker ps --format '{{.Names}}' | grep -E '^miden-proxy-001--' | head -1)

if [[ -z "$BP" || -z "$PX" ]]; then
    echo "ERROR: bridge postgres or proxy container not found (is kurtosis enclave running?)" >&2
    exit 1
fi

q() {
    docker exec "$BP" psql -U bridge_user -d bridge_db -At -c "$1" 2>/dev/null
}

DEP=$(q "SELECT count(*) FROM sync.deposit WHERE dest_net=$DEST_NET;")
RDY=$(q "SELECT count(*) FROM sync.deposit WHERE dest_net=$DEST_NET AND ready_for_claim=true;")
MON=$(q "SELECT count(*) FROM sync.monitored_txs;")
CLA=$(q "SELECT count(*) FROM sync.claim;")
LATEST=$(q "SELECT index FROM sync.claim ORDER BY index DESC LIMIT 1;")

LOGS=$(docker logs --since "$WINDOW" "$PX" 2>&1)
RATE=$(printf '%s\n' "$LOGS" | grep -c "submitted claim note txn" || true)
DNS=$(printf '%s\n' "$LOGS" | grep -c "dns error" || true)
NO_ACCT=$(printf '%s\n' "$LOGS" | grep -c "no known Miden AccountId" || true)
OTHER_ERR=$(printf '%s\n' "$LOGS" | grep "\[31mERROR\[0m" | grep -vE "no known Miden AccountId|dns error" | wc -l | tr -d ' ')

printf '%s  deposits=%s  ready=%s  queued=%s  claimed=%s  latest_idx=%s  last%s: +%s claims, %s dns, %s no-acct, %s other-err\n' \
    "$(date +%H:%M:%S)" "$DEP" "$RDY" "$MON" "$CLA" "${LATEST:-none}" \
    "$WINDOW" "$RATE" "$DNS" "$NO_ACCT" "$OTHER_ERR"
