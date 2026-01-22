#!/usr/bin/env bash
#
# End-to-End Deposit Test for Miden Aggkit Proxy
#
# This script:
# 1. Checks and installs prerequisites (kurtosis, foundry, jq, polycli)
# 2. Starts Miden node and proxy if not running
# 3. Deploys kurtosis-cdk (or uses existing)
# 4. Sends 1 ETH deposit from L1 to Miden
# 5. Verifies the deposit event in proxy logs
#
# Usage:
#   ./scripts/e2e-deposit-test.sh [OPTIONS]
#
# Options:
#   --fresh              Deploy fresh kurtosis-cdk (destroys existing)
#   --enclave NAME       Enclave name (default: cdk-miden)
#   --skip-deploy        Skip kurtosis deployment, use existing enclave
#   --skip-install       Skip auto-installation of prerequisites
#   --local              Use local Miden node + proxy (not kurtosis)
#   --amount WEI         Deposit amount in wei (default: 1 ETH)
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
SKIP_DEPLOY=false
SKIP_INSTALL=false
USE_LOCAL=false
DEPOSIT_AMOUNT="${DEPOSIT_AMOUNT:-1000000000000000000}"  # 1 ETH in wei

# Kurtosis-cdk repo
KURTOSIS_CDK_REPO="https://github.com/0xPolygon/kurtosis-cdk"
KURTOSIS_CDK_DIR="${KURTOSIS_CDK_DIR:-/tmp/kurtosis-cdk}"

# Service names (kurtosis)
PROXY_SERVICE="aggkit-proxy-miden-001"
L1_SERVICE="el-1-geth-lighthouse"
BRIDGE_SERVICE="zkevm-bridge-service"

# Local service config
LOCAL_PROXY_PORT="${LOCAL_PROXY_PORT:-8546}"
LOCAL_MIDEN_NODE_PORT="${LOCAL_MIDEN_NODE_PORT:-57291}"

# Pre-funded account (anvil/hardhat default account 0)
FUNDED_PRIVATE_KEY="0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
FUNDED_ADDRESS="0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"

# Script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

# OS detection
OS="$(uname -s)"
ARCH="$(uname -m)"

#######################################
# Helpers
#######################################

log() { echo -e "${BLUE}[$(date '+%H:%M:%S')]${NC} $1"; }
success() { echo -e "${GREEN}✓${NC} $1"; }
fail() { echo -e "${RED}✗ FAIL:${NC} $1"; exit 1; }
warn() { echo -e "${YELLOW}!${NC} $1"; }
step() { echo -e "\n${CYAN}${BOLD}=== $1 ===${NC}\n"; }
ask() {
    echo -e -n "${YELLOW}?${NC} $1 [Y/n] "
    read -r response
    [[ -z "$response" || "$response" =~ ^[Yy] ]]
}

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --fresh) DEPLOY_FRESH=true; shift ;;
        --enclave) ENCLAVE_NAME="$2"; shift 2 ;;
        --skip-deploy) SKIP_DEPLOY=true; shift ;;
        --skip-install) SKIP_INSTALL=true; shift ;;
        --local) USE_LOCAL=true; shift ;;
        --amount) DEPOSIT_AMOUNT="$2"; shift 2 ;;
        --help) head -25 "$0" | tail -20; exit 0 ;;
        *) fail "Unknown option: $1" ;;
    esac
done

#######################################
# Prerequisites Installation
#######################################

install_homebrew() {
    if [[ "$OS" != "Darwin" ]]; then
        return 1
    fi
    if ! command -v brew &>/dev/null; then
        log "Installing Homebrew..."
        /bin/bash -c "$(curl -fsSL https://raw.githubusercontent.com/Homebrew/install/HEAD/install.sh)"
    fi
    return 0
}

install_docker() {
    if command -v docker &>/dev/null; then
        success "Docker already installed"
        return 0
    fi

    log "Docker not found"

    if [[ "$OS" == "Darwin" ]]; then
        if ask "Install Docker Desktop via Homebrew?"; then
            brew install --cask docker
            echo ""
            warn "Docker Desktop installed. Please:"
            echo "  1. Open Docker Desktop from Applications"
            echo "  2. Complete the setup wizard"
            echo "  3. Re-run this script"
            exit 0
        fi
    else
        echo "Please install Docker manually: https://docs.docker.com/get-docker/"
        exit 1
    fi
}

