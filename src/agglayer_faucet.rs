//! Agglayer faucet creation and deployment module.
//!
//! This module provides functionality for creating and deploying an agglayer faucet
//! to the Miden network. The agglayer faucet is required for processing CLAIM notes
//! that mint tokens to destination accounts.

use miden_agglayer::{create_agglayer_faucet, create_bridge_account};
use miden_client::keystore::FilesystemKeyStore;
use miden_client::transaction::TransactionRequestBuilder;
use miden_client::Client;
use miden_protocol::account::AccountId;
use miden_protocol::{Felt, Word};
use sha3::{Digest, Keccak256};
use tracing::{debug, error, info};

use crate::ClientError;

/// Result of creating and deploying an agglayer faucet.
#[derive(Debug, Clone)]
pub struct AgglayerFaucetResult {
    /// The deployed agglayer faucet account ID.
    pub faucet_id: AccountId,
    /// The bridge account ID used for faucet validation.
    pub bridge_account_id: AccountId,
}

/// Creates and deploys an agglayer faucet to the Miden network.
///
/// This function performs the following steps:
/// 1. Creates a bridge account (local reference for faucet validation)
/// 2. Creates an agglayer faucet with deterministic seed
/// 3. Adds both accounts to the client
/// 4. Deploys the faucet to the network
/// 5. Syncs state to ensure the client tracks the deployed faucet
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
pub async fn create_and_deploy_agglayer_faucet(
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
    info!("  ✓ Agglayer faucet added to client");

    // Deploy the agglayer faucet to the network
    // Reference: https://github.com/0xMiden/miden-client/blob/e235c726/bin/miden-cli/src/commands/new_account.rs#L393-L428
    info!("  Deploying agglayer faucet to network...");
    let auth_procedure_mast_root = agglayer_faucet
        .code()
        .get(0)
        .expect("faucet code should contain at least one procedure")
        .mast_root();
    info!(
        "    - Auth procedure MAST root: {:?}",
        auth_procedure_mast_root
    );

    let auth_script = client
        .code_builder()
        .compile_tx_script(
            "begin
                mem_storew_be.4000 push.4000
                dyncall
            end",
        )
        .map_err(|e| {
            ClientError::InitializationError(format!("Failed to compile auth script: {}", e))
        })?;
    info!("    - Auth script compiled");

    let deploy_tx_request = TransactionRequestBuilder::new()
        .script_arg(*auth_procedure_mast_root)
        .custom_script(auth_script)
        .build()
        .map_err(|e| {
            ClientError::InitializationError(format!("Failed to build deploy transaction: {}", e))
        })?;
    info!("    - Deploy transaction request built");

    let faucet_deploy_result = client
        .submit_new_transaction(agglayer_faucet_id, deploy_tx_request)
        .await;

    // Log before match to ensure error details are captured
    info!(
        "    - Faucet deploy result: {:?}",
        faucet_deploy_result.as_ref().map(|_| "Ok")
    );
    if let Err(ref e) = faucet_deploy_result {
        error!("  ✗ Failed to deploy agglayer faucet");
        error!("    - Account ID: {}", agglayer_faucet_id);
        error!("    - Error (Display): {}", e);
        error!("    - Error (Debug): {:#?}", e);
        // Try to get source error chain
        let mut source = std::error::Error::source(e);
        let mut depth = 0;
        while let Some(s) = source {
            depth += 1;
            error!("    - Cause {}: {}", depth, s);
            error!("    - Cause {} (debug): {:?}", depth, s);
            source = std::error::Error::source(s);
        }
    }

    match faucet_deploy_result {
        Ok(result) => {
            info!("  ✓ Agglayer faucet deployed to network");
            debug!("    - Deploy tx result: {:?}", result);
        }
        Err(e) => {
            return Err(ClientError::InitializationError(format!(
                "Failed to deploy agglayer faucet: {}",
                e
            )));
        }
    }

    // Sync state to ensure client tracks the deployed faucet
    info!("  Syncing state after deploying agglayer faucet...");
    let sync_result = client
        .sync_state()
        .await
        .map_err(|e| ClientError::SyncError(e.to_string()))?;
    info!(
        "  ✓ Sync complete at block {} - agglayer faucet deployed and ready",
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
