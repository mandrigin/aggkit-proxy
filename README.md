# miden-rpc-proxy

JSON-RPC proxy server bridging Ethereum-style RPC to Miden network. Enables AggLayer bridge claim processing by translating `eth_sendRawTransaction` calls containing `claimAsset` transactions into Miden CLAIM notes.

## Architecture

```
                                    +------------------+
                                    |   Miden Node     |
                                    |  (gRPC :57291)   |
                                    +--------^---------+
                                             |
+-------------+    JSON-RPC    +-------------+-------------+
|   Bridge    | ------------> |      miden-rpc-proxy      |
|   Service   |   :8546       |                           |
+-------------+               | +-------+ +-------------+ |
                              | |Decoder| |Address      | |
                              | |       | |Mapper       | |
                              | +-------+ +-------------+ |
                              | +-------+ +-------------+ |
                              | |CLAIM  | |Miden Client | |
                              | |Notes  | |Wrapper      | |
                              | +-------+ +-------------+ |
                              +---------------------------+
```

## How It Works

### CLAIM Note Flow

1. **L1 Deposit**: User deposits ETH to the bridge contract on L1 (Ethereum)
2. **Bridge DB Sync**: kurtosis-cdk's bridge-service monitors L1 and records deposits
3. **Ready for Claim**: Once L1 finality is reached, deposit becomes `ready_for_claim=true`
4. **Bridge Service**: Sends `claimAsset` transaction to the proxy
5. **Proxy Processing**:
   - Decodes the `claimAsset` calldata (SMT proofs, roots, amounts, etc.)
   - Creates a CLAIM note using `miden-agglayer` library
   - Submits transaction to Miden network
6. **Token Minting**: Agglayer faucet consumes CLAIM note and mints tokens to recipient

### Key Components

| Component | Purpose |
|-----------|---------|
| `src/main.rs` | RPC server, claim processing, account initialization |
| `src/agglayer_faucet.rs` | Bridge/faucet account creation |
| `src/decode.rs` | RLP transaction and claimAsset calldata decoding |
| `src/receipt.rs` | Ethereum-format receipt generation |
| `src/client.rs` | Miden client wrapper |
| `src/block_state.rs` | Synthetic EVM block state for kurtosis-cdk |
| `src/log_synthesis.rs` | Synthetic EVM logs for bridge-service |

## Account Initialization

The proxy creates three Miden accounts at startup to enable CLAIM note processing:

### 1. Ephemeral Submitter Account
- **Purpose**: Submits CLAIM note transactions to the network
- **Type**: `RegularAccountUpdatableCode` with `BasicWallet` component
- **Auth**: `RpoFalcon512` (key stored in filesystem keystore)
- **Created**: Once at startup, reused for ALL claims

### 2. Agglayer Faucet Account
- **Purpose**: Processes CLAIM notes - validates SMT proofs and mints tokens
- **Components**: `agglayer_faucet_component` from `miden-agglayer`
- **Token**: "LUMIA" with 8 decimals
- **Seed**: Deterministic from `BRIDGE_FAUCET_ID` for reproducibility

### 3. Bridge Account (Local Reference)
- **Purpose**: Provides `bridge_account_id` for faucet validation
- **Auth**: NoAuth (not deployed - actual bridge is in miden-node genesis)
- **Seed**: Deterministic from `BRIDGE_FAUCET_ID`

### Why Pre-Initialize?

The Miden client's `add_account()` fails if an account is "already being tracked".
Originally, accounts were created per-claim, causing the second claim to fail:

```
Failed to add bridge account: account with id 0x... is already being tracked
```

Solution: Initialize all accounts ONCE at startup, store their IDs in config,
and reuse them for all subsequent claims.

## Prerequisites

- Rust 1.82 or later
- Docker (for kurtosis-cdk integration)
- Miden node **agglayer-v0.1** tag (required for compatibility)
  - Built from source via `Dockerfile.miden-node`
  - Source: https://github.com/0xPolygonMiden/miden-node (tag: agglayer-v0.1)
- kurtosis-cdk deployment (for bridge-service integration)

## Building

```bash
# Clone the repository
git clone <repo-url>
cd miden-rpc-proxy

# Debug build
cargo build

# Release build (optimized)
cargo build --release

# Or use Make
make build      # debug
make release    # release
```

## Configuration

### Environment Variables

| Variable | Required | Default | Description |
|----------|----------|---------|-------------|
| `MIDEN_RPC_URL` | No | `http://localhost:57291` | Miden node gRPC endpoint |
| `BRIDGE_FAUCET_ID` | Yes | - | Bridge faucet account ID from genesis (hex) |
| `BRIDGE_ADDRESS` | No | `0xc8cbebf950b9df44d987c8619f092bea980ff038` | L2 bridge contract for receipts |
| `CHAIN_ID` | No | `2` | Chain ID returned by `eth_chainId` |
| `LISTEN_HOST` | No | `0.0.0.0` | HTTP server bind address |
| `LISTEN_PORT` | No | `8546` | HTTP server port |
| `MIDEN_STORE_PATH` | No | `/app/data/miden-client` | SQLite store directory |
| `RUST_LOG` | No | - | Logging level (e.g., `info`, `debug`) |

### Finding Configuration Values

**BRIDGE_FAUCET_ID** - Get from miden-node genesis:
```bash
# From kurtosis deployment
kurtosis service exec cdk-miden miden-node-001 \
  "cat /opt/miden-node/config/genesis.toml" | grep -A2 'faucet'
```

**BRIDGE_ADDRESS** - Get from kurtosis-cdk contracts:
```bash
kurtosis service exec cdk-miden contracts-001 \
  "cat /opt/zkevm/combined.json" | jq -r '.polygonZkEVMBridgeAddress'
```

