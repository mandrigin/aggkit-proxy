use jsonrpsee::core::async_trait;
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::server::Server;
use jsonrpsee::types::ErrorObjectOwned;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use std::collections::HashMap;
use std::path::PathBuf;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

// Import library modules for claim processing
use miden_rpc_proxy::{
    decode_transaction, is_claim_asset, parse_claim_asset, AddressMapper, AddressMapperConfig,
    ClaimTracker, EthAddress,
};

// Import Miden client config (actual submission stubbed out for now)
use miden_rpc_proxy::client::MidenClientConfig;
use miden_protocol::account::AccountId;

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
    /// Claim tracker for replay prevention
    claim_tracker: ClaimTracker,
    /// Address mapper for Eth -> Miden address resolution (wrapped in Mutex for Sync)
    address_mapper: parking_lot::Mutex<AddressMapper>,
    /// Miden client config for on-demand client creation (Client is not Send/Sync)
    miden_config: Option<MidenClientConfig>,
}

impl BridgeState {
    pub fn new() -> Self {
        info!("Initializing BridgeState with in-memory claim tracker and address mapper");

        let claim_tracker = ClaimTracker::in_memory();
        info!("ClaimTracker initialized (in-memory mode)");

        let address_mapper =
            AddressMapper::in_memory(AddressMapperConfig::default()).expect("Failed to init AddressMapper");
        info!("AddressMapper initialized (in-memory mode)");

        Self {
            nonces: RwLock::new(HashMap::new()),
            transactions: RwLock::new(HashMap::new()),
            block_height: RwLock::new(0),
            claim_tracker,
            address_mapper: parking_lot::Mutex::new(address_mapper),
            miden_config: None,
        }
    }

    /// Set the Miden client configuration for on-demand client creation
    pub fn set_miden_config(&mut self, config: MidenClientConfig) {
        info!(rpc_endpoint = %config.rpc_endpoint, "Miden client config set");
        self.miden_config = Some(config);
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
        let chain_id = format!("{:#x}", MIDEN_CHAIN_ID);
        info!(chain_id = %chain_id, "eth_chainId: Returning Miden chain ID");
        Ok(chain_id)
    }

    async fn gas_price(&self) -> Result<String, ErrorObjectOwned> {
        // Miden bridge has no gas fees
        info!("eth_gasPrice: Returning 0x0 (Miden has no gas fees)");
        Ok("0x0".to_string())
    }

    async fn estimate_gas(
        &self,
        tx: serde_json::Value,
        block: Option<String>,
    ) -> Result<String, ErrorObjectOwned> {
        let estimate = format!("{:#x}", FIXED_GAS_ESTIMATE);
        info!(
            estimate = %estimate,
            block = ?block,
            tx_to = ?tx.get("to"),
            "eth_estimateGas: Returning fixed gas estimate for bridge operations"
        );
        Ok(estimate)
    }

    async fn get_transaction_count(
        &self,
        address: String,
        block: Option<String>,
    ) -> Result<String, ErrorObjectOwned> {
        let nonce = self.state.get_nonce(&address);
        let nonce_hex = format!("{:#x}", nonce);
        info!(
            address = %address,
            block = ?block,
            nonce = nonce,
            nonce_hex = %nonce_hex,
            "eth_getTransactionCount: Returning synthetic nonce"
        );
        Ok(nonce_hex)
    }

