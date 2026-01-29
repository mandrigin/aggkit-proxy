# Kurtosis-CDK Integration Plan for Miden Aggkit Proxy

> **Status: COMPLETED** — This is a historical planning document. All 9 methods
> described below have been implemented. The proxy now supports all 17 RPC methods
> required for full kurtosis-cdk bridge-service integration.

## Executive Summary

This plan adds **9 supplementary RPC methods** required for kurtosis-cdk/zkevm-bridge-service integration while **preserving the existing working claim flow**.

## Current State Analysis

### Implemented Methods (8) - DO NOT MODIFY
| Method | Status | Usage |
|--------|--------|-------|
| `eth_chainId` | Working | Chain identification |
| `eth_gasPrice` | Working | Returns 0x0 (Miden has no gas) |
| `eth_estimateGas` | Working | Returns fixed 21000 |
| `eth_getTransactionCount` | Working | Synthetic nonces |
| `eth_sendRawTransaction` | Working | **Core claim flow** |
| `eth_getTransactionReceipt` | Working | **Claim confirmation** |
| `eth_call` | Working | Contract read calls |
| `eth_blockNumber` | Working | Current Miden block |

### Previously Missing Methods (9) - NOW IMPLEMENTED
| Method | Priority | Bridge Service Usage |
|--------|----------|---------------------|
| `eth_getLogs` | **CRITICAL** | BridgeEvent detection (polled every 10s) |
| `eth_getBlockByNumber` | **CRITICAL** | Block structure for sync |
| `eth_getBlockByHash` | HIGH | Block lookup by hash |
| `eth_getTransactionByHash` | HIGH | TX details lookup |
| `net_version` | MEDIUM | Network identification |
| `eth_getBalance` | MEDIUM | Account balance queries |
| `eth_getCode` | LOW | Contract code queries |
| `eth_getStorageAt` | LOW | Storage slot queries |
| `eth_getBlockTransactionCountByNumber` | LOW | TX count per block |

## Architecture Decisions

### 1. Block Synthesis Strategy

Miden batches map to synthetic EVM blocks:
```
Miden Batch N → EVM Block N
  - blockNumber: batch number
  - timestamp: batch timestamp
  - stateRoot: Miden state commitment (hashed to 32 bytes)
  - parentHash: hash of previous synthetic block
  - transactions: translated Miden transactions
```

### 2. Log Synthesis for BridgeEvent

Bridge service expects `BridgeEvent` logs (topic `0x501781209a1f8899323b96b4ef08b168df93e0a90c673d1e4cce39366cb62f9b`).

**Strategy**: Index Miden notes as synthetic logs:
- CLAIM notes → BridgeEvent logs
- Note ID → transactionHash
- Batch number → blockNumber

### 3. State Isolation

New methods will have their own state structures, separate from:
- `BridgeState` (existing claim tracking)
- `ClaimTracker` (replay prevention)
- `AddressMapper` (Eth ↔ Miden addresses)

## Implementation Plan

### Phase 1: Block State Infrastructure
**Files**: `src/block_state.rs` (new), `src/main.rs`

Create block state tracking:
```rust
// src/block_state.rs
pub struct BlockState {
    /// Current Miden block/batch number
    block_number: RwLock<u64>,
    /// Block number → SyntheticBlock
    blocks: RwLock<HashMap<u64, SyntheticBlock>>,
    /// Block hash → block number
    hash_to_number: RwLock<HashMap<[u8; 32], u64>>,
}

pub struct SyntheticBlock {
    pub number: u64,
    pub hash: [u8; 32],
    pub parent_hash: [u8; 32],
    pub timestamp: u64,
    pub state_root: [u8; 32],
    pub transactions: Vec<String>,  // tx hashes
}
```

### Phase 2: Critical Methods (eth_getLogs, eth_getBlockByNumber)
**Files**: `src/main.rs`

#### 2.1 eth_getBlockByNumber
```rust
#[method(name = "eth_getBlockByNumber")]
async fn get_block_by_number(
    &self,
    block_number: String,  // hex or "latest"/"pending"
    full_transactions: bool,
) -> Result<Option<serde_json::Value>, ErrorObjectOwned>;
```

Response format:
```json
{
  "number": "0x1b4",
  "hash": "0x...",
  "parentHash": "0x...",
  "timestamp": "0x5f5e100",
  "stateRoot": "0x...",
  "transactionsRoot": "0x...",
  "receiptsRoot": "0x...",
  "transactions": [],
  "gasUsed": "0x0",
  "gasLimit": "0x1c9c380"
}
```