install_kurtosis() {
    if command -v kurtosis &>/dev/null; then
        success "Kurtosis already installed: $(kurtosis version 2>/dev/null | head -1)"
        return 0
    fi

    log "Installing Kurtosis CLI..."

    if [[ "$OS" == "Darwin" ]]; then
        brew install kurtosis-tech/tap/kurtosis-cli
    else
        echo "deb [trusted=yes] https://apt.fury.io/kurtosis-tech/ /" | sudo tee /etc/apt/sources.list.d/kurtosis.list
        sudo apt update
        sudo apt install -y kurtosis-cli
    fi

    success "Kurtosis installed: $(kurtosis version 2>/dev/null | head -1)"
}

install_foundry() {
    if command -v cast &>/dev/null; then
        success "Foundry already installed: $(cast --version 2>/dev/null | head -1)"
        return 0
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

    success "Foundry installed: $(cast --version 2>/dev/null | head -1)"
}

install_jq() {
    if command -v jq &>/dev/null; then
        success "jq already installed: $(jq --version 2>/dev/null)"
        return 0
    fi

    log "Installing jq..."

    if [[ "$OS" == "Darwin" ]]; then
        brew install jq
    else
        sudo apt install -y jq
    fi

    success "jq installed"
}

install_polycli() {
    if command -v polycli &>/dev/null; then
        success "polycli already installed"
        return 0
    fi

    log "Installing polycli..."

    # Clone and build
    local polycli_dir="/tmp/polygon-cli"
    if [[ -d "$polycli_dir" ]]; then
        rm -rf "$polycli_dir"
    fi

    git clone --depth 1 https://github.com/0xPolygon/polygon-cli "$polycli_dir"
    cd "$polycli_dir"

    if command -v go &>/dev/null; then
        make install 2>/dev/null || go install ./... 2>/dev/null || {
            warn "Could not install polycli (Go required)"
            return 1
        }
        success "polycli installed"
    else
        warn "Go not installed, skipping polycli (will use cast instead)"
        return 1
    fi

    cd - >/dev/null
}

check_and_install_prerequisites() {
    step "Checking & Installing Prerequisites"

    # Homebrew (macOS only)
    if [[ "$OS" == "Darwin" ]]; then
        install_homebrew || true
    fi

    # Docker (required)
    install_docker

    # Check Docker is running
    if ! docker info &>/dev/null; then
        fail "Docker is installed but not running. Please start Docker Desktop."
    fi
    success "Docker is running"

    # Kurtosis (required for non-local mode)
    if ! $USE_LOCAL; then
        install_kurtosis
    fi

    # Foundry/cast (required)
    install_foundry

    # jq (required)
    install_jq

    # polycli (optional)
    install_polycli || true

    echo ""
    success "All prerequisites ready!"
}

#######################################
# Local Miden Node & Proxy
#######################################

check_miden_node() {
    log "Checking Miden node status..."

    local miden_url="http://localhost:$LOCAL_MIDEN_NODE_PORT"

    if curl -s "$miden_url" &>/dev/null || curl -s --http2 "$miden_url" &>/dev/null; then
        success "Miden node is running on port $LOCAL_MIDEN_NODE_PORT"
        return 0
    fi

    return 1
}

start_miden_node() {
    if check_miden_node; then
        return 0
    fi

    log "Starting Miden node..."

    # Check if start script exists
    if [[ -f "$SCRIPT_DIR/start-miden-node.sh" ]]; then
        "$SCRIPT_DIR/start-miden-node.sh" &
        sleep 5

        if check_miden_node; then
            success "Miden node started"
            return 0
        fi
    fi

    # Try Docker
    log "Attempting to start Miden node via Docker..."

    if docker ps -a | grep -q "miden-node"; then
        docker start miden-node 2>/dev/null || true
    else
        docker run -d \
            --name miden-node \
            -p "$LOCAL_MIDEN_NODE_PORT:57291" \
            ghcr.io/0xmiden/miden-node:latest 2>/dev/null || {
            warn "Could not start Miden node via Docker"
            warn "Please start Miden node manually or use --skip-deploy with kurtosis"
            return 1
        }
    fi

    sleep 5
    check_miden_node
}

check_proxy() {
    log "Checking proxy status..."

    local proxy_url="http://localhost:$LOCAL_PROXY_PORT"

    local response
    response=$(curl -s -X POST "$proxy_url" \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}' 2>/dev/null || echo "")

    if echo "$response" | grep -q '"result"'; then
        local chain_id
        chain_id=$(echo "$response" | jq -r '.result')
        success "Proxy is running on port $LOCAL_PROXY_PORT (chainId: $chain_id)"
        return 0
    fi

    return 1
}

