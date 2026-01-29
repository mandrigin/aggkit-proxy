# Debugging Guide: Deposits Not Appearing as Claims on Miden

When an L1 deposit doesn't result in a claimed asset on Miden, the problem is
somewhere in a 6-stage pipeline. This guide walks through each stage with exact
commands for the kurtosis-cdk deployment.

## The Pipeline

```
Stage 1          Stage 2           Stage 3           Stage 4           Stage 5           Stage 6
L1 Deposit  →  Bridge DB     →  GER Injection  →  Bridge Claims  →  Proxy CLAIM    →  Faucet Mint
(user/test)    (postgres)       (aggoracle→proxy)  (bridge-service)   (Phase 1)         (Phase 2)
                                                                      commit note       consume → P2ID
```

A failure at any stage stops the flow. Work backwards from where you expect
to see results, or forwards from the deposit.

---

## Quick Triage

Run this first to narrow down the stage:

```bash
# 1. Is the deposit in the bridge DB?
kurtosis service exec miden-cdk postgres-001 \
  "psql -U bridge_user -d bridge_db \
   -c \"SELECT deposit_cnt, dest_net, amount, ready_for_claim, dest_addr
        FROM sync.deposit WHERE dest_net = 2
        ORDER BY deposit_cnt DESC LIMIT 5;\""

# 2. Have GERs been injected?
kurtosis service logs miden-cdk miden-proxy-001 | grep -c "GER injection processed"

# 3. Has bridge-service sent any claims?
kurtosis service logs miden-cdk miden-proxy-001 | grep -c "claimAsset parameters parsed"

# 4. Did claims complete?
kurtosis service logs miden-cdk miden-proxy-001 | grep -c "CLAIM PROCESSING COMPLETE"
```

**Reading the results:**

| Deposit in DB? | ready_for_claim? | GERs injected? | Claims sent? | Claims complete? | Problem stage |
|:-:|:-:|:-:|:-:|:-:|:--|
| No | - | - | - | - | Stage 1: L1 deposit |
| Yes | No | No | - | - | Stage 3: GER injection |
| Yes | No | Yes | - | - | Stage 3: GER not matching deposit |
| Yes | Yes | Yes | No | - | Stage 4: Bridge-service |
| Yes | Yes | Yes | Yes | No | Stage 5/6: Proxy claim processing |
| Yes | Yes | Yes | Yes | Yes | Not stuck — check P2ID note |

---

## Stage 1: L1 Deposit

**What happens:** User or test script calls `bridgeAsset()` on the L1 bridge contract.

**Check: Did the deposit transaction land on L1?**

```bash
# Check L1 block number is advancing
kurtosis service exec miden-cdk geth-1l \
  "cast block-number --rpc-url http://localhost:8545"

# If using send-deposit.sh, check its output for the tx hash
./scripts/send-deposit.sh 0.01
# Look for: "Transaction hash: 0x..."
```

**Common failures:**
- L1 node (anvil/geth) not running or not mining
- Insufficient funds in sender account
- Wrong bridge contract address

---

## Stage 2: Bridge DB (PostgreSQL)

**What happens:** Bridge-service monitors L1 and records deposits in PostgreSQL.

**Service:** `postgres-001` (port 5432, user `bridge_user`, db `bridge_db`)

### Check deposits

```bash
# All deposits targeting Miden (dest_net = 2)
kurtosis service exec miden-cdk postgres-001 \
  "psql -U bridge_user -d bridge_db -c \"
    SELECT deposit_cnt, dest_net, orig_net,
           amount, ready_for_claim, dest_addr,
           block_num, tx_hash
    FROM sync.deposit
    WHERE dest_net = 2
    ORDER BY deposit_cnt DESC
    LIMIT 20;\""
```

### Check claim status

```bash
# Have any claims been recorded?
kurtosis service exec miden-cdk postgres-001 \
  "psql -U bridge_user -d bridge_db -c \"
    SELECT index, orig_net, dest_net, dest_addr, amount, block_num
    FROM sync.claim
    ORDER BY index DESC
    LIMIT 10;\""
```

### Check what's pending

```bash
# Deposits that are ready but not yet claimed
kurtosis service exec miden-cdk postgres-001 \
  "psql -U bridge_user -d bridge_db -c \"
    SELECT deposit_cnt, amount, ready_for_claim, dest_addr
    FROM sync.deposit
    WHERE dest_net = 2 AND ready_for_claim = true
    ORDER BY deposit_cnt DESC;\""
```

