//! Log Synthesis - Generate synthetic EVM logs for bridge service compatibility.
//!
//! Synthesizes BridgeEvent and ClaimEvent logs from Miden claim transactions.

use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;

/// BridgeEvent topic hash: keccak256("BridgeEvent(uint8,uint32,address,uint32,address,uint256,bytes,uint32)")
pub const BRIDGE_EVENT_TOPIC: &str =
    "0x501781209a1f8899323b96b4ef08b168df93e0a90c673d1e4cce39366cb62f9b";

/// ClaimEvent topic hash: keccak256("ClaimEvent(uint256,uint32,address,address,uint256)")
pub const CLAIM_EVENT_TOPIC: &str =
    "0x25308c93ceeed162da955b3f7ce3e3f93606579e40fb92029faa9efe27545983";

/// UpdateGlobalExitRoot topic hash: keccak256("UpdateGlobalExitRoot(bytes32,bytes32)")
/// Emitted when aggoracle injects a new GER into L2
pub const UPDATE_GER_TOPIC: &str =
    "0x61014378f82a0d809aefaf87a8ac9505b89c321808287a6e7810f29304c1fce3";

/// L2 GlobalExitRoot contract address (receives GER updates from aggoracle)
pub const L2_GLOBAL_EXIT_ROOT_ADDRESS: &str = "0xa40D5f56745a118D0906a34E69aeC8C0Db1cB8fA";

/// updateExitRoot(bytes32,bytes32) function selector
pub const UPDATE_EXIT_ROOT_SELECTOR: [u8; 4] = [0x73, 0x6c, 0xa7, 0xf4];

/// Synthetic log entry for eth_getLogs
#[derive(Debug, Clone, Serialize, Deserialize)]
pub struct SyntheticLog {
    /// Contract address that emitted the log
    pub address: String,
    /// Indexed topics (max 4)
    pub topics: Vec<String>,
    /// Non-indexed data
    pub data: String,
    /// Block number
    pub block_number: u64,
    /// Block hash
    pub block_hash: [u8; 32],
    /// Transaction hash
    pub transaction_hash: String,
    /// Transaction index in block
    pub transaction_index: u64,
    /// Log index in block
    pub log_index: u64,
    /// Whether log was removed (always false for us)
    pub removed: bool,
}

impl SyntheticLog {
    /// Format as JSON-RPC response
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "address": self.address,
            "topics": self.topics,
            "data": self.data,
            "blockNumber": format!("0x{:x}", self.block_number),
            "blockHash": format!("0x{}", hex::encode(self.block_hash)),
            "transactionHash": self.transaction_hash,
            "transactionIndex": format!("0x{:x}", self.transaction_index),
            "logIndex": format!("0x{:x}", self.log_index),
            "removed": self.removed
        })
    }
}

/// Log filter for eth_getLogs
#[derive(Debug, Clone, Default, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct LogFilter {
    /// Start block (hex or "earliest"/"latest"/"pending")
    pub from_block: Option<String>,
    /// End block (hex or "earliest"/"latest"/"pending")
    pub to_block: Option<String>,
    /// Contract address(es) to filter
    pub address: Option<AddressFilter>,
    /// Topic filters (up to 4)
    pub topics: Option<Vec<Option<TopicFilter>>>,
    /// Block hash (alternative to from/to block)
    pub block_hash: Option<String>,
}

/// Address filter can be single or array
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum AddressFilter {
    /// Single address to match
    Single(String),
    /// Multiple addresses (OR matching)
    Multiple(Vec<String>),
}

/// Topic filter can be single or array (OR matching)
#[derive(Debug, Clone, Deserialize)]
#[serde(untagged)]
pub enum TopicFilter {
    /// Single topic to match
    Single(String),
    /// Multiple topics (OR matching)
    Multiple(Vec<String>),
}

impl LogFilter {
    /// Parse block number from string
    pub fn parse_block_number(&self, s: &str, current_block: u64) -> u64 {
        match s.to_lowercase().as_str() {
            "earliest" => 0,
            "latest" | "pending" => current_block,
            hex if hex.starts_with("0x") => {
                u64::from_str_radix(&hex[2..], 16).unwrap_or(current_block)
            }
            _ => current_block,
        }
    }

    /// Get from block number
    pub fn from_block_number(&self, current_block: u64) -> u64 {
        self.from_block
            .as_ref()
            .map(|s| self.parse_block_number(s, current_block))
            .unwrap_or(current_block)
    }

