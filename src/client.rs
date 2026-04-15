//! Miden Client Integration
//!
//! Provides a wrapper around the Miden client for:
//! - Client initialization with RPC endpoint
//! - CLAIM note creation for bridge claims using miden-agglayer
//! - Transaction submission via TransactionRequestBuilder
//! - State synchronization for confirmation tracking

use std::path::PathBuf;
use std::sync::Arc;

use anyhow::Result;
use miden_agglayer::{
    create_claim_note, ClaimNoteStorage, EthAddress, EthAmount, ExitRoot, GlobalIndex,
    LeafData, MetadataHash, ProofData, SmtNode,
};
use miden_client::Client;
use miden_protocol::{
    account::AccountId,
    crypto::rand::FeltRng,
    note::Note,
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

/// Parameters for creating a bridge CLAIM note.
///
/// Uses the typed representations from miden-agglayer 0.14.
#[derive(Debug, Clone)]
pub struct BridgeClaimParams {
    // === SMT Proof Data ===
    /// SMT proof for local exit root (32 siblings, each 32 bytes)
    pub smt_proof_local_exit_root: Vec<[u8; 32]>,
    /// SMT proof for rollup exit root (32 siblings, each 32 bytes)
    pub smt_proof_rollup_exit_root: Vec<[u8; 32]>,
    /// Global index (uint256 as 32 bytes)
    pub global_index: [u8; 32],
    /// Mainnet exit root hash (32 bytes)
    pub mainnet_exit_root: [u8; 32],
    /// Rollup exit root hash (32 bytes)
    pub rollup_exit_root: [u8; 32],

    // === Leaf Data ===
    /// Origin network identifier (uint32)
    pub origin_network: u32,
    /// Origin token address (20 bytes)
    pub origin_token_address: [u8; 20],
    /// Destination network identifier (uint32)
    pub destination_network: u32,
    /// Destination address (20 bytes)
    pub destination_address: [u8; 20],
    /// Amount as 32-byte big-endian uint256
    pub amount: [u8; 32],
    /// Metadata hash (32 bytes, keccak256 of metadata)
    pub metadata_hash: [u8; 32],

    // === CLAIM Note Parameters ===
    /// Miden claim amount (scaled-down token amount as Felt)
    pub miden_claim_amount: Felt,
    /// Account ID that creates the CLAIM note
    pub claim_note_creator_account_id: AccountId,
    /// Agglayer faucet AccountId (target for the CLAIM note)
    pub agglayer_faucet_account_id: AccountId,
}

/// Create a CLAIM note for bridge claims.
///
/// Converts BridgeClaimParams into the typed ClaimNoteStorage and calls
/// miden-agglayer's create_claim_note().
#[instrument(skip(params, rng), fields(
    creator = %params.claim_note_creator_account_id,
    faucet = %params.agglayer_faucet_account_id,
))]
pub fn create_bridge_claim_note<R>(
    params: BridgeClaimParams,
    rng: &mut R,
) -> Result<Note, ClientError>
where
    R: FeltRng,
{
    info!(
        creator = %params.claim_note_creator_account_id,
        faucet = %params.agglayer_faucet_account_id,
        "Creating bridge CLAIM note using miden-agglayer"
    );

    // Convert SMT proofs to [SmtNode; 32] arrays
    let smt_local: [SmtNode; 32] = params
        .smt_proof_local_exit_root
        .iter()
        .map(|bytes| SmtNode::new(*bytes))
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|v: Vec<_>| {
            ClientError::NoteCreationError(format!(
                "Expected 32 SMT nodes for local exit root, got {}",
                v.len()
            ))
        })?;

    let smt_rollup: [SmtNode; 32] = params
        .smt_proof_rollup_exit_root
        .iter()
        .map(|bytes| SmtNode::new(*bytes))
        .collect::<Vec<_>>()
        .try_into()
        .map_err(|v: Vec<_>| {
            ClientError::NoteCreationError(format!(
                "Expected 32 SMT nodes for rollup exit root, got {}",
                v.len()
            ))
        })?;

    let proof_data = ProofData {
        smt_proof_local_exit_root: smt_local,
        smt_proof_rollup_exit_root: smt_rollup,
        global_index: GlobalIndex::new(params.global_index),
        mainnet_exit_root: ExitRoot::new(params.mainnet_exit_root),
        rollup_exit_root: ExitRoot::new(params.rollup_exit_root),
    };

    let leaf_data = LeafData {
        origin_network: params.origin_network,
        origin_token_address: EthAddress::new(params.origin_token_address),
        destination_network: params.destination_network,
        destination_address: EthAddress::new(params.destination_address),
        amount: EthAmount::new(params.amount),
        metadata_hash: MetadataHash::new(params.metadata_hash),
    };

    let storage = ClaimNoteStorage {
        proof_data,
        leaf_data,
        miden_claim_amount: params.miden_claim_amount,
    };

    debug!("Calling miden-agglayer create_claim_note...");
    let note = create_claim_note(
        storage,
        params.agglayer_faucet_account_id,
        params.claim_note_creator_account_id,
        rng,
    )
    .map_err(|e| {
        warn!(error = %e, "Failed to create CLAIM note");
        ClientError::NoteCreationError(e.to_string())
    })?;

    let note_id = note.id();
    info!(?note_id, "Bridge CLAIM note created successfully");

    Ok(note)
}

