#!/usr/bin/env bash
# List notes from the Miden node with P2ID/CLAIM detection and faucet info.
#
# Usage: ./scripts/list-notes.sh [options]
#   --p2id          Show only P2ID notes (output from faucet to recipient)
#   --claims        Show only CLAIM notes (input to faucet)
#   --unconsumed    Show only unconsumed notes
#   --consumed      Show only consumed notes
#   --all           Show all notes (default)
#   --faucets       Show faucet registry from proxy DB
#
# Examples:
#   ./scripts/list-notes.sh --p2id --unconsumed   # P2ID notes waiting to be claimed
#   ./scripts/list-notes.sh --faucets              # List known faucets

set -euo pipefail

FILTER_TYPE=""
FILTER_CONSUMED=""
SHOW_FAUCETS=false

while [[ $# -gt 0 ]]; do
    case $1 in
        --p2id) FILTER_TYPE="p2id"; shift ;;
        --claims|--claim) FILTER_TYPE="claim"; shift ;;
        --unconsumed) FILTER_CONSUMED="no"; shift ;;
        --consumed) FILTER_CONSUMED="yes"; shift ;;
        --all) FILTER_TYPE=""; FILTER_CONSUMED=""; shift ;;
        --faucets) SHOW_FAUCETS=true; shift ;;
        *) echo "Unknown option: $1"; exit 1 ;;
    esac
done

if $SHOW_FAUCETS; then
    AGPG=$(docker ps --filter "name=miden-agglayer-postgres" --format "{{.Names}}" | head -1)
    if [[ -z "$AGPG" ]]; then
        echo "ERROR: miden-agglayer-postgres container not found" >&2
        exit 1
    fi
    echo "=== Faucet Registry ==="
    docker exec "$AGPG" psql -U agglayer -d agglayer_store -c \
        "SELECT faucet_id, symbol, origin_decimals as l1_dec, miden_decimals as miden_dec, scale, encode(origin_address, 'hex') as l1_token FROM faucet_registry ORDER BY created_at;" 2>&1
    echo ""
fi

CONTAINER=$(docker ps --filter "name=miden-node-001" --format '{{.ID}}' | head -1)
if [[ -z "$CONTAINER" ]]; then
    echo "ERROR: miden-node-001 container not found" >&2
    exit 1
fi

DB_PATH="/app/data/miden-store.sqlite3"
TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

docker cp "$CONTAINER:$DB_PATH"     "$TMPDIR/db.sqlite3"
docker cp "$CONTAINER:${DB_PATH}-wal" "$TMPDIR/db.sqlite3-wal" 2>/dev/null || true
docker cp "$CONTAINER:${DB_PATH}-shm" "$TMPDIR/db.sqlite3-shm" 2>/dev/null || true

# Get faucet IDs from proxy DB for labeling
AGPG=$(docker ps --filter "name=miden-agglayer-postgres" --format "{{.Names}}" 2>/dev/null | head -1)
FAUCET_MAP=""
if [[ -n "$AGPG" ]]; then
    FAUCET_MAP=$(docker exec "$AGPG" psql -t -U agglayer -d agglayer_store -c \
        "SELECT UPPER(REPLACE(faucet_id, '0x', '')) || '=' || symbol FROM faucet_registry;" 2>&1 | tr -d ' ' | grep '=')
fi

