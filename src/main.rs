use jsonrpsee::core::async_trait;
use jsonrpsee::proc_macros::rpc;
use jsonrpsee::server::Server;
use jsonrpsee::types::ErrorObjectOwned;
use parking_lot::RwLock;
use serde::{Deserialize, Serialize};
use sha3::{Digest, Keccak256};
use std::collections::HashMap;
use std::sync::Arc;
use tracing::{debug, error, info, warn};

use std::path::PathBuf;

// Import library modules for claim processing (P2ID mint approach)
use miden_rpc_proxy::{
    decode_transaction, init_client, is_claim_asset, parse_claim_asset, submit_transaction,
    AddressMapper, AddressMapperConfig, ClaimTracker, ClientError, EthAddress, MidenClientConfig,
    CLAIM_ASSET_SELECTOR,
};

use alloy_primitives::{Address, Bytes};

// Miden protocol types
use miden_protocol::account::{AccountId, AccountIdV0};

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

/// Configuration for Miden network submission
#[derive(Debug, Clone)]
pub struct MidenSubmissionConfig {
    /// RPC endpoint for Miden node
    pub rpc_endpoint: String,
    /// Path to SQLite store for client state
    pub store_path: PathBuf,
    /// Bridge faucet account ID (hex string, e.g., "0x...")
    pub bridge_faucet_id_hex: String,
    /// Path to faucet account file (.mac) containing keys
    pub faucet_account_file: Option<PathBuf>,
}

impl Default for MidenSubmissionConfig {
    fn default() -> Self {
        Self {
            rpc_endpoint: "http://localhost:57291".to_string(),
            store_path: PathBuf::from("/tmp/miden-bridge-client"),
            bridge_faucet_id_hex: String::new(),
            faucet_account_file: None,
        }
    }
}

/// Implementation of the Ethereum JSON-RPC API for Miden bridge
pub struct EthApiImpl {
    state: Arc<BridgeState>,
    miden_config: Option<MidenSubmissionConfig>,
    /// Miden node RPC URL for block height queries
    miden_rpc_url: String,
    /// Store path for miden client state
    miden_store_path: PathBuf,
}

impl EthApiImpl {
    pub fn new(state: Arc<BridgeState>, miden_rpc_url: String) -> Self {
        Self {
            state,
            miden_config: None,
            miden_rpc_url,
            miden_store_path: PathBuf::from("/app/data/miden-client"),
        }
    }

    pub fn with_miden_config(state: Arc<BridgeState>, config: MidenSubmissionConfig, miden_rpc_url: String) -> Self {
        Self {
            state,
            miden_config: Some(config),
            miden_rpc_url,
            miden_store_path: PathBuf::from("/app/data/miden-client"),
        }
    }
}

/// Data needed for Miden CLAIM note submission (must be Send + 'static for spawn_blocking)
#[derive(Debug, Clone)]
struct ClaimSubmissionData {
    // === SMT Proof Data (from claimAsset calldata) ===
    /// SMT proof for local exit root (32 siblings, each 32 bytes)
    smt_proof_local_exit_root: Vec<[u8; 32]>,
    /// SMT proof for rollup exit root (32 siblings, each 32 bytes)
    smt_proof_rollup_exit_root: Vec<[u8; 32]>,
    /// Global index (uint256)
    global_index: [u8; 32],
    /// Mainnet exit root hash (32 bytes)
    mainnet_exit_root: [u8; 32],
    /// Rollup exit root hash (32 bytes)
    rollup_exit_root: [u8; 32],

    // === Leaf Data ===
    /// Origin network identifier (uint32)
    origin_network: u32,
    /// Origin token address (20 bytes)
    origin_token_address: [u8; 20],
    /// Destination network identifier (uint32)
    destination_network: u32,
    /// Destination address (20 bytes)
    destination_address: [u8; 20],
    /// Amount (scaled to Miden decimals)
    amount: u64,
    /// Metadata bytes
    metadata: Vec<u8>,

    // === Miden-specific ===
    /// Recipient's Miden account ID (15 bytes)
    recipient_account_bytes: [u8; 15],
}

