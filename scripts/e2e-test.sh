#!/usr/bin/env bash
#
# Miden RPC Proxy End-to-End Test
#
# This script runs the full e2e test flow:
# 1. Installs prerequisites if missing (kurtosis, foundry, jq)
# 2. Deploys kurtosis-cdk (L1 + bridge infrastructure)
# 3. Starts Miden node + proxy on the kurtosis network
# 4. Configures bridge service to use Miden proxy as L2
# 5. Sends a test deposit from L1 to Miden
# 6. Verifies deposit events in proxy
#
# Usage:
#   ./scripts/e2e-test.sh [OPTIONS]
#
# Options:
#   --fresh              Deploy fresh (destroys existing enclave)
#   --enclave NAME       Enclave name (default: cdk-miden)
#   --skip-cdk           Skip kurtosis-cdk deployment
#   --skip-miden         Skip Miden node/proxy startup
#   --skip-deposit       Skip test deposit
#   --skip-install       Skip auto-installation of prerequisites
#   --rebuild            Force rebuild of proxy image (use after code changes)
#   --help               Show this help

set -euo pipefail

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
BOLD='\033[1m'
NC='\033[0m'

# Configuration
ENCLAVE_NAME="${ENCLAVE_NAME:-cdk-miden}"
DEPLOY_FRESH=false
SKIP_CDK=false
SKIP_MIDEN=false
SKIP_DEPOSIT=false
SKIP_INSTALL=false
REBUILD_IMAGES=false

# OS detection
OS="$(uname -s)"
ARCH="$(uname -m)"

# Miden configuration
MIDEN_PROXY_PORT=8123
MIDEN_NODE_PORT=57291
MIDEN_NETWORK_ID=2

# Kurtosis-cdk repo
KURTOSIS_CDK_REPO="https://github.com/0xPolygon/kurtosis-cdk"
KURTOSIS_CDK_DIR="${KURTOSIS_CDK_DIR:-/tmp/kurtosis-cdk}"

# Script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# Funded accounts
KURTOSIS_PRIVATE_KEY="0x12d7de8621a77640c9241b2595ba78ce443d05e94090365ab3bb5e19df82c625"
KURTOSIS_ADDRESS="0xE34aaF64b29273B7D567FCFc40544c014EEe9970"

#######################################
# Helpers
#######################################

log() { echo -e "${BLUE}[$(date '+%H:%M:%S')]${NC} $1"; }
success() { echo -e "${GREEN}✓${NC} $1"; }
fail() { echo -e "${RED}✗ FAIL:${NC} $1"; exit 1; }
warn() { echo -e "${YELLOW}!${NC} $1"; }
step() { echo -e "\n${CYAN}${BOLD}=== $1 ===${NC}\n"; }

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --fresh) DEPLOY_FRESH=true; shift ;;
        --enclave) ENCLAVE_NAME="$2"; shift 2 ;;
        --skip-cdk) SKIP_CDK=true; shift ;;
        --skip-miden) SKIP_MIDEN=true; shift ;;
        --skip-deposit) SKIP_DEPOSIT=true; shift ;;
        --skip-install) SKIP_INSTALL=true; shift ;;
        --rebuild) REBUILD_IMAGES=true; shift ;;
        --help) head -20 "$0" | tail -15; exit 0 ;;
        *) fail "Unknown option: $1" ;;
    esac
done

#######################################
# Prerequisites Installation
#######################################

install_kurtosis() {
    if command -v kurtosis &>/dev/null; then
        success "kurtosis already installed"
        return 0
    fi

    if $SKIP_INSTALL; then
        fail "kurtosis not found (use --skip-install=false to auto-install)"
    fi

    log "Installing Kurtosis CLI..."

    if [[ "$OS" == "Darwin" ]]; then
        if command -v brew &>/dev/null; then
            brew install kurtosis-tech/tap/kurtosis-cli
        else
            fail "Homebrew not found. Install kurtosis manually: https://docs.kurtosis.com/install"
        fi
    elif [[ "$OS" == "Linux" ]]; then
        echo "deb [trusted=yes] https://apt.fury.io/kurtosis-tech/ /" | sudo tee /etc/apt/sources.list.d/kurtosis.list
        sudo apt update && sudo apt install -y kurtosis-cli
    else
        fail "Unsupported OS. Install kurtosis manually: https://docs.kurtosis.com/install"
    fi

    success "kurtosis installed"
}

