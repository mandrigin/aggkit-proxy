# Miden RPC Proxy - Best Practices

Lessons learned from development. Follow these to avoid common pitfalls.

## Docker & Deployment

### Use Git SHA in Image Tags
```yaml
# docker-compose.local.yml
image: proxy:${GIT_COMMIT:-latest}
```
- Never use `:latest` or `:local` - makes it hard to identify stale images
- Export `GIT_COMMIT=$(git rev-parse --short HEAD)` before `docker compose build`

### GIT_COMMIT Must Be Runtime Env Var
Build args don't persist to runtime. Pass as both:
```yaml
build:
  args:
    GIT_COMMIT: ${GIT_COMMIT}
environment:
  GIT_COMMIT: ${GIT_COMMIT:-unknown}
```

### Always Test Full Flow
Before reporting any fix as complete:
```bash
./scripts/start-all.sh --clean
./scripts/test-lumia-claims.sh 1
docker logs <proxy-container>  # Check for errors
```

### Rebuild Images When Testing
Use `docker compose up -d --build` to force rebuild, not just `up -d`.

## Miden Client Integration

### Keystore Path Must Be Separate from SQLite Path
WRONG:
```rust
let store_path = PathBuf::from("/app/data/miden-client");
let keystore_path = store_path.join("keystore");  // Tries to create dir inside a FILE
```

RIGHT:
```rust
let store_path = PathBuf::from("/app/data/miden-client");
let keystore_path = store_path.parent().unwrap().join("keystore");  // Sibling directory
```

### BRIDGE_FAUCET_ID Format
AccountIdV0 expects: `0x` prefix + 30 hex chars (15 bytes), left-padded with zeros
```bash
# Extract and format correctly
RAW_HEX=$(sqlite3 ... "SELECT hex(account_id) FROM accounts ...")
BRIDGE_FAUCET_ID="0x$(printf '%030s' "$RAW_HEX" | tr ' ' '0')"
```

### Don't Poll - Use On-Demand Fetch
Polling for block numbers doesn't work reliably. Instead:
- Persist the Miden client
- Fetch block height on-demand when needed
- Crash on startup if miden-node is unreachable (fail fast)

### CLAIM Note Flow Requirements
The CLAIM note approach (using `miden_agglayer::create_claim_note()`) requires specific infrastructure:

1. **Agglayer-enabled faucet** - The faucet must be an agglayer faucet, not a standard
   `NetworkFungibleFaucet`. Standard faucets don't understand CLAIM notes.

2. **L1 bridge deposits first** - The faucet must have received L1 bridge deposits before
   claims can be processed. The CLAIM note tells the faucet "release these bridged tokens
   to me", but the tokens must exist in the faucet's vault first.

3. **Ephemeral account sync** - After creating an ephemeral account for CLAIM submission,
   you MUST call `client.sync_state().await` to ensure the client properly tracks it:
   ```rust
   client.add_account(&ephemeral_account, false).await?;
   client.sync_state().await?;  // Required! Otherwise vault witness errors occur
   ```

**Test limitation**: Local testing with `test-lumia-claims.sh` uses real Lumia mainnet
claim data, but the local genesis faucet doesn't have bridged LUMIA tokens. End-to-end
testing requires an agglayer testnet with actual L1→Miden bridge deposits.

## Logging & Debugging

### Log Claim Details
When processing claimAsset transactions, always log:
- Transaction hash
- Amount (raw wei AND human readable, e.g., "2.31 LUMIA")
- Destination address
- Origin token

This makes debugging amount parsing issues much easier.

### Version in Startup Logs
Always log version/commit SHA on startup with GitHub link:
```rust
info!("Version: {} (https://github.com/owner/repo/commit/{})", commit, commit);
```

### Config Table on Startup
Print all configuration as a table for easy verification:
```
Configuration:
  MIDEN_RPC_URL:      http://miden-node:57291
  BRIDGE_FAUCET_ID:   0x...
  LISTEN_HOST:        0.0.0.0
  LISTEN_PORT:        8546
```

### Detailed Error Logging
Always strive for detailed error logs. On any error:
- Log the full error chain (use `{:?}` or `.source()`)
- Include relevant context (tx_hash, account_id, amounts)
- Log input data that caused the error
- Make errors actionable - someone reading the log should know what went wrong

```rust
// BAD
error!("Transaction failed");

// GOOD
error!(
    tx_hash = %tx_hash,
    account_id = %account_id,
    amount = %amount,
    error = ?e,
    "Miden submission failed: {}", e
);
```

### Log Account IDs When Loading
When loading accounts from files, log the account ID to verify the correct account is being used.

## Transaction Decoding

### Handle Both Raw Calldata and RLP
The proxy receives transactions in two formats:
1. Raw claimAsset calldata (from test scripts)
2. RLP-encoded signed Ethereum transactions (from real clients)

Detect format and decode appropriately.

### Validate Amount Parsing
If you see `u64::MAX` (18446744073709551615) in amount errors, the calldata offset is wrong.
- Check byte offsets in ABI decoding
- Verify amount field position matches the actual calldata structure

## Development Workflow

### Pre-flight Checks
Before sending transactions, verify proxy is healthy:
```bash
curl -s http://localhost:8546 -X POST -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","id":1}'
```

### Use Proper Listen Address
In Docker: `LISTEN_HOST=0.0.0.0` (not `127.0.0.1`)
Otherwise container-to-container communication fails.

### Docker Service Names
Inside Docker network, use service names:
- `MIDEN_RPC_URL=http://miden-node:57291` (not `localhost`)

## Git Workflow

### Keep Branches Up to Date
Before starting work and before submitting:
```bash
git fetch origin
git rebase origin/main
```

When new work lands on main, rebase your branch immediately to avoid conflicts.

### Rebase After Main Updates
When a fix lands on main that others depend on:
1. Merge to main first
2. Notify other workers to rebase: `git fetch origin && git rebase origin/main`
3. This prevents merge conflicts and ensures everyone has latest fixes

### Push Frequently
Don't accumulate local commits. Push after each logical change to:
- Make work visible to others
- Enable early conflict detection
- Prevent lost work
