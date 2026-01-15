#!/bin/bash
# Start all services for local testing
#
# This script starts miden-node and proxy in background mode.
#
# Usage:
#   ./start-all.sh           # start services in background
#   ./start-all.sh --clean   # clean volumes first, then start

set -e

SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

cd "$PROJECT_DIR"

# Use local compose file (minimal dependencies)
COMPOSE_FILE="docker-compose.local.yml"

# Check for --clean flag
if [ "$1" = "--clean" ]; then
    echo "Cleaning up existing volumes..."
    docker compose -f "$COMPOSE_FILE" down -v
    shift
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
    sleep 5
done

if [ $RETRY_COUNT -eq $MAX_RETRIES ]; then
    echo "ERROR: miden-node did not become healthy in time!"
    echo ""
    echo "Logs:"
    docker compose -f "$COMPOSE_FILE" logs miden-node
    exit 1
fi

# Start proxy now that miden-node is healthy
echo "Starting proxy..."
docker compose -f "$COMPOSE_FILE" up -d proxy

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
    sleep 5
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
echo "  Miden node:  http://localhost:57291"
echo "  Proxy:       http://localhost:8546"
echo ""
echo "Commands:"
echo "  View logs:   docker compose -f $COMPOSE_FILE logs -f"
echo "  Stop:        docker compose -f $COMPOSE_FILE down"
echo "  Stop+clean:  docker compose -f $COMPOSE_FILE down -v"
echo "============================================"