**Key columns:**
| Column | Meaning |
|--------|---------|
| `deposit_cnt` | Deposit counter (used as `localRootIndex` in globalIndex) |
| `dest_net` | Destination network (`2` = Miden) |
| `ready_for_claim` | `true` once L1 finality reached AND GER injected on L2 |
| `amount` | Wei amount (18 decimals) |
| `dest_addr` | Ethereum-format recipient address |

**If deposit is missing from DB:**
- Bridge-service may not be syncing L1 — check its logs
- L1 block hasn't been confirmed yet

```bash
kurtosis service logs miden-cdk zkevm-bridge-service-001 2>&1 | tail -50
```

**If `ready_for_claim = false`:**
- L1 finality not reached (~64 blocks on mainnet, faster on local anvil)
- GER not injected yet (see Stage 3)

---

## Stage 3: GER Injection (aggoracle → proxy)

**What happens:** The aggoracle reads Global Exit Roots from L1 and injects them
into L2 by sending `updateExitRoot` or `insertGlobalExitRoot` transactions to
the proxy (via the nginx forwarder on port 8545).

The proxy intercepts these, stores the GER, and emits synthetic
`UpdateHashChainValue` events. Bridge-service polls `eth_getLogs` for these
events — when it sees them, deposits matching that exit root become
`ready_for_claim = true`.

### Check if GERs are being injected

```bash
# Count GER injections
kurtosis service logs miden-cdk miden-proxy-001 | \
  grep -E "GER injection.*detected" | wc -l

# See the actual GER values
kurtosis service logs miden-cdk miden-proxy-001 | \
  grep -E "Parsed GER|Computed GER|GER injection processed"

# Check for duplicate GERs (normal, handled by dedup)
kurtosis service logs miden-cdk miden-proxy-001 | \
  grep "GER already seen"
```

### Check aggoracle is running and sending

```bash
# aggoracle is part of the aggkit service
kurtosis service logs miden-cdk aggkit-001 2>&1 | \
  grep -iE "exit.?root|GER|inject" | tail -20
```

### Check bridge-service is polling for GER events

```bash
# Bridge-service polls eth_getLogs for UpdateHashChainValue events
kurtosis service logs miden-cdk miden-proxy-001 | \
  grep "eth_getLogs" | tail -10
```

**Topic hash for UpdateHashChainValue:**
`0x65d3bf36615f1f02a134d12dfa9ea6b1d4a52386e825973cd27ddb70895c2319`

### Verify manually that GER endpoint works

```bash
# Call eth_getLogs directly to see GER events
kurtosis service exec miden-cdk miden-proxy-001 \
  "curl -s http://localhost:8546 -X POST \
    -H 'Content-Type: application/json' \
    -d '{\"jsonrpc\":\"2.0\",\"method\":\"eth_getLogs\",\"params\":[{
      \"fromBlock\":\"0x0\",\"toBlock\":\"latest\",
      \"topics\":[[\"0x65d3bf36615f1f02a134d12dfa9ea6b1d4a52386e825973cd27ddb70895c2319\"]]
    }],\"id\":1}'" | python3 -m json.tool
```

**Common failures:**
- aggoracle not started or crashlooping
- nginx forwarder not routing 8545 → 8546
- Proxy rejecting GER transactions (check logs for errors)
- L1 hasn't produced enough blocks for exit root finality

---

## Stage 4: Bridge-Service Sends Claims

**What happens:** Once `ready_for_claim = true`, bridge-service builds a
`claimAsset` transaction with SMT proofs and sends it to the L2 RPC endpoint.

### Check bridge-service claim activity

```bash
# Look for claim-related activity
kurtosis service logs miden-cdk zkevm-bridge-service-001 2>&1 | \
  grep -iE "claim|send.*transaction|ready" | tail -20

# Check for errors
kurtosis service logs miden-cdk zkevm-bridge-service-001 2>&1 | \
  grep -iE "error|fail" | tail -20
```

### Verify proxy received claimAsset transactions

```bash
# Did the proxy see any claimAsset calls?
kurtosis service logs miden-cdk miden-proxy-001 | \
  grep "claimAsset parameters parsed"

# Look for the specific deposit index
kurtosis service logs miden-cdk miden-proxy-001 | \
  grep "local_root_index"
```

**Common failures:**
- Bridge-service polling interval hasn't triggered yet (can take minutes)
- Bridge-service can't reach L2 endpoint (nginx forwarder issue)
- Bridge-service failed to build SMT proofs (L1 data issue)

