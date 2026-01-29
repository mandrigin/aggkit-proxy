#!/usr/bin/env bash
# List all notes from the Miden node's SQLite database.
# Usage: ./scripts/list-notes.sh [enclave] [service]
#   enclave  - Kurtosis enclave name (default: miden-cdk)
#   service  - Miden node service name (default: miden-node-001)

set -euo pipefail

ENCLAVE="${1:-miden-cdk}"
SERVICE="${2:-miden-node-001}"
DB_PATH="/data/miden-store.sqlite3"

TMPDIR=$(mktemp -d)
trap 'rm -rf "$TMPDIR"' EXIT

CONTAINER=$(docker ps --filter "name=$SERVICE" --format '{{.ID}}' | head -1)
if [ -z "$CONTAINER" ]; then
  echo "Error: container matching '$SERVICE' not found" >&2
  exit 1
fi

# Copy DB + WAL + SHM for consistent read
docker cp "$CONTAINER:$DB_PATH"     "$TMPDIR/miden-store.sqlite3"
docker cp "$CONTAINER:${DB_PATH}-wal" "$TMPDIR/miden-store.sqlite3-wal" 2>/dev/null || true
docker cp "$CONTAINER:${DB_PATH}-shm" "$TMPDIR/miden-store.sqlite3-shm" 2>/dev/null || true

sqlite3 -header -column "$TMPDIR/miden-store.sqlite3" "
SELECT
  hex(note_id) as note_id,
  CASE WHEN consumed_at IS NOT NULL THEN 'YES' ELSE 'NO' END as consumed,
  coalesce(consumed_at, '') as consumed_at,
  hex(sender) as sender,
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