    /// Get to block number
    pub fn to_block_number(&self, current_block: u64) -> u64 {
        self.to_block
            .as_ref()
            .map(|s| self.parse_block_number(s, current_block))
            .unwrap_or(current_block)
    }

    /// Check if a log matches this filter
    pub fn matches(&self, log: &SyntheticLog, current_block: u64) -> bool {
        // Check block range
        if let Some(ref block_hash) = self.block_hash {
            let log_hash = format!("0x{}", hex::encode(log.block_hash));
            if log_hash.to_lowercase() != block_hash.to_lowercase() {
                return false;
            }
        } else {
            let from = self.from_block_number(current_block);
            let to = self.to_block_number(current_block);
            if log.block_number < from || log.block_number > to {
                return false;
            }
        }

        // Check address filter
        if let Some(ref addr_filter) = self.address {
            let log_addr = log.address.to_lowercase();
            let matches_addr = match addr_filter {
                AddressFilter::Single(a) => a.to_lowercase() == log_addr,
                AddressFilter::Multiple(addrs) => {
                    addrs.iter().any(|a| a.to_lowercase() == log_addr)
                }
            };
            if !matches_addr {
                return false;
            }
        }

        // Check topic filters
        if let Some(ref topic_filters) = self.topics {
            for (i, topic_filter) in topic_filters.iter().enumerate() {
                if let Some(filter) = topic_filter {
                    // Must have this topic
                    if i >= log.topics.len() {
                        return false;
                    }
                    let log_topic = log.topics[i].to_lowercase();
                    let matches_topic = match filter {
                        TopicFilter::Single(t) => t.to_lowercase() == log_topic,
                        TopicFilter::Multiple(topics) => {
                            topics.iter().any(|t| t.to_lowercase() == log_topic)
                        }
                    };
                    if !matches_topic {
                        return false;
                    }
                }
                // None means "any value" - matches anything
            }
        }

        true
    }
}

/// Log store for synthetic logs
pub struct LogStore {
    /// All logs indexed by block number
    logs_by_block: RwLock<HashMap<u64, Vec<SyntheticLog>>>,
    /// Transaction hash → logs
    logs_by_tx: RwLock<HashMap<String, Vec<SyntheticLog>>>,
    /// Global log counter for unique log indices
    log_counter: RwLock<u64>,
}

impl LogStore {
    /// Create new log store
    pub fn new() -> Self {
        Self {
            logs_by_block: RwLock::new(HashMap::new()),
            logs_by_tx: RwLock::new(HashMap::new()),
            log_counter: RwLock::new(0),
        }
    }

    /// Add a synthetic log
    pub fn add_log(&self, mut log: SyntheticLog) {
        let mut counter = self.log_counter.write();
        log.log_index = *counter;
        *counter += 1;

        let block_num = log.block_number;
        let tx_hash = log.transaction_hash.clone();

        self.logs_by_block
            .write()
            .entry(block_num)
            .or_default()
            .push(log.clone());

        self.logs_by_tx
            .write()
            .entry(tx_hash)
            .or_default()
            .push(log);
    }

    /// Create a ClaimEvent log for a confirmed claim
    pub fn add_claim_event(
        &self,
        bridge_address: &str,
        block_number: u64,
        block_hash: [u8; 32],
        tx_hash: &str,
        global_index: &[u8; 32],
        origin_network: u32,
        origin_address: &[u8; 20],
        destination_address: &[u8; 20],
        amount: u64,
    ) {
        // ClaimEvent(uint256 globalIndex, uint32 originNetwork, address originAddress, address destinationAddress, uint256 amount)
        let log = SyntheticLog {
            address: bridge_address.to_string(),
            topics: vec![
                CLAIM_EVENT_TOPIC.to_string(),
                format!("0x{}", hex::encode(global_index)), // globalIndex indexed
            ],
            data: encode_claim_event_data(origin_network, origin_address, destination_address, amount),
            block_number,
            block_hash,
            transaction_hash: tx_hash.to_string(),
            transaction_index: 0,
            log_index: 0, // Will be set by add_log
            removed: false,
        };
        self.add_log(log);
    }