### Check connectivity through the forwarder

```bash
# Verify the nginx forwarder routes correctly
kurtosis service exec miden-cdk miden-l2-forwarder-001 \
  "curl -s http://localhost:8545 -X POST \
    -H 'Content-Type: application/json' \
    -d '{\"jsonrpc\":\"2.0\",\"method\":\"eth_chainId\",\"params\":[],\"id\":1}'"
# Expected: {"jsonrpc":"2.0","result":"0x2","id":1}
```

---

## Stage 5: Proxy Processes Claim (Phase 1 — Commit CLAIM Note)

**What happens:** The proxy decodes the `claimAsset` calldata, checks for replay,
scales the amount from 18 to 8 decimals, builds a CLAIM note with the SMT proofs
embedded, and submits it to the Miden network.

### Watch the full claim flow

```bash
# The proxy logs steps 1-7 with box-drawing headers
kurtosis service logs miden-cdk miden-proxy-001 | \
  grep -E "STEP [1-7]:|CLAIM ASSET DETAILS|CLAIM PROCESSING COMPLETE"
```

### Check Phase 1 specifically

```bash
kurtosis service logs miden-cdk miden-proxy-001 | \
  grep -E "Phase 1|CLAIM note submitted|Waiting for CLAIM note"
```

**What to look for:**

```
✓ Phase 1 complete - CLAIM note submitted to network     → Phase 1 OK
Waiting for CLAIM note to be included in a proven block   → Waiting for block
⚠ Timeout waiting for block to advance                    → Block didn't advance
```

### Check for replay rejection

```bash
# If the same deposit is claimed twice, the tracker rejects it
kurtosis service logs miden-cdk miden-proxy-001 | \
  grep -iE "already claimed|duplicate|replay"
```

**Claim tracker persistence file:** `claims.json` inside the container at
`$MIDEN_STORE_PATH/../` (default: `/app/data/claims.json`).

### Check amount scaling

```bash
# Look for the claim details block
kurtosis service logs miden-cdk miden-proxy-001 | \
  grep -A5 "CLAIM ASSET DETAILS"
```

The proxy scales amounts: `amount_miden = amount_wei / 10^10`.
If you see `u64::MAX` (18446744073709551615), the ABI decode offset is wrong.

**Common failures:**
- Miden node not reachable (gRPC to `miden-node-001:57291`)
- Ephemeral submitter account not initialized
- Block doesn't advance (miden-node stuck)

### Check miden-node health

```bash
# Is miden-node responding?
kurtosis service exec miden-cdk miden-proxy-001 \
  "curl -s http://localhost:8546 -X POST \
    -H 'Content-Type: application/json' \
    -d '{\"jsonrpc\":\"2.0\",\"method\":\"eth_blockNumber\",\"params\":[],\"id\":1}'"

# Check miden-node logs
kurtosis service logs miden-cdk miden-node-001 2>&1 | tail -20
```

---

## Stage 6: Faucet Consumes CLAIM Note (Phase 2 — Mint P2ID)

**What happens:** The agglayer faucet creates a transaction that consumes the
CLAIM note, validates the SMT proofs, and mints a P2ID (Pay-to-ID) note to the
recipient's Miden account.

### Check Phase 2

```bash
kurtosis service logs miden-cdk miden-proxy-001 | \
  grep -E "Phase 2|consume CLAIM|mint P2ID|CLAIM PROCESSING COMPLETE"
```

**What success looks like:**

```
Now the faucet will consume the CLAIM note and mint P2ID...
Submitting faucet transaction to consume CLAIM and mint P2ID...
║  CLAIM PROCESSING COMPLETE                                        ║
```

### Check for Phase 2 failures

```bash
kurtosis service logs miden-cdk miden-proxy-001 | \
  grep -iE "Phase 2.*error|Phase 2.*fail|faucet.*error|foreign.*account"
```

