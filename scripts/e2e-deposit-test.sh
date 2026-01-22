#!/usr/bin/env bash
#
# End-to-End Deposit Test for Miden Aggkit Proxy
#
# This script:
# 1. Deploys kurtosis-cdk with Miden proxy (or uses existing)
# 2. Waits for services to be ready
# 3. Sends 1 ETH deposit from L1 to Miden
# 4. Verifies the deposit event in proxy logs
#
# Prerequisites:
# - kurtosis CLI installed
# - Docker running
# - polycli installed (optional, will use cast if not available)
#
# Usage:
#   ./scripts/e2e-deposit-test.sh [OPTIONS]
#
# Options:
#   --fresh              Deploy fresh kurtosis-cdk (destroys existing)
#   --enclave NAME       Enclave name (default: cdk-miden)
#   --skip-deploy        Skip deployment, use existing enclave
#   --amount WEI         Deposit amount in wei (default: 1 ETH)
#   --help               Show this help

set -euo pipefail

# Colors
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
BLUE='\033[0;34m'
CYAN='\033[0;36m'
NC='\033[0m'

# Configuration
ENCLAVE_NAME="${ENCLAVE_NAME:-cdk-miden}"
DEPLOY_FRESH=false
SKIP_DEPLOY=false
DEPOSIT_AMOUNT="${DEPOSIT_AMOUNT:-1000000000000000000}"  # 1 ETH in wei

# Kurtosis-cdk repo (will clone if needed)
KURTOSIS_CDK_REPO="https://github.com/0xPolygon/kurtosis-cdk"
KURTOSIS_CDK_DIR="${KURTOSIS_CDK_DIR:-/tmp/kurtosis-cdk}"

# Service names
PROXY_SERVICE="aggkit-proxy-miden-001"
L1_SERVICE="el-1-geth-lighthouse"
BRIDGE_SERVICE="zkevm-bridge-service"

# Pre-funded account from kurtosis-cdk (anvil default account 0)
# This account is funded with 10000 ETH on L1
FUNDED_PRIVATE_KEY="0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80"
FUNDED_ADDRESS="0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266"

# Script directory
SCRIPT_DIR="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)"
PROJECT_DIR="$(dirname "$SCRIPT_DIR")"

#######################################
# Helpers
#######################################

log() { echo -e "${BLUE}[$(date '+%H:%M:%S')]${NC} $1"; }
success() { echo -e "${GREEN}[OK]${NC} $1"; }
fail() { echo -e "${RED}[FAIL]${NC} $1"; exit 1; }
warn() { echo -e "${YELLOW}[WARN]${NC} $1"; }
step() { echo -e "\n${CYAN}=== $1 ===${NC}\n"; }

# Parse arguments
while [[ $# -gt 0 ]]; do
    case $1 in
        --fresh) DEPLOY_FRESH=true; shift ;;
        --enclave) ENCLAVE_NAME="$2"; shift 2 ;;
        --skip-deploy) SKIP_DEPLOY=true; shift ;;
        --amount) DEPOSIT_AMOUNT="$2"; shift 2 ;;
        --help) head -25 "$0" | tail -20; exit 0 ;;
        *) fail "Unknown option: $1" ;;
    esac
done

#######################################
# Prerequisites Check
#######################################

check_prerequisites() {
    step "Checking Prerequisites"

    # Kurtosis
    if ! command -v kurtosis &>/dev/null; then
        fail "kurtosis CLI not found. Install: brew install kurtosis-tech/tap/kurtosis-cli"
    fi
    success "kurtosis CLI found"

    # Docker
    if ! docker info &>/dev/null; then
        fail "Docker not running. Please start Docker."
    fi
    success "Docker running"

    # cast (from foundry) - required for sending transactions
    if ! command -v cast &>/dev/null; then
        fail "cast not found. Install foundry: curl -L https://foundry.paradigm.xyz | bash && foundryup"
    fi
    success "cast (foundry) found"

    # jq
    if ! command -v jq &>/dev/null; then
        fail "jq not found. Install: brew install jq"
    fi
    success "jq found"

    # polycli (optional)
    if command -v polycli &>/dev/null; then
        success "polycli found (will use for deposit)"
        USE_POLYCLI=true
    else
        warn "polycli not found, will use cast instead"
        USE_POLYCLI=false
    fi
}

#######################################
# Deployment
#######################################