### Example Configuration

```bash
export MIDEN_RPC_URL="http://miden-node-001:57291"
export BRIDGE_FAUCET_ID="0x4d2cddb05296de102132d80d8896be"
export BRIDGE_ADDRESS="0xc8cbebf950b9df44d987c8619f092bea980ff038"
export CHAIN_ID="2"
export LISTEN_PORT="8546"
export RUST_LOG="info,miden_rpc_proxy=debug"
```

## Running

### Standalone

```bash
# Start with environment variables
MIDEN_RPC_URL=http://localhost:57291 \
BRIDGE_FAUCET_ID=0x... \
./target/release/miden-rpc-proxy

# With debug logging
RUST_LOG=debug ./target/release/miden-rpc-proxy
```

### With Docker (Kurtosis)

The proxy is typically deployed as a container in kurtosis-cdk:

```bash
# Build the image
docker build -t miden-rpc-proxy:latest .

# Run with kurtosis network
docker run --rm \
  --network kurtosis-cdk \
  -e MIDEN_RPC_URL=http://miden-node-001:57291 \
  -e BRIDGE_FAUCET_ID=0x... \
  -p 8546:8546 \
  miden-rpc-proxy:latest
```

## Integration with kurtosis-cdk

### Bridge Service Configuration

Update the bridge-service config to point to the proxy:

```toml
# In bridge-service config
l2_rpc_url = "http://miden-proxy:8546"
```

### Deposit Flow

1. **Send L1 Deposit**:
   ```bash
   ./scripts/send-deposit.sh 0.1234
   ```

2. **Check Bridge DB**:
   ```bash
   ./scripts/list-deposits.sh
   ```

3. **Wait for ready_for_claim**:
   ```bash
   ./scripts/wait-deposit.sh <deposit_num>
   ```

4. **Verify CLAIM Notes**:
   ```bash
   ./scripts/verify-claim-notes.sh miden-proxy-kurtosis
   ```

### Global Exit Root (GER) Handling

The proxy handles GER injection from aggoracle:

1. **updateExitRoot** transactions target `0xa40d5f56745a118d0906a34e69aec8c0db1cb8fa`
2. Proxy extracts `mainnet_exit_root` and `rollup_exit_root`
3. Computes `global_exit_root = keccak256(mainnet_exit_root || rollup_exit_root)`
4. Stores for future SMT proof validation

### Synthetic EVM State

For bridge-service compatibility, the proxy maintains:

- **Block state**: Synthetic blocks with incrementing numbers and timestamps
- **Log store**: `UpdateGlobalExitRoot` and `ClaimEvent` logs
- **Receipt tracking**: Maps Miden tx IDs to Ethereum-format receipts

## Testing

```bash
# Run all tests
make test

# Unit tests only (fast)
make test-phase1

# Integration tests
make test-phase2

# Development workflow
make dev
```

### Manual RPC Test

```bash
# Test eth_chainId
curl -X POST http://localhost:8546 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}'

# Test eth_blockNumber
curl -X POST http://localhost:8546 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'
```

## Scripts

| Script | Purpose |
|--------|---------|
| `scripts/send-deposit.sh <amount>` | Send ETH deposit to L1 bridge |
| `scripts/list-deposits.sh` | List all deposits in bridge DB |
| `scripts/wait-deposit.sh <num>` | Wait for deposit to be ready_for_claim |
| `scripts/verify-notes.sh --note-id <id>` | Verify a note exists on miden-node |
| `scripts/verify-claim-notes.sh [container]` | Verify all CLAIM notes from proxy logs |

## Troubleshooting

### "account with id ... is already being tracked"

**Cause**: Account creation was attempted multiple times.

**Solution**: This is handled automatically by `src/agglayer_faucet.rs` which
checks for "already being tracked" and treats it as success.

### Claims stop processing after N deposits

**Possible causes**:
- Bridge-service polling interval (deposits need L1 finality)
- Miden-node sync issues
- Proxy restart required

**Debug**:
```bash
# Check proxy logs
docker logs miden-proxy-kurtosis

# Check bridge DB status
./scripts/list-deposits.sh

# Verify notes exist
./scripts/verify-claim-notes.sh
```

### "Note not found" in verification

**Possible causes**:
- Note was consumed by faucet (normal for processed claims)
- Transaction didn't complete
- Wrong note ID format

**Debug**:
```bash
# Verify specific note
./scripts/verify-notes.sh --note-id 0x...

# Check miden-node state
docker exec miden-node-001 miden-node info
```

## Project Structure

```
src/
├── main.rs              # RPC server, claim processing, account init
├── lib.rs               # Library root, re-exports
├── agglayer_faucet.rs   # Bridge/faucet account creation
├── client.rs            # Miden client wrapper
├── config.rs            # TOML configuration (legacy)
├── decode.rs            # RLP and claimAsset decoding
├── receipt.rs           # Ethereum receipt generation
├── types.rs             # Core types (ClaimAssetParams)
├── error.rs             # Error types with JSON-RPC codes
├── address_mapper.rs    # Ethereum <-> Miden address mapping
├── storage.rs           # SQLite persistence
├── block_state.rs       # Synthetic EVM blocks
└── log_synthesis.rs     # Synthetic EVM logs

scripts/
├── send-deposit.sh          # Send L1 deposit
├── list-deposits.sh         # List bridge DB deposits
├── wait-deposit.sh          # Wait for ready_for_claim
├── verify-notes.sh          # Verify single note
└── verify-claim-notes.sh    # Verify all CLAIM notes
```

## License

MIT
