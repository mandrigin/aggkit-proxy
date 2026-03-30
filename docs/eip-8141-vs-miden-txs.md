# EIP-8141 (Frame Transaction) vs Miden Transaction Model

## 1. Purpose and Scope

**EIP-8141 ("Frame Transaction")**
- A new Ethereum transaction type (`0x06`) under EIP-2718.
- Goal: decouple transaction validation, execution, and gas payment from ECDSA. Enables native account abstraction and post-quantum signature schemes.
- Authors: Vitalik Buterin, lightclient, Felix Lange, Yoav Weiss, Alex Forshtat, Dror Tirosh, Shahaf Nacson, Derek Chiang. Status: Draft (created 2026-01-29).
- Scope: L1 Ethereum protocol change. Modifies how transactions are structured, validated, and paid for. Does not change the EVM execution model itself -- it wraps EVM calls into a new "frame" abstraction within a single transaction envelope.

**Miden Transaction Model**
- The native transaction model of the Miden rollup (ZK rollup built on Miden VM).
- Goal: enable parallel, private transaction execution with client-side proving. A transaction is the state transition of a single account, consuming/producing notes (UTXO-like objects).
- Scope: entire rollup protocol. Defines execution semantics, proving, account model, note model, and asset model from the ground up.

## 2. Transaction Structure

**EIP-8141**
```
[chain_id, nonce, sender, frames, max_priority_fee_per_gas, max_fee_per_gas, max_fee_per_blob_gas, blob_versioned_hashes]
frames = [[mode, target, gas_limit, data], ...]
```
- A transaction is a list of **frames**, each with a mode (DEFAULT, VERIFY, SENDER), target address, gas limit, and data.
- Frames execute sequentially. Each frame is essentially a top-level EVM call with a specific caller context.
- VERIFY frames handle authentication (signature verification, approval). SENDER frames execute on behalf of the sender. DEFAULT frames execute as `ENTRY_POINT`.
- The `APPROVE` opcode is the key new primitive -- it sets transaction-scoped `sender_approved` and `payer_approved` flags.
- Gas is allocated per-frame; unused gas does not carry over between frames.
- Supports atomic batching of consecutive SENDER frames.

**Miden**

A transaction is a Miden VM program with inputs:
- A single **account** (the entity whose state transitions)
- Zero or more **input notes** (consumed)
- Blockchain state (reference block)
- Optional **transaction script** (executor-defined code)
- Optional transaction arguments and foreign account data

Execution flow:
1. **Prologue**: validate on-chain commitments
2. **Note processing**: execute each note's script against the account sequentially
3. **Transaction script processing**: execute executor-defined code (e.g., sign the transaction, mint tokens)
4. **Epilogue**: compute final state, verify nonce increment, check asset conservation, generate ZK proof

Outputs: updated account state + zero or more **output notes**.

## 3. Key Structural Differences

| Dimension | EIP-8141 | Miden |
|-----------|----------|-------|
| **Granularity** | Single tx touches multiple accounts via frames | Single tx transitions exactly one account |
| **Multi-party interaction** | One tx can call N contracts in N frames | Requires two transactions: sender creates note, receiver consumes note |
| **Ordering** | Frames within a tx are ordered; txs within a block are ordered globally | Transactions on different accounts can execute in parallel; no global ordering needed |
| **Batching** | Atomic batch flag on consecutive SENDER frames | Up to 1000 notes consumed + 1000 produced per transaction; batching at the note level |
| **Validation location** | On-chain (EVM execution) | Client-side (local Miden VM execution + proof generation); network only verifies proof |
| **Privacy** | Fully transparent (standard Ethereum) | Private accounts/notes possible; only commitments stored on-chain |
| **Proof** | None (standard EVM execution) | ZK-STARK proof generated per transaction |

## 4. Account Model Comparison

**EIP-8141**
- Retains Ethereum's account model: EOAs and contract accounts at 20-byte addresses.
- The key innovation is that accounts no longer need ECDSA. Any account with code can define its own validation logic via VERIFY frames. EOAs get "default code" that supports both secp256k1 and P256 signatures.
- `sender` is an explicit field in the transaction (no longer derived from signature recovery).
- Nonce is still a single uint64 per account, incremented during APPROVE.
- Account state: balance, nonce, code, storage (unchanged from current Ethereum).

**Miden**
- Accounts are smart contracts with: ID, code (interface), storage (up to 256 slots of Merkle-tree-backed data), vault (asset collection), nonce.
- Account types: basic accounts, faucets (can mint assets). Can be public (state on-chain) or private (only commitment on-chain).
- Account code is composed of **components** (e.g., basic wallet, authentication) -- this is structurally analogous to EIP-8141's frame decomposition, where auth, execution, and system logic are separate composable units (see Section 10). The interface explicitly exposes which methods notes and transaction scripts can call.
- Authentication is fully programmable: the account's code defines `auth_tx` which is called during the epilogue. Could be single-sig, multi-sig, any scheme.
- No notion of EOA. Every account is a smart contract.

