# Bridge Flow: How miden-rpc-proxy Integrates with AggLayer

> Exhaustive documentation of every JSON-RPC method, the GER injection pipeline,
> synthetic event emission, and the end-to-end claim lifecycle.

## Table of Contents

1. [System Context](#system-context)
2. [Component Map](#component-map)
3. [JSON-RPC Methods Reference](#json-rpc-methods-reference)
4. [Flow 1: GER Injection (aggoracle → proxy)](#flow-1-ger-injection)
5. [Flow 2: Deposit Claim (bridge-service → proxy → Miden)](#flow-2-deposit-claim)
6. [Flow 3: Bridge Sync (bridge-service ← proxy via eth_getLogs)](#flow-3-bridge-sync)
7. [Synthetic EVM State](#synthetic-evm-state)
8. [Account Initialization at Startup](#account-initialization-at-startup)
9. [Replay Prevention](#replay-prevention)
10. [Address Mapping](#address-mapping)
11. [Amount Scaling](#amount-scaling)
12. [Kurtosis Deployment Architecture](#kurtosis-deployment-architecture)

---

## System Context

The miden-rpc-proxy sits between the AggLayer bridge infrastructure (designed for
EVM L2 chains) and the Miden network (a zk-rollup with a non-EVM execution model).
It translates Ethereum JSON-RPC calls into Miden operations, making Miden appear as
an EVM-compatible L2 to the bridge services.

```
┌──────────────────────────────────────────────────────────────────────┐
│                        kurtosis-cdk deployment                       │
│                                                                      │
│  ┌─────────┐  ┌──────────────┐  ┌────────────────┐  ┌────────────┐ │
│  │ L1 Geth │  │ Bridge       │  │ aggoracle      │  │ aggsender  │ │
│  │ (anvil) │  │ Service      │  │ (GER injector) │  │            │ │
│  └────┬────┘  └──────┬───────┘  └───────┬────────┘  └────────────┘ │
│       │              │                   │                           │
│       │    eth_getLogs│        eth_send   │                           │
│       │    eth_call   │        RawTx     │                           │
│       │    getBlock   │        (GER)     │                           │
│       │              │                   │                           │
│       │        ┌─────▼───────────────────▼──────┐                   │
│       │        │       miden-rpc-proxy           │                   │
│       │        │                                 │                   │
│       │        │  JSON-RPC :8546                 │                   │
│       │        │  ┌───────────┐ ┌──────────────┐ │                   │
│       │        │  │ GER Store │ │ Log Store    │ │                   │
│       │        │  │           │ │ (synthetic)  │ │                   │
│       │        │  └───────────┘ └──────────────┘ │                   │
│       │        │  ┌───────────┐ ┌──────────────┐ │                   │
│       │        │  │Block State│ │ Claim Tracker│ │                   │
│       │        │  │(synthetic)│ │ (replay prev)│ │                   │
│       │        │  └───────────┘ └──────────────┘ │                   │
│       │        └─────────────┬───────────────────┘                   │
│       │                      │ gRPC :57291                           │
│       │              ┌───────▼─────────┐                             │
│       │              │   Miden Node    │                             │
│       │              │   (miden-node)  │                             │
│       │              └─────────────────┘                             │
│       │                                                              │
│  ┌────▼──────────────┐                                              │
│  │  nginx forwarder  │  :8545 → :8546                               │
│  │  (L2 endpoint)    │  Bridge services connect here                │
│  └───────────────────┘                                              │
│                                                                      │
│  ┌───────────────────┐                                              │
│  │  Bridge UI        │  :3000 (web dApp for manual deposits)        │
│  │  (web-ui)         │                                              │
│  └───────────────────┘                                              │
└──────────────────────────────────────────────────────────────────────┘
```

### Why a proxy?

The AggLayer bridge was designed for EVM L2s (OP Stack, zkEVM). It communicates
exclusively via Ethereum JSON-RPC: polling `eth_getLogs` for events, sending
`eth_sendRawTransaction` for claims, reading `eth_getBlockByNumber` for sync.
Miden speaks gRPC and uses a note-based transaction model. The proxy translates
between these two worlds.

---

## Component Map

| Component | File(s) | Purpose |
|-----------|---------|---------|
| RPC Server | `src/main.rs` | jsonrpsee server implementing `EthApi` trait (17 methods) |
| Transaction Decoder | `src/decode.rs` | RLP decode + `claimAsset` calldata parsing |
| GER Store | `src/main.rs` (GerStore) | Tracks injected Global Exit Roots |
| Log Store | `src/log_synthesis.rs` | Synthetic EVM logs (ClaimEvent, UpdateHashChainValue) |
| Block State | `src/block_state.rs` | Synthetic EVM blocks with deterministic hashes |
| Claim Tracker | `src/claim_tracker.rs` | Replay prevention via DashSet of global indices |
| Address Mapper | `src/address_mapper.rs` | Ethereum address → Miden AccountId derivation |
| Receipt Manager | `src/receipt.rs` | Eth tx hash → Miden tx ID mapping, receipt generation |
| Agglayer Faucet | `src/agglayer_faucet.rs` | Creates bridge + faucet accounts for CLAIM notes |
| Client Wrapper | `src/client.rs` | Miden client initialization and helpers |
| Asset Mapping | `src/asset_mapping.rs` | Origin network/token → Miden faucet asset lookup |
| Types | `src/types.rs` | ClaimAssetParams struct and global index decoding |

---

## JSON-RPC Methods Reference

The proxy implements **17 methods**. Each falls into one of three categories:

### Category 1: Core Bridge Operations

These methods are actively used by the bridge-service for claim processing.

#### `eth_sendRawTransaction`

**The most important method.** Handles two distinct transaction types:

**Type A: GER Injection (from aggoracle)**
```
Input:  RLP-encoded tx targeting 0xa40D5f56745a118D0906a34E69aeC8C0Db1cB8fA
        with selector 0x736ca7f4 (updateExitRoot) or 0x12da06b2 (insertGlobalExitRoot)
Output: Original RLP tx hash (keccak256 of raw bytes)
```
See [Flow 1: GER Injection](#flow-1-ger-injection) for full details.

**Type B: Claim Transaction (from bridge-service)**
```
Input:  RLP-encoded tx with claimAsset selector 0xccaa2d11
        OR raw claimAsset calldata (starts with 0xccaa2d11)
Output: Original RLP tx hash
```
See [Flow 2: Deposit Claim](#flow-2-deposit-claim) for full details.

**Dispatch logic:**
1. Decode RLP → extract `to` address and `input` data
2. If `to` == L2 GER contract AND selector is `updateExitRoot` or `insertGlobalExitRoot` → **GER path**
3. If selector is `claimAsset` → **Claim path**
4. Else → reject with `-32602 "Only claimAsset transactions are supported"`

#### `eth_getTransactionReceipt`

Returns receipt for a transaction by its hash.

```
Input:  tx hash (keccak256 of original RLP bytes)
Output: Receipt object or null
```

**Receipt lifecycle:**
- Transaction not in state → `null`
- Status == Pending → `null` (no receipt yet)
- Status == Confirmed → Receipt with `status: "0x1"`, `blockNumber` from Miden
- Status == Failed → Receipt with `status: "0x0"`

**Key detail:** The `transactionHash` in the receipt is the keccak256 of the
original RLP-encoded transaction bytes. This matches what the bridge-service uses
to track its own transactions, ensuring receipt lookups work correctly.

#### `eth_getLogs`

Returns synthetic logs matching a filter. Two event types are emitted:

```
Input:  LogFilter { fromBlock, toBlock, address, topics }
Output: Array of log objects
```

**Events the bridge expects:**

| Event | Topic Hash | When Emitted |
|-------|-----------|--------------|
| `UpdateHashChainValue(bytes32,bytes32)` | `0x65d3bf366...` | On GER injection |
| `ClaimEvent(uint32,uint32,address,address,uint256)` | `0x25308c93c...` | On successful claim |

See [Flow 3: Bridge Sync](#flow-3-bridge-sync) for details.

#### `eth_call`

Handles read-only bridge contract queries. Returns synthetic responses for known
function selectors:

| Selector | Function | Response |
|----------|----------|----------|
| `0x0e2fcb97` | `lastUpdatedDepositCount()` | `0x0...0` (zero) |
| `0xc7bf8c9d` | `depositCount()` | `0x0...0` (zero) |
| `0xbab161bf` | `networkID()` | `0x0...02` (Miden = network 2) |
| `0x15064c96` | `getRoot()` | `0x0...0` (zero root) |
| Other | Any | `0x0...0` (default zero) |

The bridge-service calls these during initialization to verify connectivity and
read chain state. Returning sensible defaults satisfies these checks.

### Category 2: Chain State Queries

Used by bridge-service for sync and block tracking.

#### `eth_blockNumber`

Fetches the **real** current block height from miden-node via gRPC `sync_state()`.
Not synthetic — this reflects actual Miden network state.

```
Output: hex block number (e.g., "0x1a")
```

#### `eth_getBlockByNumber`

Returns a synthetic EVM block for a given block number. For `"latest"` / `"pending"`,
fetches the real block height from miden-node first.

```
Input:  block number (hex or "latest"/"pending"/"earliest"), fullTransactions bool
Output: Block object with deterministic hash
```

The synthetic block includes standard EVM fields (`hash`, `parentHash`, `timestamp`,
`stateRoot`, etc.) with Miden-appropriate values (zero gas, zero difficulty, empty
transactions list unless claims were recorded at that block).

**Deterministic hashes:** Block hashes are computed as
`keccak256("miden-synthetic-block-v1" || blockNumber || parentHash || timestamp || stateRoot)`.
The timestamp is deterministic: `GENESIS_TIMESTAMP + blockNumber * 12`. This prevents
the bridge-service from detecting false "reorgs" when the proxy restarts.

#### `eth_getBlockByHash`

Reverse lookup: finds block by hash in the hash→number index.

#### `eth_getTransactionByHash`

Returns a minimal transaction object for known tx hashes. Used by bridge-service
to check transaction existence.

#### `eth_getBlockTransactionCountByNumber`

Returns the number of transactions in a given block (from the synthetic block state).

### Category 3: Compatibility Stubs

These return safe defaults to satisfy bridge-service initialization checks.

| Method | Returns | Why |
|--------|---------|-----|
| `eth_chainId` | `0x2` (configurable via `CHAIN_ID`) | Chain identification |
| `eth_gasPrice` | `0x0` | Miden has no gas |
| `eth_estimateGas` | `0x5208` (21000) | Fixed estimate |
| `eth_getTransactionCount` | Synthetic per-address nonce | Nonce tracking |
| `net_version` | `"2"` (decimal chain ID) | EIP-155 |
| `eth_getBalance` | `0x0` | No ETH balances on Miden |
| `eth_getCode` | `0x00` | Minimal bytecode (STOP opcode) — signals "contract exists" |
| `eth_getStorageAt` | `0x0...0` (32 zero bytes) | No EVM storage |

**`eth_getCode` note:** Returns `0x00` (not `0x`). The bridge-service calls
`eth_getCode` on the bridge contract address. An empty response (`0x`) means
"no contract" and causes the bridge to fail. Returning `0x00` (one byte, STOP
opcode) signals that a contract exists at that address.

---

## Flow 1: GER Injection

**Purpose:** The aggoracle injects Global Exit Roots (GERs) from L1 into the L2.
On EVM chains, this writes to the `GlobalExitRootManagerL2SovereignChain` contract.
On Miden, the proxy intercepts these transactions and emits synthetic events.

### Why GERs Matter

GERs prove that a deposit on L1 has been finalized. The bridge-service watches for
`UpdateHashChainValue` events on L2 to know when new exit roots are available. Only
deposits whose exit roots have been injected on L2 become `ready_for_claim`.

Without GER injection, deposits stay in `ready_for_claim=false` forever.

### Sequence

```
aggoracle                    miden-rpc-proxy                    bridge-service
    │                              │                                  │
    │  eth_sendRawTransaction      │                                  │
    │  to: 0xa40D...cB8fA         │                                  │
    │  data: 0x736ca7f4 +         │                                  │
    │        mainnetExitRoot +    │                                  │
    │        rollupExitRoot       │                                  │
    │─────────────────────────────>│                                  │
    │                              │                                  │
    │                              │ 1. Detect target == L2 GER       │
    │                              │    contract address              │
    │                              │                                  │
    │                              │ 2. Detect selector:              │
    │                              │    0x736ca7f4 = updateExitRoot   │
    │                              │    0x12da06b2 = insertGlobalExitRoot
    │                              │                                  │
    │                              │ 3. Parse exit roots from calldata│
    │                              │                                  │
    │                              │ 4. Compute GER:                  │
    │                              │    keccak256(mainnet || rollup)  │
    │                              │    (for updateExitRoot)          │
    │                              │    or use directly               │
    │                              │    (for insertGlobalExitRoot)    │
    │                              │                                  │
    │                              │ 5. Check deduplication           │
    │                              │    (skip if GER already seen)    │
    │                              │                                  │
    │                              │ 6. Update hash chain:            │
    │                              │    new = keccak256(prevChain||GER)│
    │                              │                                  │
    │                              │ 7. Emit UpdateHashChainValue log │
    │                              │    topics[0]: event signature    │
    │                              │    topics[1]: newGlobalExitRoot  │
    │                              │    topics[2]: newHashChainValue  │
    │                              │    data: 0x (empty)              │
    │                              │    address: 0xa40D...cB8fA       │
    │                              │                                  │
    │                              │ 8. Record tx as Confirmed        │
    │                              │                                  │
    │  <── tx hash (original RLP)  │                                  │
    │                              │                                  │
    │                              │          eth_getLogs             │
    │                              │<─────────────────────────────────│
    │                              │                                  │
    │                              │  returns UpdateHashChainValue    │
    │                              │  logs for the block range        │
    │                              │──────────────────────────────────>│
    │                              │                                  │
    │                              │                                  │ deposits become
    │                              │                                  │ ready_for_claim
```

### GER Contract Addresses and Selectors

| Name | Value |
|------|-------|
| L2 GER Contract | `0xa40D5f56745a118D0906a34E69aeC8C0Db1cB8fA` |
| `updateExitRoot(bytes32,bytes32)` selector | `0x736ca7f4` |
| `insertGlobalExitRoot(bytes32)` selector | `0x12da06b2` |
| `UpdateHashChainValue` event topic | `0x65d3bf36615f1f02a134d12dfa9ea6b1d4a52386e825973cd27ddb70895c2319` |

### Two GER Injection Variants

**`updateExitRoot(bytes32 mainnetExitRoot, bytes32 rollupExitRoot)`:**
- Calldata: `selector (4) + mainnetExitRoot (32) + rollupExitRoot (32)` = 68 bytes
- GER computed as: `keccak256(mainnetExitRoot || rollupExitRoot)`

**`insertGlobalExitRoot(bytes32 globalExitRoot)`:**
- Calldata: `selector (4) + globalExitRoot (32)` = 36 bytes
- GER used directly from calldata

### Hash Chain Computation

The proxy maintains a cumulative hash chain value, matching the behavior of
`GlobalExitRootManagerL2SovereignChain`:

```
hashChainValue[0] = 0x0000...0000 (32 zero bytes)
hashChainValue[n] = keccak256(hashChainValue[n-1] || newGER)
```

This value appears as `topics[2]` in the `UpdateHashChainValue` event.

### Deduplication

Each GER is tracked in `LogStore.seen_gers`. If the same GER is injected twice
(e.g., due to aggoracle retry), the second injection is silently ignored — no
duplicate event is emitted.

---

## Flow 2: Deposit Claim

**Purpose:** When a user deposits ETH on L1 and the deposit becomes
`ready_for_claim`, the bridge-service sends a `claimAsset` transaction to the
L2 RPC. The proxy translates this into Miden CLAIM notes.

### Prerequisites

Before a claim can be processed:
1. User deposited ETH on L1 via `bridgeAsset()` on the bridge contract
2. ~64 L1 blocks passed (finality)
3. Bridge-service recorded the deposit in its PostgreSQL database
4. aggoracle injected the relevant GER (see [Flow 1](#flow-1-ger-injection))
5. Deposit status changed to `ready_for_claim=true`

### The claimAsset Calldata

```solidity
function claimAsset(
    bytes32[32] smtProofLocalExitRoot,   // 32 siblings × 32 bytes = 1024 bytes
    bytes32[32] smtProofRollupExitRoot,  // 32 siblings × 32 bytes = 1024 bytes
    uint256     globalIndex,              // 32 bytes
    bytes32     mainnetExitRoot,          // 32 bytes
    bytes32     rollupExitRoot,           // 32 bytes
    uint32      originNetwork,            // 32 bytes (padded)
    address     originTokenAddress,       // 32 bytes (padded)
    uint32      destinationNetwork,       // 32 bytes (padded)
    address     destinationAddress,       // 32 bytes (padded)
    uint256     amount,                   // 32 bytes
    bytes       metadata                  // dynamic
) external;
```

**Function selector:** `0xccaa2d11`

**globalIndex bit layout:**
```
Bit 64    : mainnetFlag (1 = deposit from mainnet, 0 = from rollup)
Bits 32-63: rollupIndex (source rollup ID)
Bits 0-31 : localRootIndex (deposit counter within the exit tree)
```

### Claim Processing Sequence

```
bridge-service                  miden-rpc-proxy                    Miden Network
    │                                 │                                  │
    │  eth_sendRawTransaction         │                                  │
    │  (RLP-encoded claimAsset tx)    │                                  │
    │─────────────────────────────────>│                                  │
    │                                 │                                  │
    │                           ┌─────┴─────┐                            │
    │                           │ STEP 1    │                            │
    │                           │ Decode    │                            │
    │                           └─────┬─────┘                            │
    │                                 │                                  │
    │                   1a. Compute original_tx_hash                     │
    │                       = keccak256(raw RLP bytes)                   │
    │                                 │                                  │
    │                   1b. Decode RLP envelope                          │
    │                       → from, to, input, value, chainId            │
    │                                 │                                  │
    │                   1c. Check: is_claim_asset(input)?                │
    │                       (selector == 0xccaa2d11)                     │
    │                                 │                                  │
    │                   1d. Parse claimAsset params:                     │
    │                       smtProofs, globalIndex, exitRoots,           │
    │                       originNetwork, destAddress, amount           │
    │                                 │                                  │
    │                           ┌─────┴─────┐                            │
    │                           │ STEP 2    │                            │
    │                           │ Validate  │                            │
    │                           └─────┬─────┘                            │
    │                                 │                                  │
    │                   2a. Replay check:                                │
    │                       claim_tracker.try_claim(globalIndex)         │
    │                       → reject if already claimed (-32000)         │
    │                                 │                                  │
    │                   2b. Resolve Ethereum address to Miden:           │
    │                       address_mapper.get_or_create(destAddr)       │
    │                       → MidenAccountId (15 bytes)                  │
    │                                 │                                  │
    │                   2c. Record tx as Pending                         │
    │                                 │                                  │
    │                           ┌─────┴─────┐                            │
    │                           │ STEP 3    │                            │
    │                           │ Scale     │                            │
    │                           └─────┬─────┘                            │
    │                                 │                                  │
    │                   3a. Convert amount:                              │
    │                       18 decimals (wei) → 8 decimals (Miden)       │
    │                       amount_miden = amount_wei / 10^10            │
    │                                 │                                  │
    │                           ┌─────┴─────┐                            │
    │                           │ STEP 4    │                            │
    │                           │ Build     │                            │
    │                           │ CLAIM Note│                            │
    │                           └─────┬─────┘                            │
    │                                 │                                  │
    │                   4a. Convert SMT proofs: bytes → Felts            │
    │                       Each [u8;32] → 8 Felt values (4 bytes each) │
    │                                 │                                  │
    │                   4b. Build BridgeClaimParams:                     │
    │                       smtProofs, globalIndex, exitRoots,           │
    │                       originNetwork, destAddress, amount,          │
    │                       submitter account, faucet account,           │
    │                       P2ID serial number                           │
    │                                 │                                  │
    │                   4c. Call create_bridge_claim_note()              │
    │                       from miden-agglayer library                  │
    │                       → CLAIM note with embedded SMT proofs        │
    │                                 │                                  │
    │                           ┌─────┴─────┐                            │
    │                           │ STEP 5    │                            │
    │                           │ Phase 1:  │                            │
    │                           │ Publish   │                            │
    │                           └─────┬─────┘                            │
    │                                 │                                  │
    │                   5a. Build tx: output = CLAIM note                │
    │                       executor = ephemeral submitter account       │
    │                                 │                                  │
    │                   5b. submit_transaction(submitter, tx)            │
    │                                 │─────────────────────────────────>│
    │                                 │             CLAIM note committed │
    │                                 │<─────────────────────────────────│
    │                                 │                                  │
    │                   5c. Wait for block to advance                    │
    │                       (poll sync_state until blockNum > initial)   │
    │                                 │                                  │
    │                           ┌─────┴─────┐                            │
    │                           │ STEP 6    │                            │
    │                           │ Phase 2:  │                            │
    │                           │ Consume   │                            │
    │                           └─────┬─────┘                            │
    │                                 │                                  │
    │                   6a. Build tx:                                    │
    │                       input = CLAIM note                           │
    │                       executor = agglayer faucet                   │
    │                       foreign_accounts = [bridge account]          │
    │                       expected_outputs = [P2ID note]               │
    │                                 │                                  │
    │                   6b. submit_transaction(faucet, tx)               │
    │                                 │─────────────────────────────────>│
    │                                 │   Faucet validates SMT proofs,   │
    │                                 │   mints P2ID note to recipient   │
    │                                 │<─────────────────────────────────│
    │                                 │                                  │
    │                           ┌─────┴─────┐                            │
    │                           │ STEP 7    │                            │
    │                           │ Record    │                            │
    │                           └─────┬─────┘                            │
    │                                 │                                  │
    │                   7a. Record tx as Confirmed(block_number)         │
    │                   7b. Update block state                           │
    │                   7c. Emit ClaimEvent log:                         │
    │                       topics[0]: ClaimEvent signature              │
    │                       data: leafType + originNetwork +             │
    │                             originAddr + destAddr + amount         │
    │                             (160 bytes, ABI-encoded)               │
    │                                 │                                  │
    │  <── tx hash (original RLP)     │                                  │
    │                                 │                                  │
    │  eth_getTransactionReceipt      │                                  │
    │─────────────────────────────────>│                                  │
    │                                 │                                  │
    │  <── receipt { status: "0x1" }  │                                  │
```

### Two-Phase CLAIM Note Flow

This is the most important architectural detail. CLAIM notes go through two phases:

**Phase 1: Commit** — The ephemeral submitter account creates a transaction with
the CLAIM note as an **output**. This publishes the note on the Miden network.
The note contains the embedded SMT proofs, exit roots, and claim parameters.

**Phase 2: Consume** — The agglayer faucet creates a transaction with the CLAIM
note as an **input**. The faucet's `agglayer_faucet_component` procedures validate
the SMT proofs and exit roots, then mint a P2ID (Pay-to-ID) note directed to the
recipient account. The bridge account is included as a "foreign account" for
Foreign Procedure Invocation (FPI) during proof verification.

**Why two phases?** The Miden execution model requires notes to be committed to the
network before they can be consumed. The note must exist in a proven block, which
requires at least one block advancement between Phase 1 and Phase 2.

### P2ID Script Registration

The first time a claim is processed, the proxy registers the P2ID note script
with the Miden client store. This is a one-time operation (tracked by a static
`AtomicBool`). The P2ID script is needed because the CLAIM note's output is a
P2ID note — the executor must know the P2ID script to verify the transaction.

### Error Recovery

If claim submission fails (Phase 1 or Phase 2):
1. The claim tracker **unclaims** the global index (`claim_tracker.unclaim()`)
2. The transaction is recorded as `Failed { reason }`
3. The RPC returns error `-32000` with the failure message
4. The bridge-service can retry the claim later

---

## Flow 3: Bridge Sync

**Purpose:** The bridge-service continuously polls the proxy to detect GER updates
and claim completions. This keeps the bridge database synchronized with L2 state.

### What the Bridge Polls

The bridge-service's "Synchronizer" component polls two things:

**1. GER Updates** — via `eth_getLogs` with `UpdateHashChainValue` topic filter:
```json
{
  "fromBlock": "0x...",
  "toBlock": "latest",
  "address": "0xa40D5f56745a118D0906a34E69aeC8C0Db1cB8fA",
  "topics": [["0x65d3bf36615f1f02a134d12dfa9ea6b1d4a52386e825973cd27ddb70895c2319"]]
}
```

When the bridge sees a new `UpdateHashChainValue` event, it knows a new GER has
been injected. It updates its internal L2 exit root state, which may cause pending
deposits to become `ready_for_claim`.

**2. Claim Events** — via `eth_getLogs` with `ClaimEvent` topic filter:
```json
{
  "fromBlock": "0x...",
  "toBlock": "latest",
  "address": "<BRIDGE_ADDRESS>",
  "topics": [["0x25308c93ceeed162da955b3f7ce3e3f93606579e40fb92029faa9efe27545983"]]
}
```

When the bridge sees a `ClaimEvent`, it marks the corresponding deposit as claimed
in its database.

### ClaimEvent Data Encoding

The bridge-service v2 ABI decoder expects **exactly 160 bytes** of non-indexed data:

```
Offset  Size  Field                Type      Notes
0       32    leafType             uint32    0 = asset transfer, 1 = message
32      32    originNetwork        uint32    Network where asset originated
64      32    originAddress        address   Token contract on origin network
96      32    destinationAddress   address   Recipient on destination network
128     32    amount               uint256   Token amount
```

**Critical:** The ClaimEvent has **1 topic** (event signature only) and **no indexed
parameters**. Previous bugs:
- Emitting `globalIndex` as `topics[1]` caused "topic/field count mismatch"
- Emitting 128 bytes (missing `leafType`) caused "length insufficient 128 require 160"

### Block Progression

The bridge tracks L2 blocks via `eth_getBlockByNumber("latest")`. It expects:
- Blocks to have increasing numbers
- Block hashes to be stable (same block number → same hash)
- Parent hashes to form a consistent chain

The proxy ensures all of these via deterministic block generation (see
[Synthetic EVM State](#synthetic-evm-state)).

---

## Synthetic EVM State

### Block State (`src/block_state.rs`)

The proxy creates synthetic EVM blocks that map to Miden block numbers:

```
Miden Block N → Synthetic EVM Block N
  number:     N
  timestamp:  1704067200 + N * 12   (deterministic, 12s intervals from 2024-01-01)
  hash:       keccak256("miden-synthetic-block-v1" || N || parentHash || timestamp || stateRoot)
  parentHash: hash of block N-1
  stateRoot:  [0; 32]  (empty)
```

**Deterministic hashes are critical.** If the proxy restarts and generates different
hashes for the same block number, the bridge-service detects a "reorg" and resynchronizes.
This was a real bug that was fixed by removing all randomness from block generation.

The block chain is built lazily: when block N is requested, all missing blocks from
genesis to N are created in order to maintain consistent parent hash chains.

### Log Store (`src/log_synthesis.rs`)

Stores synthetic logs indexed by block number and transaction hash. Supports the
full `eth_getLogs` filter specification:
- Block range (`fromBlock` / `toBlock`)
- Block hash filter
- Address filter (single or array)
- Topic filters (up to 4 positions, with OR matching per position)

**Maximum 1000 logs per query** (per Ethereum JSON-RPC spec).

---

## Account Initialization at Startup

At proxy startup, three Miden accounts are created **once** and reused for all claims:

### 1. Ephemeral Submitter Account
- **Type:** `RegularAccountUpdatableCode`
- **Storage:** Public
- **Auth:** `RpoFalcon512` (key stored in filesystem keystore)
- **Component:** `BasicWallet`
- **Purpose:** Submits CLAIM note transactions (Phase 1)

### 2. Agglayer Faucet Account
- **Auth:** NoAuth (permissionless CLAIM processing)
- **Components:** `agglayer_faucet_component` from `miden-agglayer`
- **Token:** "LUMIA" with 8 decimals, max supply `u64::MAX`
- **Seed:** Deterministic: `keccak256("agglayer_faucet:" || bridge_faucet_id_hex)`
- **Purpose:** Consumes CLAIM notes, validates SMT proofs, mints P2ID to recipients (Phase 2)

### 3. Bridge Account (Local Reference)
- **Auth:** NoAuth
- **Seed:** Deterministic: `keccak256("bridge:" || bridge_faucet_id_hex)`
- **Purpose:** Provides `bridge_account_id` for faucet validation via FPI
- **Note:** Not deployed to network — the actual bridge exists in miden-node genesis

### Why Pre-Initialize?

The Miden client's `add_account()` fails if an account is "already being tracked".
Originally accounts were created per-claim, causing the second claim to fail:
```
Failed to add bridge account: account with id 0x... is already being tracked
```

### Persistence

- **Accounts:** Stored in SQLite store (`MIDEN_STORE_PATH`)
- **Keys:** Stored in filesystem keystore (`MIDEN_STORE_PATH/../keystore`)

This ensures the proxy can sign transactions after restart with the same store paths.

---

## Replay Prevention

The `ClaimTracker` (`src/claim_tracker.rs`) prevents double-processing of claims:

```
                ┌──────────────────────┐
                │    ClaimTracker      │
                │                      │
  try_claim() ──>  DashSet<U256>      │
                │  (lock-free set of   │
                │   global indices)    │
                │                      │
                │  Optional: persist   │
                │  to JSON file        │
                └──────────────────────┘
```

- **`try_claim(globalIndex)`**: Atomically checks if claimed; if not, marks it.
  Returns `Err(AlreadyClaimed)` if duplicate.
- **`unclaim(globalIndex)`**: Rolls back a claim on submission failure.
- **Concurrent safe:** Uses `DashSet` for lock-free multi-threaded access.
- **Persistence:** Optional file-based persistence (`claimed_indices.json`).

---

## Address Mapping

The `AddressMapper` (`src/address_mapper.rs`) converts 160-bit Ethereum addresses
to 120-bit Miden AccountIds:

```
Ethereum Address (20 bytes)
    │
    ├── Lookup in SQLite: known mapping?
    │   └── Yes → return existing MidenAccountId
    │
    └── No → Derive deterministically:
            seed = keccak256("miden-account-id-v1" || eth_address)
            account_id = first 15 bytes of seed
            → Store in SQLite for future lookups
```

This is a one-way mapping. The same Ethereum address always produces the same
Miden AccountId.

---

## Amount Scaling

Bridge amounts are in 18-decimal wei (Ethereum standard). Miden faucets use 8 decimals:

```
amount_miden = amount_wei / 10^10

Example:
  0.1 ETH = 100000000000000000 wei
  / 10000000000
  = 10000000 Miden raw units
  = 0.10000000 (with 8 decimals)
```

The division happens in `send_raw_transaction` before building the `ClaimSubmissionData`.

---

## Kurtosis Deployment Architecture

The Kurtosis package (`kurtosis/miden-cdk/`) deploys the full Miden bridge stack:

```
kurtosis-cdk (upstream)
├── L1: geth/anvil + lighthouse
├── Bridge contracts
├── Bridge service          ──── eth_getLogs, eth_call ────┐
├── aggoracle               ──── eth_sendRawTransaction ───┤
├── aggsender               ──── reads bridge DB ──────────┤
│                                                          │
│   ┌──────────────────────────────────────────────────────┤
│   │                                                      │
│   ▼ miden-cdk (custom package)                           │
│   ├── miden-node-001      (:57291 gRPC)                  │
│   ├── miden-proxy-001     (:8546 JSON-RPC) ◄─────────────┘
│   ├── miden-l2-forwarder  (:8545 → :8546 nginx)
│   ├── miden-bridge-ui     (:80 / :3000 web dApp)
│   └── pgweb               (:8082 DB browser)
```

### Service Details

| Service | Image | Port | Purpose |
|---------|-------|------|---------|
| miden-node | `miden-infra/miden-node:agglayer-v0.1` | 57291 | Miden network node |
| miden-proxy | `miden-infra/miden-proxy:latest` | 8546 | This proxy |
| miden-l2-forwarder | `nginx:alpine` | 8545 | TCP forwarding (bridge expects 8545) |
| miden-bridge-ui | `miden-infra/bridge-ui:latest` | 80 | Deposit web UI |
| pgweb | `sosedoff/pgweb` | 8082 | Bridge DB browser |

### Why the nginx Forwarder?

The bridge-service hardcodes its L2 RPC on port 8545. The proxy listens on 8546
(to avoid conflicts). The nginx forwarder bridges the gap with a TCP stream proxy:

```nginx
stream {
    upstream miden_proxy {
        server miden-proxy-001:8546;
    }
    server {
        listen 8545;
        proxy_pass miden_proxy;
    }
}
```

### Configuration Flow

Bridge configs (`aggkit-bridge`, `zkevm-bridge-service`) are patched to point
their L2 RPC URLs to `http://miden-l2-forwarder-001:8545` instead of the default
OP-Geth endpoint. This single URL change is what makes the entire bridge stack
talk to Miden instead of an EVM L2.

### Web UI (Deposit dApp)

The deposit UI (`web-ui/`) is a single-page vanilla JS application served by nginx:
- Connects to any EIP-1193 wallet (MetaMask, etc.)
- Calls `bridgeAsset()` on the L1 bridge contract
- Bridge contract address injected at container start via environment variable
- Deployed as `miden-bridge-ui` service in Kurtosis (`deploy_web_ui: true` in params)

---

## Environment Variables

| Variable | Default | Description |
|----------|---------|-------------|
| `MIDEN_RPC_URL` | `http://localhost:57291` | Miden node gRPC endpoint |
| `BRIDGE_FAUCET_ID` | (required) | Bridge faucet account ID from genesis (hex) |
| `BRIDGE_ADDRESS` | `0xc8cb...ff038` | L1 bridge contract address for receipts/events |
| `CHAIN_ID` | `2` | Chain ID for `eth_chainId` and EIP-155 |
| `LISTEN_HOST` | `0.0.0.0` | HTTP server bind address |
| `LISTEN_PORT` | `8546` | HTTP server port |
| `MIDEN_STORE_PATH` | `/app/data/miden-client` | SQLite store directory |
| `RUST_LOG` | (none) | Logging level (e.g., `info,miden_rpc_proxy=debug`) |

---

## Debugging Tools

| Script | Purpose |
|--------|---------|
| `scripts/list-notes.sh` | List all notes tracked by the proxy |
| `scripts/list-unclaimed-notes.sh` | List notes that have not been claimed yet |
| `scripts/health-check.sh` | Check proxy and miden-node health status |
| `scripts/verify-claim-notes.sh` | Verify all CLAIM notes from proxy logs |

Use `list-notes.sh` and `list-unclaimed-notes.sh` to inspect note state during
claim flow debugging. These scripts query the proxy's internal note tracking.
