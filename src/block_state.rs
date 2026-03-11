//! Block State - Synthetic EVM block tracking for kurtosis-cdk integration.
//!
//! Maps Miden batches to synthetic EVM blocks for bridge service compatibility.
//!
//! Block hashes are deterministic: given a block number, the hash is always the same.
//! This prevents reorg detection issues when the proxy restarts or blocks are re-queried.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use std::collections::HashMap;

/// Genesis timestamp for synthetic blocks (2024-01-01 00:00:00 UTC)
const GENESIS_TIMESTAMP: u64 = 1704067200;

/// Block time in seconds (12s like Ethereum mainnet)
const BLOCK_TIME: u64 = 12;

/// Synthetic EVM block generated from Miden batch
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntheticBlock {
    /// Block number (= Miden batch number)
    pub number: u64,
    /// Block hash (deterministic from number + state)
    pub hash: [u8; 32],
    /// Parent block hash
    pub parent_hash: [u8; 32],
    /// Block timestamp (Unix seconds)
    pub timestamp: u64,
    /// State root (Miden state commitment)
    pub state_root: [u8; 32],
    /// Transaction hashes in this block
    pub transactions: Vec<String>,
}

impl SyntheticBlock {
    /// Create a new synthetic block.
    /// Hash and parent hash are derived purely from block numbers.
    pub fn new(number: u64, timestamp: u64) -> Self {
        let hash = Self::compute_hash_for_number(number);
        let parent_hash = if number == 0 {
            [0u8; 32]
        } else {
            Self::compute_hash_for_number(number - 1)
        };
        Self {
            number,
            hash,
            parent_hash,
            timestamp,
            state_root: [0u8; 32],
            transactions: Vec::new(),
        }
    }

    /// Compute deterministic block hash from block number alone.
    ///
    /// Hash = keccak256("miden_block_evm_<number>")
    ///
    /// This is a pure function of the block number — no parent hash, timestamp, or
    /// state root dependency. This guarantees identical hashes regardless of creation
    /// order, proxy restarts, or concurrent access patterns.
    pub fn compute_hash_for_number(number: u64) -> [u8; 32] {
        let mut hasher = Keccak256::new();
        hasher.update(format!("miden_block_evm_{}", number).as_bytes());
        hasher.finalize().into()
    }

    /// Format as JSON-RPC response
    pub fn to_json(&self, full_transactions: bool) -> serde_json::Value {
        let txs = if full_transactions {
            // Full transaction objects not supported yet, return hashes
            serde_json::json!(self.transactions)
        } else {
            serde_json::json!(self.transactions)
        };

        serde_json::json!({
            "number": format!("0x{:x}", self.number),
            "hash": format!("0x{}", hex::encode(self.hash)),
            "parentHash": format!("0x{}", hex::encode(self.parent_hash)),
            "timestamp": format!("0x{:x}", self.timestamp),
            "stateRoot": format!("0x{}", hex::encode(self.state_root)),
            "transactionsRoot": "0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421",
            "receiptsRoot": "0x56e81f171bcc55a6ff8345e692c0f86e5b48e01b996cadc001622fb5e363b421",
            "logsBloom": format!("0x{}", "00".repeat(256)),
            "difficulty": "0x0",
            "totalDifficulty": "0x0",
            "gasLimit": "0x1c9c380",
            "gasUsed": "0x0",
            "miner": "0x0000000000000000000000000000000000000000",
            "extraData": "0x",
            "nonce": "0x0000000000000000",
            "mixHash": "0x0000000000000000000000000000000000000000000000000000000000000000",
            "sha3Uncles": "0x1dcc4de8dec75d7aab85b567b6ccd41ad312451b948a7413f0a142fd40d49347",
            "uncles": [],
            "size": "0x200",
            "transactions": txs,
            "baseFeePerGas": "0x0"
        })
    }
}

/// Block state tracking for synthetic EVM blocks
pub struct BlockState {
    /// Block number → SyntheticBlock
    blocks: RwLock<HashMap<u64, SyntheticBlock>>,
    /// Block hash → block number
    hash_to_number: RwLock<HashMap<[u8; 32], u64>>,
    /// Current/latest block number
    current_block: RwLock<u64>,
}

impl BlockState {
    /// Create new block state
    pub fn new() -> Self {
        let state = Self {
            blocks: RwLock::new(HashMap::new()),
            hash_to_number: RwLock::new(HashMap::new()),
            current_block: RwLock::new(0),
        };
        // Create genesis block
        state.ensure_block_exists(0);
        state
    }

