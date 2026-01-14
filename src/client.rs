//! Miden Client Integration
//!
//! Provides a wrapper around the Miden client for:
//! - Client initialization with RPC endpoint
//! - P2ID note creation for claim distribution
//! - Transaction submission via TransactionRequestBuilder
//! - State synchronization for confirmation tracking

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use miden_client::client::Client;
use miden_objects::{
    accounts::AccountId,
    assets::FungibleAsset,
    notes::{Note, NoteId, NoteType},
    Felt,
};
use tokio::sync::RwLock;
use tracing::{debug, info, instrument};

/// Error types for Miden client operations
#[derive(Debug, thiserror::Error)]
pub enum ClientError {
    /// Failed to initialize the Miden client
    #[error("failed to initialize miden client: {0}")]
    InitializationError(String),

    /// Failed to create a note
    #[error("failed to create note: {0}")]
    NoteCreationError(String),

    /// Failed to submit transaction
    #[error("failed to submit transaction: {0}")]
    TransactionError(String),

    /// Failed to sync state
    #[error("failed to sync state: {0}")]
    SyncError(String),

    /// Account not found
    #[error("account not found: {0}")]
    AccountNotFound(String),
}

/// Configuration for the Miden client wrapper
#[derive(Debug, Clone)]
pub struct MidenClientConfig {
    /// Miden node RPC endpoint URL
    pub rpc_endpoint: String,
    /// Path to SQLite store for client state
    pub store_path: PathBuf,
    /// Bridge faucet account ID for asset distribution
    pub bridge_faucet_id: AccountId,
}

/// Summary of a state sync operation
#[derive(Debug, Clone)]
pub struct SyncSummary {
    /// Current block number after sync
    pub block_num: u32,
    /// Number of new notes received
    pub new_notes: usize,
    /// Number of notes consumed
    pub consumed_notes: usize,
    /// Number of accounts updated
    pub updated_accounts: usize,
}

/// Wrapper around the Miden client providing bridge-specific operations
pub struct MidenClientWrapper<C> {
    /// Inner Miden client instance
    client: Arc<RwLock<C>>,
    /// Configuration
    config: MidenClientConfig,
}

impl<C> MidenClientWrapper<C> {
    /// Create wrapper from existing client
    pub fn from_client(client: C, config: MidenClientConfig) -> Self {
        Self {
            client: Arc::new(RwLock::new(client)),
            config,
        }
    }

    /// Get the bridge faucet account ID
    pub fn bridge_faucet_id(&self) -> AccountId {
        self.config.bridge_faucet_id
    }

    /// Get reference to inner client
    pub async fn client(&self) -> tokio::sync::RwLockReadGuard<'_, C> {
        self.client.read().await
    }

    /// Get mutable reference to inner client
    pub async fn client_mut(&self) -> tokio::sync::RwLockWriteGuard<'_, C> {
        self.client.write().await
    }
}

/// Create a P2ID (Pay to ID) note for claim distribution
///
/// This is the core function for creating bridge claim notes that transfer
/// assets from the bridge faucet to a recipient's Miden account.
///
/// # Arguments
/// * `sender_account_id` - The bridge faucet account sending the assets
/// * `recipient_account_id` - The target Miden account receiving the claim
/// * `assets` - The fungible assets being transferred
/// * `note_type` - Type of note (Public, Private, etc.)
/// * `rng` - Random number generator for note creation
///
/// # Returns
/// The created Note, or an error
#[instrument(skip(rng), fields(
    sender = %sender_account_id,
    recipient = %recipient_account_id,
))]
pub fn create_bridge_claim_note<R: miden_crypto::rand::FeltRng>(
    sender_account_id: AccountId,
    recipient_account_id: AccountId,
    assets: Vec<FungibleAsset>,
    note_type: NoteType,
    rng: &mut R,
) -> Result<Note, ClientError> {
    use miden_lib::notes::create_p2id_note;
    use miden_objects::assets::Asset;

    info!("Creating bridge claim P2ID note");

    let assets: Vec<Asset> = assets.into_iter().map(Asset::Fungible).collect();

    // Create the P2ID note using miden-lib's helper
    // P2ID notes can only be consumed by the target account
    let (note, _note_details) = create_p2id_note(
        sender_account_id,
        recipient_account_id,
        assets,
        note_type,
        Felt::new(0), // recall height
        rng,
    )
    .map_err(|e| ClientError::NoteCreationError(e.to_string()))?;

    let note_id = note.id();
    debug!(?note_id, "Bridge claim note created");

    Ok(note)
}