    async fn send_raw_transaction(&self, data: String) -> Result<String, ErrorObjectOwned> {
        info!(
            data_len = data.len(),
            data_prefix = %data.chars().take(20).collect::<String>(),
            "eth_sendRawTransaction: Received raw transaction"
        );

        // Step 1: Decode hex to bytes
        let raw_bytes = data.strip_prefix("0x").unwrap_or(&data);
        let tx_bytes = match hex::decode(raw_bytes) {
            Ok(bytes) => {
                debug!(byte_len = bytes.len(), "Decoded hex to {} bytes", bytes.len());
                bytes
            }
            Err(e) => {
                error!(error = %e, "Failed to decode hex data");
                return Err(ErrorObjectOwned::owned(
                    -32602,
                    format!("Invalid hex data: {}", e),
                    None::<()>,
                ));
            }
        };

        // Step 2: Decode RLP transaction
        info!("Decoding RLP transaction envelope...");
        let decoded_tx = match decode_transaction(&tx_bytes) {
            Ok(tx) => {
                info!(
                    from = %tx.from,
                    to = ?tx.to,
                    value = %tx.value,
                    input_len = tx.input.len(),
                    chain_id = ?tx.chain_id,
                    "Transaction decoded successfully"
                );
                tx
            }
            Err(e) => {
                error!(error = %e, "Failed to decode transaction");
                return Err(ErrorObjectOwned::owned(
                    -32602,
                    format!("Transaction decode error: {}", e),
                    None::<()>,
                ));
            }
        };

        // Step 3: Check if this is a claimAsset call
        if !is_claim_asset(&decoded_tx.input) {
            warn!(
                input_len = decoded_tx.input.len(),
                selector = ?decoded_tx.input.get(..4),
                "Transaction is NOT a claimAsset call - rejecting"
            );
            return Err(ErrorObjectOwned::owned(
                -32602,
                "Only claimAsset transactions are supported",
                None::<()>,
            ));
        }
        info!("Transaction identified as claimAsset call");

        // Step 4: Parse claimAsset parameters
        info!("Parsing claimAsset calldata...");
        let claim_params = match parse_claim_asset(&decoded_tx.input) {
            Ok(params) => {
                info!(
                    global_index_raw = %params.global_index_raw,
                    mainnet_flag = params.global_index.mainnet_flag,
                    rollup_index = params.global_index.rollup_index,
                    local_root_index = params.global_index.local_root_index,
                    origin_network = params.origin_network,
                    destination_network = params.destination_network,
                    destination_address = %params.destination_address,
                    amount = %params.amount,
                    "claimAsset parameters parsed successfully"
                );
                debug!(
                    mainnet_exit_root = %hex::encode(params.mainnet_exit_root),
                    rollup_exit_root = %hex::encode(params.rollup_exit_root),
                    origin_token = %params.origin_token_address,
                    metadata_len = params.metadata.len(),
                    "Additional claim details"
                );
                params
            }
            Err(e) => {
                error!(error = %e, "Failed to parse claimAsset calldata");
                return Err(ErrorObjectOwned::owned(
                    -32602,
                    format!("claimAsset decode error: {}", e),
                    None::<()>,
                ));
            }
        };

        // Step 5: Check for replay (claim already processed)
        info!(
            global_index = %claim_params.global_index_raw,
            "Checking claim tracker for replay prevention..."
        );
        if let Err(e) = self.state.claim_tracker.try_claim(claim_params.global_index_raw) {
            warn!(
                global_index = %claim_params.global_index_raw,
                error = %e,
                "REPLAY DETECTED: Claim already processed"
            );
            return Err(ErrorObjectOwned::owned(
                -32000,
                format!("Claim already processed: {}", e),
                None::<()>,
            ));
        }
        info!(
            global_index = %claim_params.global_index_raw,
            "Claim registered in tracker (not a replay)"
        );

        // Step 6: Resolve Ethereum address to Miden AccountId
        let eth_address = EthAddress::from_alloy(&claim_params.destination_address);
        info!(
            eth_address = %eth_address,
            "Resolving Ethereum address to Miden AccountId..."
        );
        let (miden_account_id, was_created) = match self.state.address_mapper.lock().get_or_create(&eth_address) {
            Ok((id, created)) => {
                if created {
                    info!(
                        eth_address = %eth_address,
                        miden_account_id = %id,
                        "NEW Miden account created for Ethereum address"
                    );
                } else {
                    info!(
                        eth_address = %eth_address,
                        miden_account_id = %id,
                        "Found existing Miden account mapping"
                    );
                }
                (id, created)
            }
            Err(e) => {
                error!(
                    eth_address = %eth_address,
                    error = %e,
                    "Failed to resolve/create Miden account"
                );
                return Err(ErrorObjectOwned::owned(
                    -32003,
                    format!("Account resolution error: {}", e),
                    None::<()>,
                ));
            }
        };

        // Step 7: Generate synthetic transaction hash
        // Hash the claim parameters to create a deterministic tx hash
        let mut hasher = Keccak256::new();
        hasher.update(b"miden-bridge-claim-v1");
        hasher.update(claim_params.global_index_raw.to_be_bytes::<32>());
        hasher.update(claim_params.destination_address.as_slice());
        hasher.update(claim_params.amount.to_be_bytes::<32>());
        let hash_bytes = hasher.finalize();
        let tx_hash = format!("0x{}", hex::encode(hash_bytes));

        info!(
            tx_hash = %tx_hash,
            eth_address = %eth_address,
            miden_account_id = %miden_account_id,
            was_new_account = was_created,
            amount = %claim_params.amount,
            "Generated synthetic transaction hash"
        );

        // Step 8: Record transaction as pending
        self.state.record_tx(tx_hash.clone(), TxStatus::Pending);
        info!(
            tx_hash = %tx_hash,
            "Transaction recorded as PENDING"
        );

        // Step 9: Log summary
        info!(
            tx_hash = %tx_hash,
            from = %decoded_tx.from,
            destination_eth = %eth_address,
            destination_miden = %miden_account_id,
            amount = %claim_params.amount,
            global_index = %claim_params.global_index_raw,
            "=== CLAIM PROCESSING COMPLETE (pending Miden submission) ==="
        );

        // Step 10: Miden submission (STUB - actual submission via spawn_blocking TODO)
        // The Miden Client is not Send/Sync, so we can't use it directly in async handlers.
        // For now, just log and mark as confirmed. Real submission will be added later
        // using spawn_blocking or a dedicated submission task.
        if self.state.miden_config.is_some() {
            info!(
                tx_hash = %tx_hash,
                recipient = %miden_account_id,
                amount = %claim_params.amount,
                "STUB: Would submit to Miden network (not implemented yet)"
            );
        }

        // Mark as confirmed for now (stub behavior)
        let block_number = self.state.get_block_height();
        self.state.record_tx(tx_hash.clone(), TxStatus::Confirmed { block_number });
        info!(
            tx_hash = %tx_hash,
            block_number = block_number,
            "Transaction marked as CONFIRMED (stub - actual Miden submission TODO)"
        );

        Ok(tx_hash)
    }