## 5. Asset/Token Handling

**EIP-8141**
- No changes to Ethereum's asset model. ETH is native; ERC-20/721/1155 are contract-based.
- Frame transactions have no `value` field. To send ETH, you use a SENDER frame that calls the target with a value via the account's code (or default code for EOAs encodes `[[target, value, data]]` in RLP).
- Gas payment abstraction: the payer can be different from the sender (paymaster pattern is first-class).

**Miden**
- **Native asset model**: every asset is encoded in 32 bytes containing the faucet ID and asset details. No ERC-20 contracts needed for standard tokens.
- Assets are stored directly in account vaults (sparse Merkle trees) and note vaults (lists up to 256 assets).
- Only faucet accounts can mint assets. Fungible assets have max supply 2^63 - 1.
- Non-fungible assets are represented by hashing asset data.
- Asset conservation is enforced at the protocol level: sum of input assets must equal sum of output assets (unless the account is a faucet).
- Asset callbacks: faucets can register callbacks invoked when their assets are added to vaults/notes (enables blocklists, pause functionality).
- Alternative asset models (e.g., ERC-20-style global state) are possible but not the default.

## 6. Execution Model

**EIP-8141**
- EVM execution. Each frame is a top-level EVM call.
- New opcodes: `APPROVE` (0xaa), `TXPARAM` (0xb0), `FRAMEDATALOAD` (0xb1), `FRAMEDATACOPY` (0xb2).
- Warm/cold state journal is shared across frames. Transient storage (`TSTORE`/`TLOAD`) is reset between frames.
- `ORIGIN` returns the frame's caller (not the EOA signer, since there may not be one).
- Max 1000 frames per transaction.
- Gas model: EIP-1559 + EIP-4844 blob fees. Per-frame gas limits. Intrinsic cost of 15,000 gas.

**Miden**
- Miden VM execution (stack-based VM with field arithmetic over a 64-bit prime field).
- Programs compile to Miden Assembly (MASM). Execution produces a STARK proof.
- Transaction kernel: a reference implementation of the protocol that mediates all account state access. Note scripts and transaction scripts cannot directly modify account state -- they must call methods exposed by the account's interface, which in turn call kernel procedures.
- Max 2^30 VM cycles per transaction.
- No gas in the Ethereum sense for local transactions. Fees are computed and deducted in the epilogue in the chain's native asset. Local proving has no gas limit -- complexity is bounded only by the cycle limit.
- Foreign procedure invocation: note/transaction scripts can read state from other accounts (oracles, price feeds) during execution.

## 7. Notes vs UTXOs vs Account State

This is the deepest conceptual divergence.

**EIP-8141**: purely account-based. There are no UTXOs or note-like objects. State lives in account storage and balances. A frame transaction is a sequence of calls that mutate account states atomically within one transaction.

**Miden**: hybrid UTXO + account model.
- **Notes** are the UTXO-like primitive. A note has assets, a script (spending conditions), storage (parameters), a serial number (for uniqueness/privacy), and metadata.
- Notes are created in one transaction and consumed in another. This two-phase transfer enables parallel execution: two accounts that don't share notes can transact simultaneously.
- **Nullifiers** (hash of note components) track consumption without revealing which note was spent -- directly borrowed from Zcash.
- Note types: P2ID (pay-to-ID), P2IDE (with time-lock/reclaim), SWAP (atomic exchange). Custom scripts can define arbitrary consumption logic.
- Notes can be private (only hash on-chain) or public (full data on-chain).
- "Erasable notes" can be consumed before being recorded on-chain, enabling sub-second settlement for specific use cases (e.g., order books).

The Miden note is conceptually closest to a Bitcoin UTXO with programmable spending conditions, but living inside an account-based system. EIP-8141 has no analog -- it stays firmly in Ethereum's account-centric world.

## 8. Validation and Authentication

**EIP-8141**
- VERIFY frames define validation. They run as STATICCALL (no state modification).
- The `APPROVE` opcode signals successful validation to the protocol.
- Signature hash is computed canonically with VERIFY frame data elided (since it contains the signature itself).
- Default code supports secp256k1 and P256 for EOA backward compatibility.
- Mempool rules constrain what validation can do: no banned opcodes, no reading external mutable state, max 100K gas for the validation prefix.

**Miden**
- Authentication is part of the account's code, called during the transaction epilogue.
- The standard `auth::singlesig::auth_tx` component verifies a signature against a stored public key and a transaction commitment. But any logic is possible.
- No mempool rules analogous to EIP-8141's -- validation happens locally on the client. The network only sees the proof.
- Note scripts define consumption authorization (who can spend a note), which is separate from account authentication.

## 9. Gas/Fee Abstraction

