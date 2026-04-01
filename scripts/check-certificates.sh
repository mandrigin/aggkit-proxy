#!/usr/bin/env bash
#
# check-certificates.sh — Check AggLayer certificate status
#
# Usage: ./scripts/check-certificates.sh [--all]
#
# Shows certificates sent by aggsender, their settlement status,
# and breakdown of exits (L2->L1) vs imported_exits (L1->L2).
#
# Options:
#   --all   Show all certificates (default: last 10)

set -euo pipefail

SHOW_ALL=false
[[ "${1:-}" == "--all" ]] && SHOW_ALL=true

echo "=== AggLayer Certificates ==="
echo ""

# Get all cert lines
CERT_LINES=$(kurtosis service logs miden-cdk aggkit-001 -a 2>&1 | grep "certificate.*sent successfully")
TOTAL=$(echo "$CERT_LINES" | wc -l | tr -d ' ')

echo "Total certificates sent: $TOTAL"
echo ""

# Parse and display
echo "Height | Block Range       | Exits | Imports | Status"
echo "-------|-------------------|-------|---------|-------"

SETTLED_LINES=$(kurtosis service logs miden-cdk aggkit-001 -a 2>&1 | grep "status: Settled")

echo "$CERT_LINES" | while read -r line; do
    HEIGHT=$(echo "$line" | grep -o 'height: [0-9]*' | cut -d' ' -f2)
    FROM=$(echo "$line" | grep -o 'from block: [0-9]*' | cut -d' ' -f3)
    TO=$(echo "$line" | grep -o 'to block: [0-9]*' | cut -d' ' -f3)
    EXITS=$(echo "$line" | grep -o 'exits: [0-9]*' | head -1 | cut -d' ' -f2)
    IMPORTS=$(echo "$line" | grep -o 'imported_exits: [0-9]*' | cut -d' ' -f2)

    # Check if settled
    CERT_ID=$(echo "$line" | grep -o 'certificate: [0-9]*/0x[a-f0-9]*' | cut -d' ' -f2)
    STATUS="Pending"
    if echo "$SETTLED_LINES" | grep -q "${HEIGHT}/"; then
        STATUS="Settled"
    fi

    if $SHOW_ALL || [[ "$HEIGHT" -ge $((TOTAL - 10)) ]]; then
        printf "%-6s | %-17s | %-5s | %-7s | %s\n" "$HEIGHT" "$FROM - $TO" "$EXITS" "$IMPORTS" "$STATUS"
    fi
done

if ! $SHOW_ALL && [[ "$TOTAL" -gt 10 ]]; then
    echo ""
    echo "(showing last 10 of $TOTAL, use --all for full list)"
fi

echo ""
echo "=== Summary ==="
EXITS_TOTAL=$(echo "$CERT_LINES" | grep -o 'exits: [0-9]*' | head -"$TOTAL" | awk '{s+=$2}END{print s}')
# That counts both exits and imported_exits. Let me do it properly
WITH_EXITS=$(echo "$CERT_LINES" | while read -r l; do echo "$l" | grep -o 'exits: [0-9]*' | head -1 | cut -d' ' -f2; done | awk '$1>0{c++}END{print c+0}')
WITH_IMPORTS=$(echo "$CERT_LINES" | while read -r l; do echo "$l" | grep -o 'imported_exits: [0-9]*' | cut -d' ' -f2; done | awk '$1>0{c++}END{print c+0}')

echo "Certs with exits (L2->L1):  $WITH_EXITS"
echo "Certs with imports (L1->L2): $WITH_IMPORTS"
echo "Certs empty:                 $((TOTAL - WITH_EXITS - WITH_IMPORTS))"