    async fn get_transaction_receipt(
        &self,
        hash: String,
    ) -> Result<Option<TransactionReceipt>, ErrorObjectOwned> {
        info!(tx_hash = %hash, "eth_getTransactionReceipt: Looking up receipt");

        let status = match self.state.get_tx_status(&hash) {
            Some(s) => {
                debug!(tx_hash = %hash, status = ?s, "Found transaction status");
                s
            }
            None => {
                info!(tx_hash = %hash, "Transaction not found - returning null");
                return Ok(None);
            }
        };

        let receipt = match status {
            TxStatus::Pending => {
                info!(tx_hash = %hash, "Transaction still PENDING - returning null (no receipt yet)");
                return Ok(None);
            }
            TxStatus::Confirmed { block_number } => {
                info!(
                    tx_hash = %hash,
                    block_number = block_number,
                    "Transaction CONFIRMED - returning success receipt"
                );
                TransactionReceipt {
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
                }
            }
            TxStatus::Failed { ref reason } => {
                warn!(
                    tx_hash = %hash,
                    reason = %reason,
                    "Transaction FAILED - returning failure receipt"
                );
                TransactionReceipt {
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
                }
            }
        };

        Ok(Some(receipt))
    }

    async fn call(
        &self,
        tx: serde_json::Value,
        block: Option<String>,
    ) -> Result<String, ErrorObjectOwned> {
        info!(
            tx_to = ?tx.get("to"),
            tx_data = ?tx.get("data").map(|d| d.as_str().map(|s| s.chars().take(20).collect::<String>())),
            block = ?block,
            "eth_call: Simulating bridge state query"
        );
        // For now return empty data - actual implementation would decode
        // the call data and query bridge state
        debug!("eth_call: Returning empty response (bridge state queries not yet implemented)");
        Ok("0x".to_string())
    }

