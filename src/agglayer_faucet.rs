//! Agglayer faucet creation module.
//!
//! This module provides functionality for creating an agglayer faucet
//! for the Miden network. The agglayer faucet is required for processing CLAIM notes
//! that mint tokens to destination accounts.
//!
//! Note: This module creates the faucet locally but does NOT deploy it to the network.
//! Deployment should be handled externally (e.g., via genesis configuration or separate tooling).

use miden_agglayer::{create_agglayer_faucet, create_bridge_account};
use miden_client::keystore::FilesystemKeyStore;
use miden_client::Client;
use miden_protocol::account::AccountId;
use miden_protocol::{Felt, Word};
use sha3::{Digest, Keccak256};
use tracing::{debug, info};

use crate::ClientError;

/// Result of creating an agglayer faucet (local only, not deployed).
#[derive(Debug, Clone)]
pub struct AgglayerFaucetResult {
    /// The agglayer faucet account ID.
    pub faucet_id: AccountId,
    /// The bridge account ID used for faucet validation.
    pub bridge_account_id: AccountId,
}

/// Creates an agglayer faucet and adds it to the client (local only, not deployed).
///
/// This function performs the following steps:
/// 1. Creates a bridge account (local reference for faucet validation)
/// 2. Creates an agglayer faucet with deterministic seed
/// 3. Adds both accounts to the client
/// 4. Syncs state to ensure the client tracks the faucet
///
/// Note: This does NOT deploy the faucet to the network. The faucet must be
/// deployed separately (e.g., via genesis configuration or external tooling).
///
/// # Arguments
///
/// * `client` - Mutable reference to the Miden client with FilesystemKeyStore
/// * `configured_faucet_id_hex` - Hex string of the configured faucet ID (used for seed derivation)
///
/// # Returns
///
/// Returns `AgglayerFaucetResult` containing the faucet and bridge account IDs,
/// or a `ClientError` if any step fails.
pub async fn create_agglayer_faucet_local(
    client: &mut Client<FilesystemKeyStore>,
    configured_faucet_id_hex: &str,
) -> Result<AgglayerFaucetResult, ClientError> {
    info!("Creating bridge account (local reference for faucet validation)");

    // Create bridge account first (required for agglayer faucet validation)
    // Derive deterministic seed from configured faucet ID for reproducibility
    debug!("Deriving deterministic seed from configured faucet ID...");
    let seed_input = format!("bridge:{}", configured_faucet_id_hex);
    let bridge_seed: Word = derive_seed_from_input(&seed_input);

    // Create bridge account (NoAuth - only used as a local reference, not deployed)
    // The bridge account provides the bridge_account_id needed for agglayer faucet creation.
    // It doesn't need to be deployed because:
    // 1. It's only referenced by the locally-created faucet
    // 2. The actual bridge on the network would be in the miden-node genesis
    let bridge_account = create_bridge_account(bridge_seed);
    let bridge_account_id = bridge_account.id();
    info!(
        bridge_account_id = %bridge_account_id,
        "Bridge account created (NoAuth - local reference only)"
    );

    // Add bridge account to client (local only, not deployed)
    client
        .add_account(&bridge_account, false)
        .await
        .map_err(|e| {
            ClientError::InitializationError(format!("Failed to add bridge account to client: {}", e))
        })?;
    debug!("Bridge account added to client (local only)");

    info!("Creating agglayer faucet (local only, not deployed)");

    // Derive deterministic seed from configured faucet ID
    let faucet_seed_input = format!("agglayer_faucet:{}", configured_faucet_id_hex);
    let faucet_seed: Word = derive_seed_from_input(&faucet_seed_input);

    // Create agglayer faucet using the library function
    debug!(
        symbol = "LUMIA",
        decimals = 8,
        max_supply = %u64::MAX,
        bridge_account_id = %bridge_account_id,
        "Creating agglayer faucet with parameters"
    );
    let agglayer_faucet = create_agglayer_faucet(
        faucet_seed,
        "LUMIA", // Token symbol (could be made configurable)
        8,       // Decimals matching ERC20 (18 decimals scaled to 8 for Miden)
        Felt::new(u64::MAX), // Max supply
        bridge_account_id,   // Bridge account for validation
    );

    let agglayer_faucet_id = agglayer_faucet.id();
    info!(
        agglayer_faucet_id = %agglayer_faucet_id,
        "Agglayer faucet created (local only)"
    );

    // Add agglayer faucet to client (local only, not deployed to network)
    client
        .add_account(&agglayer_faucet, false)
        .await
        .map_err(|e| {
            ClientError::InitializationError(format!("Failed to add agglayer faucet to client: {}", e))
        })?;
    debug!("Agglayer faucet added to client (local only, NOT deployed to network)");

    // Sync state to ensure client is up to date
    info!("Syncing state after creating agglayer faucet...");
    let sync_result = client
        .sync_state()
        .await
        .map_err(|e| ClientError::SyncError(e.to_string()))?;
    info!(
        block_num = sync_result.block_num.as_u32(),
        "Sync complete - agglayer faucet ready (local only)"
    );

    Ok(AgglayerFaucetResult {
        faucet_id: agglayer_faucet_id,
        bridge_account_id,
    })
}

/// Derives a deterministic Word seed from a string input using Keccak256.
fn derive_seed_from_input(input: &str) -> Word {
    let mut seed_bytes = [0u8; 32];
    let hash = Keccak256::digest(input.as_bytes());
    seed_bytes.copy_from_slice(&hash[..32]);
    Word::new([
        Felt::new(u64::from_le_bytes(seed_bytes[0..8].try_into().unwrap())),
        Felt::new(u64::from_le_bytes(seed_bytes[8..16].try_into().unwrap())),
        Felt::new(u64::from_le_bytes(seed_bytes[16..24].try_into().unwrap())),
        Felt::new(u64::from_le_bytes(seed_bytes[24..32].try_into().unwrap())),
    ])
}
