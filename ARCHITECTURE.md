# Architecture

This document describes the internal architecture of miden-rpc-proxy.

## Overview

miden-rpc-proxy is a translation layer between Ethereum JSON-RPC and the Miden network. It allows users to claim bridged assets using standard Ethereum tooling (MetaMask, ethers.js) while the actual settlement happens on Miden.

## Component Diagram

```
┌─────────────────────────────────────────────────────────────────────────┐
│                           miden-rpc-proxy                               │
│                                                                         │
│  ┌─────────────┐    ┌─────────────┐    ┌─────────────┐                 │
│  │   JSON-RPC  │    │  Transaction│    │   Address   │                 │
│  │   Server    │───>│   Decoder   │───>│   Mapper    │                 │
│  │ (jsonrpsee) │    │  (alloy)    │    │  (SQLite)   │                 │
│  └─────────────┘    └─────────────┘    └─────────────┘                 │
│         │                                     │                         │
│         │                                     v                         │
│         │               ┌─────────────────────────────┐                │
│         │               │      Bridge State           │                │
│         │               │  - Nonces (per address)     │                │
│         │               │  - Transaction status       │                │
│         │               │  - Block height cache       │                │
│         └──────────────>│                             │                │
│                         └─────────────────────────────┘                │
│                                     │                                   │
│                                     v                                   │
│                         ┌─────────────────────────────┐                │
│                         │     Miden Client Wrapper    │                │
│                         │  - P2ID note creation       │                │
│                         │  - Transaction submission   │                │
│                         │  - State synchronization    │                │
│                         └─────────────────────────────┘                │
│                                     │                                   │
└─────────────────────────────────────│───────────────────────────────────┘
                                      │ gRPC
                                      v
                            ┌─────────────────┐
                            │   Miden Node    │
                            └─────────────────┘
```

## Core Components

### 1. JSON-RPC Server (`main.rs`)

