# GER Injection Flow -- Security Audit Document

**System**: miden-agglayer proxy
**Repository**: https://github.com/gateway-fm/miden-agglayer (branch: `fix/remove-derive-account-id`)
**Date**: 2026-03-30
**Audience**: Security auditors

---

## 1. System Context

The miden-agglayer proxy sits between the **zkevm-bridge-service** (an Ethereum-oriented bridge UI/backend) and the **Miden L2 network**. Its purpose is to present an EVM-compatible JSON-RPC interface so that existing Polygon CDK tooling (aggoracle, bridge-service, claim-sponsor) can interact with Miden without modification.

The proxy translates a small subset of EVM JSON-RPC calls (`eth_sendRawTransaction`, `eth_getLogs`, `eth_getBlockByNumber`, etc.) into Miden-native operations (note creation, transaction submission, state queries).

The Global Exit Root (GER) injection flow is one of the two primary write paths through the proxy (the other being claim processing).

---

## 2. GER Injection Flow

### 2.1 End-to-End Sequence

```
L1 GER Contract
    |
    v
aggkit aggoracle (polls L1, detects new GER)
    |
    | eth_sendRawTransaction(insertGlobalExitRoot(bytes32))
    v
miden-agglayer proxy
    |
    |--- 1. Decode signed TX envelope (EIP-1559 or Legacy)
    |--- 2. Extract calldata: insertGlobalExitRoot(bytes32 ger)
    |--- 3. Dedup check (has_seen_ger)
    |--- 4. Sync miden-client state (sync_state)
    |--- 5. Build UpdateGerNote (B2AggNote from miden-base-agglayer)
    |--- 6. Submit note as TX from service account
    |--- 7. Poll for commitment (up to 30s, retry up to 3x)
    |--- 8. Write synthetic InsertGlobalExitRoot event to store
    |--- 9. Advance latest_block_number
    v
Miden Node (validates UpdateGerNote against bridge account GER set)
```

### 2.2 Detailed Step Breakdown

**Step 1-2: Transaction Routing** (`service_send_raw_txn.rs`)

The proxy receives a signed Ethereum transaction via `eth_sendRawTransaction`. It decodes the RLP-encoded transaction envelope (supporting both EIP-1559 and Legacy formats), extracts the calldata, and routes based on the function selector. The `insertGlobalExitRoot(bytes32)` selector triggers the `handle_ger_result` code path.

**Step 3: Deduplication** (`ger.rs` -- `insert_ger()`)

Before submitting to Miden, the proxy checks the PostgreSQL store for whether this GER hash has already been processed (`has_seen_ger`). If the GER is already present, the call returns early with success.

**Step 4-7: Miden Submission** (`ger.rs` -- `submit_ger_to_miden()`)

1. **State sync**: Calls `sync_state()` on the miden-client to ensure the local state matches the node's latest committed state. This is critical because the transaction commitment hash depends on the current account state.
2. **Note creation**: Constructs an `UpdateGerNote` using `miden-base-agglayer`'s `B2AggNote` types. The GER bytes are wrapped in an `ExitRoot` type.
3. **Transaction submission**: Submits the note as a transaction from the proxy's service account.
4. **Commitment polling**: Polls the miden-node for transaction commitment, waiting up to 30 seconds.
5. **Retry logic**: If the commitment fails due to stale state (commitment mismatch), the proxy retries up to 3 times, re-syncing state before each attempt.

**Step 8-9: Synthetic Event and Block** (`ger.rs`, `block_state.rs`, `store/postgres.rs`)

After the Miden transaction is committed:
1. A synthetic `InsertGlobalExitRoot` event log is written to PostgreSQL, encoded with the standard ABI event signature.
2. The `latest_block_number` is advanced so that the bridge-service discovers the new block on its next poll.

---

## 3. Synthetic Block and Event System

### 3.1 Purpose

The bridge-service expects to interact with an EVM chain. It polls for new blocks via `eth_getBlockByNumber` and scans for events via `eth_getLogs`. The proxy fabricates deterministic EVM blocks and events to satisfy these expectations.