/// Build a transaction request for sending a P2ID note
///
/// Uses TransactionRequestBuilder to construct a transaction that:
/// 1. Creates output notes (P2ID for the recipient)
/// 2. Specifies the sender account
///
/// # Arguments
/// * `sender_account_id` - Account ID of the sender (bridge faucet)
/// * `output_notes` - Notes to include as outputs
///
/// # Returns
/// A TransactionRequest ready for submission
pub fn build_claim_transaction_request(
    sender_account_id: AccountId,
    output_notes: Vec<Note>,
) -> Result<miden_client::client::transactions::transaction_request::TransactionRequest, ClientError>
{
    use miden_client::client::transactions::transaction_request::TransactionRequest;

    info!(
        sender = %sender_account_id,
        num_notes = output_notes.len(),
        "Building claim transaction request"
    );

    // Build transaction request with output notes
    let tx_request = TransactionRequest::new()
        .with_own_output_notes(output_notes)
        .map_err(|e| ClientError::TransactionError(e.to_string()))?;

    Ok(tx_request)
}

/// Initialize a Miden client with the given configuration
///
/// # Arguments
/// * `config` - Client configuration including RPC endpoint and store path
///
/// # Returns
/// Initialized client or error
pub async fn init_client(
    config: &MidenClientConfig,
) -> Result<Client<impl miden_client::client::rpc::NodeRpcClient>, ClientError> {
    use miden_client::{
        client::rpc::TonicRpcClient,
        config::{ClientConfig, RpcConfig},
        store::sqlite_store::SqliteStore,
    };

    info!(rpc_endpoint = %config.rpc_endpoint, "Initializing Miden client");

    // Create RPC configuration
    let rpc_config = RpcConfig {
        endpoint: config.rpc_endpoint.clone().into(),
        timeout_ms: 10_000,
    };

    // Create client configuration
    let client_config = ClientConfig {
        rpc: rpc_config,
        store: config.store_path.clone().into(),
        ..Default::default()
    };

    // Initialize the SQLite store
    let store = SqliteStore::new(&config.store_path)
        .await
        .map_err(|e| ClientError::InitializationError(e.to_string()))?;

    // Create RPC client
    let rpc_client = TonicRpcClient::new(&client_config.rpc.endpoint)
        .map_err(|e| ClientError::InitializationError(e.to_string()))?;

    // Create random number generator
    let rng = miden_crypto::rand::RpoRandomCoin::new([Felt::new(0); 4]);

    // Initialize the client
    let client = Client::new(rpc_client, rng, store, client_config.executor.clone())
        .map_err(|e| ClientError::InitializationError(e.to_string()))?;

    info!("Miden client initialized successfully");
    Ok(client)
}

/// Sync client state and return summary
pub async fn sync_state<C, R, S>(client: &mut Client<C, R, S>) -> Result<SyncSummary, ClientError>
where
    C: miden_client::client::rpc::NodeRpcClient,
    R: miden_crypto::rand::FeltRng,
    S: miden_client::store::Store,
{
    info!("Synchronizing state with Miden network");

    let sync_result = client
        .sync_state()
        .await
        .map_err(|e| ClientError::SyncError(e.to_string()))?;

    let summary = SyncSummary {
        block_num: sync_result.block_num,
        new_notes: sync_result.received_notes.len(),
        consumed_notes: sync_result.consumed_notes.len(),
        updated_accounts: sync_result.updated_accounts.len(),
    };

    info!(
        block_num = summary.block_num,
        new_notes = summary.new_notes,
        consumed_notes = summary.consumed_notes,
        "State sync complete"
    );

    Ok(summary)
}

/// Submit a transaction using the client
pub async fn submit_transaction<C, R, S>(
    client: &mut Client<C, R, S>,
    tx_request: miden_client::client::transactions::transaction_request::TransactionRequest,
) -> Result<String, ClientError>
where
    C: miden_client::client::rpc::NodeRpcClient,
    R: miden_crypto::rand::FeltRng,
    S: miden_client::store::Store,
{
    info!("Submitting transaction to Miden network");

    // Build and submit the transaction
    let executed_tx = client
        .new_transaction(tx_request)
        .await
        .map_err(|e| ClientError::TransactionError(e.to_string()))?;

    let tx_id = executed_tx.id();
    let tx_id_hex = hex::encode(tx_id.as_bytes());

    // Submit to network
    client
        .submit_transaction(executed_tx)
        .await
        .map_err(|e| ClientError::TransactionError(e.to_string()))?;

    info!(%tx_id_hex, "Transaction submitted successfully");

    Ok(tx_id_hex)
}

#[cfg(test)]
mod tests {
    use super::*;

    #[test]
    fn test_client_error_display() {
        let err = ClientError::InitializationError("test error".to_string());
        assert_eq!(
            err.to_string(),
            "failed to initialize miden client: test error"
        );
    }

    #[test]
    fn test_sync_summary_debug() {
        let summary = SyncSummary {
            block_num: 100,
            new_notes: 5,
            consumed_notes: 2,
            updated_accounts: 1,
        };
        assert!(format!("{:?}", summary).contains("block_num: 100"));
    }
}
