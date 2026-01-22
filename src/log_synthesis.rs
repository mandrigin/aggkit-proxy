//! Log Synthesis - Generates Ethereum-style event logs for bridge operations.
//!
//! This module provides a `LogStore` that tracks ClaimEvent logs emitted when
//! claims are confirmed on the Miden network. These logs can be queried via
//! eth_getLogs to allow bridge services to detect completed claims.

use alloy_primitives::{Address, B256, U256};
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use std::collections::BTreeMap;

/// Bridge contract address for log emissions
pub const BRIDGE_CONTRACT_ADDRESS: &str = "0x0000000000000000000000000000000000000001";

/// ClaimEvent topic0 - keccak256("ClaimEvent(uint256,uint32,address,address,uint256)")
/// This matches the bridge contract's ClaimEvent signature
pub fn claim_event_topic() -> B256 {
    let mut hasher = Keccak256::new();
    hasher.update(b"ClaimEvent(uint256,uint32,address,address,uint256)");
    B256::from_slice(&hasher.finalize())
}

/// A synthesized Ethereum-style event log
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct SynthesizedLog {
    /// Contract address that emitted the log
    pub address: String,
    /// Indexed topics (topic0 = event signature hash)
    pub topics: Vec<String>,
    /// Non-indexed event data (ABI-encoded)
    pub data: String,
    /// Block number where this log was emitted
    pub block_number: u64,
    /// Block hash
    pub block_hash: String,
    /// Transaction hash that generated this log
    pub transaction_hash: String,
    /// Transaction index within the block
    pub transaction_index: u64,
    /// Log index within the block
    pub log_index: u64,
    /// Whether this log was removed (always false for confirmed logs)
    pub removed: bool,
}

impl SynthesizedLog {
    /// Convert to JSON-RPC response format
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "address": self.address,
            "topics": self.topics,
            "data": self.data,
            "blockNumber": format!("0x{:x}", self.block_number),
            "blockHash": self.block_hash,
            "transactionHash": self.transaction_hash,
            "transactionIndex": format!("0x{:x}", self.transaction_index),
            "logIndex": format!("0x{:x}", self.log_index),
            "removed": self.removed
        })
    }
}

/// Parameters for creating a ClaimEvent log
#[derive(Debug, Clone)]
pub struct ClaimEventParams {
    /// Global index of the claim
    pub global_index: U256,
    /// Origin network ID
    pub origin_network: u32,
    /// Origin token address
    pub origin_token_address: Address,
    /// Destination address (recipient)
    pub destination_address: Address,
    /// Amount claimed
    pub amount: U256,
    /// Transaction hash
    pub transaction_hash: String,
    /// Block number where claim was confirmed
    pub block_number: u64,
}

/// Store for synthesized event logs
pub struct LogStore {
    /// Logs indexed by block number for efficient range queries
    /// Key: block_number, Value: Vec of logs in that block
    logs_by_block: RwLock<BTreeMap<u64, Vec<SynthesizedLog>>>,
    /// Running log index counter
    log_index_counter: RwLock<u64>,
}

impl LogStore {
    /// Create a new empty log store
    pub fn new() -> Self {
        Self {
            logs_by_block: RwLock::new(BTreeMap::new()),
            log_index_counter: RwLock::new(0),
        }
    }