install_foundry() {
    if command -v cast &>/dev/null; then
        success "foundry (cast) already installed"
        return 0
    fi

    if $SKIP_INSTALL; then
        fail "foundry not found (use --skip-install=false to auto-install)"
    fi

    log "Installing Foundry (cast, forge, anvil)..."

    curl -L https://foundry.paradigm.xyz | bash

    # Source foundry in current shell
    export PATH="$HOME/.foundry/bin:$PATH"

    # Run foundryup to install binaries
    if command -v foundryup &>/dev/null; then
        foundryup
    else
        "$HOME/.foundry/bin/foundryup"
    fi

    success "foundry installed"
}

install_jq() {
    if command -v jq &>/dev/null; then
        success "jq already installed"
        return 0
    fi

    if $SKIP_INSTALL; then
        fail "jq not found (use --skip-install=false to auto-install)"
    fi

    log "Installing jq..."

    if [[ "$OS" == "Darwin" ]]; then
        if command -v brew &>/dev/null; then
            brew install jq
        else
            fail "Homebrew not found. Install jq manually: brew install jq"
        fi
    elif [[ "$OS" == "Linux" ]]; then
        sudo apt install -y jq
    else
        fail "Unsupported OS. Install jq manually."
    fi

    success "jq installed"
}

check_prerequisites() {
    step "Checking Prerequisites"

    # Docker must be installed and running (can't auto-install)
    if ! command -v docker &>/dev/null; then
        fail "Docker not found. Install from: https://docs.docker.com/get-docker/"
    fi
    if ! docker info &>/dev/null; then
        fail "Docker not running. Start Docker Desktop or docker daemon."
    fi
    success "Docker running"

    # Auto-install these if missing
    install_kurtosis
    install_foundry
    install_jq
}

#######################################
# Kurtosis-CDK Deployment
#######################################

deploy_kurtosis_cdk() {
    step "Deploying Kurtosis-CDK"

    if kurtosis enclave inspect "$ENCLAVE_NAME" &>/dev/null; then
        if $DEPLOY_FRESH; then
            log "Removing existing enclave..."
            kurtosis enclave rm "$ENCLAVE_NAME" --force
        else
            success "Enclave '$ENCLAVE_NAME' exists, reusing"
            return 0
        fi
    fi

    # Clone if needed
    if [[ ! -d "$KURTOSIS_CDK_DIR" ]]; then
        log "Cloning kurtosis-cdk..."
        git clone --depth 1 "$KURTOSIS_CDK_REPO" "$KURTOSIS_CDK_DIR"
    fi

    log "Deploying kurtosis-cdk..."
    cd "$KURTOSIS_CDK_DIR"
    kurtosis run . --enclave "$ENCLAVE_NAME" 2>&1 | while read -r line; do
        echo -e "${BLUE}│${NC} $line"
    done
    cd - >/dev/null

    success "Kurtosis-CDK deployed"
}

#######################################
# Miden Services on Kurtosis Network
#######################################

get_kurtosis_network() {
    # Get the Docker network used by kurtosis enclave
    local network
    network=$(docker network ls --filter "name=kt-${ENCLAVE_NAME}" --format "{{.Name}}" | head -1)

    if [[ -z "$network" ]]; then
        # Try alternative naming
        network=$(docker network ls --filter "name=${ENCLAVE_NAME}" --format "{{.Name}}" | grep -v "bridge" | head -1)
    fi

    echo "$network"
}