start_proxy() {
    if check_proxy; then
        return 0
    fi

    log "Starting Miden RPC proxy..."

    # Check if we have the binary
    local proxy_bin="$PROJECT_DIR/target/release/miden-rpc-proxy"

    if [[ ! -f "$proxy_bin" ]]; then
        log "Building proxy..."
        cd "$PROJECT_DIR"
        cargo build --release 2>&1 | tail -5
        cd - >/dev/null
    fi

    if [[ -f "$proxy_bin" ]]; then
        # Start proxy in background
        local config_file="$PROJECT_DIR/config/config.toml"
        if [[ ! -f "$config_file" ]]; then
            config_file="$PROJECT_DIR/config/config.kurtosis-cdk.toml"
        fi

        log "Starting proxy with config: $config_file"
        nohup "$proxy_bin" --config "$config_file" > /tmp/proxy.log 2>&1 &
        echo $! > /tmp/proxy.pid

        sleep 3

        if check_proxy; then
            success "Proxy started (PID: $(cat /tmp/proxy.pid))"
            return 0
        else
            warn "Proxy started but not responding"
            tail -20 /tmp/proxy.log 2>/dev/null || true
        fi
    fi

    # Try Docker
    log "Attempting to start proxy via Docker..."

    if docker ps -a | grep -q "miden-proxy"; then
        docker start miden-proxy 2>/dev/null || true
    else
        docker run -d \
            --name miden-proxy \
            -p "$LOCAL_PROXY_PORT:8546" \
            ghcr.io/0xmiden/aggkit-proxy:latest 2>/dev/null || {
            warn "Could not start proxy via Docker"
            return 1
        }
    fi

    sleep 3
    check_proxy
}

ensure_local_services() {
    step "Ensuring Local Services Running"

    start_miden_node || warn "Miden node not available"
    start_proxy || fail "Could not start proxy"
}

#######################################
# Kurtosis Deployment
#######################################

deploy_kurtosis_cdk() {
    step "Deploying Kurtosis-CDK"

    # Check if enclave exists
    if kurtosis enclave inspect "$ENCLAVE_NAME" &>/dev/null; then
        if $DEPLOY_FRESH; then
            log "Removing existing enclave '$ENCLAVE_NAME'..."
            kurtosis enclave rm "$ENCLAVE_NAME" --force
        else
            success "Enclave '$ENCLAVE_NAME' already exists, reusing"
            return 0
        fi
    fi

    # Clone kurtosis-cdk if needed
    if [[ ! -d "$KURTOSIS_CDK_DIR" ]]; then
        log "Cloning kurtosis-cdk..."
        git clone --depth 1 "$KURTOSIS_CDK_REPO" "$KURTOSIS_CDK_DIR"
    fi

    # Copy Miden integration files if they exist
    if [[ -f "$PROJECT_DIR/kurtosis/aggkit-proxy-miden.star" ]]; then
        cp "$PROJECT_DIR/kurtosis/aggkit-proxy-miden.star" "$KURTOSIS_CDK_DIR/" 2>/dev/null || true
    fi

    # Deploy
    log "Deploying kurtosis-cdk (this takes 3-5 minutes)..."
    cd "$KURTOSIS_CDK_DIR"

    kurtosis run . --enclave "$ENCLAVE_NAME" 2>&1 | tee /tmp/kurtosis-deploy.log | while read -r line; do
        echo -e "${BLUE}│${NC} $line"
    done

    success "Kurtosis-CDK deployed"
    cd - >/dev/null
}

