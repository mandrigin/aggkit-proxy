#!/bin/bash
# Start Miden node for local testing
#
# This script starts the miden-node container using docker-compose.
# The node is built from source (agglayer-v0.1 tag) for compatibility with the proxy.
# First run will take longer as it builds the Rust binary.
#
# Usage:
#   ./start-miden-node.sh          # normal start
#   ./start-miden-node.sh --clean  # clear volumes first, then start

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_DIR"

# Check for --clean flag
if [ "$1" = "--clean" ]; then
    echo "Cleaning up existing volumes..."
    docker compose down -v
    shift  # Remove --clean from args
    echo ""
fi

echo "Starting Miden node (agglayer-v0.1)..."
echo "Node will be available at: http://localhost:57291"
echo ""

# Start only the miden-node service
docker compose up miden-node "$@"
