//! Agglayer faucet creation and deployment module.
//!
//! This module provides functionality for creating and deploying an agglayer faucet
//! to the Miden network. The agglayer faucet is required for processing CLAIM notes
//! that mint tokens to destination accounts.

use miden_agglayer::{create_agglayer_faucet, create_bridge_account, EthAddressFormat};
use miden_client::keystore::FilesystemKeyStore;
use miden_client::transaction::TransactionRequestBuilder;
use miden_client::Client;
use miden_protocol::account::AccountId;
use miden_protocol::{Felt, Word};
use sha3::{Digest, Keccak256};
use tracing::{info, warn};

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
/// # Arguments
///
/// * `client` - Mutable reference to the Miden client with FilesystemKeyStore
/// * `configured_faucet_id_hex` - Hex string of the configured faucet ID (for seed derivation)
/// * `bridge_admin_id` - AccountId of the bridge admin (must match genesis)
/// * `ger_manager_id` - AccountId of the GER manager (must match genesis)
/// * `origin_token_address` - Ethereum token address for the faucet
/// * `origin_network` - Origin network ID (0 for mainnet)
/// * `scale` - Decimal scaling factor (e.g., 10 for 18→8 decimal conversion)
pub async fn create_and_deploy_agglayer_faucet(
    client: &mut Client<FilesystemKeyStore>,
    configured_faucet_id_hex: &str,
    bridge_admin_id: AccountId,
    ger_manager_id: AccountId,
    origin_token_address: &EthAddressFormat,
    origin_network: u32,
    scale: u8,
) -> Result<AgglayerFaucetResult, ClientError> {
    info!("╔══════════════════════════════════════════════════════════════════╗");
    info!("║  Creating bridge account                                         ║");
    info!("╚══════════════════════════════════════════════════════════════════╝");

    // Create bridge account first (required for agglayer faucet validation)
    info!("  Deriving deterministic seed from configured faucet ID...");
    let seed_input = format!("bridge:{}", configured_faucet_id_hex);
    let bridge_seed: Word = derive_seed_from_input(&seed_input);
    info!("  Bridge seed Word: {:?}", bridge_seed);

    info!("  Calling create_bridge_account()...");
    let bridge_account = create_bridge_account(bridge_seed, bridge_admin_id, ger_manager_id);
    let bridge_account_id = bridge_account.id();
    info!("  ✓ Bridge account created (NoAuth - local reference only)");
    info!("  → Bridge account ID: {}", bridge_account_id);

    // Add bridge account to client
    info!("  Adding bridge account to client...");
    let bridge_already_tracked = match client.add_account(&bridge_account, false).await {
        Ok(_) => {
            info!("  ✓ Bridge account added to client");
            false
        }
        Err(e) => {
            let err_str = e.to_string();
            if err_str.contains("already being tracked") {
                info!("  ✓ Bridge account already tracked (reusing from previous session)");
                true
            } else {
                return Err(ClientError::InitializationError(format!(
                    "Failed to add bridge account to client: {}",
                    e
                )));
            }
        }
    };

    // Deploy bridge account to the network (required for FPI)
    if !bridge_already_tracked {
        info!("  Deploying bridge account to network...");
        let deploy_request = TransactionRequestBuilder::new()
            .build()
            .map_err(|e| ClientError::InitializationError(format!(
                "Failed to build bridge deploy request: {}", e
            )))?;

        match client.submit_new_transaction(bridge_account_id, deploy_request).await {
            Ok(tx_id) => {
                info!("  ✓ Bridge account deployed to network");
                info!("    TX ID: {}", hex::encode(tx_id.as_bytes()));
            }
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("already exists") || err_str.contains("Account already") {
                    info!("  ✓ Bridge account already exists on network");
                } else {
                    warn!("  ⚠ Failed to deploy bridge account: {}", e);
                    warn!("    Will attempt to continue - account may already exist on network");
                }
            }
        }
    } else {
        info!("  ✓ Bridge account already tracked - assuming deployed");
    }

    info!("╔══════════════════════════════════════════════════════════════════╗");
    info!("║  Creating agglayer faucet                                        ║");
    info!("╚══════════════════════════════════════════════════════════════════╝");

    // Derive deterministic seed from configured faucet ID
    let faucet_seed_input = format!("agglayer_faucet:{}", configured_faucet_id_hex);
    let faucet_seed: Word = derive_seed_from_input(&faucet_seed_input);
    info!("  Faucet seed Word: {:?}", faucet_seed);

    // Create agglayer faucet using the library function (0.14 API)
    info!("  Calling create_agglayer_faucet() with:");
    info!("    - Symbol: LUMIA");
    info!("    - Decimals: 8");
    info!("    - Max supply: {} (u64::MAX)", u64::MAX);
    info!("    - Bridge account ID: {}", bridge_account_id);
    info!("    - Origin network: {}", origin_network);
    info!("    - Scale: {}", scale);
    let agglayer_faucet = create_agglayer_faucet(
        faucet_seed,
        "LUMIA",
        8,
        Felt::new(u64::MAX),
        bridge_account_id,
        origin_token_address,
        origin_network,
        scale,
    );

    let agglayer_faucet_id = agglayer_faucet.id();
    info!("  ✓ Agglayer faucet created");
    info!("  → Agglayer faucet ID: {}", agglayer_faucet_id);

    // Add agglayer faucet to client
    info!("  Adding agglayer faucet to client...");
    let faucet_already_tracked = match client.add_account(&agglayer_faucet, false).await {
        Ok(_) => {
            info!("  ✓ Agglayer faucet added to client");
            false
        }
        Err(e) => {
            let err_str = e.to_string();
            if err_str.contains("already being tracked") {
                info!("  ✓ Agglayer faucet already tracked (reusing from previous session)");
                true
            } else {
                return Err(ClientError::InitializationError(format!(
                    "Failed to add agglayer faucet to client: {}",
                    e
                )));
            }
        }
    };

    // Deploy agglayer faucet to the network
    if !faucet_already_tracked {
        info!("  Deploying agglayer faucet to network...");
        let deploy_request = TransactionRequestBuilder::new()
            .build()
            .map_err(|e| ClientError::InitializationError(format!(
                "Failed to build faucet deploy request: {}", e
            )))?;

        match client.submit_new_transaction(agglayer_faucet_id, deploy_request).await {
            Ok(tx_id) => {
                info!("  ✓ Agglayer faucet deployed to network");
                info!("    TX ID: {}", hex::encode(tx_id.as_bytes()));
            }
            Err(e) => {
                let err_str = e.to_string();
                if err_str.contains("already exists") || err_str.contains("Account already") {
                    info!("  ✓ Agglayer faucet already exists on network");
                } else {
                    warn!("  ⚠ Failed to deploy agglayer faucet: {}", e);
                    warn!("    Will attempt to continue - faucet may already exist on network");
                }
            }
        }
    } else {
        info!("  ✓ Agglayer faucet already tracked - assuming deployed");
    }

    info!("  ✓ Both accounts ready for CLAIM note processing");

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
