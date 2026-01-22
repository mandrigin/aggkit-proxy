//! Block State - Synthetic EVM block tracking for kurtosis-cdk integration.
//!
//! Maps Miden batches to synthetic EVM blocks for bridge service compatibility.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use std::collections::HashMap;

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

    /// Ensure a block exists, creating synthetic blocks as needed
    fn ensure_block_exists(&self, block_num: u64, timestamp: u64) {
        let mut blocks = self.blocks.write();
        let mut hash_to_number = self.hash_to_number.write();

        if blocks.contains_key(&block_num) {
            return;
        }

        // Get parent hash (or genesis hash for block 0)
        let parent_hash = if block_num == 0 {
            [0u8; 32]
        } else {
            // Ensure parent exists first
            if !blocks.contains_key(&(block_num - 1)) {
                // Create parent with estimated timestamp
                let parent_ts = if timestamp > 12 { timestamp - 12 } else { 0 };
                let grandparent_hash = if block_num == 1 {
                    [0u8; 32]
                } else {
                    blocks.get(&(block_num - 2)).map(|b| b.hash).unwrap_or([0u8; 32])
                };
                let parent = SyntheticBlock::new(
                    block_num - 1,
                    grandparent_hash,
                    parent_ts,
                    [0u8; 32], // Empty state root for synthetic
                );
                hash_to_number.insert(parent.hash, block_num - 1);
                blocks.insert(block_num - 1, parent);
            }
            blocks.get(&(block_num - 1)).map(|b| b.hash).unwrap_or([0u8; 32])
        };

        // Create the block
        let block = SyntheticBlock::new(block_num, parent_hash, timestamp, [0u8; 32]);
        hash_to_number.insert(block.hash, block_num);
        blocks.insert(block_num, block);
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
}
