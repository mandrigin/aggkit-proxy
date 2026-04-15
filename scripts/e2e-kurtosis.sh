#!/usr/bin/env bash
#
# Miden RPC Proxy End-to-End Test (Kurtosis)
#
# This script runs the e2e test using the miden-cdk Kurtosis package.
# It replaces the manual post-provisioning workflow in e2e-test.sh.
#
# Usage:
#   ./scripts/e2e-kurtosis.sh [OPTIONS]
#
# Options:
#   --fresh              Deploy fresh (destroys existing enclave)
#   --enclave NAME       Enclave name (default: miden-cdk)
#   --skip-deposit       Skip test deposit
#   --params FILE        Custom params file (default: kurtosis/miden-cdk/params.yaml)
#   --help               Show this help
#
# Prerequisites:
#   - kurtosis CLI: https://docs.kurtosis.com/install
#   - Docker running
#   - Miden images built:
#       docker build -t miden-infra/miden-node:v0.14.6 -f Dockerfile.miden-node .
#       docker build https://github.com/gateway-fm/miden-agglayer.git#release/0.1 -t miden-infra/miden-proxy:release-0.1

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
ENCLAVE_NAME="${ENCLAVE_NAME:-miden-cdk}"
DEPLOY_FRESH=false
SKIP_DEPOSIT=false
PARAMS_FILE=""

# Script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"
KURTOSIS_PKG_DIR="$PROJECT_DIR/kurtosis/miden-cdk"

# Funded accounts (from kurtosis-cdk defaults)
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
        --skip-deposit) SKIP_DEPOSIT=true; shift ;;
        --params) PARAMS_FILE="$2"; shift 2 ;;
        --help) head -20 "$0" | tail -15; exit 0 ;;
        *) fail "Unknown option: $1" ;;
    esac
done

#######################################
# Prerequisites
#######################################

check_prerequisites() {
    step "Checking Prerequisites"

    if ! command -v kurtosis &>/dev/null; then
        fail "kurtosis not found. Install from: https://docs.kurtosis.com/install"
    fi
    success "kurtosis installed"

    if ! docker info &>/dev/null; then
        fail "Docker not running. Start Docker Desktop or docker daemon."
    fi
    success "Docker running"

    if ! docker image inspect miden-infra/miden-node:v0.14.6 &>/dev/null; then
        fail "miden-infra/miden-node:v0.14.6 image not found. Build it first."
    fi
    success "Miden node image found"

    if ! docker image inspect miden-infra/miden-proxy:release-0.1 &>/dev/null; then
        fail "miden-infra/miden-proxy:release-0.1 image not found. Build from miden-agglayer:
    docker build https://github.com/gateway-fm/miden-agglayer.git#release/0.1 -t miden-infra/miden-proxy:release-0.1"
    fi
    success "Miden proxy image found (miden-agglayer)"

    if ! command -v cast &>/dev/null; then
        warn "foundry (cast) not found - deposit test will be skipped"
        SKIP_DEPOSIT=true
    else
        success "foundry (cast) installed"
    fi
}

#######################################
# Deploy with Kurtosis
#######################################

deploy_miden_cdk() {
    step "Deploying Miden-CDK with Kurtosis"

    if kurtosis enclave inspect "$ENCLAVE_NAME" &>/dev/null; then
        if $DEPLOY_FRESH; then
            log "Removing existing enclave..."
            kurtosis enclave rm "$ENCLAVE_NAME" --force
        else
            success "Enclave '$ENCLAVE_NAME' exists, reusing"
            return 0
        fi
    fi

    # Determine params file
    local params_arg=""
    if [[ -n "$PARAMS_FILE" ]]; then
        params_arg="--args-file $PARAMS_FILE"
    elif [[ -f "$KURTOSIS_PKG_DIR/params.yaml" ]]; then
        params_arg="--args-file $KURTOSIS_PKG_DIR/params.yaml"
    fi

    log "Deploying miden-cdk package..."
    cd "$KURTOSIS_PKG_DIR"
    kurtosis run . --enclave "$ENCLAVE_NAME" $params_arg
    cd - >/dev/null

    success "Miden-CDK deployed"
}