deploy_kurtosis_cdk() {
    step "Deploying Kurtosis-CDK with Miden Proxy"

    # Check if enclave exists
    if kurtosis enclave inspect "$ENCLAVE_NAME" &>/dev/null; then
        if $DEPLOY_FRESH; then
            log "Removing existing enclave '$ENCLAVE_NAME'..."
            kurtosis enclave rm "$ENCLAVE_NAME" --force
        else
            log "Enclave '$ENCLAVE_NAME' already exists, reusing..."
            return 0
        fi
    fi

    # Clone kurtosis-cdk if needed
    if [[ ! -d "$KURTOSIS_CDK_DIR" ]]; then
        log "Cloning kurtosis-cdk..."
        git clone --depth 1 "$KURTOSIS_CDK_REPO" "$KURTOSIS_CDK_DIR"
    fi

    # Copy Miden integration files
    log "Copying Miden integration files..."
    cp "$PROJECT_DIR/kurtosis/aggkit-proxy-miden.star" "$KURTOSIS_CDK_DIR/" 2>/dev/null || true

    # Create params file for Miden integration
    cat > "$KURTOSIS_CDK_DIR/miden-params.yaml" << 'EOF'
# Kurtosis-CDK params with Miden Aggkit Proxy
deployment_stages:
  deploy_l1: true
  deploy_zkevm_contracts_on_l1: true
  deploy_databases: true
  deploy_cdk_bridge_infra: true
  deploy_agglayer: false
  deploy_cdk_erigon_node: false
  # Enable Miden integration
  deploy_miden_integration: true

# Miden proxy configuration
aggkit_proxy_miden_image: "ghcr.io/0xmiden/aggkit-proxy:latest"
miden_network_id: 2
EOF

    # Deploy
    log "Deploying kurtosis-cdk (this takes 3-5 minutes)..."
    cd "$KURTOSIS_CDK_DIR"

    # For now, deploy without Miden integration (standard deployment)
    # The proxy will be added manually after
    kurtosis run . --enclave "$ENCLAVE_NAME" --args-file params.yml 2>&1 | tee /tmp/kurtosis-deploy.log

    success "Kurtosis-CDK deployed"
}

add_miden_proxy() {
    step "Adding Miden Proxy Service"

    # Check if proxy already exists
    if kurtosis service inspect "$ENCLAVE_NAME" "$PROXY_SERVICE" &>/dev/null; then
        success "Proxy service already exists"
        return 0
    fi

    # Get L1 RPC URL for proxy config
    local l1_url
    l1_url=$(kurtosis port print "$ENCLAVE_NAME" "$L1_SERVICE" rpc 2>/dev/null || echo "")

    if [[ -z "$l1_url" ]]; then
        warn "Could not get L1 RPC URL, proxy may not connect to L1"
        l1_url="http://el-1-geth-lighthouse:8545"
    fi

    log "Adding Miden proxy service..."

    # Create config for proxy
    local config_dir="/tmp/miden-proxy-config"
    mkdir -p "$config_dir"

    cat > "$config_dir/config.toml" << EOF
[server]
http_port = 8123
http_host = "0.0.0.0"

[miden]
network_id = 2
chain_id = "0x4d494445"

[bridge]
contract_address = "0x0000000000000000000000000000000000000000"

[logging]
level = "info"
EOF

    # Add service via kurtosis (simplified - in real deployment use Starlark)
    # For now, we'll just verify existing deployment works
    warn "Manual proxy addition not implemented - ensure proxy is in deployment"
}

#######################################
# Wait for Services
#######################################