/// Submit a claim to the Miden network using CLAIM notes
///
/// Uses `create_bridge_claim_note()` from miden-agglayer to create a CLAIM note
/// that instructs the agglayer faucet to mint tokens to the destination account.
///
/// # CLAIM Note Flow
///
/// 1. Create ephemeral user account (submitter of the CLAIM note)
/// 2. Build BridgeClaimParams from claimAsset calldata (SMT proofs, roots, etc.)
/// 3. Call create_bridge_claim_note() to create the CLAIM note
/// 4. Submit transaction from ephemeral user with the CLAIM note as output
/// 5. Agglayer faucet consumes CLAIM note, validates SMT proofs, mints to recipient
///
/// # Infrastructure Requirement
///
/// CLAIM notes require an agglayer-enabled faucet with `agglayer_faucet_component`
/// procedures. The standard `NetworkFungibleFaucet` from genesis.toml will NOT work.
/// If testing with a native faucet, CLAIM note submission will fail.
async fn submit_claim_to_miden(
    config: MidenSubmissionConfig,
    claim_data: ClaimSubmissionData,
) -> Result<u64, ClientError> {
    use miden_client::crypto::FeltRng;
    use miden_client::transaction::{OutputNote, TransactionRequestBuilder};
    use miden_protocol::crypto::rand::RpoRandomCoin;
    use miden_protocol::note::{NoteExecutionMode, NoteTag};
    use miden_protocol::{Felt, FieldElement, Word};
    use miden_rpc_proxy::{create_bridge_claim_note, BridgeClaimParams};

    info!(
        recipient = hex::encode(&claim_data.recipient_account_bytes),
        amount = claim_data.amount,
        rpc_endpoint = %config.rpc_endpoint,
        "Starting Miden claim submission (CLAIM note approach)"
    );

    let runtime_handle = tokio::runtime::Handle::current();

    let result = tokio::task::spawn_blocking(move || {
        runtime_handle.block_on(async {
            // Step 1: Parse the agglayer faucet ID from hex
            let agglayer_faucet_id = parse_account_id_from_hex(&config.bridge_faucet_id_hex)
                .map_err(|e| ClientError::InitializationError(format!("Invalid agglayer faucet ID: {}", e)))?;
            info!(agglayer_faucet_id = ?agglayer_faucet_id, "Parsed agglayer faucet account ID");

            // Step 2: Convert recipient bytes to AccountId
            let recipient_account_id = bytes_to_account_id(&claim_data.recipient_account_bytes)
                .map_err(|e| ClientError::AccountNotFound(format!("Invalid recipient account: {}", e)))?;
            info!(recipient_account_id = ?recipient_account_id, "Converted recipient to AccountId");

            // Step 3: Initialize the Miden client
            let client_config = MidenClientConfig {
                rpc_endpoint: config.rpc_endpoint.clone(),
                store_path: config.store_path.clone(),
                bridge_faucet_id: agglayer_faucet_id,
            };
            let mut client = init_client(&client_config).await?;
            info!("Miden client initialized");

            // Step 4: Sync state to get current block info
            let sync_result = client.sync_state().await
                .map_err(|e| ClientError::SyncError(e.to_string()))?;
            let block_num = sync_result.block_num.as_u32();
            info!(block_num = block_num, "State synced with network");

            // Step 5: Create ephemeral user account for CLAIM note submission
            // Following https://docs.miden.xyz/miden-tutorials/rust-client/create_deploy_tutorial
            use miden_client::account::component::BasicWallet;
            use miden_client::keystore::FilesystemKeyStore;
            use miden_protocol::account::{AccountBuilder, AccountStorageMode, AccountType};
            use miden_protocol::account::auth::AuthSecretKey;
            use miden_standards::account::auth::AuthRpoFalcon512;
            use rand::RngCore;

            info!("Creating ephemeral user account for CLAIM note submission...");

            // Generate account seed
            let mut init_seed = [0u8; 32];
            client.rng().fill_bytes(&mut init_seed);

            // Generate key pair for signing
            let key_pair = AuthSecretKey::new_falcon512_rpo();

            // Build the ephemeral account
            let ephemeral_account = AccountBuilder::new(init_seed)
                .account_type(AccountType::RegularAccountUpdatableCode)
                .storage_mode(AccountStorageMode::Public)
                .with_auth_component(AuthRpoFalcon512::new(key_pair.public_key().to_commitment()))
                .with_component(BasicWallet)
                .build()
                .map_err(|e| ClientError::InitializationError(format!(
                    "Failed to build ephemeral account: {}", e
                )))?;

            let ephemeral_account_id = ephemeral_account.id();
            info!(ephemeral_account_id = ?ephemeral_account_id, "Ephemeral account built");

            // Add account to client (local only, deployed on first tx)
            client.add_account(&ephemeral_account, false).await
                .map_err(|e| ClientError::InitializationError(format!(
                    "Failed to add ephemeral account to client: {}", e
                )))?;
            info!("Ephemeral account added to client");

            // Add signing key to keystore
            let keystore_path = config.store_path
                .parent()
                .unwrap_or(std::path::Path::new("."))
                .join("keystore");
            let keystore = FilesystemKeyStore::new(keystore_path)
                .map_err(|e| ClientError::InitializationError(format!(
                    "Failed to open keystore: {}", e
                )))?;
            keystore.add_key(&key_pair)
                .map_err(|e| ClientError::InitializationError(format!(
                    "Failed to add key to keystore: {}", e
                )))?;
            info!("Signing key added to keystore");

            let submitter_account_id = ephemeral_account_id;
            info!(submitter_account_id = ?submitter_account_id, "Using ephemeral account as CLAIM note submitter");

            // Step 6: Import faucet from network for vault state
            // The CLAIM note references the faucet's assets, so client needs vault state
            info!(faucet_id = ?agglayer_faucet_id, "Importing faucet account from network (for vault state)...");
            client.import_account_by_id(agglayer_faucet_id).await
                .map_err(|e| ClientError::AccountNotFound(format!(
                    "Failed to import faucet account from network: {}", e
                )))?;
            info!("Faucet account imported from network");

            // Sync again to fetch the faucet's vault merkle paths
            info!("Syncing state to fetch faucet vault merkle paths...");
            let sync_result2 = client.sync_state().await
                .map_err(|e| ClientError::SyncError(e.to_string()))?;
            info!(block_num = sync_result2.block_num.as_u32(), "Second sync complete - vault state fetched");

            // Step 7: Convert SMT proofs from bytes to Felts
            // Each 32-byte hash becomes 8 Felt values (4 bytes each as u32)
            let smt_proof_local: Vec<Felt> = claim_data.smt_proof_local_exit_root
                .iter()
                .flat_map(|hash| bytes_to_felts_32(hash))
                .collect();
            let smt_proof_rollup: Vec<Felt> = claim_data.smt_proof_rollup_exit_root
                .iter()
                .flat_map(|hash| bytes_to_felts_32(hash))
                .collect();

            // Convert global_index (32 bytes) to 8 Felts
            let global_index: [Felt; 8] = bytes_to_felts_32(&claim_data.global_index);

            // Convert amount to 8 Felts (treat as u256, but we only use lower bits)
            let amount_felts: [Felt; 8] = {
                let mut felts = [Felt::ZERO; 8];
                // Put the amount in the lowest Felt (little-endian)
                felts[0] = Felt::new(claim_data.amount);
                felts
            };

            // Metadata as 8 Felts (pad or truncate)
            let metadata_felts: [Felt; 8] = {
                let mut felts = [Felt::ZERO; 8];
                for (i, chunk) in claim_data.metadata.chunks(8).take(8).enumerate() {
                    let mut bytes = [0u8; 8];
                    bytes[..chunk.len()].copy_from_slice(chunk);
                    felts[i] = Felt::new(u64::from_le_bytes(bytes));
                }
                felts
            };

            // Generate random P2ID serial number
            let seed = generate_rng_seed();
            let mut rng = RpoRandomCoin::new(seed.map(Felt::new).into());
            let p2id_serial_number: Word = [
                rng.draw_element(),
                rng.draw_element(),
                rng.draw_element(),
                rng.draw_element(),
            ].into();

            // Step 7: Build BridgeClaimParams
            let bridge_claim_params = BridgeClaimParams {
                smt_proof_local_exit_root: smt_proof_local,
                smt_proof_rollup_exit_root: smt_proof_rollup,
                global_index,
                mainnet_exit_root: claim_data.mainnet_exit_root,
                rollup_exit_root: claim_data.rollup_exit_root,
                origin_network: Felt::new(claim_data.origin_network as u64),
                origin_token_address: claim_data.origin_token_address,
                destination_network: Felt::new(claim_data.destination_network as u64),
                destination_address: claim_data.destination_address,
                amount: amount_felts,
                metadata: metadata_felts,
                claim_note_creator_account_id: submitter_account_id,
                agglayer_faucet_account_id: agglayer_faucet_id,
                output_note_tag: NoteTag::for_public_use_case(0, 0, NoteExecutionMode::Local)
                    .map_err(|e| ClientError::NoteCreationError(format!("Failed to create note tag: {:?}", e)))?,
                p2id_serial_number,
                destination_account_id: recipient_account_id,
            };

            info!(
                creator = ?submitter_account_id,
                faucet = ?agglayer_faucet_id,
                destination = ?recipient_account_id,
                "Creating CLAIM note with SMT proofs..."
            );

            // Step 8: Create the CLAIM note using miden-agglayer
            let claim_note = create_bridge_claim_note(bridge_claim_params, &mut rng)?;
            info!(note_id = ?claim_note.id(), "CLAIM note created successfully");

            // Step 9: Build transaction request with the CLAIM note as output
            // The CLAIM note is sent TO the agglayer faucet
            let tx_request = TransactionRequestBuilder::new()
                .own_output_notes(vec![OutputNote::Full(claim_note)])
                .build()
                .map_err(|e| ClientError::TransactionError(format!(
                    "Failed to build transaction request: {}", e
                )))?;
            info!("Built CLAIM note transaction request");

            // Step 10: Submit the transaction from the faucet account
            let miden_tx_id = submit_transaction(&mut client, submitter_account_id, tx_request).await?;
            info!(
                miden_tx_id = %miden_tx_id,
                submitter = ?submitter_account_id,
                agglayer_faucet = ?agglayer_faucet_id,
                recipient = ?recipient_account_id,
                amount = claim_data.amount,
                block_num = block_num,
                "CLAIM note transaction submitted successfully"
            );

            Ok::<u64, ClientError>(block_num as u64)
        })
    })
    .await
    .map_err(|e| ClientError::TransactionError(format!("Task join error: {}", e)))?;

    result
}