    /// Add a ClaimEvent log for a confirmed claim
    pub fn add_claim_event(&self, params: ClaimEventParams) {
        let topic0 = format!("0x{}", hex::encode(claim_event_topic()));

        // Topic1: global_index (indexed)
        let topic1 = format!("0x{}", hex::encode(params.global_index.to_be_bytes::<32>()));

        // Topic2: origin_network (indexed, padded to 32 bytes)
        let mut origin_network_bytes = [0u8; 32];
        origin_network_bytes[28..32].copy_from_slice(&params.origin_network.to_be_bytes());
        let topic2 = format!("0x{}", hex::encode(origin_network_bytes));

        // Data: ABI-encoded non-indexed parameters
        // (origin_token_address, destination_address, amount)
        let mut data = Vec::with_capacity(96);
        // origin_token_address (padded to 32 bytes)
        data.extend_from_slice(&[0u8; 12]);
        data.extend_from_slice(params.origin_token_address.as_slice());
        // destination_address (padded to 32 bytes)
        data.extend_from_slice(&[0u8; 12]);
        data.extend_from_slice(params.destination_address.as_slice());
        // amount (32 bytes)
        data.extend_from_slice(&params.amount.to_be_bytes::<32>());

        // Generate synthetic block hash from block number
        let mut block_hasher = Keccak256::new();
        block_hasher.update(b"miden-bridge-block-v1");
        block_hasher.update(params.block_number.to_be_bytes());
        let block_hash = format!("0x{}", hex::encode(block_hasher.finalize()));

        // Get next log index
        let log_index = {
            let mut counter = self.log_index_counter.write();
            let idx = *counter;
            *counter += 1;
            idx
        };

        let log = SynthesizedLog {
            address: BRIDGE_CONTRACT_ADDRESS.to_string(),
            topics: vec![topic0, topic1, topic2],
            data: format!("0x{}", hex::encode(data)),
            block_number: params.block_number,
            block_hash,
            transaction_hash: params.transaction_hash,
            transaction_index: 0,
            log_index,
            removed: false,
        };

        // Insert into block-indexed storage
        let mut logs = self.logs_by_block.write();
        logs.entry(params.block_number)
            .or_insert_with(Vec::new)
            .push(log);
    }

    /// Get logs within a block range (inclusive)
    pub fn get_logs(&self, from_block: u64, to_block: u64) -> Vec<SynthesizedLog> {
        let logs = self.logs_by_block.read();
        logs.range(from_block..=to_block)
            .flat_map(|(_, block_logs)| block_logs.iter().cloned())
            .collect()
    }

    /// Get logs for a specific block
    pub fn get_logs_for_block(&self, block_number: u64) -> Vec<SynthesizedLog> {
        let logs = self.logs_by_block.read();
        logs.get(&block_number).cloned().unwrap_or_default()
    }

    /// Get total number of logs stored
    pub fn log_count(&self) -> u64 {
        *self.log_index_counter.read()
    }
}

impl Default for LogStore {
    fn default() -> Self {
        Self::new()
    }
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_add_claim_event() {
        let store = LogStore::new();

        let params = ClaimEventParams {
            global_index: U256::from(42u64),
            origin_network: 1,
            origin_token_address: Address::ZERO,
            destination_address: Address::repeat_byte(0xAB),
            amount: U256::from(1000u64),
            transaction_hash: "0x1234".to_string(),
            block_number: 100,
        };

        store.add_claim_event(params);

        assert_eq!(store.log_count(), 1);

        let logs = store.get_logs_for_block(100);
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].block_number, 100);
        assert_eq!(logs[0].topics.len(), 3);
    }

    #[test]
    fn test_get_logs_range() {
        let store = LogStore::new();

        // Add logs to multiple blocks
        for block in [10, 20, 30, 40, 50] {
            store.add_claim_event(ClaimEventParams {
                global_index: U256::from(block),
                origin_network: 1,
                origin_token_address: Address::ZERO,
                destination_address: Address::ZERO,
                amount: U256::from(100u64),
                transaction_hash: format!("0x{:x}", block),
                block_number: block,
            });
        }

        // Query range 20-40
        let logs = store.get_logs(20, 40);
        assert_eq!(logs.len(), 3);
        assert!(logs.iter().all(|l| l.block_number >= 20 && l.block_number <= 40));
    }

    #[test]
    fn test_claim_event_topic() {
        let topic = claim_event_topic();
        // Just verify it's deterministic
        assert_eq!(topic, claim_event_topic());
    }

    #[test]
    fn test_log_json_format() {
        let log = SynthesizedLog {
            address: BRIDGE_CONTRACT_ADDRESS.to_string(),
            topics: vec!["0xabc".to_string()],
            data: "0x1234".to_string(),
            block_number: 100,
            block_hash: "0xdef".to_string(),
            transaction_hash: "0x999".to_string(),
            transaction_index: 0,
            log_index: 5,
            removed: false,
        };

        let json = log.to_json();
        assert_eq!(json["blockNumber"], "0x64");
        assert_eq!(json["logIndex"], "0x5");
        assert_eq!(json["removed"], false);
    }
}