**Common failures:**
- Faucet account not matching genesis faucet (account ID mismatch)
- SMT proof validation failed inside faucet component
- Bridge account not available for Foreign Procedure Invocation (FPI)
- CLAIM note not yet in a proven block (Phase 1 block didn't advance)

### Verify notes on miden-node

```bash
# List all notes (including consumed CLAIM notes)
./scripts/list-notes.sh

# List only unclaimed P2ID notes (the result of successful claims)
./scripts/list-unclaimed-notes.sh

# Verify a specific CLAIM note exists
./scripts/verify-notes.sh --note-id 0x<note_id>
```

**Important:** After a successful claim, the CLAIM note will show as **consumed**
(it was consumed by the faucet). The resulting P2ID note has a **different ID**.
So "note not found" for a CLAIM note ID is expected after successful processing.

---

## Full Diagnostic Dump

When you need everything at once:

```bash
#!/bin/bash
# Save as: debug-deposits.sh
echo "=== PROXY HEALTH ==="
kurtosis service exec miden-cdk miden-proxy-001 \
  "curl -s http://localhost:8546 -X POST \
    -H 'Content-Type: application/json' \
    -d '{\"jsonrpc\":\"2.0\",\"method\":\"eth_blockNumber\",\"params\":[],\"id\":1}'"
echo ""

echo "=== DEPOSITS IN BRIDGE DB ==="
kurtosis service exec miden-cdk postgres-001 \
  "psql -U bridge_user -d bridge_db -c \"
    SELECT deposit_cnt, amount, ready_for_claim, dest_addr
    FROM sync.deposit WHERE dest_net = 2
    ORDER BY deposit_cnt DESC LIMIT 10;\""

echo "=== CLAIMS IN BRIDGE DB ==="
kurtosis service exec miden-cdk postgres-001 \
  "psql -U bridge_user -d bridge_db -c \"
    SELECT index, orig_net, dest_net, amount
    FROM sync.claim
    ORDER BY index DESC LIMIT 10;\""

echo "=== GER INJECTION COUNT ==="
kurtosis service logs miden-cdk miden-proxy-001 | \
  grep -c "GER injection processed"

echo "=== CLAIMS RECEIVED ==="
kurtosis service logs miden-cdk miden-proxy-001 | \
  grep "claimAsset parameters parsed"

echo "=== CLAIMS COMPLETED ==="
kurtosis service logs miden-cdk miden-proxy-001 | \
  grep "CLAIM PROCESSING COMPLETE"

echo "=== RECENT ERRORS (proxy) ==="
kurtosis service logs miden-cdk miden-proxy-001 | \
  grep -iE "error|panic|fatal" | tail -10

echo "=== RECENT ERRORS (bridge-service) ==="
kurtosis service logs miden-cdk zkevm-bridge-service-001 2>&1 | \
  grep -iE "error|fail" | tail -10

echo "=== UNCLAIMED NOTES ==="
./scripts/list-unclaimed-notes.sh 2>/dev/null || echo "(script not available)"
```

---

## Service Reference

| Service | Kurtosis Name | Port | What to Check |
|---------|---------------|------|---------------|
| L1 node | `geth-1l` | 8545 | Block advancing, deposit tx confirmed |
| PostgreSQL | `postgres-001` | 5432 | `sync.deposit`, `sync.claim` tables |
| pgweb | `pgweb-001` | 8081 | Browser UI for bridge DB |
| Bridge service | `zkevm-bridge-service-001` | 8080 | Claim generation logs |
| Aggkit (aggoracle) | `aggkit-001` | 5576 | GER injection activity |
| nginx forwarder | `miden-l2-forwarder-001` | 8545 | Routes to proxy:8546 |
| Proxy | `miden-proxy-001` | 8546 | GER events, claim processing |
| Miden node | `miden-node-001` | 57291 | Block height, note state |

## Key Selectors and Topics

| Name | Value |
|------|-------|
| `claimAsset` selector | `0xccaa2d11` |
| `updateExitRoot` selector | `0x736ca7f4` |
| `insertGlobalExitRoot` selector | `0x12da06b2` |
| `UpdateHashChainValue` event topic | `0x65d3bf3661...c2319` |
| `ClaimEvent` event topic | `0x25308c93ce...5983` |
| L2 GER contract address | `0xa40D5f56745a118D0906a34E69aeC8C0Db1cB8fA` |

## Timing Expectations (Local Kurtosis)

| Event | Typical Wait |
|-------|-------------|
| Deposit appears in bridge DB | ~30s |
| L1 finality (64 blocks at ~2s) | ~2-3 min |
| GER injection after finality | ~5-15s |
| `ready_for_claim = true` | After GER injection |
| Bridge-service sends claim | 1-5 min after ready |
| Phase 1 (CLAIM note commit) | ~2-6s |
| Block advancement | ~3-10s |
| Phase 2 (faucet mint P2ID) | ~2-6s |
| **Total deposit → claimed** | **~5-15 min** |