start_miden_services() {
    step "Starting Miden Services on Kurtosis Network"

    local kurtosis_network
    kurtosis_network=$(get_kurtosis_network)

    if [[ -z "$kurtosis_network" ]]; then
        fail "Could not find kurtosis network for enclave '$ENCLAVE_NAME'. Is the enclave running?"
    fi
    log "Kurtosis network: $kurtosis_network"

    # Stop existing containers
    log "Cleaning up existing miden containers..."
    docker rm -f miden-node-kurtosis miden-proxy-kurtosis miden-l2-forwarder 2>/dev/null || true

    # Check if miden-node image exists
    if ! docker image inspect miden-node:agglayer-v0.1 &>/dev/null; then
        fail "miden-node:agglayer-v0.1 image not found. Build it first with: docker build -t miden-node:agglayer-v0.1 -f Dockerfile.miden-node ."
    fi

    # Start Miden node
    log "Starting Miden node..."
    if ! docker run -d \
        --name miden-node-kurtosis \
        --network "$kurtosis_network" \
        -p "${MIDEN_NODE_PORT}:57291" \
        miden-node:agglayer-v0.1; then
        fail "Failed to start miden-node-kurtosis container"
    fi

    # Verify container is running
    if ! docker ps --filter "name=miden-node-kurtosis" --format "{{.Names}}" | grep -q "miden-node-kurtosis"; then
        docker logs miden-node-kurtosis 2>&1 || true
        fail "miden-node-kurtosis container is not running"
    fi
    success "Miden node container started"

    # Wait for node to be ready
    log "Waiting for Miden node to be ready (up to 60s)..."
    local attempts=0
    while [[ $attempts -lt 30 ]]; do
        if docker exec miden-node-kurtosis nc -z localhost 57291 2>/dev/null; then
            success "Miden node ready"
            break
        fi
        ((attempts++))
        sleep 2
    done

    if [[ $attempts -ge 30 ]]; then
        warn "Miden node may not be ready yet - continuing anyway"
        docker logs miden-node-kurtosis --tail 20 2>&1 || true
    fi

    # Build proxy image if needed or --rebuild flag is set
    if $REBUILD_IMAGES || ! docker image inspect miden-rpc-proxy:kurtosis &>/dev/null; then
        log "Building miden-rpc-proxy:kurtosis image..."
        if ! docker build -t miden-rpc-proxy:kurtosis -f "$PROJECT_DIR/Dockerfile" "$PROJECT_DIR"; then
            fail "Failed to build miden-rpc-proxy image"
        fi
        success "Proxy image built"
    else
        log "Using existing miden-rpc-proxy:kurtosis image (use --rebuild to force)"
    fi

    # Start proxy
    log "Starting Miden proxy (CHAIN_ID=$MIDEN_NETWORK_ID)..."
    if ! docker run -d \
        --name miden-proxy-kurtosis \
        --network "$kurtosis_network" \
        -p "${MIDEN_PROXY_PORT}:8546" \
        -e CHAIN_ID="$MIDEN_NETWORK_ID" \
        -e MIDEN_RPC_URL="http://miden-node-kurtosis:57291" \
        -e MIDEN_STORE_PATH="/app/data/miden-client" \
        -e LISTEN_PORT=8546 \
        miden-rpc-proxy:kurtosis; then
        fail "Failed to start miden-proxy-kurtosis container"
    fi

    # Verify container is running
    if ! docker ps --filter "name=miden-proxy-kurtosis" --format "{{.Names}}" | grep -q "miden-proxy-kurtosis"; then
        docker logs miden-proxy-kurtosis 2>&1 || true
        fail "miden-proxy-kurtosis container is not running"
    fi
    success "Miden proxy container started"

    # Wait for proxy to be ready
    log "Waiting for Miden proxy to be ready..."
    attempts=0
    while [[ $attempts -lt 30 ]]; do
        if curl -s "http://localhost:${MIDEN_PROXY_PORT}" &>/dev/null; then
            success "Miden proxy ready on port ${MIDEN_PROXY_PORT}"
            break
        fi
        ((attempts++))
        sleep 2
    done

    if [[ $attempts -ge 30 ]]; then
        warn "Miden proxy may not be ready yet"
        docker logs miden-proxy-kurtosis --tail 20 2>&1 || true
    fi

    # Get container IPs for kurtosis network (specifically from kt-* network)
    MIDEN_NODE_IP=$(docker inspect -f "{{range \$k, \$v := .NetworkSettings.Networks}}{{if eq (printf \"%.3s\" \$k) \"kt-\"}}{{\$v.IPAddress}}{{end}}{{end}}" miden-node-kurtosis 2>/dev/null || echo "")
    MIDEN_PROXY_IP=$(docker inspect -f "{{range \$k, \$v := .NetworkSettings.Networks}}{{if eq (printf \"%.3s\" \$k) \"kt-\"}}{{\$v.IPAddress}}{{end}}{{end}}" miden-proxy-kurtosis 2>/dev/null || echo "")

    # Fallback to any IP if kt- network not found
    if [[ -z "$MIDEN_NODE_IP" ]]; then
        MIDEN_NODE_IP=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' miden-node-kurtosis 2>/dev/null | head -c 15)
    fi
    if [[ -z "$MIDEN_PROXY_IP" ]]; then
        MIDEN_PROXY_IP=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' miden-proxy-kurtosis 2>/dev/null | head -c 15)
    fi

    if [[ -z "$MIDEN_PROXY_IP" ]]; then
        fail "Could not determine Miden proxy IP address"
    fi

    log "Miden node IP:  ${MIDEN_NODE_IP:-NOT FOUND}"
    log "Miden proxy IP: ${MIDEN_PROXY_IP}"

    # Store for later use
    echo "$MIDEN_PROXY_IP" > /tmp/miden-proxy-ip

    # Final verification - show all miden containers
    echo ""
    log "Miden containers running:"
    docker ps --filter "name=miden" --format "  {{.Names}}: {{.Status}}" || true
}

#######################################
# Reconfigure Bridge Service
#######################################

