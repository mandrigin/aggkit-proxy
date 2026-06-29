#!/bin/bash
# verify-notes.sh — DEPRECATED redirect shim.
#
# The verify-notes Rust binary (src/bin/verify_notes.rs) was removed when the
# vibecoded proxy lib was supplanted by miden-agglayer. Note inspection now goes
# through the node store directly (works against both local stacks):
#
#   ./scripts/list-notes.sh             # all notes, consumed/unconsumed
#   ./scripts/list-unclaimed-notes.sh   # unclaimed P2ID notes with amounts
#
# For published CLAIM-note verification from the proxy logs:
#   ./scripts/verify-claim-notes.sh
#
# Select a stack with TOPOLOGY=compose|kurtosis (auto-detected otherwise).

cat >&2 <<'EOF'
verify-notes: removed — the proxy is now miden-agglayer.

Use instead:
  ./scripts/list-notes.sh             # all notes on the miden-node
  ./scripts/list-unclaimed-notes.sh   # unclaimed P2ID notes (with amounts)
  ./scripts/verify-claim-notes.sh     # CLAIM notes parsed from proxy logs

All honor TOPOLOGY=compose|kurtosis (auto-detected from running containers).
EOF
exit 2