    async fn block_number(&self) -> Result<String, ErrorObjectOwned> {
        let height = self.state.get_block_height();
        let height_hex = format!("{:#x}", height);
        info!(
            block_number = height,
            block_number_hex = %height_hex,
            "eth_blockNumber: Returning current Miden block height"
        );
        Ok(height_hex)
    }
}

#[tokio::main]
async fn main() -> anyhow::Result<()> {
    // Initialize tracing with verbose output
    tracing_subscriber::fmt()
        .with_env_filter(
            tracing_subscriber::EnvFilter::from_default_env()
                .add_directive("miden_rpc=debug".parse()?)
                .add_directive("miden_rpc_proxy=debug".parse()?),
        )
        .with_target(true)
        .with_thread_ids(true)
        .with_file(true)
        .with_line_number(true)
        .init();

    info!("=======================================================");
    info!("  Miden RPC Proxy - Ethereum JSON-RPC to Miden Bridge  ");
    info!("=======================================================");
    info!("Chain ID: {:#x} (MIDE)", MIDEN_CHAIN_ID);
    info!("Fixed gas estimate: {}", FIXED_GAS_ESTIMATE);

    info!("Initializing bridge state...");
    let mut state = BridgeState::new();

    // Initialize Miden client if environment variables are set
    if let (Ok(rpc_endpoint), Ok(faucet_id_str)) = (
        std::env::var("MIDEN_RPC_ENDPOINT"),
        std::env::var("MIDEN_BRIDGE_FAUCET_ID"),
    ) {
        info!(
            rpc_endpoint = %rpc_endpoint,
            faucet_id = %faucet_id_str,
            "Miden client configuration found"
        );

        // Parse the faucet account ID (supports decimal u128 or hex with 0x prefix)
        let faucet_id = if faucet_id_str.starts_with("0x") || faucet_id_str.starts_with("0X") {
            // Hex string format
            let hex_str = &faucet_id_str[2..];
            let id = u128::from_str_radix(hex_str, 16).map_err(|e| {
                anyhow::anyhow!("Failed to parse hex MIDEN_BRIDGE_FAUCET_ID: {}", e)
            })?;
            AccountId::try_from(id).map_err(|e| {
                anyhow::anyhow!("Invalid bridge faucet ID: {}", e)
            })?
        } else {
            // Decimal format
            let id = faucet_id_str.parse::<u128>().map_err(|e| {
                anyhow::anyhow!("Failed to parse MIDEN_BRIDGE_FAUCET_ID: {}", e)
            })?;
            AccountId::try_from(id).map_err(|e| {
                anyhow::anyhow!("Invalid bridge faucet ID: {}", e)
            })?
        };

        let store_path = std::env::var("MIDEN_STORE_PATH")
            .map(PathBuf::from)
            .unwrap_or_else(|_| PathBuf::from("./miden_store"));

        let config = MidenClientConfig {
            rpc_endpoint,
            store_path,
            bridge_faucet_id: faucet_id,
        };

        state.set_miden_config(config);
    } else {
        info!("Miden client not configured (set MIDEN_RPC_ENDPOINT and MIDEN_BRIDGE_FAUCET_ID to enable)");
    }

    let state = Arc::new(state);
    info!("Bridge state initialized successfully");

    let rpc_impl = EthApiImpl::new(state);
    info!("EthApi implementation created");

    let addr = "127.0.0.1:8545";
    info!("Starting Miden RPC server on {}", addr);
    info!("Supported methods: eth_chainId, eth_gasPrice, eth_estimateGas, eth_getTransactionCount, eth_sendRawTransaction, eth_getTransactionReceipt, eth_call, eth_blockNumber");

    let server = Server::builder().build(addr).await?;
    let handle = server.start(rpc_impl.into_rpc());

    info!("Miden RPC server running on http://{}", addr);
    handle.stopped().await;

    Ok(())
}
