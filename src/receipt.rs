//! Receipt Management - Maps synthetic Eth tx hashes to Miden transaction IDs
//! and generates Ethereum-formatted receipts.

use std::collections::HashMap;
use std::sync::RwLock;

/// Ethereum transaction hash (32 bytes)
pub type EthTxHash = [u8; 32];

/// Miden transaction ID
pub type MidenTxId = String;

/// Bridge contract address for receipt `to` field and ClaimEvent emission
/// Must match the L2 bridge address that the bridge service queries for events
pub const BRIDGE_CONTRACT_ADDRESS: &str = "0xc8cbebf950b9df44d987c8619f092bea980ff038";

/// Transaction status in the Miden network
#[derive(Debug, Clone, Copy, PartialEq, Eq)]
pub enum TxStatus {
    /// Transaction submitted, awaiting confirmation
    Pending,
    /// Transaction confirmed successfully
    Confirmed,
    /// Transaction failed
    Failed,
}

/// Tracks the mapping between Ethereum tx hashes and Miden tx IDs
pub struct TxHashMap {
    /// Eth hash -> Miden tx ID
    hash_to_id: RwLock<HashMap<EthTxHash, MidenTxId>>,
    /// Miden tx ID -> status
    tx_status: RwLock<HashMap<MidenTxId, TxStatus>>,
    /// Miden tx ID -> block info (block_number, block_hash)
    tx_block_info: RwLock<HashMap<MidenTxId, (u64, [u8; 32])>>,
}

impl TxHashMap {
    /// Create a new empty transaction hash map
    pub fn new() -> Self {
        Self {
            hash_to_id: RwLock::new(HashMap::new()),
            tx_status: RwLock::new(HashMap::new()),
            tx_block_info: RwLock::new(HashMap::new()),
        }
    }

    /// Register a new transaction mapping
    pub fn register(&self, eth_hash: EthTxHash, miden_tx_id: MidenTxId) {
        self.hash_to_id.write().unwrap().insert(eth_hash, miden_tx_id.clone());
        self.tx_status.write().unwrap().insert(miden_tx_id, TxStatus::Pending);
    }

    /// Get Miden tx ID from Eth hash
    pub fn get_miden_id(&self, eth_hash: &EthTxHash) -> Option<MidenTxId> {
        self.hash_to_id.read().unwrap().get(eth_hash).cloned()
    }

    /// Update transaction status
    pub fn set_status(&self, miden_tx_id: &MidenTxId, status: TxStatus) {
        self.tx_status.write().unwrap().insert(miden_tx_id.clone(), status);
    }

    /// Get transaction status
    pub fn get_status(&self, miden_tx_id: &MidenTxId) -> Option<TxStatus> {
        self.tx_status.read().unwrap().get(miden_tx_id).copied()
    }

    /// Set block info when transaction is confirmed
    pub fn set_block_info(&self, miden_tx_id: &MidenTxId, block_number: u64, block_hash: [u8; 32]) {
        self.tx_block_info.write().unwrap().insert(miden_tx_id.clone(), (block_number, block_hash));
    }

    /// Get block info for a transaction
    pub fn get_block_info(&self, miden_tx_id: &MidenTxId) -> Option<(u64, [u8; 32])> {
        self.tx_block_info.read().unwrap().get(miden_tx_id).copied()
    }
}

impl Default for TxHashMap {
    fn default() -> Self {
        Self::new()
    }
}

/// Ethereum-formatted transaction receipt
#[derive(Debug, Clone)]
pub struct EthReceipt {
    /// Transaction hash (32 bytes)
    pub transaction_hash: EthTxHash,
    /// Block number containing this transaction
    pub block_number: u64,
    /// Block hash (32 bytes)
    pub block_hash: [u8; 32],
    /// Transaction status: 0x1 for success, 0x0 for failure
    pub status: u8,
    /// Recipient address
    pub to: String,
    /// Index of this transaction within the block
    pub transaction_index: u64,
    /// Cumulative gas used in the block up to this transaction
    pub cumulative_gas_used: u64,
    /// Gas used by this specific transaction
    pub gas_used: u64,
}

impl EthReceipt {
    /// Format as JSON-RPC response object
    pub fn to_json(&self) -> serde_json::Value {
        serde_json::json!({
            "transactionHash": format!("0x{}", hex::encode(self.transaction_hash)),
            "blockNumber": format!("0x{:x}", self.block_number),
            "blockHash": format!("0x{}", hex::encode(self.block_hash)),
            "status": format!("0x{:x}", self.status),
            "to": self.to,
            "transactionIndex": format!("0x{:x}", self.transaction_index),
            "cumulativeGasUsed": format!("0x{:x}", self.cumulative_gas_used),
            "gasUsed": format!("0x{:x}", self.gas_used),
            "logs": [],
            "logsBloom": "0x00000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000000"
        })
    }
}