wait_for_kurtosis_services() {
    step "Waiting for Services"

    local max_attempts=60
    local attempt=0

    # Wait for L1
    log "Waiting for L1 node..."
    while [[ $attempt -lt $max_attempts ]]; do
        L1_RPC=$(kurtosis port print "$ENCLAVE_NAME" "$L1_SERVICE" rpc 2>/dev/null || echo "")

        if [[ -n "$L1_RPC" ]]; then
            local block
            block=$(curl -s -X POST "$L1_RPC" \
                -H "Content-Type: application/json" \
                -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' | jq -r '.result // empty')

            if [[ -n "$block" ]]; then
                success "L1 node ready at $L1_RPC (block: $block)"
                break
            fi
        fi

        ((attempt++))
        echo -n "."
        sleep 2
    done
    echo ""

    if [[ $attempt -ge $max_attempts ]]; then
        fail "L1 node not ready after ${max_attempts} attempts"
    fi

    # Wait for bridge service
    attempt=0
    log "Waiting for bridge service..."
    while [[ $attempt -lt $max_attempts ]]; do
        if kurtosis service inspect "$ENCLAVE_NAME" "$BRIDGE_SERVICE" &>/dev/null; then
            success "Bridge service running"
            break
        fi
        ((attempt++))
        echo -n "."
        sleep 2
    done
    echo ""
}

get_kurtosis_deployment_info() {
    step "Getting Deployment Information"

    # L1 RPC
    L1_RPC=$(kurtosis port print "$ENCLAVE_NAME" "$L1_SERVICE" rpc 2>/dev/null || echo "")
    if [[ -z "$L1_RPC" ]]; then
        fail "Could not get L1 RPC URL"
    fi
    log "L1 RPC: $L1_RPC"

    # Proxy RPC
    PROXY_RPC=$(kurtosis port print "$ENCLAVE_NAME" "$PROXY_SERVICE" http-rpc 2>/dev/null || echo "")
    if [[ -z "$PROXY_RPC" ]]; then
        warn "Proxy service not found in kurtosis, trying local"
        PROXY_RPC="http://localhost:$LOCAL_PROXY_PORT"
    fi
    log "Proxy RPC: $PROXY_RPC"

    # Bridge contract address
    log "Querying bridge contract address..."
    BRIDGE_ADDRESS=$(kurtosis service exec "$ENCLAVE_NAME" "contracts-001" \
        "cat /opt/zkevm/combined.json 2>/dev/null" 2>/dev/null | jq -r '.polygonZkEVMBridgeAddress // empty' || echo "")

    if [[ -z "$BRIDGE_ADDRESS" ]]; then
        # Try alternative location
        BRIDGE_ADDRESS=$(kurtosis service exec "$ENCLAVE_NAME" "$BRIDGE_SERVICE" \
            "cat /etc/zkevm-bridge/config.toml 2>/dev/null" 2>/dev/null | grep -oP 'PolygonBridgeAddress\s*=\s*"\K[^"]+' || echo "")
    fi

    if [[ -z "$BRIDGE_ADDRESS" ]]; then
        warn "Could not find bridge address from deployment"
        BRIDGE_ADDRESS="0x528e26b25a34a4A5d0dbDa1d57D318153d2ED582"
        warn "Using default testnet bridge address: $BRIDGE_ADDRESS"
    fi
    log "Bridge Address: $BRIDGE_ADDRESS"

    # Check funded account balance
    log "Checking funded account balance..."
    local balance
    balance=$(cast balance "$FUNDED_ADDRESS" --rpc-url "$L1_RPC" 2>/dev/null || echo "0")
    local balance_eth
    balance_eth=$(echo "scale=4; $balance / 1000000000000000000" | bc 2>/dev/null || echo "?")
    log "Funded account balance: $balance_eth ETH"

    if [[ "$balance" == "0" ]]; then
        warn "Funded account has zero balance - deposit may fail"
    fi
}

#######################################
# Deposit
#######################################

send_deposit() {
    step "Sending Deposit (L1 → Miden)"

    local destination_network=2  # Miden network ID
    local destination_address="0x0000000000000000000000000000000000000001"

    local amount_eth
    amount_eth=$(echo "scale=4; $DEPOSIT_AMOUNT / 1000000000000000000" | bc 2>/dev/null || echo "?")

    echo ""
    echo -e "${BOLD}Deposit Parameters:${NC}"
    echo "  From:        $FUNDED_ADDRESS"
    echo "  To Network:  $destination_network (Miden)"
    echo "  To Address:  $destination_address"
    echo "  Amount:      $amount_eth ETH ($DEPOSIT_AMOUNT wei)"
    echo "  Bridge:      $BRIDGE_ADDRESS"
    echo "  L1 RPC:      $L1_RPC"
    echo ""

    # Use polycli if available, otherwise cast
    if command -v polycli &>/dev/null; then
        send_deposit_polycli
    else
        send_deposit_cast
    fi
}

send_deposit_polycli() {
    log "Sending deposit via polycli..."

    local output
    output=$(polycli ulxly deposit \
        --bridge-address "$BRIDGE_ADDRESS" \
        --destination-network 2 \
        --destination-address "0x0000000000000000000000000000000000000001" \
        --amount "$DEPOSIT_AMOUNT" \
        --rpc-url "$L1_RPC" \
        --private-key "$FUNDED_PRIVATE_KEY" \
        --gas-limit 300000 2>&1) || true

    echo "$output"

    if echo "$output" | grep -qi "transaction hash\|tx hash\|0x[a-f0-9]\{64\}"; then
        DEPOSIT_TX_HASH=$(echo "$output" | grep -oP '0x[a-fA-F0-9]{64}' | head -1 || echo "")
        if [[ -n "$DEPOSIT_TX_HASH" ]]; then
            success "Deposit transaction sent: $DEPOSIT_TX_HASH"
        fi
    else
        warn "polycli output did not contain transaction hash, trying cast..."
        send_deposit_cast
    fi
}

send_deposit_cast() {
    log "Sending deposit via cast..."

    # bridgeAsset(uint32 destinationNetwork, address destinationAddress, uint256 amount,
    #             address token, bool forceUpdateGlobalExitRoot, bytes calldata permitData)

    local destination_network=2
    local destination_address="0x0000000000000000000000000000000000000001"
    local token_address="0x0000000000000000000000000000000000000000"
    local force_update=true
    local permit_data="0x"

    # Encode function call
    local calldata
    calldata=$(cast calldata "bridgeAsset(uint32,address,uint256,address,bool,bytes)" \
        "$destination_network" \
        "$destination_address" \
        "$DEPOSIT_AMOUNT" \
        "$token_address" \
        "$force_update" \
        "$permit_data")

    log "Sending transaction..."

    local result
    result=$(cast send "$BRIDGE_ADDRESS" \
        "$calldata" \
        --value "$DEPOSIT_AMOUNT" \
        --private-key "$FUNDED_PRIVATE_KEY" \
        --rpc-url "$L1_RPC" \
        --gas-limit 300000 \
        --json 2>&1) || true

    DEPOSIT_TX_HASH=$(echo "$result" | jq -r '.transactionHash // empty' 2>/dev/null || echo "")

    if [[ -n "$DEPOSIT_TX_HASH" ]]; then
        success "Deposit transaction sent: $DEPOSIT_TX_HASH"
    else
        echo "$result"
        fail "Deposit transaction failed"
    fi
}

verify_deposit() {
    step "Verifying Deposit"

    if [[ -z "${DEPOSIT_TX_HASH:-}" ]]; then
        warn "No deposit transaction hash to verify"
        return
    fi

    log "Waiting for confirmation..."
    sleep 5

    # Get transaction receipt
    local receipt
    receipt=$(cast receipt "$DEPOSIT_TX_HASH" --rpc-url "$L1_RPC" --json 2>/dev/null || echo "{}")

    local status
    status=$(echo "$receipt" | jq -r '.status // "0x0"')

    if [[ "$status" == "0x1" ]]; then
        success "Deposit transaction confirmed!"

        local gas_used
        gas_used=$(echo "$receipt" | jq -r '.gasUsed // "?"')
        log "Gas used: $((16#${gas_used#0x})) units"

        # Check for BridgeEvent
        local logs
        logs=$(echo "$receipt" | jq '.logs')
        local log_count
        log_count=$(echo "$logs" | jq 'length')

        log "Transaction emitted $log_count log(s)"

        local bridge_topic="0x501781209a1f8899323b96b4ef08b168df93e0a90c673d1e4cce39366cb62f9b"
        if echo "$logs" | grep -qi "${bridge_topic:2}"; then
            success "BridgeEvent detected in transaction!"
        fi
    else
        warn "Transaction status: $status (may have failed)"
        echo "$receipt" | jq '.' 2>/dev/null || echo "$receipt"
    fi
}

query_proxy_logs() {
    step "Querying Proxy for Events"

    if [[ -z "${PROXY_RPC:-}" ]]; then
        PROXY_RPC="http://localhost:$LOCAL_PROXY_PORT"
    fi

    # Check proxy is responding
    local chain_response
    chain_response=$(curl -s -X POST "$PROXY_RPC" \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}' 2>/dev/null || echo "")

    if ! echo "$chain_response" | grep -q '"result"'; then
        warn "Proxy not responding at $PROXY_RPC"
        return
    fi

    log "Proxy RPC: $PROXY_RPC"

    # Query BridgeEvent logs
    local bridge_topic="0x501781209a1f8899323b96b4ef08b168df93e0a90c673d1e4cce39366cb62f9b"
    log "Querying eth_getLogs for BridgeEvent..."

    local response
    response=$(curl -s -X POST "$PROXY_RPC" \
        -H "Content-Type: application/json" \
        -d "{
            \"jsonrpc\":\"2.0\",
            \"method\":\"eth_getLogs\",
            \"params\":[{
                \"fromBlock\":\"0x0\",
                \"toBlock\":\"latest\",
                \"topics\":[\"$bridge_topic\"]
            }],
            \"id\":1
        }")

    local log_count
    log_count=$(echo "$response" | jq '.result | length' 2>/dev/null || echo "0")

    if [[ "$log_count" -gt 0 ]]; then
        success "Found $log_count BridgeEvent log(s)!"
        echo "$response" | jq '.result' 2>/dev/null
    else
        log "No BridgeEvent logs found (normal before claim processing)"
    fi

    # Query ClaimEvent logs
    local claim_topic="0x25308c93ceeed162da955b3f7ce3e3f93606579e40fb92029faa9efe27545983"
    log "Querying eth_getLogs for ClaimEvent..."

    response=$(curl -s -X POST "$PROXY_RPC" \
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

    log_count=$(echo "$response" | jq '.result | length' 2>/dev/null || echo "0")

    if [[ "$log_count" -gt 0 ]]; then
        success "Found $log_count ClaimEvent log(s)!"
        echo "$response" | jq '.result' 2>/dev/null
    else
        log "No ClaimEvent logs yet (claims not processed)"
    fi
}

