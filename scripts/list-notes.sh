#!/usr/bin/env bash
# List all notes from the Miden node's SQLite database.
#
# Works against both local stacks (see scripts/lib/topology.sh):
#   TOPOLOGY=compose  ./scripts/list-notes.sh   # miden-agglayer docker-compose stack
#   TOPOLOGY=kurtosis ./scripts/list-notes.sh   # kurtosis miden-cdk enclave
# Auto-detects when omitted. Override node container / DB with MIDEN_NODE_CONTAINER / MIDEN_NODE_DB.

set -euo pipefail
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
source "$SCRIPT_DIR/lib/topology.sh"

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

CONTAINER=$(topology_cid "$MIDEN_NODE_CONTAINER")
if [ -z "$CONTAINER" ]; then
  echo "Error: miden-node container '$MIDEN_NODE_CONTAINER' not found (TOPOLOGY=$TOPOLOGY)" >&2
  exit 1
fi

DB_PATH=$(topology_node_db)
if [ -z "$DB_PATH" ]; then
  echo "Error: could not locate miden-store.sqlite3 in $MIDEN_NODE_CONTAINER under $MIDEN_NODE_DATA_DIR" >&2
  exit 1
fi

# Copy DB + WAL + SHM for consistent read
docker cp "$CONTAINER:$DB_PATH"       "$TMPDIR/miden-store.sqlite3"
docker cp "$CONTAINER:${DB_PATH}-wal" "$TMPDIR/miden-store.sqlite3-wal" 2>/dev/null || true
docker cp "$CONTAINER:${DB_PATH}-shm" "$TMPDIR/miden-store.sqlite3-shm" 2>/dev/null || true

sqlite3 -header -column "$TMPDIR/miden-store.sqlite3" "
SELECT
  hex(note_id) as note_id,
  CASE WHEN consumed_at IS NOT NULL THEN 'YES' ELSE 'NO' END as consumed,
  coalesce(consumed_at, '') as consumed_at,
  hex(sender) as sender,
  coalesce(hex(target_account_id), '(none)') as target,
  tag,
  CASE note_type
    WHEN 1 THEN 'Public'
    WHEN 2 THEN 'Private'
    WHEN 3 THEN 'Encrypted'
    ELSE cast(note_type as text)
  END as type,
  committed_at as block
FROM notes
ORDER BY committed_at, batch_index, note_index;
"
