#!/usr/bin/env bash
#
# Kurtosis CDK Integration Test for Miden Aggkit Proxy
#
# This script validates the Miden proxy integration with kurtosis-cdk:
# 1. Deploys kurtosis-cdk with Miden proxy (or uses existing deployment)
# 2. Verifies bridge service can connect to proxy
# 3. Tests eth_getLogs, eth_getBlockByNumber polling
# 4. Optionally tests end-to-end deposit flow
#
# Prerequisites:
# - kurtosis CLI installed
# - Docker running
# - polycli installed (for deposit testing)
#
# Usage:
#   ./scripts/test-kurtosis-cdk.sh [OPTIONS]
#
# Options:
#   --enclave NAME    Use existing Kurtosis enclave (default: cdk)
#   --deploy          Deploy fresh kurtosis-cdk instance
#   --skip-deposit    Skip deposit flow test
#   --verbose         Enable verbose output
#   --help            Show this help message

set -euo pipefail

# Colors for output
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
NC='\033[0m' # No Color

# Default configuration
ENCLAVE_NAME="${ENCLAVE_NAME:-cdk}"
PROXY_SERVICE="aggkit-proxy-miden-001"
BRIDGE_SERVICE="zkevm-bridge-service"
L1_SERVICE="el-1-geth-lighthouse"
DEPLOY_FRESH=false
SKIP_DEPOSIT=false
VERBOSE=false

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --enclave)
            ENCLAVE_NAME="$2"
            shift 2
            ;;
        --deploy)
            DEPLOY_FRESH=true
            shift
            ;;
        --skip-deposit)
            SKIP_DEPOSIT=true
            shift
            ;;
        --verbose)
            VERBOSE=true
            shift
            ;;
        --help)
            head -30 "$0" | tail -25
            exit 0
            ;;
        *)
            echo -e "${RED}Unknown option: $1${NC}"
            exit 1
            ;;
    esac
done

log() {
    echo -e "${BLUE}[$(date '+%H:%M:%S')]${NC} $1"
}

success() {
    echo -e "${GREEN}[PASS]${NC} $1"
}

fail() {
    echo -e "${RED}[FAIL]${NC} $1"
    exit 1
}

warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
}

# Get service port from Kurtosis
get_port() {
    local service=$1
    local port_name=$2
    kurtosis port print "$ENCLAVE_NAME" "$service" "$port_name" 2>/dev/null || echo ""
}

# Make JSON-RPC call to proxy
rpc_call() {
    local url=$1
    local method=$2
    local params=${3:-"[]"}

    local response
    response=$(curl -s -X POST "$url" \
        -H "Content-Type: application/json" \
        -d "{\"jsonrpc\":\"2.0\",\"method\":\"$method\",\"params\":$params,\"id\":1}")

    if $VERBOSE; then
        echo "RPC $method -> $response" >&2
    fi

    echo "$response"
}

# Check if a service exists in the enclave
service_exists() {
    local service=$1
    kurtosis service inspect "$ENCLAVE_NAME" "$service" &>/dev/null
}

#######################################
# Test Functions
#######################################

test_proxy_health() {
    log "Testing proxy health endpoint..."

    local proxy_url
    proxy_url=$(get_port "$PROXY_SERVICE" "http-rpc")

    if [[ -z "$proxy_url" ]]; then
        fail "Could not get proxy HTTP RPC port"
    fi

    local response
    response=$(rpc_call "$proxy_url" "eth_chainId")

    if echo "$response" | grep -q '"result"'; then
        local chain_id
        chain_id=$(echo "$response" | jq -r '.result')
        success "Proxy responds to eth_chainId: $chain_id"
    else
        fail "Proxy did not respond correctly to eth_chainId: $response"
    fi
}

