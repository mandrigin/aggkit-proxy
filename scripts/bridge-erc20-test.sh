#!/usr/bin/env bash
#
# Bridge ERC20 Test - Deploy ERC20 tokens on L1, bridge them to Miden
#
# Usage: ./scripts/bridge-erc20-test.sh [num_tokens]
#
# Deploys N ERC20 contracts, mints tokens, approves bridge, and bridges them.
# Default: 10 tokens.

set -euo pipefail

NUM_TOKENS="${1:-10}"
DEST="0x00000000a417929a101b89100dda63bf4f692800"
DEST_NET=1

# Kurtosis funded account
PRIVATE_KEY="0x12d7de8621a77640c9241b2595ba78ce443d05e94090365ab3bb5e19df82c625"
FROM_ADDRESS="0xE34aaF64b29273B7D567FCFc40544c014EEe9970"

# Auto-detect L1 RPC
L1_RPC="${L1_RPC:-}"
if [[ -z "$L1_RPC" ]]; then
    L1_CONTAINER=$(docker ps --filter "name=el-1-geth" --format "{{.Names}}" | head -1)
    if [[ -n "$L1_CONTAINER" ]]; then
        L1_PORT=$(docker port "$L1_CONTAINER" 8545 2>/dev/null | cut -d: -f2 || true)
        [[ -n "$L1_PORT" ]] && L1_RPC="http://localhost:$L1_PORT"
    fi
fi
[[ -z "$L1_RPC" ]] && L1_RPC=$(kurtosis port print miden-cdk el-1-geth-lighthouse rpc 2>/dev/null) || true
[[ -z "$L1_RPC" ]] && { echo "ERROR: Cannot find L1 RPC"; exit 1; }

# Auto-detect bridge address
BRIDGE_ADDRESS="${BRIDGE_ADDRESS:-}"
if [[ -z "$BRIDGE_ADDRESS" ]]; then
    BRIDGE_ADDRESS=$(kurtosis service exec miden-cdk contracts-001 \
        "cat /opt/output/combined.json 2>/dev/null" 2>/dev/null | jq -r '.polygonZkEVMBridgeAddress // empty' || true)
fi
[[ -z "$BRIDGE_ADDRESS" ]] && BRIDGE_ADDRESS="0xC8cbEBf950B9Df44d987c8619f092beA980fF038"

echo "=== ERC20 Bridge Test ==="
echo "L1 RPC:    $L1_RPC"
echo "Bridge:    $BRIDGE_ADDRESS"
echo "From:      $FROM_ADDRESS"
echo "Dest:      $DEST"
echo "Tokens:    $NUM_TOKENS"
echo ""

# Minimal ERC20 bytecode — constructor(name, symbol, decimals)
# Using a simple inline Solidity contract deployed via forge
ERC20_SOL='
// SPDX-License-Identifier: MIT
pragma solidity ^0.8.20;

contract TestToken {
    string public name;
    string public symbol;
    uint8 public decimals;
    uint256 public totalSupply;
    mapping(address => uint256) public balanceOf;
    mapping(address => mapping(address => uint256)) public allowance;

    event Transfer(address indexed from, address indexed to, uint256 value);
    event Approval(address indexed owner, address indexed spender, uint256 value);

    constructor(string memory _name, string memory _symbol, uint8 _decimals) {
        name = _name;
        symbol = _symbol;
        decimals = _decimals;
    }

    function mint(address to, uint256 amount) external {
        totalSupply += amount;
        balanceOf[to] += amount;
        emit Transfer(address(0), to, amount);
    }

    function approve(address spender, uint256 amount) external returns (bool) {
        allowance[msg.sender][spender] = amount;
        emit Approval(msg.sender, spender, amount);
        return true;
    }

    function transfer(address to, uint256 amount) external returns (bool) {
        require(balanceOf[msg.sender] >= amount, "insufficient");
        balanceOf[msg.sender] -= amount;
        balanceOf[to] += amount;
        emit Transfer(msg.sender, to, amount);
        return true;
    }

    function transferFrom(address from, address to, uint256 amount) external returns (bool) {
        require(allowance[from][msg.sender] >= amount, "not approved");
        require(balanceOf[from] >= amount, "insufficient");
        allowance[from][msg.sender] -= amount;
        balanceOf[from] -= amount;
        balanceOf[to] += amount;
        emit Transfer(from, to, amount);
        return true;
    }
}
'

# Write the contract to a temp file
TMPDIR=$(mktemp -d)
mkdir -p "$TMPDIR/src"
echo "$ERC20_SOL" > "$TMPDIR/src/TestToken.sol"
cat > "$TMPDIR/foundry.toml" << 'TOML'
[profile.default]
src = "src"
out = "out"
solc_version = "0.8.28"
TOML

echo "--- Step 1: Compiling ERC20 contract ---"
(cd "$TMPDIR" && forge build --quiet 2>&1) || { echo "Forge build failed"; exit 1; }
echo "✓ Compiled"
echo ""

# Get current nonce
NONCE=$(cast nonce "$FROM_ADDRESS" --rpc-url "$L1_RPC")

TOKENS=()
NAMES=("ALPHA" "BETA" "GAMMA" "DELTA" "EPSILON" "ZETA" "ETA" "THETA" "IOTA" "KAPPA"
       "LAMBDA" "MU" "NU" "XI" "OMICRON" "PI" "RHO" "SIGMA" "TAU" "UPSILON")