/// Parse an AccountId from a hex string
///
/// Supports formats: "0x..." or plain hex
/// Uses AccountIdV0::from_hex which expects a 15-byte (120-bit) hex representation
fn parse_account_id_from_hex(hex_str: &str) -> Result<AccountId, String> {
    // AccountIdV0::from_hex handles both "0x" prefix and plain hex
    let id_v0 = AccountIdV0::from_hex(hex_str)
        .map_err(|e| format!("Invalid account ID hex: {}", e))?;

    // Convert AccountIdV0 to AccountId
    Ok(AccountId::from(id_v0))
}

/// Convert 15-byte MidenAccountId to miden_protocol::AccountId
///
/// Uses AccountIdV0::try_from([u8; 15]) to properly convert the bytes
fn bytes_to_account_id(bytes: &[u8; 15]) -> Result<AccountId, String> {
    // AccountIdV0 implements TryFrom<[u8; 15]>
    let id_v0 = AccountIdV0::try_from(*bytes)
        .map_err(|e| format!("Invalid account ID bytes: {}", e))?;

    // Convert AccountIdV0 to AccountId
    Ok(AccountId::from(id_v0))
}

/// Convert 32-byte hash to 8 Felt values
///
/// Each Felt holds 4 bytes (as u32) from the hash
fn bytes_to_felts_32(bytes: &[u8; 32]) -> [miden_protocol::Felt; 8] {
    use miden_protocol::{Felt, FieldElement};
    let mut felts = [<Felt as FieldElement>::ZERO; 8];
    for (i, chunk) in bytes.chunks(4).enumerate() {
        let value = u32::from_le_bytes(chunk.try_into().unwrap_or([0; 4]));
        felts[i] = Felt::new(value as u64);
    }
    felts
}