#######################################
# Fix Proposer Mismatch
#######################################

fix_proposer() {
    step "Fixing AggLayer Proposer"

    # The aggkit uses 0x0b68058E... as its signing key (from aggoracle.keystore).
    # The AggLayer and L1 rollup contract default to a different address.
    # Fix both so certificate submission works.
    local AGGKIT_ADDR="0x0b68058E5b2592b1f472AdFe106305295A332A7C"
    local ROLLUP_ADDR="0x414e9E227e4b589aF92200508aF5399576530E4e"

    # 1. Update L1 trustedSequencer
    if [[ -n "$L1_RPC" ]]; then
        cast send "$ROLLUP_ADDR" "setTrustedSequencer(address)" "$AGGKIT_ADDR" \
            --private-key "$KURTOSIS_PRIVATE_KEY" --rpc-url "$L1_RPC" --gas-limit 100000 --json > /dev/null 2>&1 \
            && success "L1 trustedSequencer → $AGGKIT_ADDR" \
            || warn "Failed to update L1 trustedSequencer"
    fi

    # 2. Update AggLayer proof-signers config (only the proof-signers line, not all addresses)
    kurtosis service exec "$ENCLAVE_NAME" agglayer \
        "sed -i '/\[proof-signers\]/{n;s|\"0x[a-fA-F0-9]*\"|\"'$AGGKIT_ADDR'\"|;}' /etc/agglayer/config.toml" 2>/dev/null \
        && success "AggLayer proof-signers → $AGGKIT_ADDR" \
        || warn "Failed to update AggLayer config"

    # 3. Update AggLayer full-node-rpcs to point to our proxy
    kurtosis service exec "$ENCLAVE_NAME" agglayer \
        "sed -i 's|http://cdk-erigon-rpc-001:8545|http://miden-l2-forwarder-001:8545|' /etc/agglayer/config.toml" 2>/dev/null \
        && success "AggLayer full-node-rpcs → miden forwarder" \
        || warn "Failed to update AggLayer RPC endpoint"

    # 4. Restart agglayer + aggkit
    kurtosis service stop "$ENCLAVE_NAME" agglayer > /dev/null 2>&1
    sleep 2
    kurtosis service start "$ENCLAVE_NAME" agglayer > /dev/null 2>&1
    kurtosis service stop "$ENCLAVE_NAME" aggkit-001 > /dev/null 2>&1
    sleep 2
    kurtosis service start "$ENCLAVE_NAME" aggkit-001 > /dev/null 2>&1
    success "Restarted agglayer + aggkit"
}

#######################################
# Get Service URLs
#######################################

get_service_urls() {
    step "Getting Service URLs"

    # Get L1 RPC
    L1_RPC=$(kurtosis port print "$ENCLAVE_NAME" el-1-geth-lighthouse rpc 2>/dev/null || echo "")
    if [[ -z "$L1_RPC" ]]; then
        # Try alternative name
        L1_RPC=$(kurtosis port print "$ENCLAVE_NAME" el-1-geth-lighthouse-001 rpc 2>/dev/null || echo "")
    fi
    log "L1 RPC: ${L1_RPC:-NOT FOUND}"

    # Get Miden proxy port
    PROXY_RPC=$(kurtosis port print "$ENCLAVE_NAME" miden-proxy-001 rpc 2>/dev/null || echo "")
    log "Miden Proxy RPC: ${PROXY_RPC:-NOT FOUND}"

    # Get bridge address from contracts service
    BRIDGE_ADDRESS=$(kurtosis service exec "$ENCLAVE_NAME" contracts-001 \
        "cat /opt/output/combined.json 2>/dev/null" 2>/dev/null | jq -r '.polygonZkEVMBridgeAddress // empty' || echo "")
    if [[ -z "$BRIDGE_ADDRESS" ]]; then
        BRIDGE_ADDRESS="0xC8cbEBf950B9Df44d987c8619f092beA980fF038"
    fi
    log "Bridge Address: $BRIDGE_ADDRESS"
}

#######################################
# Test Proxy
#######################################