/// Convert a Miden transaction to an Ethereum-formatted receipt
pub fn miden_tx_to_eth_receipt(
    tx_map: &TxHashMap,
    eth_hash: &EthTxHash,
) -> Option<EthReceipt> {
    let miden_tx_id = tx_map.get_miden_id(eth_hash)?;
    let status = tx_map.get_status(&miden_tx_id)?;

    // Only return receipt if confirmed or failed
    match status {
        TxStatus::Pending => None,
        TxStatus::Confirmed | TxStatus::Failed => {
            let (block_number, block_hash) = tx_map.get_block_info(&miden_tx_id)
                .unwrap_or((0, [0u8; 32]));

            Some(EthReceipt {
                transaction_hash: *eth_hash,
                block_number,
                block_hash,
                status: if status == TxStatus::Confirmed { 0x1 } else { 0x0 },
                to: BRIDGE_CONTRACT_ADDRESS.to_string(),
                transaction_index: 0,
                cumulative_gas_used: 21000,
                gas_used: 21000,
            })
        }
    }
}

/// Poll Miden state for transaction confirmation
pub async fn poll_tx_confirmation<F, Fut>(
    tx_map: &TxHashMap,
    miden_tx_id: &MidenTxId,
    check_fn: F,
    max_attempts: u32,
    delay_ms: u64,
) -> Result<TxStatus, &'static str>
where
    F: Fn(MidenTxId) -> Fut,
    Fut: std::future::Future<Output = Option<(TxStatus, Option<(u64, [u8; 32])>)>>,
{
    for _ in 0..max_attempts {
        if let Some((status, block_info)) = check_fn(miden_tx_id.clone()).await {
            tx_map.set_status(miden_tx_id, status);
            if let Some((block_num, block_hash)) = block_info {
                tx_map.set_block_info(miden_tx_id, block_num, block_hash);
            }
            if status != TxStatus::Pending {
                return Ok(status);
            }
        }
        tokio::time::sleep(tokio::time::Duration::from_millis(delay_ms)).await;
    }
    Err("Transaction confirmation timeout")
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_tx_hash_map_register_and_lookup() {
        let map = TxHashMap::new();
        let eth_hash = [0x42u8; 32];
        let miden_id = "miden_tx_123".to_string();

        map.register(eth_hash, miden_id.clone());

        assert_eq!(map.get_miden_id(&eth_hash), Some(miden_id.clone()));
        assert_eq!(map.get_status(&miden_id), Some(TxStatus::Pending));
    }

    #[test]
    fn test_status_update() {
        let map = TxHashMap::new();
        let eth_hash = [0x42u8; 32];
        let miden_id = "miden_tx_456".to_string();

        map.register(eth_hash, miden_id.clone());
        map.set_status(&miden_id, TxStatus::Confirmed);
        map.set_block_info(&miden_id, 100, [0xAAu8; 32]);

        assert_eq!(map.get_status(&miden_id), Some(TxStatus::Confirmed));
        assert_eq!(map.get_block_info(&miden_id), Some((100, [0xAAu8; 32])));
    }

    #[test]
    fn test_miden_tx_to_eth_receipt_pending() {
        let map = TxHashMap::new();
        let eth_hash = [0x42u8; 32];
        let miden_id = "miden_tx_789".to_string();

        map.register(eth_hash, miden_id);

        // Pending transactions should not return a receipt
        assert!(miden_tx_to_eth_receipt(&map, &eth_hash).is_none());
    }

    #[test]
    fn test_miden_tx_to_eth_receipt_confirmed() {
        let map = TxHashMap::new();
        let eth_hash = [0x42u8; 32];
        let miden_id = "miden_tx_confirmed".to_string();

        map.register(eth_hash, miden_id.clone());
        map.set_status(&miden_id, TxStatus::Confirmed);
        map.set_block_info(&miden_id, 500, [0xBBu8; 32]);

        let receipt = miden_tx_to_eth_receipt(&map, &eth_hash).unwrap();
        assert_eq!(receipt.status, 0x1);
        assert_eq!(receipt.block_number, 500);
        assert_eq!(receipt.to, BRIDGE_CONTRACT_ADDRESS);
    }

    #[test]
    fn test_miden_tx_to_eth_receipt_failed() {
        let map = TxHashMap::new();
        let eth_hash = [0x42u8; 32];
        let miden_id = "miden_tx_failed".to_string();

        map.register(eth_hash, miden_id.clone());
        map.set_status(&miden_id, TxStatus::Failed);
        map.set_block_info(&miden_id, 501, [0xCCu8; 32]);

        let receipt = miden_tx_to_eth_receipt(&map, &eth_hash).unwrap();
        assert_eq!(receipt.status, 0x0);
    }

    #[test]
    fn test_receipt_json_format() {
        let receipt = EthReceipt {
            transaction_hash: [0x42u8; 32],
            block_number: 100,
            block_hash: [0xAAu8; 32],
            status: 0x1,
            to: BRIDGE_CONTRACT_ADDRESS.to_string(),
            transaction_index: 0,
            cumulative_gas_used: 21000,
            gas_used: 21000,
        };

        let json = receipt.to_json();
        assert_eq!(json["status"], "0x1");
        assert_eq!(json["blockNumber"], "0x64");
        assert_eq!(json["to"], BRIDGE_CONTRACT_ADDRESS);
    }
}