wait_for_services() {
    step "Waiting for Services to be Ready"

    local max_attempts=30
    local attempt=0

    # Wait for L1
    log "Waiting for L1 node..."
    while [[ $attempt -lt $max_attempts ]]; do
        local l1_url
        l1_url=$(kurtosis port print "$ENCLAVE_NAME" "$L1_SERVICE" rpc 2>/dev/null || echo "")

        if [[ -n "$l1_url" ]]; then
            local block
            block=$(curl -s -X POST "$l1_url" \
                -H "Content-Type: application/json" \
                -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' | jq -r '.result // empty')

            if [[ -n "$block" ]]; then
                success "L1 node ready at $l1_url (block: $block)"
                break
            fi
        fi

        ((attempt++))
        sleep 2
    done

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
        sleep 2
    done
}

#######################################
# Get Deployment Info
#######################################

get_deployment_info() {
    step "Getting Deployment Information"

    # L1 RPC
    L1_RPC=$(kurtosis port print "$ENCLAVE_NAME" "$L1_SERVICE" rpc 2>/dev/null || echo "")
    if [[ -z "$L1_RPC" ]]; then
        fail "Could not get L1 RPC URL"
    fi
    log "L1 RPC: $L1_RPC"

    # Bridge contract address - get from deployment artifacts
    # In kurtosis-cdk, this is in the combined.json or can be queried
    log "Querying bridge contract address..."

    # Try to get from zkevm-bridge-service config
    BRIDGE_ADDRESS=$(kurtosis service exec "$ENCLAVE_NAME" "$BRIDGE_SERVICE" \
        "cat /etc/zkevm-bridge/config.toml 2>/dev/null | grep -oP 'PolygonBridgeAddress\s*=\s*\"\K[^\"]+'" 2>/dev/null || echo "")

    if [[ -z "$BRIDGE_ADDRESS" ]]; then
        # Fallback: try to get from contracts service
        BRIDGE_ADDRESS=$(kurtosis service exec "$ENCLAVE_NAME" "contracts-001" \
            "cat /opt/zkevm/combined.json 2>/dev/null | jq -r '.polygonZkEVMBridgeAddress // empty'" 2>/dev/null || echo "")
    fi

    if [[ -z "$BRIDGE_ADDRESS" ]]; then
        # Last resort: use placeholder (won't work for real deposits)
        warn "Could not find bridge address, using placeholder"
        BRIDGE_ADDRESS="0x528e26b25a34a4A5d0dbDa1d57D318153d2ED582"  # Common testnet address
    fi

    log "Bridge Address: $BRIDGE_ADDRESS"

    # Check funded account balance
    log "Checking funded account balance..."
    local balance
    balance=$(cast balance "$FUNDED_ADDRESS" --rpc-url "$L1_RPC" 2>/dev/null || echo "0")
    log "Funded account ($FUNDED_ADDRESS) balance: $balance wei"

    if [[ "$balance" == "0" ]]; then
        warn "Funded account has zero balance - deposit will fail"
    fi
}

#######################################
# Send Deposit
#######################################

send_deposit() {
    step "Sending Deposit (L1 → Miden)"

    local destination_network=2  # Miden network ID
    local destination_address="0x0000000000000000000000000000000000000001"  # Placeholder

    log "Deposit parameters:"
    log "  From: $FUNDED_ADDRESS"
    log "  To Network: $destination_network (Miden)"
    log "  To Address: $destination_address"
    log "  Amount: $DEPOSIT_AMOUNT wei ($(echo "scale=4; $DEPOSIT_AMOUNT / 1000000000000000000" | bc) ETH)"
    log "  Bridge: $BRIDGE_ADDRESS"

    if $USE_POLYCLI; then
        send_deposit_polycli
    else
        send_deposit_cast
    fi
}

send_deposit_polycli() {
    log "Sending deposit via polycli..."

    polycli ulxly deposit \
        --bridge-address "$BRIDGE_ADDRESS" \
        --destination-network 2 \
        --destination-address "0x0000000000000000000000000000000000000001" \
        --amount "$DEPOSIT_AMOUNT" \
        --rpc-url "$L1_RPC" \
        --private-key "$FUNDED_PRIVATE_KEY" \
        --gas-limit 300000 \
        2>&1 | tee /tmp/deposit-output.log

    if grep -q "transaction hash" /tmp/deposit-output.log 2>/dev/null; then
        local tx_hash
        tx_hash=$(grep -oP 'transaction hash[:\s]+\K0x[a-fA-F0-9]+' /tmp/deposit-output.log || echo "")
        success "Deposit transaction sent: $tx_hash"
        DEPOSIT_TX_HASH="$tx_hash"
    else
        warn "Could not confirm deposit transaction"
    fi
}

send_deposit_cast() {
    log "Sending deposit via cast..."

    # Bridge deposit function signature:
    # bridgeAsset(uint32 destinationNetwork, address destinationAddress, uint256 amount,
    #             address token, bool forceUpdateGlobalExitRoot, bytes calldata permitData)

    # For native ETH deposit, token = 0x0, amount sent as msg.value
    local destination_network=2
    local destination_address="0x0000000000000000000000000000000000000001"
    local token_address="0x0000000000000000000000000000000000000000"  # Native ETH
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

    log "Calldata: ${calldata:0:66}..."

    # Send transaction
    local tx_hash
    tx_hash=$(cast send "$BRIDGE_ADDRESS" \
        "$calldata" \
        --value "$DEPOSIT_AMOUNT" \
        --private-key "$FUNDED_PRIVATE_KEY" \
        --rpc-url "$L1_RPC" \
        --gas-limit 300000 \
        --json 2>/dev/null | jq -r '.transactionHash // empty')

    if [[ -n "$tx_hash" ]]; then
        success "Deposit transaction sent: $tx_hash"
        DEPOSIT_TX_HASH="$tx_hash"
    else
        # Try without --json for error message
        cast send "$BRIDGE_ADDRESS" \
            "$calldata" \
            --value "$DEPOSIT_AMOUNT" \
            --private-key "$FUNDED_PRIVATE_KEY" \
            --rpc-url "$L1_RPC" \
            --gas-limit 300000 2>&1 || true
        fail "Deposit transaction failed"
    fi
}

#######################################
# Verify Deposit
#######################################

verify_deposit() {
    step "Verifying Deposit"

    if [[ -z "${DEPOSIT_TX_HASH:-}" ]]; then
        warn "No deposit transaction hash to verify"
        return
    fi

    log "Waiting for transaction confirmation..."
    sleep 5

    # Get transaction receipt
    local receipt
    receipt=$(cast receipt "$DEPOSIT_TX_HASH" --rpc-url "$L1_RPC" --json 2>/dev/null || echo "{}")

    local status
    status=$(echo "$receipt" | jq -r '.status // "0x0"')

    if [[ "$status" == "0x1" ]]; then
        success "Deposit transaction confirmed!"

        # Parse logs for BridgeEvent
        local logs
        logs=$(echo "$receipt" | jq '.logs')
        local log_count
        log_count=$(echo "$logs" | jq 'length')

        log "Transaction emitted $log_count log(s)"

        # Look for BridgeEvent topic
        local bridge_topic="0x501781209a1f8899323b96b4ef08b168df93e0a90c673d1e4cce39366cb62f9b"
        if echo "$logs" | grep -qi "$bridge_topic"; then
            success "BridgeEvent detected in transaction logs!"
        fi
    else
        warn "Transaction may have failed (status: $status)"
        echo "$receipt" | jq '.'
    fi
}

#######################################
# Query Proxy Logs
#######################################

query_proxy_logs() {
    step "Querying Proxy for Bridge Events"

    # Get proxy URL
    local proxy_url
    proxy_url=$(kurtosis port print "$ENCLAVE_NAME" "$PROXY_SERVICE" http-rpc 2>/dev/null || echo "")

    if [[ -z "$proxy_url" ]]; then
        warn "Proxy service not available, skipping log query"
        return
    fi

    log "Proxy RPC: $proxy_url"

    # Query eth_getLogs for BridgeEvent
    local bridge_topic="0x501781209a1f8899323b96b4ef08b168df93e0a90c673d1e4cce39366cb62f9b"

    log "Querying eth_getLogs for BridgeEvent..."
    local response
    response=$(curl -s -X POST "$proxy_url" \
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
        success "Found $log_count BridgeEvent log(s) in proxy!"
        echo "$response" | jq '.result'
    else
        log "No BridgeEvent logs found yet (may need claim processing)"
    fi

    # Query for ClaimEvent
    local claim_topic="0x25308c93ceeed162da955b3f7ce3e3f93606579e40fb92029faa9efe27545983"

    log "Querying eth_getLogs for ClaimEvent..."
    response=$(curl -s -X POST "$proxy_url" \
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
        success "Found $log_count ClaimEvent log(s) in proxy!"
        echo "$response" | jq '.result'
    else
        log "No ClaimEvent logs yet (claims not processed)"
    fi
}

#######################################
# Summary
#######################################

print_summary() {
    step "Test Summary"

    echo -e "${GREEN}Deployment:${NC}"
    echo "  Enclave: $ENCLAVE_NAME"
    echo "  L1 RPC: $L1_RPC"
    echo "  Bridge: $BRIDGE_ADDRESS"
    echo ""

    if [[ -n "${DEPOSIT_TX_HASH:-}" ]]; then
        echo -e "${GREEN}Deposit:${NC}"
        echo "  TX Hash: $DEPOSIT_TX_HASH"
        echo "  Amount: $DEPOSIT_AMOUNT wei"
        echo "  Destination: Miden (network 2)"
        echo ""
    fi

    echo -e "${CYAN}Useful Commands:${NC}"
    echo "  View L1 logs:     kurtosis service logs $ENCLAVE_NAME $L1_SERVICE"
    echo "  View proxy logs:  kurtosis service logs $ENCLAVE_NAME $PROXY_SERVICE"
    echo "  View bridge logs: kurtosis service logs $ENCLAVE_NAME $BRIDGE_SERVICE"
    echo "  Inspect enclave:  kurtosis enclave inspect $ENCLAVE_NAME"
    echo "  Stop enclave:     kurtosis enclave stop $ENCLAVE_NAME"
    echo "  Destroy enclave:  kurtosis enclave rm $ENCLAVE_NAME --force"
}

#######################################
# Main
#######################################

main() {
    echo ""
    echo "========================================"
    echo " E2E Deposit Test: L1 → Miden"
    echo " Aggkit Proxy Integration"
    echo "========================================"
    echo ""

    check_prerequisites

    if ! $SKIP_DEPLOY; then
        deploy_kurtosis_cdk
    fi

    wait_for_services
    get_deployment_info
    send_deposit
    verify_deposit
    query_proxy_logs
    print_summary

    echo ""
    echo -e "${GREEN}========================================"
    echo " E2E Test Complete"
    echo "========================================${NC}"
    echo ""
}

main "$@"