**EIP-8141**
- First-class paymaster support. A VERIFY frame can `APPROVE(0x2)` to pay gas from any account.
- Canonical paymaster contract pattern for mempool safety (timelocked withdrawals, reservation accounting).
- Non-canonical paymasters allowed with strict limits (max 1 pending tx per non-canonical paymaster).
- EOAs can act as paymasters via default code.

**Miden**
- Fees are deducted from the account's vault in the native asset during the epilogue.
- No paymaster pattern needed at the protocol level -- since users prove locally, there is no "who pays for execution" problem in the same way. The account itself always pays.
- Network transactions (proven by the operator) have fee parameters defined by the reference block.
- Local transactions are "free" in terms of on-chain gas -- the user bears the computational cost of proving.

## 10. The Deepest Parallel: Components as Frames

The most striking structural similarity between EIP-8141 and Miden is one that's easy to miss: **Miden's component-based account code is the account-side analog of EIP-8141's frame decomposition.**

In EIP-8141, a transaction is broken into typed frames:
- **VERIFY** frame: authentication logic (is this tx authorized?)
- **SENDER** frame: user-intended execution (transfer, swap, etc.)
- **DEFAULT** frame: system/third-party logic (callbacks, hooks)

In Miden, an account's code is broken into typed **components**:
- **Authentication component** (e.g., `auth::singlesig::auth_tx`): validates the transaction during the epilogue — directly analogous to a VERIFY frame
- **Wallet component** (e.g., `basic_wallet`): exposes `send_asset`/`receive_asset` — the execution logic that note scripts and transaction scripts call, analogous to SENDER frames
- **Custom components**: any additional interface methods the account exposes — analogous to DEFAULT frames that handle external calls

Both designs arrive at the same decomposition principle: **separate authentication from execution from system logic**, and make each piece independently composable.

The key difference is *where* this decomposition lives:

| | EIP-8141 | Miden |
|--|----------|-------|
| **Decomposed at** | Transaction level (frames in the tx envelope) | Account level (components in the account code) |
| **Composed by** | Transaction sender (chooses which frames to include) | Account deployer (chooses which components to install) |
| **Swappable** | Per-transaction (different frames each time) | Per-account (components are fixed after deployment, but accounts can be upgraded) |
| **Auth logic** | VERIFY frame runs as STATICCALL on the account | Auth component runs inside the ZK proof during epilogue |
| **Execution logic** | SENDER frame calls account code | Note/tx scripts call wallet component methods |

This means EIP-8141 gives *callers* flexibility (compose frames per-tx), while Miden gives *accounts* flexibility (compose components per-account). Miden's approach is more restrictive per-transaction (the account's interface is fixed) but more powerful at the account level (components can define entirely new execution semantics, not just different call contexts).

The convergence is not accidental. Both are solving the same fundamental problem: Ethereum's original design fused "who is this?" (authentication), "what do they want?" (execution), and "who pays?" (gas) into a single indivisible transaction. EIP-8141 untangles these at the protocol level within the existing EVM. Miden untangles them from the ground up in the account model itself, leveraging the fact that a clean-sheet ZK rollup doesn't need backward compatibility with EOAs.

## 11. Conceptual Overlaps

| EIP-8141 Concept | Closest Miden Analog | Notes |
|-------------------|---------------------|-------|
| Frame (VERIFY) | Account authentication procedure | Both separate validation from execution |
| Frame (SENDER) | Transaction script | Both execute user-intended operations |
| Frame (DEFAULT) | Note script execution / foreign calls | Both handle third-party or system logic |
| Atomic batch | Single transaction (inherently atomic) | Miden txs are atomic by default since they transition one account |
| Paymaster | N/A (client-side proving) | Miden has no equivalent; prover delegation is the closest |
| `APPROVE` opcode | Nonce increment in epilogue | Both gate transaction finality |
| Transaction signature hash | Transaction commitment (proven in ZK) | Miden's is a cryptographic commitment verified inside the proof |
| Account code | Account code (components) | Both are programmable smart accounts |
| N/A | Notes (UTXO-like) | EIP-8141 has no UTXO concept |
| N/A | Nullifiers | EIP-8141 has no private consumption tracking |
| N/A | ZK proof | EIP-8141 is pre-proof, standard EVM execution |

## 12. Bottom Line

EIP-8141 is an evolutionary improvement to Ethereum's transaction format, decomposing the monolithic tx into a sequence of typed frames to unlock account abstraction, post-quantum signatures, and gas sponsorship -- all while staying within the existing EVM and account model.

Miden is a ground-up redesign built around ZK proofs, a hybrid UTXO/account model, and client-side execution. Its transaction model is fundamentally different: one-account-per-transaction, note-based inter-account communication, private state, and local proving.

The two designs solve some of the same problems (signature agility, programmable validation, fee flexibility) but from radically different starting points. EIP-8141's frame abstraction is the closest Ethereum L1 has come to Miden's separation of concerns between validation and execution, but it does so without introducing UTXOs, privacy, or off-chain proving.
