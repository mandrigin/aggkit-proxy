use jsonrpsee::core::async_trait;
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::server::Server;
use jsonrpsee::types::ErrorObjectOwned;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{info, warn};

/// Miden chain ID (placeholder - configure as needed)
const MIDEN_CHAIN_ID: u64 = 0x4d494445; // "MIDE" in hex

/// Fixed gas estimate for bridge operations
const FIXED_GAS_ESTIMATE: u64 = 21000;

/// Transaction status in the Miden bridge
#[derive(Debug, Clone, Serialize, Deserialize)]
pub enum TxStatus {
    Pending,
    Confirmed { block_number: u64 },
    Failed { reason: String },
}

/// Transaction receipt mapped from Miden format to Ethereum format
#[derive(Debug, Clone, Serialize, Deserialize)]
#[serde(rename_all = "camelCase")]
pub struct TransactionReceipt {
    pub transaction_hash: String,
    pub block_number: String,
    pub block_hash: String,
    pub transaction_index: String,
    pub from: String,
    pub to: Option<String>,
    pub gas_used: String,
    pub cumulative_gas_used: String,
    pub status: String,
    pub logs: Vec<serde_json::Value>,
    pub logs_bloom: String,
    #[serde(rename = "type")]
    pub tx_type: String,
    pub effective_gas_price: String,
}

/// State for tracking transactions and nonces
pub struct BridgeState {
    /// Synthetic nonces per account address
    nonces: RwLock<HashMap<String, u64>>,
    /// Transaction status tracking
    transactions: RwLock<HashMap<String, TxStatus>>,
    /// Current Miden block height (fetched from node)
    block_height: RwLock<u64>,
}

impl BridgeState {
    pub fn new() -> Self {
        Self {
            nonces: RwLock::new(HashMap::new()),
            transactions: RwLock::new(HashMap::new()),
            block_height: RwLock::new(0),
        }
    }

    pub fn get_nonce(&self, address: &str) -> u64 {
        let addr = address.to_lowercase();
        *self.nonces.read().get(&addr).unwrap_or(&0)
    }

    pub fn increment_nonce(&self, address: &str) -> u64 {
        let addr = address.to_lowercase();
        let mut nonces = self.nonces.write();
        let nonce = nonces.entry(addr).or_insert(0);
        *nonce += 1;
        *nonce - 1
    }

    pub fn record_tx(&self, hash: String, status: TxStatus) {
        self.transactions.write().insert(hash, status);
    }

    pub fn get_tx_status(&self, hash: &str) -> Option<TxStatus> {
        self.transactions.read().get(hash).cloned()
    }

    pub fn set_block_height(&self, height: u64) {
        *self.block_height.write() = height;
    }

    pub fn get_block_height(&self) -> u64 {
        *self.block_height.read()
    }
}

impl Default for BridgeState {
    fn default() -> Self {
        Self::new()
    }
}

/// JSON-RPC API definition using jsonrpsee macros
#[rpc(server)]
pub trait EthApi {
    /// Returns the chain ID
    #[method(name = "eth_chainId")]
    async fn chain_id(&self) -> Result<String, ErrorObjectOwned>;

    /// Returns the current gas price (always 0 for Miden bridge)
    #[method(name = "eth_gasPrice")]
    async fn gas_price(&self) -> Result<String, ErrorObjectOwned>;

    /// Returns a fixed gas estimate
    #[method(name = "eth_estimateGas")]
    async fn estimate_gas(
        &self,
        tx: serde_json::Value,
        block: Option<String>,
    ) -> Result<String, ErrorObjectOwned>;

    /// Returns the transaction count (nonce) for an address
    #[method(name = "eth_getTransactionCount")]
    async fn get_transaction_count(
        &self,
        address: String,
        block: Option<String>,
    ) -> Result<String, ErrorObjectOwned>;

    /// Submits a raw transaction for processing
    #[method(name = "eth_sendRawTransaction")]
    async fn send_raw_transaction(&self, data: String) -> Result<String, ErrorObjectOwned>;

    /// Returns the receipt of a transaction by hash
    #[method(name = "eth_getTransactionReceipt")]
    async fn get_transaction_receipt(
        &self,
        hash: String,
    ) -> Result<Option<TransactionReceipt>, ErrorObjectOwned>;

    /// Executes a call without creating a transaction
    #[method(name = "eth_call")]
    async fn call(
        &self,
        tx: serde_json::Value,
        block: Option<String>,
    ) -> Result<String, ErrorObjectOwned>;

    /// Returns the current block number
    #[method(name = "eth_blockNumber")]
    async fn block_number(&self) -> Result<String, ErrorObjectOwned>;
}

