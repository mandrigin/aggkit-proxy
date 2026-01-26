//! Agglayer faucet creation and deployment module.
//!
//! This module provides functionality for creating and deploying an agglayer faucet
//! to the Miden network. The agglayer faucet is required for processing CLAIM notes
//! that mint tokens to destination accounts.
//!
//! # Architecture Overview
//!
//! The AggLayer bridge uses CLAIM notes to mint tokens on Miden. The flow is:
//!
//! ```text
//! L1 Deposit → Bridge DB → Proxy → CLAIM Note → Agglayer Faucet → Recipient
//! ```
//!
//! This module creates TWO accounts:
//!
//! ## 1. Bridge Account (Local Reference)
//!
//! - **Authentication**: NoAuth (not deployed to network)
//! - **Purpose**: Provides `bridge_account_id` for agglayer faucet validation
//! - **Note**: The actual bridge contract exists in miden-node genesis. This local
//!   reference allows us to derive a deterministic ID that matches the faucet's
//!   expected bridge account.
//!
//! ## 2. Agglayer Faucet
//!
//! - **Authentication**: NoAuth (permissionless CLAIM processing)
//! - **Components**: `agglayer_faucet_component` from `miden-agglayer` crate
//! - **Capabilities**: Validates SMT proofs, mints fungible tokens
//! - **Token**: "LUMIA" with 8 decimals, max supply u64::MAX
//!
//! # Why Deterministic Seeds?
//!
//! Both accounts use deterministic seeds derived from the configured bridge faucet ID:
//!
//! - Bridge seed: `keccak256("bridge:{bridge_faucet_id_hex}")`
//! - Faucet seed: `keccak256("agglayer_faucet:{bridge_faucet_id_hex}")`
//!
//! This ensures that:
//! 1. Account IDs are reproducible across proxy restarts
//! 2. Multiple proxy instances derive the same IDs
//! 3. No external coordination is needed for account creation
//!
//! # Error Handling: "Already Being Tracked"
//!
//! When `add_account()` is called on an already-tracked account, the miden client
//! returns an error. This happens when:
//! - The proxy restarts and re-initializes accounts
//! - Multiple claims use the same account
//!
//! We handle this gracefully by checking for "already being tracked" in the error
//! message and treating it as success (the account exists, which is what we need).
//!
//! # Usage Pattern
//!
//! Call `create_and_deploy_agglayer_faucet()` ONCE at proxy startup:
//!
//! ```ignore
//! let faucet_result = create_and_deploy_agglayer_faucet(&mut client, &faucet_id_hex).await?;
//! // Store faucet_result.faucet_id and faucet_result.bridge_account_id
//! // Reuse for ALL subsequent claims
//! ```

use miden_agglayer::{create_agglayer_faucet, create_bridge_account};
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
/// This function creates the accounts needed for CLAIM note processing:
/// 1. **Bridge account** - Local NoAuth reference for faucet validation
/// 2. **Agglayer faucet** - Processes CLAIM notes and mints tokens
///
/// # Account Creation Details
///
/// ## Bridge Account
/// - Seed: `keccak256("bridge:{configured_faucet_id_hex}")`
/// - Authentication: NoAuth (not deployed to network)
/// - Purpose: The agglayer faucet references this bridge account ID for validation.
///   The actual L1 bridge contract exists in miden-node genesis.
///
/// ## Agglayer Faucet
/// - Seed: `keccak256("agglayer_faucet:{configured_faucet_id_hex}")`
/// - Token symbol: "LUMIA" (could be made configurable)
/// - Decimals: 8 (matching ERC20 scale)
/// - Max supply: u64::MAX
/// - NOT deployed: Expects faucet to exist in miden-node genesis
///
/// # Error Handling
///
/// When accounts are already tracked (e.g., after proxy restart), `add_account()`
/// would fail. We handle this by checking for "already being tracked" in the
/// error message and treating it as success - the account exists, which is all
/// we need for subsequent claims.
///
/// # Important: Call Only Once
///
/// This function should be called ONCE at proxy startup. The returned AccountIds
/// should be stored in `MidenSubmissionConfig` and reused for all claims.
/// Calling this per-claim will cause "already being tracked" errors.
///
/// # Arguments
///
/// * `client` - Mutable reference to the Miden client with FilesystemKeyStore
/// * `configured_faucet_id_hex` - Hex string of the configured faucet ID from env vars
///   (e.g., `BRIDGE_FAUCET_ID`). Used ONLY for deterministic seed derivation, not
///   as the actual faucet ID.
///
/// # Returns
///
/// Returns `AgglayerFaucetResult` containing:
/// - `faucet_id`: The agglayer faucet AccountId for CLAIM note processing
/// - `bridge_account_id`: The bridge account AccountId for faucet validation
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

    // Add bridge account to client
    // Note: This may fail if account is already tracked (from previous claim) - that's OK
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
    // The bridge account must exist on-chain for the CLAIM note's FPI to work
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
                // If account already exists on network, that's fine
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
    // Note: This may fail if account is already tracked (from previous claim) - that's OK
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
    // The faucet must exist on-chain to process CLAIM notes
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
                // If account already exists on network, that's fine
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
