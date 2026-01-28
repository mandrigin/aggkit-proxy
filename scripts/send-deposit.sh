#!/usr/bin/env bash
#
# Send a test deposit from L1 to Miden (network 2)
#
# Usage:
#   ./scripts/send-deposit.sh [amount_in_eth] [destination_miden_address]
#
# Example:
#   ./scripts/send-deposit.sh 0.1
#   ./scripts/send-deposit.sh 0.1 0x112233445566778899aabbccddeeff  # Send to specific Miden address
#
# The destination can be:
#   - A Miden AccountId (15 bytes / 30 hex chars): 0x112233...
#   - Omitted to send to the sender's address
#
# Miden addresses are converted to Eth format by prepending 5 zero bytes:
#   Miden: 0x112233445566778899aabbccddeeff
#   Eth:   0x0000000000112233445566778899aabbccddeeff
#

set -euo pipefail

# ============================================================================
# Address conversion utilities (Miden <-> Eth)
# ============================================================================

# Convert Miden AccountId (15 bytes / 30 hex chars) to Ethereum address (20 bytes / 40 hex chars)
# Pads with 5 bytes (10 hex chars) of leading zeros
miden_to_eth() {
    local miden_addr="$1"
    # Remove 0x prefix if present
    miden_addr="${miden_addr#0x}"
    # Validate length (should be 30 hex chars)
    if [[ ${#miden_addr} -ne 30 ]]; then
        echo "ERROR: Miden address must be 30 hex chars (15 bytes), got ${#miden_addr}" >&2
        return 1
    fi
    # Miden is 30 hex chars, Eth is 40 hex chars, so pad with 10 zeros
    echo "0x0000000000${miden_addr}"
}

# ============================================================================
# Parse arguments
# ============================================================================

# Default amount: 0.01 ETH
AMOUNT_ETH="${1:-0.01}"

# Destination: optional Miden address
DEST_MIDEN="${2:-}"

# Convert to wei
AMOUNT_WEI=$(echo "$AMOUNT_ETH * 1000000000000000000" | bc | cut -d'.' -f1)

# Kurtosis funded account
PRIVATE_KEY="0x12d7de8621a77640c9241b2595ba78ce443d05e94090365ab3bb5e19df82c625"
FROM_ADDRESS="0xE34aaF64b29273B7D567FCFc40544c014EEe9970"

# Destination address: use provided Miden address or fall back to sender
if [[ -n "$DEST_MIDEN" ]]; then
    DEST_ADDRESS=$(miden_to_eth "$DEST_MIDEN") || exit 1
    echo "Destination: $DEST_MIDEN (Miden) -> $DEST_ADDRESS (Eth)"
else
    DEST_ADDRESS="$FROM_ADDRESS"
    echo "Destination: $FROM_ADDRESS (sender's address)"
fi

# Bridge contract address - get from kurtosis or use override
BRIDGE_ADDRESS="${BRIDGE_ADDRESS:-}"
if [[ -z "$BRIDGE_ADDRESS" ]]; then
    # Get from kurtosis combined.json
    BRIDGE_ADDRESS=$(kurtosis service exec miden-cdk contracts-001 "cat /opt/output/combined.json" 2>/dev/null | jq -r '.polygonZkEVMBridgeAddress // empty')
fi
if [[ -z "$BRIDGE_ADDRESS" ]]; then
    echo "ERROR: Cannot determine bridge address"
    echo "Set BRIDGE_ADDRESS env var or ensure kurtosis miden-cdk is running"
    exit 1
fi

# Destination network (Miden = 2)
DEST_NETWORK=2

# Get L1 RPC - try multiple methods
L1_RPC="${L1_RPC:-}"
if [[ -z "$L1_RPC" ]]; then
    # Try kurtosis
    L1_RPC=$(kurtosis port print miden-cdk el-1-geth-lighthouse rpc 2>/dev/null || true)
fi
if [[ -z "$L1_RPC" ]]; then
    # Try to find the container directly
    L1_CONTAINER=$(docker ps --filter "name=el-1-geth" --format "{{.Names}}" | head -1)
    if [[ -n "$L1_CONTAINER" ]]; then
        L1_PORT=$(docker port "$L1_CONTAINER" 8545 2>/dev/null | cut -d: -f2 || true)
        if [[ -n "$L1_PORT" ]]; then
            L1_RPC="http://localhost:$L1_PORT"
        fi
    fi
fi
if [[ -z "$L1_RPC" ]]; then
    echo "ERROR: Cannot find L1 RPC endpoint"
    echo ""
    echo "Options:"
    echo "  1. Set L1_RPC environment variable: L1_RPC=http://... $0"
    echo "  2. Deploy kurtosis-cdk: ./scripts/e2e-test.sh"
    echo "  3. Start existing enclave: kurtosis enclave start miden-cdk"
    exit 1
fi

echo "=== Miden Deposit Script ==="
echo "L1 RPC:      $L1_RPC"
echo "From:        $FROM_ADDRESS"
echo "To (Eth):    $DEST_ADDRESS"
if [[ -n "$DEST_MIDEN" ]]; then
echo "To (Miden):  $DEST_MIDEN"
fi
echo "Bridge:      $BRIDGE_ADDRESS"
echo "Dest Net:    $DEST_NETWORK"
echo "Amount:      $AMOUNT_ETH ETH ($AMOUNT_WEI wei)"
echo ""

# Check balance
BALANCE=$(cast balance "$FROM_ADDRESS" --rpc-url "$L1_RPC" 2>/dev/null || echo "0")
BALANCE_ETH=$(echo "scale=4; $BALANCE / 1000000000000000000" | bc 2>/dev/null || echo "?")
echo "Balance:     $BALANCE_ETH ETH"

if [[ "$BALANCE" == "0" ]]; then
    echo "ERROR: No balance in funded account"
    exit 1
fi

# Encode bridgeAsset call
# bridgeAsset(uint32 destinationNetwork, address destinationAddress, uint256 amount, address token, bool forceUpdateGlobalExitRoot, bytes permitData)
CALLDATA=$(cast calldata "bridgeAsset(uint32,address,uint256,address,bool,bytes)" \
    "$DEST_NETWORK" \
    "$DEST_ADDRESS" \
    "$AMOUNT_WEI" \
    "0x0000000000000000000000000000000000000000" \
    true \
    "0x")

echo "Calldata:    ${CALLDATA:0:20}..."
echo ""
echo "Sending deposit transaction..."

# Send transaction
RESULT=$(cast send "$BRIDGE_ADDRESS" \
    "$CALLDATA" \
    --value "$AMOUNT_WEI" \
    --private-key "$PRIVATE_KEY" \
    --rpc-url "$L1_RPC" \
    --gas-limit 300000 \
    --json 2>&1) || {
    echo "ERROR: Transaction failed"
    echo "$RESULT"
    exit 1
}

TX_HASH=$(echo "$RESULT" | jq -r '.transactionHash // empty')

if [[ -n "$TX_HASH" ]]; then
    echo "✓ Transaction sent: $TX_HASH"

    # Wait for confirmation
    sleep 2
    RECEIPT=$(cast receipt "$TX_HASH" --rpc-url "$L1_RPC" --json 2>/dev/null || echo "{}")
    STATUS=$(echo "$RECEIPT" | jq -r '.status // "0x0"')

    if [[ "$STATUS" == "0x1" ]]; then
        BLOCK_NUM=$(echo "$RECEIPT" | jq -r '.blockNumber // "unknown"')
        # Convert hex to decimal
        BLOCK_DEC=$((BLOCK_NUM))
        echo "✓ Transaction confirmed at block $BLOCK_DEC"

        # Get deposit count from logs
        LOGS=$(echo "$RECEIPT" | jq -r '.logs')
        echo ""
        echo "Check deposit in bridge DB:"
        echo "  docker exec \$(docker ps --filter 'name=postgres' -q) psql -U bridge_user -d bridge_db -c \"SELECT id, deposit_cnt, dest_net, ready_for_claim FROM sync.deposit WHERE dest_net = 2 ORDER BY id DESC LIMIT 5;\""
    else
        echo "⚠ Transaction status: $STATUS"
    fi
else
    echo "ERROR: No transaction hash returned"
    echo "$RESULT"
fi