# Identify known script roots
# These are discovered by grouping note script_root values
P2ID_ROOT=$(sqlite3 "$TMPDIR/db.sqlite3" "
    SELECT hex(script_root) FROM notes
    WHERE script_root IS NOT NULL
    GROUP BY script_root
    ORDER BY COUNT(*) DESC
    LIMIT 1;
")

CLAIM_ROOT=$(sqlite3 "$TMPDIR/db.sqlite3" "
    SELECT hex(script_root) FROM notes
    WHERE script_root IS NOT NULL AND hex(script_root) != '$P2ID_ROOT'
    GROUP BY script_root
    ORDER BY COUNT(*) DESC
    LIMIT 1;
")

# Build WHERE clause
WHERE="1=1"
if [[ "$FILTER_TYPE" == "p2id" && -n "$P2ID_ROOT" ]]; then
    WHERE="$WHERE AND hex(script_root) = '$P2ID_ROOT'"
elif [[ "$FILTER_TYPE" == "claim" && -n "$CLAIM_ROOT" ]]; then
    WHERE="$WHERE AND hex(script_root) = '$CLAIM_ROOT'"
fi
if [[ "$FILTER_CONSUMED" == "yes" ]]; then
    WHERE="$WHERE AND consumed_at IS NOT NULL"
elif [[ "$FILTER_CONSUMED" == "no" ]]; then
    WHERE="$WHERE AND consumed_at IS NULL"
fi

echo "=== Notes ==="
[[ -n "$P2ID_ROOT" ]] && echo "P2ID script root:  ${P2ID_ROOT:0:16}..."
[[ -n "$CLAIM_ROOT" ]] && echo "CLAIM script root: ${CLAIM_ROOT:0:16}..."
echo ""

# Query and format
RESULTS=$(sqlite3 -separator '|' "$TMPDIR/db.sqlite3" "
    SELECT
        hex(note_id),
        hex(sender),
        COALESCE(hex(target_account_id), ''),
        tag,
        note_type,
        committed_at,
        COALESCE(consumed_at, 0),
        COALESCE(hex(script_root), '')
    FROM notes
    WHERE $WHERE
    ORDER BY committed_at, batch_index, note_index;
")

if [[ -z "$RESULTS" ]]; then
    echo "(no notes matching filter)"
    exit 0
fi

printf "%-6s %-18s %-16s %-16s %-8s %-6s %-5s %s\n" \
    "TYPE" "NOTE_ID" "SENDER" "TARGET" "FAUCET" "BLOCK" "CONS" "TAG"
printf "%-6s %-18s %-16s %-16s %-8s %-6s %-5s %s\n" \
    "------" "------------------" "----------------" "----------------" "--------" "------" "-----" "---"

echo "$RESULTS" | while IFS='|' read -r NOTE_ID SENDER TARGET TAG NOTE_TYPE BLOCK CONSUMED SCRIPT_ROOT; do
    # Determine note type
    TYPE="???"
    if [[ "$SCRIPT_ROOT" == "$P2ID_ROOT" ]]; then
        TYPE="P2ID"
    elif [[ "$SCRIPT_ROOT" == "$CLAIM_ROOT" ]]; then
        TYPE="CLAIM"
    elif [[ -n "$SCRIPT_ROOT" ]]; then
        TYPE="OTHER"
    fi

    # Consumed?
    CONS="no"
    [[ "$CONSUMED" != "0" ]] && CONS="$CONSUMED"

    # Match sender to faucet name
    FAUCET=""
    SENDER_UPPER=$(echo "$SENDER" | tr '[:lower:]' '[:upper:]')
    while IFS= read -r mapping; do
        FAUCET_HEX="${mapping%%=*}"
        FAUCET_SYM="${mapping#*=}"
        if [[ "$SENDER_UPPER" == "$FAUCET_HEX" ]]; then
            FAUCET="$FAUCET_SYM"
            break
        fi
    done <<< "$FAUCET_MAP"

    # Also check target for faucet name
    if [[ -z "$FAUCET" && -n "$TARGET" ]]; then
        TARGET_UPPER=$(echo "$TARGET" | tr '[:lower:]' '[:upper:]')
        while IFS= read -r mapping; do
            FAUCET_HEX="${mapping%%=*}"
            FAUCET_SYM="${mapping#*=}"
            if [[ "$TARGET_UPPER" == "$FAUCET_HEX" ]]; then
                FAUCET="→$FAUCET_SYM"
                break
            fi
        done <<< "$FAUCET_MAP"
    fi

    printf "%-6s %-18s %-16s %-16s %-8s %-6s %-5s %s\n" \
        "$TYPE" "${NOTE_ID:0:18}" "${SENDER:0:16}" "${TARGET:0:16}" "$FAUCET" "$BLOCK" "$CONS" "$TAG"
done

echo ""
TOTAL=$(echo "$RESULTS" | wc -l | tr -d ' ')
echo "Total: $TOTAL notes"