#### 2.2 eth_getLogs
```rust
#[method(name = "eth_getLogs")]
async fn get_logs(
    &self,
    filter: LogFilter,
) -> Result<Vec<serde_json::Value>, ErrorObjectOwned>;

struct LogFilter {
    from_block: Option<String>,
    to_block: Option<String>,
    address: Option<String>,
    topics: Option<Vec<Option<String>>>,
}
```

**Implementation**:
- Query confirmed claims from `BridgeState.transactions`
- Synthesize BridgeEvent logs for matching filters
- Return max 1000 entries per spec

### Phase 3: Secondary Methods
**Files**: `src/main.rs`

#### 3.1 eth_getBlockByHash
```rust
#[method(name = "eth_getBlockByHash")]
async fn get_block_by_hash(
    &self,
    block_hash: String,
    full_transactions: bool,
) -> Result<Option<serde_json::Value>, ErrorObjectOwned>;
```

#### 3.2 eth_getTransactionByHash
```rust
#[method(name = "eth_getTransactionByHash")]
async fn get_transaction_by_hash(
    &self,
    tx_hash: String,
) -> Result<Option<serde_json::Value>, ErrorObjectOwned>;
```

#### 3.3 net_version
```rust
#[method(name = "net_version")]
async fn net_version(&self) -> Result<String, ErrorObjectOwned> {
    // Return Miden network ID as decimal string
    Ok(format!("{}", MIDEN_CHAIN_ID))
}
```

### Phase 4: Stub Methods (Return Safe Defaults)
**Files**: `src/main.rs`

These methods return safe defaults - they're required but not actively used:

```rust
#[method(name = "eth_getBalance")]
async fn get_balance(&self, address: String, block: Option<String>)
    -> Result<String, ErrorObjectOwned> {
    // Return 0 balance - Miden doesn't use ETH balances
    Ok("0x0".to_string())
}

#[method(name = "eth_getCode")]
async fn get_code(&self, address: String, block: Option<String>)
    -> Result<String, ErrorObjectOwned> {
    // Return empty code - no EVM contracts
    Ok("0x".to_string())
}

#[method(name = "eth_getStorageAt")]
async fn get_storage_at(&self, address: String, position: String, block: Option<String>)
    -> Result<String, ErrorObjectOwned> {
    // Return zero - no EVM storage
    Ok("0x0000000000000000000000000000000000000000000000000000000000000000".to_string())
}

#[method(name = "eth_getBlockTransactionCountByNumber")]
async fn get_block_tx_count(&self, block: String)
    -> Result<String, ErrorObjectOwned> {
    // Return 0 - synthetic blocks have no standard txs
    Ok("0x0".to_string())
}
```

## File Changes Summary

| File | Change Type | Description |
|------|-------------|-------------|
| `src/block_state.rs` | **NEW** | Block/batch state tracking |
| `src/log_synthesis.rs` | **NEW** | BridgeEvent log synthesis |
| `src/main.rs` | MODIFY | Add 9 new RPC methods to `EthApi` trait |
| `src/lib.rs` | MODIFY | Export new modules |
| `Cargo.toml` | NO CHANGE | No new dependencies needed |

## Testing Strategy

### 1. Unit Tests
- Block synthesis from Miden batches
- Log filter matching
- BridgeEvent encoding

### 2. Integration Tests
```bash
# Test eth_getLogs with BridgeEvent topic
curl -X POST http://localhost:8546 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc":"2.0",
    "method":"eth_getLogs",
    "params":[{
      "topics":["0x501781209a1f8899323b96b4ef08b168df93e0a90c673d1e4cce39366cb62f9b"]
    }],
    "id":1
  }'

# Test eth_getBlockByNumber
curl -X POST http://localhost:8546 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_getBlockByNumber","params":["latest",false],"id":1}'
```

### 3. kurtosis-cdk Integration Test
```bash
# Deploy with Miden integration
kurtosis run --args-file params.yml .

# Verify bridge service connects
kurtosis service logs cdk zkevm-bridge-service | grep "aggkit-proxy"
```

## Implementation Order

1. **Phase 1**: Block state infrastructure (foundation)
2. **Phase 2**: `eth_getLogs` + `eth_getBlockByNumber` (critical path)
3. **Phase 3**: Secondary methods (complete coverage)
4. **Phase 4**: Stub methods (satisfy all 17 required)

## Risk Assessment

| Risk | Impact | Mitigation |
|------|--------|------------|
| Breaking existing claim flow | HIGH | New methods isolated, existing code untouched |
| eth_getLogs performance | MEDIUM | Limit to 1000 entries, index by block range |
| Block synthesis consistency | MEDIUM | Use Miden batch number as source of truth |

## Success Criteria

1. All 17 RPC methods respond correctly
2. Existing `eth_sendRawTransaction` → claim flow unchanged
3. Bridge service Synchronizer connects and polls successfully
4. No regressions in current proxy functionality
