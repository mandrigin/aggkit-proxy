#!/bin/bash
# Start all services for local testing
#
# This script starts miden-node and proxy in background mode.
#
# Usage:
#   ./start-all.sh                                    # start services in background
#   ./start-all.sh --clean                            # clean volumes first, then start
#   ./start-all.sh --proxy-port 8547 --node-port 57292  # use custom ports

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_DIR"

# Use local compose file (minimal dependencies)
COMPOSE_FILE="docker-compose.local.yml"

# Export GIT_COMMIT for proxy Docker build args (full hash for GitHub URL)
export GIT_COMMIT=$(git rev-parse HEAD)

# Export MIDEN_NODE_COMMIT for miden-node image tag
# This is the tag/branch name from https://github.com/0xMiden/miden-node
# The actual commit SHA (c932d27) is captured during Docker build
export MIDEN_NODE_COMMIT="agglayer-v0.1"

# Default ports
PROXY_PORT=8546
NODE_PORT=57291
CLEAN_VOLUMES=false

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --clean)
            CLEAN_VOLUMES=true
            shift
            ;;
        --proxy-port)
            PROXY_PORT="$2"
            shift 2
            ;;
        --node-port)
            NODE_PORT="$2"
            shift 2
            ;;
        *)
            echo "Unknown argument: $1"
            echo "Usage: $0 [--clean] [--proxy-port PORT] [--node-port PORT]"
            exit 1
            ;;
    esac
done

# Export ports for docker-compose
export PROXY_PORT
export NODE_PORT

# Clean volumes if requested
if [ "$CLEAN_VOLUMES" = true ]; then
    echo "Cleaning up existing volumes..."
    docker compose -f "$COMPOSE_FILE" down -v
    echo ""
fi

echo "Starting services in background..."
docker compose -f "$COMPOSE_FILE" up -d miden-node

# Wait for miden-node to be healthy
echo "Waiting for miden-node to be healthy..."
MAX_RETRIES=30
RETRY_COUNT=0

while [ $RETRY_COUNT -lt $MAX_RETRIES ]; do
    if docker compose -f "$COMPOSE_FILE" ps miden-node | grep -q "healthy"; then
        echo "miden-node is healthy!"
        break
    fi

    # Check if service exited/failed
    if docker compose -f "$COMPOSE_FILE" ps miden-node | grep -qE "Exit|exited"; then
        echo "ERROR: miden-node failed to start!"
        echo ""
        echo "Logs:"
        docker compose -f "$COMPOSE_FILE" logs miden-node
        exit 1
    fi

    RETRY_COUNT=$((RETRY_COUNT + 1))
    echo "  Waiting... ($RETRY_COUNT/$MAX_RETRIES)"
    sleep 2
done

if [ $RETRY_COUNT -eq $MAX_RETRIES ]; then
    echo "ERROR: miden-node did not become healthy in time!"
    echo ""
    echo "Logs:"
    docker compose -f "$COMPOSE_FILE" logs miden-node
    exit 1
fi

# Extract BRIDGE_FAUCET_ID from miden-node's database
# The faucet accounts have "faucet" in their storage field
# We skip the native faucet (first one) and take the fungible faucet (LUMIA)
echo "Extracting BRIDGE_FAUCET_ID from miden-node..."

# Get the docker volume name for miden-node data
VOLUME_NAME=$(docker compose -f "$COMPOSE_FILE" config --format json | \
    python3 -c "import sys, json; c=json.load(sys.stdin); print(c['services']['miden-node']['volumes'][0].split(':')[0])" 2>/dev/null || \
    echo "miden_miden_node_data")

# Query the SQLite database to get the fungible faucet account ID (LUMIA)
# OFFSET 1 skips the native MIDEN faucet and returns the fungible faucet
# Note: SQLite hex() returns variable-length output without 0x prefix
# AccountIdV0::from_hex() requires exactly "0x" + 30 hex chars (15 bytes)
RAW_HEX=$(docker run --rm -v "${VOLUME_NAME}:/data" alpine sh -c \
    "apk add --no-cache sqlite >/dev/null 2>&1 && \
     sqlite3 /data/miden-store.sqlite3 \
     \"SELECT hex(account_id) FROM accounts WHERE is_latest = 1 AND storage LIKE '%faucet%' ORDER BY account_id LIMIT 1 OFFSET 1;\"" 2>/dev/null)

if [ -z "$RAW_HEX" ]; then
    echo "WARNING: Could not extract BRIDGE_FAUCET_ID from miden-node database"
    echo "         Proxy will start without Miden submission support"
else
    # Format: left-pad to 30 hex chars with zeros, add 0x prefix
    # AccountIdV0 is 15 bytes = 30 hex chars
    BRIDGE_FAUCET_ID="0x$(printf '%30s' "$RAW_HEX" | tr ' ' '0')"
    echo "  BRIDGE_FAUCET_ID: $BRIDGE_FAUCET_ID (raw: $RAW_HEX)"
    export BRIDGE_FAUCET_ID
fi

# Start proxy now that miden-node is healthy
echo "Starting proxy..."
echo "  Proxy GIT_COMMIT: $GIT_COMMIT"
echo "  Miden-node tag:   $MIDEN_NODE_COMMIT"
docker compose -f "$COMPOSE_FILE" up -d --build proxy

# Wait for proxy to be healthy
echo "Waiting for proxy to be healthy..."
RETRY_COUNT=0

while [ $RETRY_COUNT -lt $MAX_RETRIES ]; do
    if docker compose -f "$COMPOSE_FILE" ps proxy | grep -q "healthy"; then
        echo "proxy is healthy!"
        break
    fi

    # Check if service exited/failed
    if docker compose -f "$COMPOSE_FILE" ps proxy | grep -qE "Exit|exited"; then
        echo "ERROR: proxy failed to start!"
        echo ""
        echo "Logs:"
        docker compose -f "$COMPOSE_FILE" logs proxy
        exit 1
    fi

    RETRY_COUNT=$((RETRY_COUNT + 1))
    echo "  Waiting... ($RETRY_COUNT/$MAX_RETRIES)"
    sleep 2
done

if [ $RETRY_COUNT -eq $MAX_RETRIES ]; then
    echo "ERROR: proxy did not become healthy in time!"
    echo ""
    echo "Logs:"
    docker compose -f "$COMPOSE_FILE" logs proxy
    exit 1
fi

echo ""
echo "============================================"
echo "Ready! Services running."
echo ""
echo "Endpoints:"
echo "  Miden node:  http://localhost:$NODE_PORT"
echo "  Proxy:       http://localhost:$PROXY_PORT"
echo ""
echo "Commands:"
echo "  View logs:   docker compose -f $COMPOSE_FILE logs -f"
echo "  Stop:        docker compose -f $COMPOSE_FILE down"
echo "  Stop+clean:  docker compose -f $COMPOSE_FILE down -v"
echo "============================================"