### 3.2 Block Construction (`block_state.rs`)

- Each significant event (GER injection, claim completion) triggers a new synthetic block.
- Block numbers are sequential integers, advanced only after the underlying Miden operation commits.
- Block hashes are computed as `keccak256(rlp(header))` where the header fields are deterministic (no miner randomness, no uncle hashes, etc.).
- This deterministic construction satisfies the bridge-service's `checkReorg` detection, which compares stored block hashes against queried block hashes.

### 3.3 Event Encoding (`log_synthesis.rs`)

- The `InsertGlobalExitRoot` event uses the standard Solidity event topic.
- Event data matches the ABI encoding that the bridge-service expects to decode.
- Events are stored in PostgreSQL and returned via `eth_getLogs` filtered by topic and block range.

---

## 4. Trust Assumptions

| # | Assumption | Implication |
|---|-----------|-------------|
| 1 | The aggoracle is honest and correctly relays L1 GERs. | The proxy validates the TX envelope (signature, RLP structure) but does **not** independently verify the GER value against L1. A compromised aggoracle could inject arbitrary GER values. |
| 2 | The proxy's service account private key is securely managed. | This account is the sole entity authorized to create `UpdateGerNote` transactions. Compromise of this key allows arbitrary GER injection into Miden. |
| 3 | The miden-node validates `UpdateGerNote` against the bridge account's registered GER set. | This provides a second layer of validation at the protocol level, but only if the bridge account's GER set is itself correctly maintained. |
| 4 | Synthetic events in PostgreSQL are trusted by the bridge-service. | The bridge-service has no way to independently verify that a synthetic event corresponds to an actual Miden transaction. It trusts the proxy's PostgreSQL store entirely. |
| 5 | The PostgreSQL instance is not accessible to unauthorized parties. | Direct modification of the store could inject phantom events or suppress real ones. |

---

## 5. Security Considerations

### 5.1 What the Proxy Controls

- **Synthetic block numbers and hashes**: Purely deterministic from the event sequence. No manipulation is possible without also modifying the store.
- **`latest_block_number` advancement**: Controls what the bridge-service can observe. Delaying advancement delays bridge-service awareness; premature advancement (before event write) can cause the bridge to miss events.
- **GER-to-Miden mapping**: The proxy is the sole translator between L1 GER events and Miden `UpdateGerNote` transactions.

### 5.2 Attack Vectors

#### 5.2.1 Stale GER Injection

**Vector**: If the proxy's miden-client state is stale (e.g., another transaction committed between sync and submit), the `UpdateGerNote` transaction will be rejected by the node due to commitment mismatch.

**Mitigation**: `sync_state()` is called before each submission attempt. On commitment mismatch, the proxy retries up to 3 times with a fresh sync before each retry.

**Residual risk**: If the node is under heavy load and state changes rapidly, all 3 retries may fail. The GER injection is then lost until the aggoracle retries (which introduces its own issues; see Known Issues).

#### 5.2.2 Block Number Race Condition

**Vector**: The synthetic block number was previously assigned **before** the Miden transaction committed. This caused the bridge-service to poll the new block before the event was written to the store, resulting in the bridge seeing an empty block and never re-scanning it.

**Mitigation**: Fixed by assigning the block number **after** the Miden transaction commits and the synthetic event is written to the store. The ordering is now: Miden commit -> event write -> block number advance.

**Residual risk**: A crash between event write and block number advance would leave the event orphaned (written but not discoverable). The `restore.rs` module can reconstruct state from consumed notes to recover.

#### 5.2.3 Duplicate GER

**Vector**: The aggoracle may retry a GER injection if it does not receive a timely response.

**Mitigation**: Two layers of deduplication:
1. The proxy checks `has_seen_ger` in the PostgreSQL store before submission.
2. The miden-node validates against the bridge account's GER set and rejects duplicates.

**Residual risk**: Minimal. Both layers must fail for a duplicate to be accepted.

#### 5.2.4 Missing GER (Crash Recovery)