test_proxy() {
    step "Testing Miden Proxy"

    if [[ -z "$PROXY_RPC" ]]; then
        warn "Proxy URL not found, skipping test"
        return
    fi

    local chain_id
    chain_id=$(curl -s -X POST "$PROXY_RPC" \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}' | jq -r '.result // empty')

    if [[ -n "$chain_id" ]]; then
        success "Proxy responding (chainId: $chain_id)"
    else
        warn "Proxy not responding"
    fi

    local block_num
    block_num=$(curl -s -X POST "$PROXY_RPC" \
        -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' | jq -r '.result // empty')
    log "Current block: ${block_num:-N/A}"
}

#######################################
# Test Deposit
#######################################

send_test_deposit() {
    step "Sending Test Deposit (L1 → Miden)"

    if [[ -z "$L1_RPC" ]] || [[ -z "$BRIDGE_ADDRESS" ]]; then
        warn "Missing L1 RPC or bridge address, skipping deposit"
        return
    fi

    # Distinctive amount: 0.1 ETH (scaled to 8 decimals for Miden)
    local amount="100000000000000000"  # 0.1 ETH

    log "Deposit: 0.1 ETH to Miden (network 1)"
    log "From: $KURTOSIS_ADDRESS"
    log "Bridge: $BRIDGE_ADDRESS"

    # Encode bridgeAsset call.
    # dest_net=1: the rollup's l2NetworkID from rollup manager (rollupID), NOT the
    # agglayer network ID (which is 2 in our setup). The bridge service's
    # claimtxman calls UpdateL1DepositsStatus(ctx, ger.ExitRoots[0], tm.l2NetworkID, ...)
    # where l2NetworkID is the rollupID. Depositing with dest_net=2 leaves the deposit
    # stuck at ready_for_claim=false because the UPDATE's `dest_net = $2` clause never matches.
    # destination_address uses the claimable address format.
    local calldata
    calldata=$(cast calldata "bridgeAsset(uint32,address,uint256,address,bool,bytes)" \
        1 \
        "0x00000000a417929a101b89100dda63bf4f692800" \
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
# Summary
#######################################

print_summary() {
    step "Summary"

    echo -e "${BOLD}Services:${NC}"
    echo "  Kurtosis enclave: $ENCLAVE_NAME"
    echo "  L1 RPC:          ${L1_RPC:-N/A}"
    echo "  Miden Proxy:     ${PROXY_RPC:-N/A}"
    echo "  Bridge:          ${BRIDGE_ADDRESS:-N/A}"
    echo ""

    echo -e "${BOLD}Useful Commands:${NC}"
    echo "  View services:    kurtosis enclave inspect $ENCLAVE_NAME"
    echo "  Proxy logs:       kurtosis service logs $ENCLAVE_NAME miden-proxy-001"
    echo "  Node logs:        kurtosis service logs $ENCLAVE_NAME miden-node-001"
    echo "  Bridge logs:      kurtosis service logs $ENCLAVE_NAME zkevm-bridge-service-001"
    echo "  Stop all:         kurtosis enclave rm $ENCLAVE_NAME --force"
    echo ""

    if [[ -n "$PROXY_RPC" ]]; then
        echo -e "${BOLD}Test Proxy:${NC}"
        echo "  curl -X POST $PROXY_RPC -H 'Content-Type: application/json' -d '{\"jsonrpc\":\"2.0\",\"method\":\"eth_chainId\",\"params\":[],\"id\":1}'"
    fi
}

#######################################
# Main
#######################################

main() {
    echo ""
    echo -e "${CYAN}${BOLD}========================================"
    echo " Miden-CDK E2E Test (Kurtosis)"
    echo "========================================${NC}"
    echo ""

    check_prerequisites
    deploy_miden_cdk
    get_service_urls
    test_proxy
    fix_proposer

    if ! $SKIP_DEPOSIT; then
        send_test_deposit
    fi

    print_summary

    echo ""
    echo -e "${GREEN}${BOLD}========================================"
    echo " E2E Test Complete"
    echo "========================================${NC}"
    echo ""
}

main "$@"
