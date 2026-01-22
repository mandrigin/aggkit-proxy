#!/usr/bin/env bash
#
# Send a test deposit from L1 to Miden (network 2)
#
# Usage:
#   ./scripts/send-deposit.sh [amount_in_eth]
#
# Example:
#   ./scripts/send-deposit.sh 0.1
#

set -euo pipefail

# Default amount: 0.01 ETH
AMOUNT_ETH="${1:-0.01}"

# Convert to wei
AMOUNT_WEI=$(echo "$AMOUNT_ETH * 1000000000000000000" | bc | cut -d'.' -f1)

# Kurtosis funded account
PRIVATE_KEY="0x12d7de8621a77640c9241b2595ba78ce443d05e94090365ab3bb5e19df82c625"
FROM_ADDRESS="0xE34aaF64b29273B7D567FCFc40544c014EEe9970"

# Bridge contract address
BRIDGE_ADDRESS="0xD71f8F956AD979Cc2988381B8A743a2fE280537D"

# Destination network (Miden = 2)
DEST_NETWORK=2

# Get L1 RPC - try multiple methods
L1_RPC="${L1_RPC:-}"
if [[ -z "$L1_RPC" ]]; then
    # Try kurtosis
    L1_RPC=$(kurtosis port print cdk-miden el-1-geth-lighthouse rpc 2>/dev/null || true)
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
    echo "  3. Start existing enclave: kurtosis enclave start cdk-miden"
    exit 1
fi

echo "=== Miden Deposit Script ==="
echo "L1 RPC:      $L1_RPC"
echo "From:        $FROM_ADDRESS"
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
    "$FROM_ADDRESS" \
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
        echo "✓ Transaction confirmed!"

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