**Vector**: If the proxy crashes after the Miden transaction commits but before the synthetic log is written to PostgreSQL, the GER exists on Miden but is invisible to the bridge-service.

**Mitigation**: The `restore.rs` module can reconstruct proxy state by scanning consumed notes on the Miden network and replaying them as synthetic events.

**Residual risk**: Recovery requires manual intervention (or a restart that triggers restore logic). During the gap, the bridge-service is unaware of the GER.

#### 5.2.5 Concurrent Claims and GER Injections

**Vector**: Claims and GER injections both use the proxy's service account. If both operations are in flight simultaneously, their state snapshots can conflict, causing commitment mismatches on one or both operations.

**Mitigation**: `sync_state()` before each operation plus retry logic. The miden-client access is serialized (see `miden_client.rs`).

**Residual risk**: Under high concurrency, retry storms can degrade throughput. No data corruption risk -- only liveness impact.

#### 5.2.6 Unauthorized GER Injection

**Vector**: An attacker sends a crafted `eth_sendRawTransaction` with the `insertGlobalExitRoot` selector.

**Mitigation**: The proxy validates the TX envelope (RLP decode, signature extraction). However, the proxy does not enforce a sender allowlist for GER injections -- it processes any well-formed transaction with the correct selector. The miden-node provides the authorization check via the service account's permissions.

**Residual risk**: If sender validation is desired at the proxy layer, it must be explicitly implemented.

### 5.3 Data Flow Integrity

```
L1 GER (bytes32) --> aggoracle calldata --> proxy decodes --> ExitRoot type --> UpdateGerNote --> Miden
```

- The 32-byte GER value is passed through without modification at every stage.
- The proxy adds metadata (block number, timestamp, hash chain value) but does not alter the GER itself.
- The `UpdateGerNote` wraps the GER in an `ExitRoot` type from `miden-base-agglayer`, which is the type the Miden bridge account expects.

---

## 6. Key Management and Account Authorization

### 6.1 Account Hierarchy

The proxy manages several Miden accounts, each with a specific role:

| Account | Role | Created by | Has private key |
|---------|------|-----------|-----------------|
| **Service** | Signs GER injection and claim TXs | Proxy init (`init.rs:55`) | Yes (in proxy's keystore) |
| **Bridge** | Receives and consumes CLAIM/B2AGG notes | Proxy init (`init.rs:62`) | Yes |
| **ETH Faucet** | Mints bridged ETH tokens | Proxy init (`faucet_ops.rs:50`) | Yes |
| **AGG Faucet** | Mints bridged AGG tokens | Proxy init | Yes |
| **wallet_hardhat** | Receives P2ID notes from faucets | Proxy init | Yes |
| **Dynamic faucets** | Mints bridged ERC-20 tokens | On-demand (`claim.rs:111`) | Yes |

All private keys are stored in the proxy's miden-client SQLite store at `/var/lib/miden-agglayer-service/store.sqlite3` and the `keystore/` directory.

### 6.2 GER Injection Authorization

The miden-node enforces authorization for GER injection through the following chain:

1. **Note creation**: The proxy's **service account** creates an `UpdateGerNote` (source: `ger.rs:107`). The note targets the **bridge account**.

2. **Note script validation**: The `UpdateGerNote` script (from `miden-base-agglayer` crate) verifies that the note was created by an authorized account. The bridge account's storage contains a list of authorized GER updaters.

3. **Bridge account registration**: During proxy init, the service account is registered as an authorized GER updater on the bridge account (source: `init.rs`, faucet registration flow).

4. **Miden node validation**: The node's block producer validates the transaction against the accounts' current state. If the service account is not in the bridge's authorized set, the TX is rejected.

### 6.3 How the Node Allowlists the GER Injector

The bridge account on Miden has an internal storage map that tracks:
- Registered faucets (which tokens can be minted)
- Authorized GER updaters (which accounts can inject GERs)
- The current GER set (which GERs have been injected)

When the proxy registers faucets during init (`faucet_ops.rs:89-117`), it also registers the service account as authorized. This registration is itself a Miden transaction that modifies the bridge account's storage.

**Key insight**: If the service account's private key is compromised, an attacker can inject arbitrary GERs into the bridge account. The miden-node will accept them because the service account is in the authorized set. There is no secondary validation of the GER value against L1.

### 6.4 External Key: aggoracle Signing Key

The aggoracle (part of aggkit) signs `eth_sendRawTransaction` calls with its own Ethereum private key (`aggoracle.keystore`). The proxy validates the EIP-1559/Legacy TX envelope but does **not** check the signer address against an allowlist. Any valid Ethereum signature is accepted.

The proxy routes the transaction based on the **calldata selector** (`insertGlobalExitRoot`, `updateExitRoot`, `claimAsset`), not the sender. This means any account can send GER injection transactions to the proxy.

**Recommendation**: Consider adding an optional sender allowlist in the proxy for GER injection transactions, configurable via environment variable or config file.

---

## 7. Key Source Files

| File | Responsibility |
|------|---------------|
| `src/ger.rs` | GER injection logic: `insert_ger()`, `submit_ger_to_miden()`, dedup, retry |
| `src/service_send_raw_txn.rs` | Transaction routing, calldata decoding, `handle_ger_result()` |
| `src/block_state.rs` | Synthetic block number and hash management |
| `src/store/postgres.rs` | Persistent storage for synthetic events and GER dedup state |
| `src/log_synthesis.rs` | Event encoding (`InsertGlobalExitRoot` topic and ABI data) |
| `src/miden_client.rs` | Miden node client wrapper with serialized access to `sync_state()` and TX submission |
| `src/restore.rs` | Crash recovery: reconstructs proxy state from consumed Miden notes |

---

## 7. Known Issues

### 7.1 aggoracle ethtxmanager Stuck State

**Symptom**: After a GER injection fails (e.g., commitment mismatch returned as an RPC error), the aggoracle's ethtxmanager marks the transaction as evicted but does not clear it from its in-memory queue. All subsequent GER injections are blocked.

**Impact**: New L1 GERs are not propagated to Miden until the aggkit is restarted.

**Workaround**: Restart the aggkit process to clear in-memory ethtxmanager state.

**Root cause**: The ethtxmanager was designed for Ethereum, where "already exists" means the TX is in the mempool. In the proxy context, an error means the TX was rejected and should be retried, but the etxtxmanager has no "rejected, please retry" state.

### 7.2 bridge-service checkReorg Event Discovery Gap

**Symptom**: The bridge-service's `checkReorg` mode only compares block hashes via `eth_getBlockByNumber` -- it does not call `eth_getLogs`. If the proxy writes a synthetic event to a block that the bridge has already passed during a `checkReorg` cycle, the event is never discovered.

**Impact**: The bridge-service misses the GER event. Deposits gated on that GER cannot be claimed.

**Workaround**: Restart the bridge-service to force a full resync from the last finalized block.

**Root cause**: The bridge-service assumes that `checkReorg` only needs to verify block continuity, not re-scan for events. This assumption holds for real EVM chains but not for the proxy's synthetic block system where events can be written to blocks after the block number is published.

---

## 8. Recommendations for Auditors

1. **Verify GER passthrough integrity**: Confirm that the 32-byte GER value in the aggoracle's calldata is identical to the value in the `UpdateGerNote` submitted to Miden. Key code path: `service_send_raw_txn.rs` decode -> `ger.rs` -> `B2AggNote` construction.

2. **Review service account authorization**: The service account is the trust anchor for all proxy-to-Miden operations. Verify how the account ID and credentials are provisioned, stored, and rotated.

3. **Assess crash recovery completeness**: The `restore.rs` module is the primary recovery mechanism. Verify that it correctly handles all edge cases (partial writes, duplicate notes, reorged Miden blocks).

4. **Evaluate sender validation**: The proxy does not enforce a sender allowlist for GER injection transactions. Assess whether this is acceptable given the deployment context or whether an allowlist should be added.

5. **Test concurrent operations**: Submit GER injections and claims simultaneously to verify that the serialized miden-client access and retry logic correctly handle contention without data loss.