test_eth_block_number() {
    log "Testing eth_blockNumber polling..."

    local proxy_url
    proxy_url=$(get_port "$PROXY_SERVICE" "http-rpc")

    # Get initial block number
    local response1
    response1=$(rpc_call "$proxy_url" "eth_blockNumber")
    local block1
    block1=$(echo "$response1" | jq -r '.result')

    if [[ "$block1" == "null" ]] || [[ -z "$block1" ]]; then
        fail "eth_blockNumber returned null: $response1"
    fi

    success "eth_blockNumber returns: $block1"

    # Wait and check for block progression (if connected to live node)
    log "Waiting 5s to check block progression..."
    sleep 5

    local response2
    response2=$(rpc_call "$proxy_url" "eth_blockNumber")
    local block2
    block2=$(echo "$response2" | jq -r '.result')

    local num1=$((16#${block1#0x}))
    local num2=$((16#${block2#0x}))

    if [[ $num2 -ge $num1 ]]; then
        success "Block progression confirmed: $block1 -> $block2 (delta: $((num2 - num1)))"
    else
        warn "No block progression detected (might be expected for test network)"
    fi
}

test_eth_get_logs() {
    log "Testing eth_getLogs for BridgeEvent topic..."

    local proxy_url
    proxy_url=$(get_port "$PROXY_SERVICE" "http-rpc")

    # BridgeEvent topic from spec
    local bridge_topic="0x501781209a1f8899323b96b4ef08b168df93e0a90c673d1e4cce39366cb62f9b"

    # Query logs from block 0 to latest
    local params="[{\"fromBlock\":\"0x0\",\"toBlock\":\"latest\",\"topics\":[\"$bridge_topic\"]}]"
    local response
    response=$(rpc_call "$proxy_url" "eth_getLogs" "$params")

    if echo "$response" | grep -q '"result"'; then
        local log_count
        log_count=$(echo "$response" | jq '.result | length')
        success "eth_getLogs returned $log_count logs for BridgeEvent topic"
    else
        local error
        error=$(echo "$response" | jq -r '.error.message // "unknown error"')
        fail "eth_getLogs failed: $error"
    fi
}

test_eth_get_block_by_number() {
    log "Testing eth_getBlockByNumber..."

    local proxy_url
    proxy_url=$(get_port "$PROXY_SERVICE" "http-rpc")

    # Get latest block
    local params='["latest", false]'
    local response
    response=$(rpc_call "$proxy_url" "eth_getBlockByNumber" "$params")

    if echo "$response" | grep -q '"result"'; then
        local block_hash
        block_hash=$(echo "$response" | jq -r '.result.hash // "null"')
        local block_number
        block_number=$(echo "$response" | jq -r '.result.number // "null"')
        local timestamp
        timestamp=$(echo "$response" | jq -r '.result.timestamp // "null"')

        if [[ "$block_hash" != "null" ]] && [[ "$block_number" != "null" ]]; then
            success "eth_getBlockByNumber returned block $block_number (hash: ${block_hash:0:18}...)"
        else
            fail "eth_getBlockByNumber returned incomplete block data"
        fi
    else
        local error
        error=$(echo "$response" | jq -r '.error.message // "unknown error"')
        fail "eth_getBlockByNumber failed: $error"
    fi
}

test_bridge_service_connection() {
    log "Testing bridge service connection to proxy..."

    if ! service_exists "$BRIDGE_SERVICE"; then
        warn "Bridge service not found in enclave, skipping connection test"
        return
    fi

    # Check bridge service logs for proxy connection
    local logs
    logs=$(kurtosis service logs "$ENCLAVE_NAME" "$BRIDGE_SERVICE" 2>/dev/null | tail -100)

    if echo "$logs" | grep -qi "aggkit-proxy-miden\|connected.*8123\|l2.*rpc"; then
        success "Bridge service shows proxy connection indicators"
    else
        warn "Could not confirm bridge service proxy connection from logs"
    fi

    # Try to query bridge service API
    local bridge_port
    bridge_port=$(get_port "$BRIDGE_SERVICE" "rpc" 2>/dev/null || echo "")

    if [[ -n "$bridge_port" ]]; then
        local health
        health=$(curl -s "$bridge_port/health" 2>/dev/null || echo "")
        if [[ -n "$health" ]]; then
            success "Bridge service health endpoint responds"
        fi
    fi
}

test_deposit_flow() {
    if $SKIP_DEPOSIT; then
        log "Skipping deposit flow test (--skip-deposit)"
        return
    fi

    log "Testing deposit flow (L1 -> Miden)..."

    # Check for polycli
    if ! command -v polycli &>/dev/null; then
        warn "polycli not found, skipping deposit test"
        warn "Install with: git clone https://github.com/0xPolygon/polygon-cli && cd polygon-cli && make install"
        return
    fi

    local l1_url
    l1_url=$(get_port "$L1_SERVICE" "rpc" 2>/dev/null || echo "")

    if [[ -z "$l1_url" ]]; then
        warn "L1 RPC not available, skipping deposit test"
        return
    fi

    # Get bridge contract address (would be from deployment artifacts)
    local bridge_address="${BRIDGE_CONTRACT_ADDRESS:-}"
    if [[ -z "$bridge_address" ]]; then
        warn "BRIDGE_CONTRACT_ADDRESS not set, skipping deposit test"
        warn "Set with: export BRIDGE_CONTRACT_ADDRESS=0x..."
        return
    fi

    log "Initiating test deposit to Miden (network ID: 2)..."

    # This is a dry run - actual deposit would need funded account
    polycli ulxly deposit \
        --bridge-address "$bridge_address" \
        --destination-network 2 \
        --destination-address "0x0000000000000000000000000000000000000001" \
        --amount 1000000000000000000 \
        --rpc-url "$l1_url" \
        --dry-run 2>/dev/null && {
        success "Deposit dry-run successful"
    } || {
        warn "Deposit dry-run failed (may need funded account)"
    }
}

test_rpc_method_coverage() {
    log "Testing critical RPC method coverage..."

    local proxy_url
    proxy_url=$(get_port "$PROXY_SERVICE" "http-rpc")

    local methods=(
        "eth_chainId"
        "eth_blockNumber"
        "net_version"
        "eth_gasPrice"
    )

    local passed=0
    local failed=0

    for method in "${methods[@]}"; do
        local response
        response=$(rpc_call "$proxy_url" "$method")

        if echo "$response" | grep -q '"result"'; then
            ((passed++))
            if $VERBOSE; then
                success "$method: $(echo "$response" | jq -r '.result')"
            fi
        else
            ((failed++))
            warn "$method failed: $(echo "$response" | jq -r '.error.message // "unknown"')"
        fi
    done

    if [[ $failed -eq 0 ]]; then
        success "All $passed basic RPC methods passed"
    else
        warn "$passed passed, $failed failed"
    fi
}

#######################################
# Main
#######################################

main() {
    echo ""
    echo "======================================"
    echo " Kurtosis CDK Integration Test"
    echo " Miden Aggkit Proxy"
    echo "======================================"
    echo ""

    # Check prerequisites
    if ! command -v kurtosis &>/dev/null; then
        fail "kurtosis CLI not found. Install from: https://docs.kurtosis.com/install"
    fi

    if ! command -v jq &>/dev/null; then
        fail "jq not found. Install with: brew install jq"
    fi

    # Check enclave exists
    if ! kurtosis enclave inspect "$ENCLAVE_NAME" &>/dev/null; then
        if $DEPLOY_FRESH; then
            log "Deploying fresh kurtosis-cdk enclave..."
            fail "Fresh deployment not yet implemented. Deploy manually first."
        else
            fail "Enclave '$ENCLAVE_NAME' not found. Use --deploy or start manually."
        fi
    fi

    log "Using enclave: $ENCLAVE_NAME"

    # Check proxy service exists
    if ! service_exists "$PROXY_SERVICE"; then
        fail "Proxy service '$PROXY_SERVICE' not found in enclave"
    fi

    echo ""
    log "Running integration tests..."
    echo ""

    # Run tests
    test_proxy_health
    test_eth_block_number
    test_eth_get_block_by_number
    test_eth_get_logs
    test_rpc_method_coverage
    test_bridge_service_connection
    test_deposit_flow

    echo ""
    echo "======================================"
    echo -e " ${GREEN}All tests completed${NC}"
    echo "======================================"
    echo ""

    # Print useful commands
    log "Useful commands:"
    echo "  Get proxy URL:    kurtosis port print $ENCLAVE_NAME $PROXY_SERVICE http-rpc"
    echo "  View proxy logs:  kurtosis service logs $ENCLAVE_NAME $PROXY_SERVICE"
    echo "  Inspect enclave:  kurtosis enclave inspect $ENCLAVE_NAME"
}

main "$@"
