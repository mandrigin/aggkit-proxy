//! Miden Client Integration
//!
//! Provides a wrapper around the Miden client for:
//! - Client initialization with RPC endpoint
//! - P2ID note creation for claim distribution
//! - Transaction submission via TransactionRequestBuilder
//! - State synchronization for confirmation tracking
//!
//! NOTE: When miden-agglayer becomes compatible with miden-client (version alignment),
//! switch to using miden-agglayer's create_claim_note() for bridge-specific validation.

use std::path::{Path, PathBuf};
use std::sync::Arc;

use anyhow::Result;
use miden_client::Client;
use miden_protocol::{
    account::AccountId,
    asset::FungibleAsset,
    note::{Note, NoteType},
    Felt,
};
use tokio::sync::RwLock;
use tracing::{debug, error, info, instrument, warn};

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

/// Parameters for creating a bridge claim note
#[derive(Debug, Clone)]
pub struct BridgeClaimParams {
    /// Bridge faucet account sending the assets
    pub sender_account_id: AccountId,
    /// Target Miden account receiving the claim
    pub recipient_account_id: AccountId,
    /// Fungible assets being transferred
    pub assets: Vec<FungibleAsset>,
    /// Type of note (Public, Private, etc.)
    pub note_type: NoteType,
}

/// Create a P2ID (Pay to ID) note for claim distribution
///
/// This is the core function for creating bridge claim notes that transfer
/// assets from the bridge faucet to a recipient's Miden account.
///
/// NOTE: When miden-agglayer becomes version-compatible with miden-client,
/// switch to using miden-agglayer's create_claim_note() for bridge-specific
/// SMT proof validation.
///
/// # Arguments
/// * `params` - Bridge claim parameters
/// * `rng` - Random number generator for note creation
///
/// # Returns
/// The created Note, or an error
#[instrument(skip(rng), fields(
    sender = %params.sender_account_id,
    recipient = %params.recipient_account_id,
    asset_count = params.assets.len(),
))]
pub fn create_bridge_claim_note<R>(
    params: BridgeClaimParams,
    rng: &mut R,
) -> Result<Note, ClientError>
where
    R: miden_protocol::crypto::rand::FeltRng,
{
    use miden_standards::note::create_p2id_note;
    use miden_protocol::asset::Asset;

    info!(
        sender = %params.sender_account_id,
        recipient = %params.recipient_account_id,
        asset_count = params.assets.len(),
        note_type = ?params.note_type,
        "Creating bridge claim P2ID note"
    );

    let assets: Vec<Asset> = params.assets.into_iter().map(Asset::Fungible).collect();

    debug!(
        asset_count = assets.len(),
        "Converted assets to Asset enum"
    );

    // Create the P2ID note using miden-standards helper
    // P2ID notes can only be consumed by the target account
    info!("Calling miden-standards create_p2id_note...");
    let note = create_p2id_note(
        params.sender_account_id,
        params.recipient_account_id,
        assets,
        params.note_type,
        Felt::new(0), // aux field
        rng,
    )
    .map_err(|e| {
        warn!(error = %e, "Failed to create P2ID note");
        ClientError::NoteCreationError(e.to_string())
    })?;

    let note_id = note.id();
    info!(?note_id, "Bridge claim P2ID note created successfully");

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
    _sender_account_id: AccountId,
    output_notes: Vec<Note>,
) -> Result<miden_client::transaction::TransactionRequest, ClientError>
{
    use miden_client::transaction::{TransactionRequestBuilder, OutputNote};

    info!(
        num_notes = output_notes.len(),
        "Building claim transaction request"
    );

    // Convert Notes to OutputNotes for the builder
    let output_notes: Vec<OutputNote> = output_notes
        .into_iter()
        .map(OutputNote::Full)
        .collect();

    // Build transaction request with output notes
    let tx_request = TransactionRequestBuilder::new()
        .own_output_notes(output_notes)
        .build()
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
) -> Result<Client<impl miden_client::auth::TransactionAuthenticator>, ClientError> {
    use miden_client::builder::ClientBuilder;
    use miden_client::rpc::Endpoint;
    use miden_client_sqlite_store::SqliteStore;
    use std::sync::Arc;

    info!(rpc_endpoint = %config.rpc_endpoint, "Initializing Miden client");

    // Initialize the SQLite store
    let store = SqliteStore::new(config.store_path.clone())
        .await
        .map_err(|e| ClientError::InitializationError(e.to_string()))?;

    // Parse the RPC endpoint
    let endpoint = Endpoint::try_from(config.rpc_endpoint.as_str())
        .map_err(|e| ClientError::InitializationError(format!("Invalid endpoint: {}", e)))?;

    // Build the client using the new builder pattern
    // Use parent directory of store_path (which is a file) for keystore
    let keystore_path = config
        .store_path
        .parent()
        .unwrap_or(Path::new("."))
        .join("keystore");
    let keystore_path_str = keystore_path.to_string_lossy();
    let client: Client<miden_client::keystore::FilesystemKeyStore> = ClientBuilder::new()
        .grpc_client(&endpoint, Some(10_000))
        .store(Arc::new(store))
        .filesystem_keystore(&keystore_path_str)
        .build()
        .await
        .map_err(|e| ClientError::InitializationError(e.to_string()))?;

    info!("Miden client initialized successfully");
    Ok(client)
}

/// Sync client state and return summary
pub async fn sync_state<AUTH>(
    client: &mut Client<AUTH>,
) -> Result<SyncSummary, ClientError>
where
    AUTH: miden_client::auth::TransactionAuthenticator + Sync + 'static,
{
    info!("Synchronizing state with Miden network");

    let sync_result = client
        .sync_state()
        .await
        .map_err(|e| ClientError::SyncError(e.to_string()))?;

    let summary = SyncSummary {
        block_num: sync_result.block_num.as_u32(),
        new_notes: sync_result.new_public_notes.len(),
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
///
/// This function executes, proves, and submits a transaction in one operation.
pub async fn submit_transaction<AUTH>(
    client: &mut Client<AUTH>,
    account_id: AccountId,
    tx_request: miden_client::transaction::TransactionRequest,
) -> Result<String, ClientError>
where
    AUTH: miden_client::auth::TransactionAuthenticator + Sync + 'static,
{
    info!("Submitting transaction to Miden network");

    // Execute, prove, and submit the transaction in one operation
    let tx_id = client
        .submit_new_transaction(account_id, tx_request)
        .await
        .map_err(|e| {
            // Log detailed error information
            error!(
                error = %e,
                error_debug = ?e,
                account_id = ?account_id,
                "Transaction submission failed"
            );
            ClientError::TransactionError(e.to_string())
        })?;

    let tx_id_hex = hex::encode(tx_id.as_bytes());

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