/// Generate a random seed for RpoRandomCoin
///
/// Uses system time and thread ID as entropy sources
fn generate_rng_seed() -> [u64; 4] {
    use std::collections::hash_map::DefaultHasher;
    use std::hash::{Hash, Hasher};
    use std::time::{SystemTime, UNIX_EPOCH};

    let now = SystemTime::now()
        .duration_since(UNIX_EPOCH)
        .unwrap_or_default();

    let mut hasher = DefaultHasher::new();
    now.as_nanos().hash(&mut hasher);
    std::thread::current().id().hash(&mut hasher);
    let h1 = hasher.finish();

    hasher = DefaultHasher::new();
    (now.as_nanos() ^ 0xDEADBEEF).hash(&mut hasher);
    let h2 = hasher.finish();

    hasher = DefaultHasher::new();
    (now.as_secs() * 1000000000 + now.subsec_nanos() as u64).hash(&mut hasher);
    let h3 = hasher.finish();

    hasher = DefaultHasher::new();
    (h1 ^ h2 ^ h3).hash(&mut hasher);
    let h4 = hasher.finish();

    [h1, h2, h3, h4]
}

/// Fetch the current block height from miden-node on-demand
///
/// This function creates a miden client, syncs state, and returns the block number.
/// Uses spawn_blocking because miden_client::Client is !Send.
async fn fetch_block_height(rpc_endpoint: &str, store_path: &PathBuf) -> Result<u32, ClientError> {
    use miden_client::rpc::Endpoint;
    use miden_client::builder::ClientBuilder;
    use miden_client_sqlite_store::SqliteStore;

    debug!(rpc_endpoint = %rpc_endpoint, "Fetching block height from miden-node");

    let rpc_endpoint = rpc_endpoint.to_string();
    let store_path = store_path.clone();
    let runtime_handle = tokio::runtime::Handle::current();

    // Use spawn_blocking because miden_client::Client is !Send
    let result = tokio::task::spawn_blocking(move || {
        runtime_handle.block_on(async {
            // Ensure the PARENT directory exists (SqliteStore creates the actual store)
            if let Some(parent) = store_path.parent() {
                std::fs::create_dir_all(parent)
                    .map_err(|e| ClientError::InitializationError(format!("Failed to create parent dir {}: {}", parent.display(), e)))?;
            }

            debug!(store_path = %store_path.display(), "Initializing SQLite store");

            // Initialize SQLite store (store_path should be a DIRECTORY where SqliteStore creates its files)
            let store = SqliteStore::new(store_path.clone())
                .await
                .map_err(|e| ClientError::InitializationError(e.to_string()))?;

            // Parse the RPC endpoint
            let endpoint = Endpoint::try_from(rpc_endpoint.as_str())
                .map_err(|e| ClientError::InitializationError(format!("Invalid endpoint: {}", e)))?;

            // Keystore goes in a sibling directory to avoid conflict with SqliteStore
            let keystore_path = store_path.parent()
                .map(|p| p.join("keystore"))
                .unwrap_or_else(|| PathBuf::from("/app/data/keystore"));
            let keystore_path_str = keystore_path.to_string_lossy();
            let mut client: miden_client::Client<miden_client::keystore::FilesystemKeyStore> =
                ClientBuilder::new()
                    .grpc_client(&endpoint, Some(10_000))
                    .store(Arc::new(store))
                    .filesystem_keystore(&keystore_path_str)
                    .build()
                    .await
                    .map_err(|e| ClientError::InitializationError(e.to_string()))?;

            // Sync state to get current block number
            let sync_result = client.sync_state().await
                .map_err(|e| ClientError::SyncError(e.to_string()))?;

            Ok::<u32, ClientError>(sync_result.block_num.as_u32())
        })
    })
    .await
    .map_err(|e| ClientError::SyncError(format!("Task join error: {}", e)))?;

    result
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

        // Step 2: Detect input format and extract calldata
        // Support two formats:
        // 1. Raw claimAsset calldata (starts with selector 0xccaa2d11)
        // 2. RLP-encoded signed transaction
        let (input_data, sender_address) = if tx_bytes.len() >= 4 && tx_bytes[..4] == CLAIM_ASSET_SELECTOR {
            // Raw calldata format - used by bridge systems
            info!(
                calldata_len = tx_bytes.len(),
                "Detected raw claimAsset calldata (not RLP-encoded transaction)"
            );
            (Bytes::copy_from_slice(&tx_bytes), Address::ZERO)
        } else {
            // RLP-encoded transaction format - used by Ethereum wallets
            info!("Decoding RLP transaction envelope...");
            match decode_transaction(&tx_bytes) {
                Ok(tx) => {
                    info!(
                        from = %tx.from,
                        to = ?tx.to,
                        value = %tx.value,
                        input_len = tx.input.len(),
                        chain_id = ?tx.chain_id,
                        "Transaction decoded successfully"
                    );
                    // Check if this is a claimAsset call
                    if !is_claim_asset(&tx.input) {
                        warn!(
                            input_len = tx.input.len(),
                            selector = ?tx.input.get(..4),
                            "Transaction is NOT a claimAsset call - rejecting"
                        );
                        return Err(ErrorObjectOwned::owned(
                            -32602,
                            "Only claimAsset transactions are supported",
                            None::<()>,
                        ));
                    }
                    (tx.input, tx.from)
                }
                Err(e) => {
                    error!(error = %e, "Failed to decode transaction");
                    return Err(ErrorObjectOwned::owned(
                        -32602,
                        format!("Transaction decode error: {}", e),
                        None::<()>,
                    ));
                }
            }
        };
        info!("Transaction identified as claimAsset call");

        // Step 3: Parse claimAsset parameters
        info!("Parsing claimAsset calldata...");
        let claim_params = match parse_claim_asset(&input_data) {
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

        // Format amount for human readability (assuming 18 decimals like most ERC20s)
        // Convert U256 to string, then parse for division to avoid precision loss
        let amount_wei = claim_params.amount;
        let amount_human = {
            // Convert to f64 for display (may lose precision for very large amounts)
            let wei_str = amount_wei.to_string();
            if let Ok(wei_f64) = wei_str.parse::<f64>() {
                let tokens = wei_f64 / 1e18;
                format!("{:.6}", tokens)
            } else {
                "overflow".to_string()
            }
        };

        info!("╔══════════════════════════════════════════════════════════════════╗");
        info!("║                    CLAIM ASSET DETAILS                           ║");
        info!("╠══════════════════════════════════════════════════════════════════╣");
        info!("║ TX Hash:     {}", tx_hash);
        info!("║ Amount:      {} wei ({} tokens)", amount_wei, amount_human);
        info!("║ Destination: {} (ETH)", claim_params.destination_address);
        info!("║ Dest Miden:  {}", miden_account_id);
        info!("║ Origin Token: {}", claim_params.origin_token_address);
        info!("║ Global Index: {}", claim_params.global_index_raw);
        info!("╚══════════════════════════════════════════════════════════════════╝");

        debug!(
            tx_hash = %tx_hash,
            eth_address = %eth_address,
            miden_account_id = %miden_account_id,
            was_new_account = was_created,
            amount_wei = %amount_wei,
            amount_tokens = %amount_human,
            origin_token = %claim_params.origin_token_address,
            "Claim details (structured)"
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
            from = %sender_address,
            destination_eth = %eth_address,
            destination_miden = %miden_account_id,
            amount = %claim_params.amount,
            global_index = %claim_params.global_index_raw,
            "=== CLAIM PROCESSING COMPLETE (pending Miden submission) ==="
        );

        // Step 10: Submit to Miden network (blocking - errors propagate to RPC client)
        if let Some(ref config) = self.miden_config {
            // Convert U256 amount from 18 decimals (ERC20 wei) to 3 decimals (genesis faucet)
            // Scale factor: 10^15 (18 - 3 = 15 decimal places)
            // Genesis faucet config: decimals=3, max_supply=100_000_000
            const DECIMAL_SCALE: u128 = 1_000_000_000_000_000; // 10^15
            const MIDEN_MAX_AMOUNT: u64 = 100_000_000; // Genesis faucet max_supply

            let scaled_amount = claim_params.amount / alloy_primitives::U256::from(DECIMAL_SCALE);
            let amount_u64: u64 = scaled_amount.try_into().unwrap_or_else(|_| {
                warn!(
                    original_amount = %claim_params.amount,
                    scaled_amount = %scaled_amount,
                    "Scaled amount still exceeds u64::MAX, capping at genesis faucet max"
                );
                MIDEN_MAX_AMOUNT
            });
            // Cap at genesis faucet max if needed
            let amount_u64 = amount_u64.min(MIDEN_MAX_AMOUNT);
            info!(
                original_wei = %claim_params.amount,
                scaled_miden = amount_u64,
                "Converted ERC20 amount (18 decimals) to Miden amount (3 decimals)"
            );

            // Build full CLAIM note submission data from claimAsset calldata
            let claim_data = ClaimSubmissionData {
                // SMT proof data - already [[u8; 32]; 32] arrays
                smt_proof_local_exit_root: claim_params.smt_proof_local_exit_root.to_vec(),
                smt_proof_rollup_exit_root: claim_params.smt_proof_rollup_exit_root.to_vec(),
                // Convert global_index_raw U256 to [u8; 32]
                global_index: claim_params.global_index_raw.to_be_bytes::<32>(),
                // These are already [u8; 32]
                mainnet_exit_root: claim_params.mainnet_exit_root,
                rollup_exit_root: claim_params.rollup_exit_root,
                // Leaf data
                origin_network: claim_params.origin_network,
                // Address has .0.0 to get [u8; 20] via FixedBytes<20>
                origin_token_address: claim_params.origin_token_address.0 .0,
                destination_network: claim_params.destination_network,
                destination_address: claim_params.destination_address.0 .0,
                amount: amount_u64,
                metadata: claim_params.metadata.to_vec(),
                // Miden-specific
                recipient_account_bytes: miden_account_id.to_bytes(),
            };

            info!(
                tx_hash = %tx_hash,
                "Submitting to Miden network (blocking)..."
            );

            // Blocking submission - errors propagate back to RPC client
            match submit_claim_to_miden(config.clone(), claim_data).await {
                Ok(block_num) => {
                    info!(
                        tx_hash = %tx_hash,
                        block_num = block_num,
                        "Miden submission SUCCEEDED"
                    );
                    self.state.record_tx(
                        tx_hash.clone(),
                        TxStatus::Confirmed { block_number: block_num },
                    );
                }
                Err(e) => {
                    error!(
                        tx_hash = %tx_hash,
                        error = %e,
                        "Miden submission FAILED"
                    );
                    self.state.record_tx(
                        tx_hash.clone(),
                        TxStatus::Failed { reason: e.to_string() },
                    );
                    // Return error to RPC client instead of silently failing
                    return Err(ErrorObjectOwned::owned(
                        -32000,
                        format!("Miden transaction failed: {}", e),
                        None::<()>,
                    ));
                }
            }
        } else {
            warn!(
                tx_hash = %tx_hash,
                "Miden submission config not available - transaction will remain pending"
            );
        }

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
        // Fetch block height on-demand from miden-node
        match fetch_block_height(&self.miden_rpc_url, &self.miden_store_path).await {
            Ok(height) => {
                let height_hex = format!("{:#x}", height);
                info!(
                    block_number = height,
                    block_number_hex = %height_hex,
                    "eth_blockNumber: Fetched current Miden block height"
                );
                Ok(height_hex)
            }
            Err(e) => {
                error!(error = %e, "eth_blockNumber: Failed to fetch block height from miden-node");
                Err(ErrorObjectOwned::owned(
                    -32000,
                    format!("Failed to fetch block height: {}", e),
                    None::<()>,
                ))
            }
        }
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

    let git_commit = std::env::var("GIT_COMMIT").unwrap_or_else(|_| "unknown".to_string());
    let version = if git_commit == "unknown" {
        "unknown".to_string()
    } else {
        format!("{} (https://github.com/mandrigin/aggkit-proxy/commit/{})", git_commit, git_commit)
    };

    // Collect all config from environment
    let miden_rpc_url = std::env::var("MIDEN_RPC_URL")
        .unwrap_or_else(|_| "http://localhost:57291".to_string());
    let bridge_faucet_id = std::env::var("BRIDGE_FAUCET_ID").unwrap_or_default();
    let faucet_account_file = std::env::var("BRIDGE_FAUCET_ACCOUNT_FILE")
        .ok()
        .filter(|s| !s.is_empty())
        .map(PathBuf::from);
    let listen_host = std::env::var("LISTEN_HOST").unwrap_or_else(|_| "0.0.0.0".to_string());
    let listen_port = std::env::var("LISTEN_PORT").unwrap_or_else(|_| "8546".to_string());
    let store_path = PathBuf::from("/app/data/miden-client");

    info!("=======================================================");
    info!("  Miden RPC Proxy - Ethereum JSON-RPC to Miden Bridge  ");
    info!("=======================================================");
    info!("Version: {}", version);
    info!("Chain ID: {:#x} (MIDE)", MIDEN_CHAIN_ID);
    info!("Fixed gas estimate: {}", FIXED_GAS_ESTIMATE);
    info!("Configuration:");
    info!("  MIDEN_RPC_URL:      {}", miden_rpc_url);
    info!("  BRIDGE_FAUCET_ID:   {}", if bridge_faucet_id.is_empty() { "(not set)" } else { &bridge_faucet_id });
    info!("  FAUCET_ACCOUNT_FILE:{}", faucet_account_file.as_ref().map(|p| p.display().to_string()).unwrap_or_else(|| "(not set)".to_string()));
    info!("  LISTEN_HOST:        {}", listen_host);
    info!("  LISTEN_PORT:        {}", listen_port);
    info!("  Store Path:         {}", store_path.display());

    info!("Initializing bridge state...");
    let state = Arc::new(BridgeState::new());
    info!("Bridge state initialized successfully");

    // Pre-flight check: verify we can connect to miden-node
    // Server MUST crash if miden-node is unreachable - fail fast and loud
    info!("Performing pre-flight check: connecting to miden-node...");
    match fetch_block_height(&miden_rpc_url, &store_path).await {
        Ok(block_num) => {
            info!(
                block_number = block_num,
                "Pre-flight check PASSED: miden-node is reachable, current block: {}", block_num
            );
        }
        Err(e) => {
            error!(error = %e, "Pre-flight check FAILED: cannot connect to miden-node");
            panic!("FATAL: Cannot connect to miden-node at {}. Error: {}", miden_rpc_url, e);
        }
    }

    // Initialize Miden submission config for claim processing
    let miden_config = MidenSubmissionConfig {
        rpc_endpoint: miden_rpc_url.clone(),
        store_path: store_path.clone(),
        bridge_faucet_id_hex: bridge_faucet_id.clone(),
        faucet_account_file: faucet_account_file.clone(),
    };

    let rpc_impl = if bridge_faucet_id.is_empty() {
        warn!("Starting without Miden submission support (BRIDGE_FAUCET_ID not configured)");
        EthApiImpl::new(state, miden_rpc_url)
    } else {
        info!(
            bridge_faucet_id = %bridge_faucet_id,
            "Miden submission config initialized"
        );
        EthApiImpl::with_miden_config(state, miden_config, miden_rpc_url)
    };
    info!("EthApi implementation created");

    let addr = format!("{}:{}", listen_host, listen_port);
    info!("Starting Miden RPC server on {}", addr);
    info!("Supported methods: eth_chainId, eth_gasPrice, eth_estimateGas, eth_getTransactionCount, eth_sendRawTransaction, eth_getTransactionReceipt, eth_call, eth_blockNumber");

    let server = Server::builder().build(&addr).await?;
    let handle = server.start(rpc_impl.into_rpc());

    info!("Miden RPC server running on http://{}", addr);
    handle.stopped().await;

    Ok(())
}
