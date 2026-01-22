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
    /// Create a new synthetic block
    pub fn new(
        number: u64,
        parent_hash: [u8; 32],
        timestamp: u64,
        state_root: [u8; 32],
    ) -> Self {
        let hash = Self::compute_hash(number, &parent_hash, timestamp, &state_root);
        Self {
            number,
            hash,
            parent_hash,
            timestamp,
            state_root,
            transactions: Vec::new(),
        }
    }

    /// Compute deterministic block hash
    fn compute_hash(
        number: u64,
        parent_hash: &[u8; 32],
        timestamp: u64,
        state_root: &[u8; 32],
    ) -> [u8; 32] {
        let mut hasher = Keccak256::new();
        hasher.update(b"miden-synthetic-block-v1");
        hasher.update(number.to_be_bytes());
        hasher.update(parent_hash);
        hasher.update(timestamp.to_be_bytes());
        hasher.update(state_root);
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
        state.ensure_block_exists(0, 0);
        state
    }

    /// Get current block number
    pub fn current_block_number(&self) -> u64 {
        *self.current_block.read()
    }

    /// Update current block number from Miden sync
    pub fn set_current_block(&self, block_num: u64, timestamp: u64) {
        self.ensure_block_exists(block_num, timestamp);
        *self.current_block.write() = block_num;
    }

    /// Compute deterministic timestamp for a block number.
    /// This ensures the same block number always produces the same hash.
    fn deterministic_timestamp(block_num: u64) -> u64 {
        GENESIS_TIMESTAMP + block_num * BLOCK_TIME
    }

    /// Ensure a block exists, creating synthetic blocks as needed.
    /// The timestamp parameter is ignored - we use deterministic timestamps
    /// to ensure consistent block hashes across proxy restarts.
    fn ensure_block_exists(&self, block_num: u64, _timestamp: u64) {
        let mut blocks = self.blocks.write();
        let mut hash_to_number = self.hash_to_number.write();

        if blocks.contains_key(&block_num) {
            return;
        }

        // Build the chain from genesis up to block_num to ensure consistent parent hashes.
        // This guarantees the hash chain is always built in order.
        let mut first_missing = block_num;
        while first_missing > 0 && !blocks.contains_key(&(first_missing - 1)) {
            first_missing -= 1;
        }

        // Now create all missing blocks in order
        for num in first_missing..=block_num {
            if blocks.contains_key(&num) {
                continue;
            }

            let parent_hash = if num == 0 {
                [0u8; 32]
            } else {
                blocks.get(&(num - 1)).map(|b| b.hash).unwrap_or([0u8; 32])
            };

            let block = SyntheticBlock::new(
                num,
                parent_hash,
                Self::deterministic_timestamp(num),
                [0u8; 32], // Empty state root for synthetic
            );
            hash_to_number.insert(block.hash, num);
            blocks.insert(num, block);
        }
    }

    /// Get block by number
    pub fn get_block_by_number(&self, block_num: u64) -> Option<SyntheticBlock> {
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

    /// Get block hash for a block number
    pub fn get_block_hash(&self, block_num: u64) -> Option<[u8; 32]> {
        self.blocks.read().get(&block_num).map(|b| b.hash)
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
    fn test_synthetic_block_creation() {
        let block = SyntheticBlock::new(100, [0u8; 32], 1234567890, [0xAA; 32]);
        assert_eq!(block.number, 100);
        assert_eq!(block.timestamp, 1234567890);
        assert_ne!(block.hash, [0u8; 32]); // Hash should be computed
    }

    #[test]
    fn test_block_state_genesis() {
        let state = BlockState::new();
        let genesis = state.get_block_by_number(0);
        assert!(genesis.is_some());
        assert_eq!(genesis.unwrap().number, 0);
    }

    #[test]
    fn test_block_state_update() {
        let state = BlockState::new();
        state.set_current_block(100, 1234567890);
        assert_eq!(state.current_block_number(), 100);

        let block = state.get_block_by_number(100);
        assert!(block.is_some());
    }

    #[test]
    fn test_block_lookup_by_hash() {
        let state = BlockState::new();
        state.set_current_block(50, 1234567890);

        let block = state.get_block_by_number(50).unwrap();
        let by_hash = state.get_block_by_hash(&block.hash);
        assert!(by_hash.is_some());
        assert_eq!(by_hash.unwrap().number, 50);
    }

    #[test]
    fn test_deterministic_hashes_across_instances() {
        // This test verifies that block hashes are deterministic across
        // different BlockState instances (simulating proxy restarts).
        // This is critical for preventing false reorg detection.

        let state1 = BlockState::new();
        state1.set_current_block(100, 1111111111); // timestamp should be ignored

        let state2 = BlockState::new();
        state2.set_current_block(100, 2222222222); // different timestamp, same block

        let block1 = state1.get_block_by_number(100).unwrap();
        let block2 = state2.get_block_by_number(100).unwrap();

        // Hashes MUST be identical regardless of when the block was created
        assert_eq!(
            block1.hash, block2.hash,
            "Block hashes must be deterministic across instances"
        );
        assert_eq!(
            block1.timestamp, block2.timestamp,
            "Block timestamps must be deterministic"
        );
    }

    #[test]
    fn test_deterministic_parent_chain() {
        // Verify that parent hashes are consistent regardless of creation order

        // Scenario 1: Create blocks in order 0 -> 50
        let state1 = BlockState::new();
        state1.set_current_block(50, 0);

        // Scenario 2: Jump to 50, then query 25 later
        let state2 = BlockState::new();
        state2.set_current_block(50, 0);
        state2.set_current_block(25, 0);

        // Both should have the same hash for block 50
        let block1 = state1.get_block_by_number(50).unwrap();
        let block2 = state2.get_block_by_number(50).unwrap();

        assert_eq!(
            block1.hash, block2.hash,
            "Block 50 hash must be consistent regardless of creation order"
        );
        assert_eq!(
            block1.parent_hash, block2.parent_hash,
            "Parent hash must be consistent"
        );
    }

    #[test]
    fn test_deterministic_timestamps() {
        let state = BlockState::new();
        state.set_current_block(10, 9999999999);

        let block = state.get_block_by_number(10).unwrap();

        // Timestamp should be deterministic: GENESIS_TIMESTAMP + block_num * BLOCK_TIME
        let expected_ts = GENESIS_TIMESTAMP + 10 * BLOCK_TIME;
        assert_eq!(
            block.timestamp, expected_ts,
            "Block timestamp should be deterministic based on block number"
        );
    }
}