    /// Create an UpdateGlobalExitRoot log for a GER injection transaction
    /// This is emitted when aggoracle sends a GER update to L2
    pub fn add_ger_update_event(
        &self,
        block_number: u64,
        block_hash: [u8; 32],
        tx_hash: &str,
        mainnet_exit_root: &[u8; 32],
        rollup_exit_root: &[u8; 32],
    ) {
        // UpdateGlobalExitRoot(bytes32 mainnetExitRoot, bytes32 rollupExitRoot)
        // Both parameters are non-indexed, so they go in data
        let mut data = Vec::with_capacity(64);
        data.extend_from_slice(mainnet_exit_root);
        data.extend_from_slice(rollup_exit_root);

        let log = SyntheticLog {
            address: L2_GLOBAL_EXIT_ROOT_ADDRESS.to_string(),
            topics: vec![UPDATE_GER_TOPIC.to_string()],
            data: format!("0x{}", hex::encode(&data)),
            block_number,
            block_hash,
            transaction_hash: tx_hash.to_string(),
            transaction_index: 0,
            log_index: 0, // Will be set by add_log
            removed: false,
        };
        self.add_log(log);
    }

    /// Query logs matching filter
    pub fn get_logs(&self, filter: &LogFilter, current_block: u64) -> Vec<SyntheticLog> {
        let mut result = Vec::new();
        let logs_by_block = self.logs_by_block.read();

        // Determine block range
        let from = filter.from_block_number(current_block);
        let to = filter.to_block_number(current_block);

        for block_num in from..=to {
            if let Some(logs) = logs_by_block.get(&block_num) {
                for log in logs {
                    if filter.matches(log, current_block) {
                        result.push(log.clone());
                    }
                }
            }
            // Limit to 1000 logs per spec
            if result.len() >= 1000 {
                break;
            }
        }

        result
    }

    /// Get logs for a specific transaction
    pub fn get_logs_for_tx(&self, tx_hash: &str) -> Vec<SyntheticLog> {
        self.logs_by_tx
            .read()
            .get(tx_hash)
            .cloned()
            .unwrap_or_default()
    }
}

impl Default for LogStore {
    fn default() -> Self {
        Self::new()
    }
}

/// Encode ClaimEvent data field
fn encode_claim_event_data(
    origin_network: u32,
    origin_address: &[u8; 20],
    destination_address: &[u8; 20],
    amount: u64,
) -> String {
    let mut data = Vec::with_capacity(128);

    // originNetwork (uint32 padded to 32 bytes)
    data.extend_from_slice(&[0u8; 28]);
    data.extend_from_slice(&origin_network.to_be_bytes());

    // originAddress (address padded to 32 bytes)
    data.extend_from_slice(&[0u8; 12]);
    data.extend_from_slice(origin_address);

    // destinationAddress (address padded to 32 bytes)
    data.extend_from_slice(&[0u8; 12]);
    data.extend_from_slice(destination_address);

    // amount (uint256)
    data.extend_from_slice(&[0u8; 24]);
    data.extend_from_slice(&amount.to_be_bytes());

    format!("0x{}", hex::encode(data))
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_log_filter_block_range() {
        let filter = LogFilter {
            from_block: Some("0x10".to_string()),
            to_block: Some("0x20".to_string()),
            ..Default::default()
        };

        assert_eq!(filter.from_block_number(100), 16);
        assert_eq!(filter.to_block_number(100), 32);
    }

    #[test]
    fn test_log_filter_latest() {
        let filter = LogFilter {
            from_block: Some("latest".to_string()),
            to_block: Some("latest".to_string()),
            ..Default::default()
        };

        assert_eq!(filter.from_block_number(500), 500);
        assert_eq!(filter.to_block_number(500), 500);
    }

    #[test]
    fn test_log_filter_topic_match() {
        let log = SyntheticLog {
            address: "0x1234".to_string(),
            topics: vec![CLAIM_EVENT_TOPIC.to_string()],
            data: "0x".to_string(),
            block_number: 100,
            block_hash: [0u8; 32],
            transaction_hash: "0xabc".to_string(),
            transaction_index: 0,
            log_index: 0,
            removed: false,
        };

        let filter = LogFilter {
            from_block: Some("0x0".to_string()),
            to_block: Some("0x200".to_string()),
            topics: Some(vec![Some(TopicFilter::Single(CLAIM_EVENT_TOPIC.to_string()))]),
            ..Default::default()
        };

        assert!(filter.matches(&log, 500));
    }

    #[test]
    fn test_log_store_add_and_query() {
        let store = LogStore::new();

        store.add_claim_event(
            "0xBridge",
            100,
            [0xAA; 32],
            "0xTxHash",
            &[0x11; 32],
            1,
            &[0x22; 20],
            &[0x33; 20],
            1000,
        );

        let filter = LogFilter {
            from_block: Some("0x0".to_string()),
            to_block: Some("0x200".to_string()),
            ..Default::default()
        };

        let logs = store.get_logs(&filter, 500);
        assert_eq!(logs.len(), 1);
        assert_eq!(logs[0].block_number, 100);
    }
}
