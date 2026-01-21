//! Agglayer faucet creation module.
//!
//! This module provides functionality for creating an agglayer faucet locally.
//! The faucet is added to the client for reference but NOT deployed to the network.
//! The agglayer faucet is required for processing CLAIM notes that mint tokens
//! to destination accounts.

use miden_agglayer::{create_agglayer_faucet, create_bridge_account};
use miden_client::keystore::FilesystemKeyStore;
use miden_client::Client;
use miden_protocol::account::AccountId;
use miden_protocol::{Felt, Word};
use sha3::{Digest, Keccak256};
use tracing::info;

use crate::ClientError;

/// Result of creating an agglayer faucet locally.
#[derive(Debug, Clone)]
pub struct AgglayerFaucetResult {
    /// The agglayer faucet account ID (local, not deployed).
    pub faucet_id: AccountId,
    /// The bridge account ID used for faucet validation.
    pub bridge_account_id: AccountId,
}

/// Creates an agglayer faucet locally (does NOT deploy to network).
///
/// This function performs the following steps:
/// 1. Creates a bridge account (local reference for faucet validation)
/// 2. Creates an agglayer faucet with deterministic seed
/// 3. Adds both accounts to the client (local only)
/// 4. Syncs state to ensure the client is up to date
///
/// NOTE: The faucet is NOT deployed to the network. The proxy should only
/// create local references for claim processing, not deploy accounts.
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
    info!("╔══════════════════════════════════════════════════════════════════╗");
    info!("║  Creating bridge account                                         ║");
    info!("╚══════════════════════════════════════════════════════════════════╝");

    // Create bridge account first (required for agglayer faucet validation)
    // Derive deterministic seed from configured faucet ID for reproducibility
    info!("  Deriving deterministic seed from configured faucet ID...");
    let seed_input = format!("bridge:{}", configured_faucet_id_hex);
    info!("  Seed input: \"{}\"", seed_input);
    let bridge_seed: Word = derive_seed_from_input(&seed_input);
    info!("  Bridge seed Word: {:?}", bridge_seed);

    // Create bridge account (NoAuth - only used as a local reference, not deployed)
    // The bridge account provides the bridge_account_id needed for agglayer faucet creation.
    // It doesn't need to be deployed because:
    // 1. It's only referenced by the locally-created faucet
    // 2. The actual bridge on the network would be in the miden-node genesis
    info!("  Calling create_bridge_account()...");
    let bridge_account = create_bridge_account(bridge_seed);
    let bridge_account_id = bridge_account.id();
    info!("  ✓ Bridge account created (NoAuth - local reference only)");
    info!("  → Bridge account ID: {}", bridge_account_id);

    // Add bridge account to client (local only, not deployed)
    info!("  Adding bridge account to client...");
    client
        .add_account(&bridge_account, false)
        .await
        .map_err(|e| {
            ClientError::InitializationError(format!("Failed to add bridge account to client: {}", e))
        })?;
    info!("  ✓ Bridge account added to client (local only)");

    info!("╔══════════════════════════════════════════════════════════════════╗");
    info!("║  Creating agglayer faucet                                        ║");
    info!("╚══════════════════════════════════════════════════════════════════╝");

    // Derive deterministic seed from configured faucet ID
    info!("  Deriving deterministic seed for agglayer faucet...");
    let faucet_seed_input = format!("agglayer_faucet:{}", configured_faucet_id_hex);
    info!("  Seed input: \"{}\"", faucet_seed_input);
    let faucet_seed: Word = derive_seed_from_input(&faucet_seed_input);
    info!("  Faucet seed Word: {:?}", faucet_seed);

    // Create agglayer faucet using the library function
    info!("  Calling create_agglayer_faucet() with:");
    info!("    - Symbol: LUMIA");
    info!("    - Decimals: 8");
    info!("    - Max supply: {} (u64::MAX)", u64::MAX);
    info!("    - Bridge account ID: {}", bridge_account_id);
    let agglayer_faucet = create_agglayer_faucet(
        faucet_seed,
        "LUMIA", // Token symbol (could be made configurable)
        8,       // Decimals matching ERC20 (18 decimals scaled to 8 for Miden)
        Felt::new(u64::MAX), // Max supply
        bridge_account_id,   // Bridge account for validation
    );

    let agglayer_faucet_id = agglayer_faucet.id();
    info!("  ✓ Agglayer faucet created");
    info!("  → Agglayer faucet ID: {}", agglayer_faucet_id);

    // Add agglayer faucet to client
    info!("  Adding agglayer faucet to client...");
    client
        .add_account(&agglayer_faucet, false)
        .await
        .map_err(|e| {
            ClientError::InitializationError(format!("Failed to add agglayer faucet to client: {}", e))
        })?;
    info!("  ✓ Agglayer faucet added to client (local only, not deployed)");

    // Sync state to ensure client is up to date
    info!("  Syncing state...");
    let sync_result = client
        .sync_state()
        .await
        .map_err(|e| ClientError::SyncError(e.to_string()))?;
    info!(
        "  ✓ Sync complete at block {} - agglayer faucet created locally",
        sync_result.block_num.as_u32()
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