Built on [jsonrpsee](https://github.com/paritytech/jsonrpsee), implements the Ethereum JSON-RPC API subset needed for bridge operations:

```rust
#[rpc(server)]
pub trait EthApi {
    async fn chain_id(&self) -> Result<String, ErrorObjectOwned>;
    async fn gas_price(&self) -> Result<String, ErrorObjectOwned>;
    async fn estimate_gas(&self, tx: Value, block: Option<String>) -> Result<String, ErrorObjectOwned>;
    async fn get_transaction_count(&self, address: String, block: Option<String>) -> Result<String, ErrorObjectOwned>;
    async fn send_raw_transaction(&self, data: String) -> Result<String, ErrorObjectOwned>;
    async fn get_transaction_receipt(&self, hash: String) -> Result<Option<TransactionReceipt>, ErrorObjectOwned>;
    async fn call(&self, tx: Value, block: Option<String>) -> Result<String, ErrorObjectOwned>;
    async fn block_number(&self) -> Result<String, ErrorObjectOwned>;
}
```

**Design decisions:**
- Gas price always returns `0x0` (Miden has no gas fees)
- Gas estimate returns fixed `21000` (standard transfer)
- Nonces are synthetic (tracked per-address in memory)
- Block numbers map to Miden block heights

### 2. Transaction Decoder (`decode.rs`)

Decodes RLP-encoded Ethereum transactions using [alloy](https://github.com/alloy-rs/alloy):

```rust
pub fn decode_transaction(raw_tx: &[u8]) -> Result<DecodedTransaction, DecodeError>;
pub fn parse_claim_asset(input: &[u8]) -> Result<ClaimAssetParams, DecodeError>;
```

**Supported transaction types:**
- Legacy (type 0)
- EIP-2930 (type 1)
- EIP-1559 (type 2)
- EIP-4844 (type 3)

**claimAsset function signature:**
```solidity
function claimAsset(
    bytes32[32] smtProofLocalExitRoot,
    bytes32[32] smtProofRollupExitRoot,
    uint256 globalIndex,
    bytes32 mainnetExitRoot,
    bytes32 rollupExitRoot,
    uint32 originNetwork,
    address originTokenAddress,
    uint32 destinationNetwork,
    address destinationAddress,
    uint256 amount,
    bytes metadata
) external;
```

**GlobalIndex bit layout:**
```
Bits 64+    : mainnetFlag (1 = mainnet origin, 0 = rollup origin)
Bits 32-63  : rollupIndex (which rollup in the tree)
Bits 0-31   : localRootIndex (deposit counter)
```

### 3. Address Mapper (`address_mapper.rs`, `storage.rs`)

Maps 160-bit Ethereum addresses to 120-bit Miden AccountIds. Implements "Option 3: Auto-create accounts":

```rust
impl AddressMapper {
    pub fn lookup(&self, eth: &EthAddress) -> Result<Option<MidenAccountId>>;
    pub fn get_or_create(&self, eth: &EthAddress) -> Result<(MidenAccountId, bool)>;
    pub fn register(&self, eth: &EthAddress, miden: &MidenAccountId) -> Result<()>;
}
```

**Derivation scheme:**
```
seed = keccak256(domain_separator || eth_address)
account_id = keccak256("miden-account-id-v1" || seed)[0..15]
```

**Storage schema (SQLite):**
```sql
CREATE TABLE address_mappings (
    eth_address BLOB PRIMARY KEY,
    miden_account_id BLOB NOT NULL,
    created_at INTEGER NOT NULL,
    auto_created INTEGER NOT NULL
);
```

### 4. Miden Client Wrapper (`client.rs`)

Wraps the Miden SDK for bridge-specific operations:

```rust
impl MidenClientWrapper {
    pub async fn create_bridge_claim_note(
        &self,
        sender: AccountId,
        recipient: AccountId,
        asset: FungibleAsset,
        recall_height: Option<u32>,
    ) -> Result<(Note, NoteId), ClientError>;

    pub async fn submit_transaction(&self, tx: TransactionRequest) -> Result<String, ClientError>;
    pub async fn sync_state(&self) -> Result<SyncSummary, ClientError>;
}
```

**P2ID (Pay-to-ID) notes:**
- Created using `miden_lib::notes::create_p2id_note`
- Can only be consumed by the target AccountId
- Public notes visible on-chain
- Optional recall height for expiration

### 5. Bridge State (`main.rs`)

In-memory state tracking using `parking_lot::RwLock`:

```rust
pub struct BridgeState {
    nonces: RwLock<HashMap<String, u64>>,      // Synthetic nonces per address
    transactions: RwLock<HashMap<String, TxStatus>>,  // Tx status tracking
    block_height: RwLock<u64>,                  // Cached Miden block height
}
```

**Transaction lifecycle:**
```
Pending -> Confirmed { block_number }
        -> Failed { reason }
```

### 6. Configuration (`config.rs`)

TOML-based configuration with serde:

```rust
#[derive(Deserialize)]
pub struct ProxyConfig {
    pub listen_port: u16,        // default: 8546
    pub miden_rpc_url: String,   // Miden node endpoint
    pub chain_id: u64,           // EIP-155 chain ID
    pub bridge_account_id: String, // Bridge faucet account
}
```

### 7. Error Handling (`error.rs`)

Structured errors with JSON-RPC error code mapping:

| Error | Code | Description |
|-------|------|-------------|
| TransactionDecode | -32602 | Invalid params |
| NotClaimAsset | -32602 | Invalid params |
| AlreadyClaimed | -32000 | Server error |
| MidenRpc | -32001 | Server error |
| MidenTransaction | -32002 | Server error |
| AccountResolution | -32003 | Server error |
| ReceiptNotFound | -32004 | Server error |
| Config/Internal | -32603 | Internal error |

### 8. Claim Tracker (`claim_tracker.rs`)

Prevents double-processing of claims using a lock-free `DashSet`:

```rust
impl ClaimTracker {
    pub fn try_claim(&self, global_index: U256) -> Result<(), AlreadyClaimed>;
    pub fn unclaim(&self, global_index: &U256);
}
```

- **Atomic check-and-insert**: `try_claim` returns `Err(AlreadyClaimed)` if the global index has already been claimed
- **Rollback**: `unclaim` removes the index on submission failure, allowing retry
- **Concurrent safe**: Uses `DashSet` for lock-free multi-threaded access
- **Persistence**: Optional file-based persistence (`claimed_indices.json`)

### 9. Asset Mapping (`asset_mapping.rs`)

Maps origin network/token pairs to Miden faucet assets:

```rust
impl AssetMapping {
    pub fn resolve(&self, origin_network: u32, origin_token: Address) -> Option<FungibleAsset>;
}
```

Determines which Miden faucet handles a given bridged token based on its L1 origin.

### 10. Log Synthesis (`log_synthesis.rs`)

Generates synthetic EVM logs for bridge-service compatibility:

- **UpdateHashChainValue** events on GER injection
- **ClaimEvent** events on successful claims
- Supports full `eth_getLogs` filter specification (block range, address, topics)
- Maximum 1000 logs per query per Ethereum JSON-RPC spec

### 11. Block State (`block_state.rs`)

Maintains synthetic EVM blocks that map to Miden block numbers:

- Deterministic hashes: `keccak256("miden-synthetic-block-v1" || blockNumber || parentHash || timestamp || stateRoot)`
- Deterministic timestamps: `GENESIS_TIMESTAMP + blockNumber * 12`
- Lazy chain building: missing blocks from genesis to N created on demand
- Prevents false "reorg" detection by bridge-service on proxy restart

## Data Flow

### Claim Processing Flow

```
1. User signs claimAsset tx in MetaMask
   │
2. eth_sendRawTransaction received
   │
   ├─> decode_transaction() - RLP decode, recover signer
   │
   ├─> parse_claim_asset() - Extract claim parameters
   │   - SMT proofs
   │   - Global index (mainnet/rollup/local)
   │   - Exit roots
   │   - Destination address
   │   - Amount
   │
   ├─> address_mapper.get_or_create() - Resolve Miden account
   │   - Lookup existing mapping
   │   - Or derive deterministically
   │
   ├─> create_bridge_claim_note() - Create P2ID note
   │   - Set recipient = resolved Miden account
   │   - Set asset = bridged amount
   │
   ├─> submit_transaction() - Send to Miden network
   │
   └─> Return synthetic tx hash
```

### Receipt Polling Flow

```
1. eth_getTransactionReceipt(hash)
   │
   ├─> Lookup in BridgeState.transactions
   │
   ├── Pending -> return null (not yet confirmed)
   │
   ├── Confirmed -> return receipt with:
   │   - status: "0x1"
   │   - blockNumber: Miden block
   │   - gasUsed: 21000 (fixed)
   │
   └── Failed -> return receipt with:
       - status: "0x0"
       - reason in logs
```

## Security Considerations

1. **Replay protection**: GlobalIndex tracks unique claims
2. **Address derivation**: Domain-separated keccak256 prevents collisions
3. **No private keys**: Proxy never holds user funds or keys
4. **Input validation**: All calldata decoded and validated before processing

## Future Improvements

1. **Persistent tx state**: Currently in-memory; should use SQLite
2. **Merkle proof verification**: Verify SMT proofs before processing
3. **Rate limiting**: Add request rate limits per address
4. **Metrics**: Prometheus metrics for monitoring
5. **WebSocket support**: For subscription-based updates
