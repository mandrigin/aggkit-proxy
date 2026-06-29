#!/usr/bin/env bash
#
# Verify CLAIM Notes - Parse proxy logs and verify all successfully published CLAIM notes
#
# Usage:
#   ./scripts/verify-claim-notes.sh [container_name]
#
# Arguments:
#   container_name   Docker container name for the proxy (default: miden-proxy-kurtosis)
#
# This script:
#   1. Parses the proxy container logs for successfully published CLAIM notes
#   2. Extracts note IDs, deposit numbers, and amounts
#   3. Verifies each note exists on the miden-node
#   4. Outputs a summary table

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib/topology.sh"

# Resolve the proxy container: explicit $1 wins, else the topology default
# (compose: miden-agglayer-miden-agglayer-1, kurtosis: miden-proxy-001).
PROXY_REF="${1:-$AGGLAYER_PROXY_CONTAINER}"
CONTAINER_NAME=$(topology_cid "$PROXY_REF")
if [[ -z "$CONTAINER_NAME" ]]; then
    echo "Error: proxy container '$PROXY_REF' not found or not running (TOPOLOGY=$TOPOLOGY)"
    echo ""
    echo "Available containers:"
    docker ps --format '  {{.Names}}' | grep -i miden || echo "  (none)"
    exit 1
fi

echo "╔══════════════════════════════════════════════════════════════════╗"
echo "║           CLAIM Note Verification Tool                           ║"
echo "╚══════════════════════════════════════════════════════════════════╝"
echo ""
echo "Proxy container: $PROXY_REF ($CONTAINER_NAME) — TOPOLOGY=$TOPOLOGY"
echo ""

# Get logs and parse CLAIM notes
echo "Parsing proxy logs for CLAIM notes..."
echo ""

# Create temp files for processing
LOGS_FILE=$(mktemp)
PARSED_FILE=$(mktemp)
RESULTS_FILE=$(mktemp)
trap "rm -f $LOGS_FILE $PARSED_FILE $RESULTS_FILE" EXIT

# Get logs and strip ANSI color codes, filter relevant lines
docker logs "$CONTAINER_NAME" 2>&1 | \
    sed 's/\x1b\[[0-9;]*m//g' | \
    grep -E "(claimAsset parameters parsed|CLAIM note ID:)" > "$LOGS_FILE"

# Parse logs using grep and sed (portable approach)
# Find claimAsset lines and extract deposit/amount, then find following note ID
pending_deposit=""
pending_amount=""

while IFS= read -r line; do
    if echo "$line" | grep -q "claimAsset parameters parsed"; then
        # Extract local_root_index
        pending_deposit=$(echo "$line" | sed -n 's/.*local_root_index=\([0-9]*\).*/\1/p')
        # Extract amount
        pending_amount=$(echo "$line" | sed -n 's/.*amount=\([0-9]*\).*/\1/p')
    elif echo "$line" | grep -q "CLAIM note ID:"; then
        if [[ -n "$pending_deposit" ]]; then
            # Extract note ID
            note_id=$(echo "$line" | sed -n 's/.*CLAIM note ID: \(0x[a-fA-F0-9]*\).*/\1/p')
            if [[ -n "$note_id" ]]; then
                echo "${pending_deposit}|${pending_amount}|${note_id}" >> "$PARSED_FILE"
            fi
            pending_deposit=""
            pending_amount=""
        fi
    fi
done < "$LOGS_FILE"

# Count notes
note_count=$(wc -l < "$PARSED_FILE" | tr -d ' ')

if [[ "$note_count" -eq 0 ]]; then
    echo "No CLAIM notes found in proxy logs."
    echo ""
    echo "Make sure:"
    echo "  1. The proxy has processed some claimAsset transactions"
    echo "  2. The transactions completed successfully"
    exit 0
fi

echo "Found $note_count CLAIM note(s) to verify"
echo ""

# Verify each note and build results
echo "Verifying notes on miden-node..."
echo ""

verified=0
failed=0

while IFS='|' read -r deposit amount note_id; do
    # Convert amount to ETH (18 decimals)
    if command -v bc &> /dev/null && [[ -n "$amount" ]]; then
        amount_eth=$(echo "scale=4; $amount / 1000000000000000000" | bc 2>/dev/null || echo "$amount")
    else
        amount_eth="$amount"
    fi

    # Verify note exists
    echo -n "  Checking deposit $deposit ($note_id)... "

    if "$SCRIPT_DIR/verify-notes.sh" --note-id "$note_id" 2>&1 | grep -q "Note found"; then
        status="✓ VERIFIED"
        verified=$((verified + 1))
        echo "$status"
    else
        status="✗ NOT FOUND"
        failed=$((failed + 1))
        echo "$status"
    fi

    echo "$deposit|$amount_eth|$note_id|$status" >> "$RESULTS_FILE"
done < "$PARSED_FILE"

echo ""
echo "═══════════════════════════════════════════════════════════════════"
echo "                         RESULTS TABLE                              "
echo "═══════════════════════════════════════════════════════════════════"
echo ""

# Print table header
printf "%-10s %-12s %-68s %s\n" "Deposit" "Amount" "Note ID" "Status"
printf "%-10s %-12s %-68s %s\n" "-------" "------" "-------" "------"

# Print results
while IFS='|' read -r deposit amount note_id status; do
    printf "%-10s %-12s %-68s %s\n" "$deposit" "$amount" "$note_id" "$status"
done < "$RESULTS_FILE"

echo ""
echo "═══════════════════════════════════════════════════════════════════"
echo "                           SUMMARY                                  "
echo "═══════════════════════════════════════════════════════════════════"
echo ""
echo "  Total notes:    $note_count"
echo "  Verified:       $verified"
echo "  Failed:         $failed"
echo ""

if [[ $failed -eq 0 ]]; then
    echo "✓ All CLAIM notes verified successfully!"
    exit 0
else
    echo "✗ Some notes could not be verified"
    exit 1
fi