/// Implementation of the Ethereum JSON-RPC API for Miden bridge
pub struct EthApiImpl {
    state: Arc<BridgeState>,
}

impl EthApiImpl {
    pub fn new(state: Arc<BridgeState>) -> Self {
        Self { state }
    }
}

#[async_trait]
impl EthApiServer for EthApiImpl {
    async fn chain_id(&self) -> Result<String, ErrorObjectOwned> {
        Ok(format!("{:#x}", MIDEN_CHAIN_ID))
    }

    async fn gas_price(&self) -> Result<String, ErrorObjectOwned> {
        // Miden bridge has no gas fees
        Ok("0x0".to_string())
    }

    async fn estimate_gas(
        &self,
        _tx: serde_json::Value,
        _block: Option<String>,
    ) -> Result<String, ErrorObjectOwned> {
        // Return fixed estimate for bridge operations
        Ok(format!("{:#x}", FIXED_GAS_ESTIMATE))
    }

    async fn get_transaction_count(
        &self,
        address: String,
        _block: Option<String>,
    ) -> Result<String, ErrorObjectOwned> {
        let nonce = self.state.get_nonce(&address);
        Ok(format!("{:#x}", nonce))
    }

    async fn send_raw_transaction(&self, data: String) -> Result<String, ErrorObjectOwned> {
        // TODO: Implement actual claim processing in separate task
        // For now, just generate a placeholder tx hash and record as pending
        let tx_hash = format!(
            "0x{:064x}",
            std::time::SystemTime::now()
                .duration_since(std::time::UNIX_EPOCH)
                .unwrap()
                .as_nanos()
        );

        warn!(
            tx_hash = %tx_hash,
            data_len = data.len(),
            "Received raw transaction - claim processing not yet implemented"
        );

        self.state.record_tx(tx_hash.clone(), TxStatus::Pending);
        Ok(tx_hash)
    }

    async fn get_transaction_receipt(
        &self,
        hash: String,
    ) -> Result<Option<TransactionReceipt>, ErrorObjectOwned> {
        let status = match self.state.get_tx_status(&hash) {
            Some(s) => s,
            None => return Ok(None),
        };

        let receipt = match status {
            TxStatus::Pending => return Ok(None),
            TxStatus::Confirmed { block_number } => TransactionReceipt {
                transaction_hash: hash,
                block_number: format!("{:#x}", block_number),
                block_hash: format!("0x{:064x}", block_number),
                transaction_index: "0x0".to_string(),
                from: "0x0000000000000000000000000000000000000000".to_string(),
                to: None,
                gas_used: format!("{:#x}", FIXED_GAS_ESTIMATE),
                cumulative_gas_used: format!("{:#x}", FIXED_GAS_ESTIMATE),
                status: "0x1".to_string(),
                logs: vec![],
                logs_bloom: format!("0x{:0512}", 0),
                tx_type: "0x0".to_string(),
                effective_gas_price: "0x0".to_string(),
            },
            TxStatus::Failed { .. } => TransactionReceipt {
                transaction_hash: hash,
                block_number: format!("{:#x}", self.state.get_block_height()),
                block_hash: format!("0x{:064x}", self.state.get_block_height()),
                transaction_index: "0x0".to_string(),
                from: "0x0000000000000000000000000000000000000000".to_string(),
                to: None,
                gas_used: format!("{:#x}", FIXED_GAS_ESTIMATE),
                cumulative_gas_used: format!("{:#x}", FIXED_GAS_ESTIMATE),
                status: "0x0".to_string(),
                logs: vec![],
                logs_bloom: format!("0x{:0512}", 0),
                tx_type: "0x0".to_string(),
                effective_gas_price: "0x0".to_string(),
            },
        };

        Ok(Some(receipt))
    }

    async fn call(
        &self,
        tx: serde_json::Value,
        _block: Option<String>,
    ) -> Result<String, ErrorObjectOwned> {
        // Simulate bridge state queries
        // For now return empty data - actual implementation would decode
        // the call data and query bridge state
        info!(tx = ?tx, "eth_call - bridge state query");
        Ok("0x".to_string())
    }

    async fn block_number(&self) -> Result<String, ErrorObjectOwned> {
        let height = self.state.get_block_height();
        Ok(format!("{:#x}", height))
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("miden_rpc=info".parse()?),
        )
        .init();

    let state = Arc::new(BridgeState::new());
    let rpc_impl = EthApiImpl::new(state);

    let addr = "127.0.0.1:8545";
    info!("Starting Miden RPC server on {}", addr);

    let server = Server::builder().build(addr).await?;
    let handle = server.start(rpc_impl.into_rpc());

    info!("Miden RPC server running on http://{}", addr);
    handle.stopped().await;

    Ok(())
}