DECIMALS=(18 18 6 8 18 6 8 18 6 18 18 8 6 18 18 6 8 18 18 6)

BYTECODE=$(jq -r '.bytecode.object' "$TMPDIR/out/TestToken.sol/TestToken.json")

echo "--- Step 2: Deploying $NUM_TOKENS ERC20 tokens ---"
for i in $(seq 0 $((NUM_TOKENS - 1))); do
    NAME="${NAMES[$i]}"
    SYM="${NAMES[$i]:0:3}"
    DEC="${DECIMALS[$i]}"

    ARGS=$(cast abi-encode "constructor(string,string,uint8)" "$NAME Token" "$SYM" "$DEC")
    RESULT=$(cast send --rpc-url "$L1_RPC" --private-key "$PRIVATE_KEY" \
        --gas-limit 1000000 --nonce "$NONCE" --json \
        --create "${BYTECODE}${ARGS#0x}" 2>&1)

    ADDR=$(echo "$RESULT" | jq -r '.contractAddress // empty')
    if [[ -z "$ADDR" ]]; then
        echo "  [FAIL] $NAME — $(echo "$RESULT" | head -1)"
        NONCE=$((NONCE + 1))
        continue
    fi

    TOKENS+=("$ADDR:$NAME:$SYM:$DEC")
    echo "  [$((i+1))/$NUM_TOKENS] $NAME ($SYM, ${DEC}dec) → $ADDR"
    NONCE=$((NONCE + 1))
done
echo "✓ Deployed ${#TOKENS[@]} tokens"
echo ""

MINT_AMOUNT_BASE="1000000"  # 1M tokens (before decimal scaling)

echo "--- Step 3: Minting & approving ---"
for TOKEN_INFO in "${TOKENS[@]}"; do
    IFS=: read -r ADDR NAME SYM DEC <<< "$TOKEN_INFO"

    # Scale mint amount by decimals
    MINT_WEI="${MINT_AMOUNT_BASE}$(printf '%0*d' "$DEC" 0)"

    # Mint
    cast send "$ADDR" "mint(address,uint256)" "$FROM_ADDRESS" "$MINT_WEI" \
        --private-key "$PRIVATE_KEY" --rpc-url "$L1_RPC" --nonce "$NONCE" --json > /dev/null 2>&1
    NONCE=$((NONCE + 1))

    # Approve bridge
    cast send "$ADDR" "approve(address,uint256)" "$BRIDGE_ADDRESS" "$MINT_WEI" \
        --private-key "$PRIVATE_KEY" --rpc-url "$L1_RPC" --nonce "$NONCE" --json > /dev/null 2>&1
    NONCE=$((NONCE + 1))

    BAL=$(cast call "$ADDR" "balanceOf(address)(uint256)" "$FROM_ADDRESS" --rpc-url "$L1_RPC" 2>/dev/null)
    echo "  $SYM: minted $MINT_AMOUNT_BASE tokens (${DEC}dec), balance=$BAL"
done
echo "✓ All minted and approved"
echo ""

# Bridge amount: 100 tokens each (scaled by decimals)
BRIDGE_BASE="100"

echo "--- Step 4: Bridging tokens one by one ---"
for TOKEN_INFO in "${TOKENS[@]}"; do
    IFS=: read -r ADDR NAME SYM DEC <<< "$TOKEN_INFO"

    BRIDGE_WEI="${BRIDGE_BASE}$(printf '%0*d' "$DEC" 0)"

    # bridgeAsset(uint32 destNet, address destAddr, uint256 amount, address token, bool forceUpdate, bytes permit)
    CALLDATA=$(cast calldata "bridgeAsset(uint32,address,uint256,address,bool,bytes)" \
        "$DEST_NET" \
        "$DEST" \
        "$BRIDGE_WEI" \
        "$ADDR" \
        true \
        "0x")

    RESULT=$(cast send "$BRIDGE_ADDRESS" "$CALLDATA" \
        --private-key "$PRIVATE_KEY" \
        --rpc-url "$L1_RPC" \
        --nonce "$NONCE" \
        --gas-limit 300000 \
        --json 2>&1)

    TX=$(echo "$RESULT" | jq -r '.transactionHash // empty')
    STATUS=$(echo "$RESULT" | jq -r '.status // "fail"')
    BLOCK=$(echo "$RESULT" | jq -r '.blockNumber // "?"')

    if [[ "$STATUS" == "0x1" ]]; then
        echo "  ✓ $SYM: bridged $BRIDGE_BASE tokens, block=$((BLOCK)), tx=${TX:0:18}..."
    else
        echo "  ✗ $SYM: FAILED — $(echo "$RESULT" | head -1)"
    fi
    NONCE=$((NONCE + 1))
done
echo ""

echo "=== Done ==="
echo "Tokens deployed and bridged. Monitor with:"
echo "  PG=\$(docker ps --filter 'name=postgres-001' --format '{{.Names}}' | grep -v agglayer | head -1)"
echo "  docker exec \$PG psql -U master_user -d bridge_db -c \"SELECT id, deposit_cnt, amount, ready_for_claim FROM sync.deposit WHERE dest_net=1 ORDER BY id DESC LIMIT 15;\""

# Cleanup
rm -rf "$TMPDIR"