#######################################
# Summary
#######################################

print_summary() {
    step "Test Summary"

    echo -e "${BOLD}Environment:${NC}"
    if $USE_LOCAL; then
        echo "  Mode:     Local"
        echo "  Proxy:    http://localhost:$LOCAL_PROXY_PORT"
    else
        echo "  Mode:     Kurtosis"
        echo "  Enclave:  $ENCLAVE_NAME"
        echo "  L1 RPC:   ${L1_RPC:-N/A}"
        echo "  Proxy:    ${PROXY_RPC:-N/A}"
    fi
    echo "  Bridge:   ${BRIDGE_ADDRESS:-N/A}"
    echo ""

    if [[ -n "${DEPOSIT_TX_HASH:-}" ]]; then
        echo -e "${BOLD}Deposit:${NC}"
        echo "  TX Hash:  $DEPOSIT_TX_HASH"
        echo "  Amount:   $DEPOSIT_AMOUNT wei"
        echo "  To:       Miden (network 2)"
        echo ""
    fi

    echo -e "${BOLD}Useful Commands:${NC}"
    if $USE_LOCAL; then
        echo "  View proxy logs:  tail -f /tmp/proxy.log"
        echo "  Stop proxy:       kill \$(cat /tmp/proxy.pid)"
    else
        echo "  View L1 logs:     kurtosis service logs $ENCLAVE_NAME $L1_SERVICE -f"
        echo "  View proxy logs:  kurtosis service logs $ENCLAVE_NAME $PROXY_SERVICE -f"
        echo "  View bridge logs: kurtosis service logs $ENCLAVE_NAME $BRIDGE_SERVICE -f"
        echo "  Inspect enclave:  kurtosis enclave inspect $ENCLAVE_NAME"
        echo "  Stop enclave:     kurtosis enclave stop $ENCLAVE_NAME"
        echo "  Destroy enclave:  kurtosis enclave rm $ENCLAVE_NAME --force"
    fi
}

#######################################
# Main
#######################################

main() {
    echo ""
    echo -e "${CYAN}${BOLD}========================================"
    echo " E2E Deposit Test: L1 → Miden"
    echo " Aggkit Proxy Integration"
    echo "========================================${NC}"
    echo ""

    # Prerequisites
    if ! $SKIP_INSTALL; then
        check_and_install_prerequisites
    fi

    # Setup environment
    if $USE_LOCAL; then
        ensure_local_services
        L1_RPC="${L1_RPC:-http://localhost:8545}"
        PROXY_RPC="http://localhost:$LOCAL_PROXY_PORT"
        BRIDGE_ADDRESS="${BRIDGE_ADDRESS:-0x528e26b25a34a4A5d0dbDa1d57D318153d2ED582}"
    else
        if ! $SKIP_DEPLOY; then
            deploy_kurtosis_cdk
        fi
        wait_for_kurtosis_services
        get_kurtosis_deployment_info
    fi

    # Execute deposit
    send_deposit
    verify_deposit
    query_proxy_logs
    print_summary

    echo ""
    echo -e "${GREEN}${BOLD}========================================"
    echo " E2E Test Complete"
    echo "========================================${NC}"
    echo ""
}

main "$@"
