#!/bin/bash
# Start Miden node for local testing
#
# This script starts the miden-node container using docker-compose.
# The node is built from source (agglayer-v0.1 tag) for compatibility with the proxy.
# First run will take longer as it builds the Rust binary.

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_DIR"

echo "Starting Miden node (agglayer-v0.1)..."
echo "Node will be available at: http://localhost:57291"
echo ""

# Start only the miden-node service
docker compose up miden-node "$@"