    /// Get current block number
    pub fn current_block_number(&self) -> u64 {
        *self.current_block.read()
    }

    /// Update current block number from Miden sync
    pub fn set_current_block(&self, block_num: u64) {
        self.ensure_block_exists(block_num);
        *self.current_block.write() = block_num;
    }

    /// Compute deterministic timestamp for a block number.
    /// This ensures the same block number always produces the same hash.
    fn deterministic_timestamp(block_num: u64) -> u64 {
        GENESIS_TIMESTAMP + block_num * BLOCK_TIME
    }

    /// Ensure a block exists in the cache.
    /// Since hashes are purely derived from block numbers, we only need to
    /// create the requested block — no chain building required.
    fn ensure_block_exists(&self, block_num: u64) {
        let mut blocks = self.blocks.write();
        if blocks.contains_key(&block_num) {
            return;
        }

        let block = SyntheticBlock::new(block_num, Self::deterministic_timestamp(block_num));
        self.hash_to_number.write().insert(block.hash, block_num);
        blocks.insert(block_num, block);
    }

    /// Get block by number, creating it lazily if needed.
    pub fn get_block_by_number(&self, block_num: u64) -> Option<SyntheticBlock> {
        self.ensure_block_exists(block_num);
        self.blocks.read().get(&block_num).cloned()
    }

    /// Get block by hash
    pub fn get_block_by_hash(&self, hash: &[u8; 32]) -> Option<SyntheticBlock> {
        let number = self.hash_to_number.read().get(hash).copied()?;
        self.blocks.read().get(&number).cloned()
    }

    /// Add transaction to a block
    pub fn add_transaction_to_block(&self, block_num: u64, tx_hash: String) {
        if let Some(block) = self.blocks.write().get_mut(&block_num) {
            block.transactions.push(tx_hash);
        }
    }

    /// Get block hash for a block number.
    /// This is a pure function of the block number — no cache lookup needed.
    pub fn get_block_hash(&self, block_num: u64) -> Option<[u8; 32]> {
        Some(SyntheticBlock::compute_hash_for_number(block_num))
    }
}

impl Default for BlockState {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_hash_is_pure_function_of_block_number() {
        // Hash = keccak256("miden_block_evm_<number>")
        let h1 = SyntheticBlock::compute_hash_for_number(42);
        let h2 = SyntheticBlock::compute_hash_for_number(42);
        assert_eq!(h1, h2, "Same block number must produce same hash");
        assert_ne!(h1, [0u8; 32]);

        let h3 = SyntheticBlock::compute_hash_for_number(43);
        assert_ne!(h1, h3, "Different block numbers must produce different hashes");
    }

    #[test]
    fn test_parent_hash_is_previous_block_hash() {
        let state = BlockState::new();
        let b10 = state.get_block_by_number(10).unwrap();
        let b9 = state.get_block_by_number(9).unwrap();
        assert_eq!(b10.parent_hash, b9.hash, "Parent hash must equal previous block's hash");
    }

    #[test]
    fn test_block_state_genesis() {
        let state = BlockState::new();
        let genesis = state.get_block_by_number(0).unwrap();
        assert_eq!(genesis.number, 0);
        assert_eq!(genesis.parent_hash, [0u8; 32]);
    }

    #[test]
    fn test_hashes_identical_across_instances() {
        let state1 = BlockState::new();
        let state2 = BlockState::new();

        // Query in different order
        let _ = state1.get_block_by_number(100);
        let _ = state2.get_block_by_number(50);
        let _ = state2.get_block_by_number(100);

        assert_eq!(
            state1.get_block_by_number(100).unwrap().hash,
            state2.get_block_by_number(100).unwrap().hash,
        );
    }

    #[test]
    fn test_deterministic_timestamps() {
        let state = BlockState::new();
        let block = state.get_block_by_number(10).unwrap();
        let expected_ts = GENESIS_TIMESTAMP + 10 * BLOCK_TIME;
        assert_eq!(block.timestamp, expected_ts);
    }

    #[test]
    fn test_get_block_hash_without_cache() {
        let state = BlockState::new();
        // get_block_hash should work even for uncached blocks
        let h = state.get_block_hash(999).unwrap();
        assert_eq!(h, SyntheticBlock::compute_hash_for_number(999));
    }
}