/// Build a transaction request for sending a P2ID note
pub fn build_claim_transaction_request(
    _sender_account_id: AccountId,
    output_notes: Vec<Note>,
) -> Result<miden_client::transaction::TransactionRequest, ClientError>
{
    use miden_client::transaction::TransactionRequestBuilder;

    info!(
        num_notes = output_notes.len(),
        "Building claim transaction request"
    );

    // own_output_notes takes Vec<Note> directly in 0.14
    let tx_request = TransactionRequestBuilder::new()
        .own_output_notes(output_notes)
        .build()
        .map_err(|e| ClientError::TransactionError(e.to_string()))?;

    Ok(tx_request)
}

/// Initialize a Miden client with the given configuration and keystore
pub async fn init_client(
    config: &MidenClientConfig,
    keystore: Arc<miden_client::keystore::FilesystemKeyStore>,
) -> Result<Client<miden_client::keystore::FilesystemKeyStore>, ClientError> {
    use miden_client::builder::ClientBuilder;
    use miden_client::rpc::Endpoint;
    use miden_client_sqlite_store::SqliteStore;

    info!(rpc_endpoint = %config.rpc_endpoint, "Initializing Miden client");

    // Initialize the SQLite store
    let store = SqliteStore::new(config.store_path.clone())
        .await
        .map_err(|e| ClientError::InitializationError(e.to_string()))?;

    // Parse the RPC endpoint
    let endpoint = Endpoint::try_from(config.rpc_endpoint.as_str())
        .map_err(|e| ClientError::InitializationError(format!("Invalid endpoint: {}", e)))?;

    // Build the client using the provided keystore via authenticator()
    let client: Client<miden_client::keystore::FilesystemKeyStore> = ClientBuilder::new()
        .grpc_client(&endpoint, Some(10_000))
        .store(Arc::new(store))
        .authenticator(keystore)
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
pub async fn submit_transaction<AUTH>(
    client: &mut Client<AUTH>,
    account_id: AccountId,
    tx_request: miden_client::transaction::TransactionRequest,
) -> Result<String, ClientError>
where
    AUTH: miden_client::auth::TransactionAuthenticator + Sync + 'static,
{
    let account_bytes: [u8; 15] = account_id.into();
    let account_hex = format!("0x{}", hex::encode(account_bytes));

    info!("═══ submit_transaction() ═══");
    info!("  Submitter account: {}", account_hex);
    info!("  Submitter account (debug): {:?}", account_id);
    info!("  Input notes to consume: {}", tx_request.input_notes().len());
    for (i, input_note) in tx_request.input_notes().iter().enumerate() {
        info!("    Note [{}]: {:?}", i, input_note);
    }
    info!("═════════════════════════════");

    info!("Calling client.submit_new_transaction()...");
    let tx_id = client
        .submit_new_transaction(account_id, tx_request)
        .await
        .map_err(|e| {
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