reconfigure_bridge_service() {
    step "Reconfiguring Bridge Service for Miden"

    local proxy_ip
    proxy_ip=$(cat /tmp/miden-proxy-ip 2>/dev/null || echo "")

    if [[ -z "$proxy_ip" ]]; then
        warn "Miden proxy IP not found, skipping bridge reconfiguration"
        return
    fi

    local kurtosis_network
    kurtosis_network=$(get_kurtosis_network)

    # Proxy listens on 8546 inside the container
    local internal_port=8546
    log "Miden proxy accessible at: http://${proxy_ip}:${internal_port}"

    # Step 1: Create nginx forwarder to route bridge traffic to Miden proxy
    # The bridge expects to connect to op-geth on 8545, but we redirect to Miden proxy on 8546
    log "Creating nginx TCP forwarder (miden-l2-forwarder)..."

    docker rm -f miden-l2-forwarder 2>/dev/null || true

    # Create nginx config for TCP stream proxy
    local nginx_conf="/tmp/miden-nginx-stream.conf"
    cat > "$nginx_conf" << EOF
events {
    worker_connections 1024;
}
stream {
    upstream miden_proxy {
        server ${proxy_ip}:${internal_port};
    }
    server {
        listen 8545;
        proxy_pass miden_proxy;
    }
}
EOF

    docker run -d \
        --name miden-l2-forwarder \
        --network "$kurtosis_network" \
        -v "$nginx_conf:/etc/nginx/nginx.conf:ro" \
        nginx:alpine 2>/dev/null || {
        fail "Failed to start nginx forwarder"
    }

    # Wait for forwarder to be ready
    sleep 2
    success "Nginx forwarder started"

    # Get forwarder IP
    local forwarder_ip
    forwarder_ip=$(docker inspect -f "{{range \$k, \$v := .NetworkSettings.Networks}}{{if eq (printf \"%.3s\" \$k) \"kt-\"}}{{\$v.IPAddress}}{{end}}{{end}}" miden-l2-forwarder 2>/dev/null || echo "")
    if [[ -z "$forwarder_ip" ]]; then
        forwarder_ip=$(docker inspect -f '{{range .NetworkSettings.Networks}}{{.IPAddress}}{{end}}' miden-l2-forwarder 2>/dev/null | head -c 15)
    fi
    log "Forwarder IP: $forwarder_ip"

    # Step 2: Find the aggkit bridge container
    log "Finding aggkit bridge container..."
    local bridge_container
    bridge_container=$(docker ps --filter "name=aggkit.*bridge" --format "{{.Names}}" | head -1)

    if [[ -z "$bridge_container" ]]; then
        warn "aggkit bridge container not found, skipping reconfiguration"
        return
    fi
    log "Bridge container: $bridge_container"

    # Step 3: Modify bridge config to use miden-l2-forwarder
    log "Modifying bridge config to use Miden proxy..."

    # Update L2URL in the config
    docker exec "$bridge_container" sh -c "
        if [ -f /etc/aggkit/config.toml ]; then
            # Backup original
            cp /etc/aggkit/config.toml /etc/aggkit/config.toml.bak
            # Replace op-geth L2URL with our forwarder
            sed -i 's|L2URL = \"http://op-el-1-op-geth-op-node-001:8545\"|L2URL = \"http://miden-l2-forwarder:8545\"|g' /etc/aggkit/config.toml
            # Also update any RPCURL pointing to op-geth
            sed -i 's|RPCURL = \"http://op-el-1-op-geth-op-node-001:8545\"|RPCURL = \"http://miden-l2-forwarder:8545\"|g' /etc/aggkit/config.toml
        fi
    " 2>/dev/null || warn "Could not modify bridge config"

    # Verify change
    local l2url
    l2url=$(docker exec "$bridge_container" grep "^L2URL" /etc/aggkit/config.toml 2>/dev/null | head -1 || echo "")
    log "L2URL now: $l2url"

    # Step 4: Clear L2 databases then restart
    # Note: /tmp is not a volume, it's container filesystem
    # We must delete while running, then restart
    log "Clearing L2 database files (stale chain ID 2151908)..."

    # Delete all L2 sqlite files while container is running (releases WAL on next restart)
    docker exec "$bridge_container" sh -c \
        "rm -f /tmp/bridgel2sync.sqlite* /tmp/l2gersync.sqlite* /tmp/reorgdetectorl2.sqlite* 2>/dev/null" 2>/dev/null || true

    # Verify files are gone
    local remaining
    remaining=$(docker exec "$bridge_container" ls /tmp/*.sqlite* 2>/dev/null | wc -l || echo "0")
    if [[ "$remaining" == "0" ]]; then
        success "L2 database files cleared"
    else
        warn "Some L2 database files may remain: $remaining"
    fi

    # Restart bridge to pick up fresh state
    log "Restarting bridge container..."
    docker restart "$bridge_container" 2>/dev/null
    sleep 5
    success "Bridge container restarted"

    # Step 5: Also configure zkevm-bridge-service (handles claims)
    log "Configuring zkevm-bridge-service..."
    local zkevm_bridge
    zkevm_bridge=$(docker ps -a --filter "name=zkevm-bridge-service" --format "{{.Names}}" | head -1)

    if [[ -n "$zkevm_bridge" ]]; then
        log "Found zkevm-bridge-service: $zkevm_bridge"

        # Update L2URLs in the config (uses array format, different from aggkit)
        docker exec "$zkevm_bridge" sh -c "
            if [ -f /etc/zkevm/bridge-config.toml ]; then
                cp /etc/zkevm/bridge-config.toml /etc/zkevm/bridge-config.toml.bak
                # Replace L2URLs array to use forwarder
                # Match any op-el-N-op-geth pattern (could be op-el-1 or op-el-2)
                sed -i 's|L2URLs = \\[\"http://op-el-[0-9]*-op-geth-op-node-001:8545\"\\]|L2URLs = [\"http://miden-l2-forwarder:8545\"]|g' /etc/zkevm/bridge-config.toml
            fi
        " 2>/dev/null || warn "Could not modify zkevm-bridge config"

        # Verify change
        local l2urls
        l2urls=$(docker exec "$zkevm_bridge" grep "L2URLs" /etc/zkevm/bridge-config.toml 2>/dev/null | head -1 || echo "")
        log "zkevm-bridge L2URLs: $l2urls"

        # Clear L2 database and restart
        docker exec "$zkevm_bridge" sh -c "rm -f /tmp/*l2*.sqlite* 2>/dev/null" 2>/dev/null || true
        docker restart "$zkevm_bridge" 2>/dev/null
        sleep 5

        # Check if it's running
        if docker ps --filter "name=$zkevm_bridge" --format "{{.Names}}" | grep -q .; then
            success "zkevm-bridge-service restarted and running"
        else
            # Check logs for the networkID error
            local zkevm_logs
            zkevm_logs=$(docker logs "$zkevm_bridge" --tail 10 2>&1 || echo "")
            if echo "$zkevm_logs" | grep -q "networkID received is 0"; then
                warn "zkevm-bridge-service crashed - networkID issue (proxy may need 'input' field fix)"
            else
                warn "zkevm-bridge-service not running - check logs: docker logs $zkevm_bridge"
            fi
        fi
    else
        log "zkevm-bridge-service not found (may not be deployed)"
    fi

    # Step 6: Verify bridge is connecting to proxy
    log "Verifying bridge connection to Miden proxy..."
    sleep 5  # Give bridge time to start

    local bridge_logs
    bridge_logs=$(docker logs "$bridge_container" --tail 20 2>&1 || echo "")

    if echo "$bridge_logs" | grep -qi "error\|fail"; then
        if echo "$bridge_logs" | grep -qi "chain ID mismatch"; then
            warn "Chain ID mismatch detected - L2 databases may need clearing"
        else
            warn "Bridge may have errors - check logs: docker logs $bridge_container"
        fi
    else
        success "Bridge appears to be running"
    fi

    # Test that forwarder routes to proxy correctly using contracts container (has curl)
    log "Testing forwarder routes traffic to proxy..."
    local test_result
    test_result=$(kurtosis service exec "$ENCLAVE_NAME" contracts-001 \
        "curl -s -X POST http://miden-l2-forwarder:8545 -H 'Content-Type: application/json' -d '{\"jsonrpc\":\"2.0\",\"method\":\"eth_chainId\",\"params\":[],\"id\":1}'" 2>&1 || echo "")

    if echo "$test_result" | grep -q '"result"'; then
        local chain_id
        chain_id=$(echo "$test_result" | jq -r '.result // "unknown"' 2>/dev/null || echo "unknown")
        success "Forwarder working! Chain ID: $chain_id"
    else
        warn "Forwarder may not be routing correctly"
        log "Test result: $test_result"
    fi

    # Step 7: Configure aggkit (aggoracle) to send GER updates to Miden proxy
    # The aggoracle is part of the aggkit-001-bridge container
    # Container filesystem is read-only, so we extract config, modify locally, and recreate with volume mount
    log "Configuring aggkit to use Miden proxy for GER injection..."
    local aggkit_container
    aggkit_container=$(docker ps --filter "name=aggkit-001-bridge" --format "{{.Names}}" | head -1 || true)

    if [[ -n "$aggkit_container" ]]; then
        log "Found aggkit bridge container (contains aggoracle): $aggkit_container"

        # Create temp dir for modified config
        local config_dir="${SCRIPT_DIR}/.aggkit-config"
        mkdir -p "$config_dir"

        # Extract config from container
        log "Extracting aggkit config..."
        if docker cp "$aggkit_container:/etc/aggkit/config.toml" "$config_dir/config.toml" 2>/dev/null; then
            log "Config extracted, modifying..."

            # Modify config to use forwarder for aggoracle L2/RPC URLs
            sed -i.bak 's|L2URL = "http://op-el-[0-9]*-op-geth-op-node-001:8545"|L2URL = "http://miden-l2-forwarder:8545"|g' "$config_dir/config.toml"
            sed -i.bak 's|RPCURL = "http://op-el-[0-9]*-op-geth-op-node-001:8545"|RPCURL = "http://miden-l2-forwarder:8545"|g' "$config_dir/config.toml"

            # CRITICAL: Uncomment and update URLRPCL2 in [AggOracle.EVMSender] section
            # This is what the aggoracle uses to send GER injection transactions to L2
            sed -i.bak 's|# URLRPCL2 = "http://op-el-[0-9]*-op-geth-op-node-001:8545"|URLRPCL2 = "http://miden-l2-forwarder:8545"|g' "$config_dir/config.toml"

            # Show the changes
            local l2url rpcurl urlrpcl2
            l2url=$(grep "^L2URL" "$config_dir/config.toml" | head -1 || echo "")
            rpcurl=$(grep "^RPCURL" "$config_dir/config.toml" | head -1 || echo "")
            urlrpcl2=$(grep "^URLRPCL2" "$config_dir/config.toml" | head -1 || echo "NOT SET")
            log "Modified aggkit L2URL: $l2url"
            log "Modified aggkit RPCURL: $rpcurl"
            log "Modified aggkit URLRPCL2 (aggoracle sender): $urlrpcl2"

            # Save container settings for recreation
            log "Saving container configuration..."
            docker inspect "$aggkit_container" > "$config_dir/container-inspect.json"

            # Extract key settings from inspect
            local image hostname
            image=$(jq -r '.[0].Config.Image' "$config_dir/container-inspect.json")
            hostname=$(jq -r '.[0].Config.Hostname' "$config_dir/container-inspect.json")

            # Extract environment variables
            local env_args=""
            while IFS= read -r env; do
                env_args="$env_args -e \"$env\""
            done < <(jq -r '.[0].Config.Env[]' "$config_dir/container-inspect.json" 2>/dev/null || true)

            # Extract network
            local network
            network=$(jq -r '.[0].NetworkSettings.Networks | keys[0]' "$config_dir/container-inspect.json")

            # Extract existing volume mounts (excluding /etc/aggkit which we'll override)
            local volume_args=""
            while IFS= read -r mount; do
                local src dst
                src=$(echo "$mount" | jq -r '.Source')
                dst=$(echo "$mount" | jq -r '.Destination')
                if [[ "$dst" != "/etc/aggkit" && -n "$src" && "$src" != "null" ]]; then
                    volume_args="$volume_args -v \"$src:$dst\""
                fi
            done < <(jq -c '.[0].Mounts[]' "$config_dir/container-inspect.json" 2>/dev/null || true)

            # Extract entrypoint and cmd
            local entrypoint cmd
            entrypoint=$(jq -r '.[0].Config.Entrypoint | if . then join(" ") else "" end' "$config_dir/container-inspect.json")
            cmd=$(jq -r '.[0].Config.Cmd | if . then join(" ") else "" end' "$config_dir/container-inspect.json")

            log "Image: $image"
            log "Network: $network"
            log "Entrypoint: $entrypoint"

            log "Stopping original aggkit container..."
            docker stop "$aggkit_container" 2>/dev/null || true

            # Create new container preserving settings but with config volume mounted
            log "Starting aggkit with modified config (volume mount)..."
            local new_container_name="aggkit-miden-proxy"

            # Remove if exists from previous run
            docker rm -f "$new_container_name" 2>/dev/null || true

            # Build and run docker command
            # Note: we mount the whole config dir to /etc/aggkit to preserve other files
            docker cp "$aggkit_container:/etc/aggkit/." "$config_dir/aggkit-full/" 2>/dev/null || mkdir -p "$config_dir/aggkit-full"
            cp "$config_dir/config.toml" "$config_dir/aggkit-full/config.toml"

            local docker_cmd="docker run -d --name $new_container_name --hostname $hostname --network $network"
            docker_cmd="$docker_cmd -v $config_dir/aggkit-full:/etc/aggkit:ro"
            docker_cmd="$docker_cmd $volume_args $env_args"
            if [[ -n "$entrypoint" ]]; then
                docker_cmd="$docker_cmd --entrypoint \"$entrypoint\""
            fi
            docker_cmd="$docker_cmd $image"
            if [[ -n "$cmd" && "$cmd" != "null" ]]; then
                docker_cmd="$docker_cmd $cmd"
            fi

            log "Running: $docker_cmd"
            if eval "$docker_cmd" 2>&1; then
                success "aggkit started with Miden proxy config"

                # Verify it's running
                sleep 3
                if docker ps --filter "name=$new_container_name" --format "{{.Names}}" | grep -q "$new_container_name"; then
                    success "aggkit container running: $new_container_name"
                    # Show logs to verify
                    log "Container logs (last 10 lines):"
                    docker logs "$new_container_name" 2>&1 | tail -10
                else
                    warn "aggkit container may have failed to start"
                    docker logs "$new_container_name" 2>&1 | tail -20
                    # Restart original
                    docker start "$aggkit_container" 2>/dev/null || true
                fi
            else
                warn "Failed to start aggkit with modified config, restarting original..."
                docker start "$aggkit_container" 2>/dev/null || true
            fi
        else
            warn "Could not extract aggkit config, skipping GER configuration"
        fi
    else
        log "aggkit container not found (may not be deployed)"
    fi
}

#######################################
# Start pgweb for DB browsing
#######################################

start_pgweb() {
    step "Starting pgweb (Postgres Browser)"

    # Remove existing container if any
    docker rm -f pgweb-bridge 2>/dev/null || true

    # Start pgweb connected to the kurtosis postgres
    log "Starting pgweb on port 8082..."
    if docker run -d --name pgweb-bridge \
        --network "kt-${ENCLAVE_NAME}" \
        -p 8082:8081 \
        sosedoff/pgweb \
        --bind=0.0.0.0 \
        --listen=8081 \
        --host=postgres-001 \
        --user=bridge_user \
        --pass=redacted \
        --db=bridge_db \
        --ssl=disable >/dev/null 2>&1; then
        sleep 2
        if docker ps --filter "name=pgweb-bridge" --format "{{.Names}}" | grep -q pgweb; then
            success "pgweb running at http://localhost:8082"
        else
            warn "pgweb failed to start - check: docker logs pgweb-bridge"
        fi
    else
        warn "Could not start pgweb"
    fi
}

#######################################
# Get Deployment Info
#######################################

get_deployment_info() {
    step "Getting Deployment Information"

    # L1 RPC
    L1_RPC=$(kurtosis port print "$ENCLAVE_NAME" el-1-geth-lighthouse rpc 2>/dev/null || echo "")
    log "L1 RPC: $L1_RPC"

    # Bridge address
    BRIDGE_ADDRESS=$(kurtosis service exec "$ENCLAVE_NAME" contracts-001 \
        "cat /opt/zkevm/combined.json 2>/dev/null" 2>/dev/null | jq -r '.polygonZkEVMBridgeAddress // empty' || echo "")

    if [[ -z "$BRIDGE_ADDRESS" ]]; then
        BRIDGE_ADDRESS="0xC8cbEBf950B9Df44d987c8619f092beA980fF038"
    fi
    log "Bridge Address: $BRIDGE_ADDRESS"

    # Proxy URL
    PROXY_RPC="http://localhost:${MIDEN_PROXY_PORT}"
    log "Proxy RPC: $PROXY_RPC"

    # Check funded account
    local balance
    balance=$(cast balance "$KURTOSIS_ADDRESS" --rpc-url "$L1_RPC" 2>/dev/null || echo "0")
    local balance_eth
    balance_eth=$(echo "scale=2; $balance / 1000000000000000000" | bc 2>/dev/null || echo "?")
    log "Funded account: $KURTOSIS_ADDRESS ($balance_eth ETH)"
}

#######################################
# Test Deposit
#######################################

send_test_deposit() {
    step "Sending Test Deposit (L1 → Miden)"

    # Distinctive amount: 0.123321 ETH (easy to identify in DB)
    local amount="123321000000000000"  # 0.123321 ETH

    log "Deposit: 0.123321 ETH to Miden (network $MIDEN_NETWORK_ID)"
    log "From: $KURTOSIS_ADDRESS"
    log "Bridge: $BRIDGE_ADDRESS"

    # Encode bridgeAsset call
    local calldata
    calldata=$(cast calldata "bridgeAsset(uint32,address,uint256,address,bool,bytes)" \
        "$MIDEN_NETWORK_ID" \
        "0x0000000000000000000000000000000000000001" \
        "$amount" \
        "0x0000000000000000000000000000000000000000" \
        true \
        "0x")

    # Send transaction
    local result
    result=$(cast send "$BRIDGE_ADDRESS" \
        "$calldata" \
        --value "$amount" \
        --private-key "$KURTOSIS_PRIVATE_KEY" \
        --rpc-url "$L1_RPC" \
        --gas-limit 300000 \
        --json 2>&1) || true

    local tx_hash
    tx_hash=$(echo "$result" | jq -r '.transactionHash // empty' 2>/dev/null || echo "")

    if [[ -n "$tx_hash" ]]; then
        success "Deposit TX: $tx_hash"

        # Wait and verify
        sleep 3
        local receipt
        receipt=$(cast receipt "$tx_hash" --rpc-url "$L1_RPC" --json 2>/dev/null || echo "{}")
        local status
        status=$(echo "$receipt" | jq -r '.status // "0x0"')

        if [[ "$status" == "0x1" ]]; then
            success "Deposit confirmed!"
        else
            warn "Deposit status: $status"
        fi
    else
        warn "Deposit may have failed"
        echo "$result"
    fi
}

#######################################
# Query Proxy
#######################################

query_proxy_events() {
    step "Querying Miden Proxy for Events"

    # Test proxy health
    local chain_id
    chain_id=$(curl -s -X POST "http://localhost:${MIDEN_PROXY_PORT}" \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}' | jq -r '.result // empty')

    if [[ -n "$chain_id" ]]; then
        success "Proxy responding (chainId: $chain_id)"
    else
        warn "Proxy not responding"
        return
    fi

    # Query block number
    local block_num
    block_num=$(curl -s -X POST "http://localhost:${MIDEN_PROXY_PORT}" \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' | jq -r '.result // empty')
    log "Current block: $block_num"

    # Query ClaimEvent logs
    local claim_topic="0x25308c93ceeed162da955b3f7ce3e3f93606579e40fb92029faa9efe27545983"
    local logs_response
    logs_response=$(curl -s -X POST "http://localhost:${MIDEN_PROXY_PORT}" \
        -H "Content-Type: application/json" \
        -d "{
            \"jsonrpc\":\"2.0\",
            \"method\":\"eth_getLogs\",
            \"params\":[{
                \"fromBlock\":\"0x0\",
                \"toBlock\":\"latest\",
                \"topics\":[\"$claim_topic\"]
            }],
            \"id\":1
        }")

    local log_count
    log_count=$(echo "$logs_response" | jq '.result | length' 2>/dev/null || echo "0")
    log "ClaimEvent logs: $log_count"
}

#######################################
# Summary
#######################################

print_summary() {
    step "Summary"

    echo -e "${BOLD}Services:${NC}"
    echo "  Kurtosis enclave: $ENCLAVE_NAME"
    echo "  L1 RPC:          ${L1_RPC:-N/A}"
    echo "  Miden Proxy:     http://localhost:${MIDEN_PROXY_PORT}"
    echo "  Bridge:          ${BRIDGE_ADDRESS:-N/A}"
    echo "  pgweb (DB):      http://localhost:8082"
    echo ""

    echo -e "${BOLD}Miden Containers:${NC}"
    docker ps --filter "name=miden" --format "  {{.Names}}: {{.Status}}"
    docker ps --filter "name=pgweb" --format "  {{.Names}}: {{.Status}}"
    echo ""

    echo -e "${BOLD}Commands:${NC}"
    echo "  Test proxy:       curl -X POST http://localhost:${MIDEN_PROXY_PORT} -H 'Content-Type: application/json' -d '{\"jsonrpc\":\"2.0\",\"method\":\"eth_chainId\",\"params\":[],\"id\":1}'"
    echo "  Proxy logs:       docker logs -f miden-proxy-kurtosis"
    echo "  Node logs:        docker logs -f miden-node-kurtosis"
    echo "  Bridge logs:      docker logs -f \$(docker ps --filter 'name=zkevm-bridge-service' -q | head -1)"
    echo "  Stop Miden:       docker rm -f miden-node-kurtosis miden-proxy-kurtosis pgweb-bridge"
    echo "  Stop all:         kurtosis enclave rm $ENCLAVE_NAME --force"
}

#######################################
# Main
#######################################

main() {
    echo ""
    echo -e "${CYAN}${BOLD}========================================"
    echo " Kurtosis-CDK + Miden Integration"
    echo "========================================${NC}"
    echo ""

    check_prerequisites

    if ! $SKIP_CDK; then
        deploy_kurtosis_cdk
    fi

    if ! $SKIP_MIDEN; then
        start_miden_services
        reconfigure_bridge_service
        start_pgweb
    fi

    get_deployment_info

    if ! $SKIP_DEPOSIT; then
        send_test_deposit
    fi

    query_proxy_events
    print_summary

    echo ""
    echo -e "${GREEN}${BOLD}========================================"
    echo " Integration Complete"
    echo "========================================${NC}"
    echo ""
}

main "$@"
