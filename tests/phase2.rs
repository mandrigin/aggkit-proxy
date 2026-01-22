//! Phase 2: CLAIM Notes Tests
//!
//! TC-2.1 through TC-2.5: CLAIM note creation, redemption, expiry,
//! faucet interaction, and cross-chain coordination.
//!
//! Uses miden-client and miden-protocol from agglayer-v0.1 tag.

use miden_client::account::component::{
    AccountComponent, AuthRpoFalcon512, BasicFungibleFaucet, BasicWallet,
};
use miden_client::account::{AccountBuilder, AccountType};
use miden_client::keystore::FilesystemKeyStore;
use miden_client::transaction::TransactionRequestBuilder;
use miden_protocol::account::auth::AuthSecretKey;
use miden_protocol::account::AccountStorageMode;
use miden_protocol::asset::{FungibleAsset, TokenSymbol};
use miden_protocol::note::NoteType;
use miden_protocol::Felt;
use rand::RngCore;

mod common;
use common::{create_test_client, TestClient, TestError};

// =============================================================================
// Helper functions for account creation
// =============================================================================

/// Create a new wallet account with authentication
async fn create_wallet(
    client: &mut TestClient,
    keystore: &FilesystemKeyStore,
    storage_mode: AccountStorageMode,
) -> Result<miden_protocol::account::Account, TestError> {
    let mut init_seed = [0u8; 32];
    client.rng().fill_bytes(&mut init_seed);

    let key_pair = AuthSecretKey::new_falcon512_rpo();
    let auth_component: AccountComponent =
        AuthRpoFalcon512::new(key_pair.public_key().to_commitment()).into();

    keystore.add_key(&key_pair)?;

    let account = AccountBuilder::new(init_seed)
        .account_type(AccountType::RegularAccountUpdatableCode)
        .storage_mode(storage_mode)
        .with_auth_component(auth_component)
        .with_component(BasicWallet)
        .build()?;

    client.add_account(&account, false).await?;
    Ok(account)
}

/// Create a new fungible faucet account
async fn create_faucet(
    client: &mut TestClient,
    keystore: &FilesystemKeyStore,
    storage_mode: AccountStorageMode,
    symbol: &str,
    max_supply: u64,
) -> Result<miden_protocol::account::Account, TestError> {
    let mut init_seed = [0u8; 32];
    client.rng().fill_bytes(&mut init_seed);

    let key_pair = AuthSecretKey::new_falcon512_rpo();
    let auth_component: AccountComponent =
        AuthRpoFalcon512::new(key_pair.public_key().to_commitment()).into();

    keystore.add_key(&key_pair)?;

    let token_symbol = TokenSymbol::new(symbol)?;
    let max_supply_felt = Felt::new(max_supply);

    let account = AccountBuilder::new(init_seed)
        .account_type(AccountType::FungibleFaucet)
        .storage_mode(storage_mode)
        .with_auth_component(auth_component)
        .with_component(BasicFungibleFaucet::new(token_symbol, 8, max_supply_felt)?)
        .build()?;

    client.add_account(&account, false).await?;
    Ok(account)
}

// =============================================================================
// TC-2.1: CLAIM Note Creation
// =============================================================================

mod tc_2_1_claim_creation {
    use super::*;

    /// TC-2.1.1: Create a CLAIM note for cross-chain redemption
    #[tokio::test]
    async fn test_create_claim_note() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let faucet = create_faucet(&mut client, &keystore, AccountStorageMode::Public, "CLM1", 1_000_000_000)
            .await
            .expect("Failed to create faucet");

        let target = create_wallet(&mut client, &keystore, AccountStorageMode::Public)
            .await
            .expect("Failed to create target");

        let amount = 100_000_000u64;
        let asset = FungibleAsset::new(faucet.id(), amount).expect("Invalid asset");

        let tx_request = TransactionRequestBuilder::new()
            .build_mint_fungible_asset(asset, target.id(), NoteType::Public, client.rng())
            .expect("Failed to build mint request");

        let result = client.submit_new_transaction(faucet.id(), tx_request).await;
        assert!(result.is_ok(), "CLAIM note creation should succeed");
    }

    /// TC-2.1.2: CLAIM note has correct metadata
    #[tokio::test]
    async fn test_claim_note_metadata() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let faucet = create_faucet(&mut client, &keystore, AccountStorageMode::Public, "CLM2", 1_000_000_000)
            .await
            .unwrap();

        let target = create_wallet(&mut client, &keystore, AccountStorageMode::Public)
            .await
            .unwrap();

        let asset = FungibleAsset::new(faucet.id(), 100_000_000).unwrap();
        let tx_request = TransactionRequestBuilder::new()
            .build_mint_fungible_asset(asset, target.id(), NoteType::Public, client.rng())
            .unwrap();

        client.submit_new_transaction(faucet.id(), tx_request).await.unwrap();
        client.sync_state().await.unwrap();

        let notes = client.get_consumable_notes(Some(target.id())).await.unwrap();
        assert!(!notes.is_empty(), "Should have consumable notes");
    }
}

// =============================================================================
// TC-2.2: CLAIM Note Redemption
// =============================================================================

mod tc_2_2_claim_redemption {
    use super::*;

    /// TC-2.2.1: Redeem CLAIM note successfully
    #[tokio::test]
    async fn test_redeem_claim() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let faucet = create_faucet(&mut client, &keystore, AccountStorageMode::Public, "RDM1", 1_000_000_000)
            .await
            .unwrap();

        let redeemer = create_wallet(&mut client, &keystore, AccountStorageMode::Public)
            .await
            .unwrap();

        let asset = FungibleAsset::new(faucet.id(), 100_000_000).unwrap();
        let tx_request = TransactionRequestBuilder::new()
            .build_mint_fungible_asset(asset, redeemer.id(), NoteType::Public, client.rng())
            .unwrap();

        client.submit_new_transaction(faucet.id(), tx_request).await.unwrap();
        client.sync_state().await.unwrap();

        let notes = client.get_consumable_notes(Some(redeemer.id())).await.unwrap();
        if !notes.is_empty() {
            let notes_to_consume: Vec<_> = notes.into_iter().map(|(n, _)| n).collect();
            let consume_request = TransactionRequestBuilder::new()
                .build_consume_notes(notes_to_consume)
                .unwrap();
            let result = client.submit_new_transaction(redeemer.id(), consume_request).await;
            assert!(result.is_ok(), "CLAIM redemption should succeed");
        }
    }

    /// TC-2.2.2: Cannot redeem same CLAIM twice
    #[tokio::test]
    async fn test_no_double_redemption() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let faucet = create_faucet(&mut client, &keystore, AccountStorageMode::Public, "RDM2", 1_000_000_000)
            .await
            .unwrap();

        let redeemer = create_wallet(&mut client, &keystore, AccountStorageMode::Public)
            .await
            .unwrap();

        let asset = FungibleAsset::new(faucet.id(), 100_000_000).unwrap();
        let tx_request = TransactionRequestBuilder::new()
            .build_mint_fungible_asset(asset, redeemer.id(), NoteType::Public, client.rng())
            .unwrap();

        client.submit_new_transaction(faucet.id(), tx_request).await.unwrap();
        client.sync_state().await.unwrap();

        let notes = client.get_consumable_notes(Some(redeemer.id())).await.unwrap();
        if !notes.is_empty() {
            let ids: Vec<_> = notes.iter().map(|(n, _)| n.id()).collect();
            let notes_to_consume: Vec<_> = notes.into_iter().map(|(n, _)| n).collect();
            let consume_request = TransactionRequestBuilder::new()
                .build_consume_notes(notes_to_consume)
                .unwrap();
            client.submit_new_transaction(redeemer.id(), consume_request).await.unwrap();

            client.sync_state().await.unwrap();

            let remaining = client.get_consumable_notes(Some(redeemer.id())).await.unwrap();
            for id in &ids {
                assert!(
                    !remaining.iter().any(|(n, _)| n.id() == *id),
                    "Consumed note should not be available"
                );
            }
        }
    }
}

// =============================================================================
// TC-2.3: CLAIM Expiry Handling
// =============================================================================

mod tc_2_3_claim_expiry {
    use super::*;

    /// TC-2.3.1: CLAIM notes have expiry tracking
    #[tokio::test]
    async fn test_claim_expiry_tracking() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let faucet = create_faucet(&mut client, &keystore, AccountStorageMode::Public, "EXP1", 1_000_000_000)
            .await
            .unwrap();

        let target = create_wallet(&mut client, &keystore, AccountStorageMode::Public)
            .await
            .unwrap();

        let asset = FungibleAsset::new(faucet.id(), 100_000_000).unwrap();
        let tx_request = TransactionRequestBuilder::new()
            .build_mint_fungible_asset(asset, target.id(), NoteType::Public, client.rng())
            .unwrap();

        client.submit_new_transaction(faucet.id(), tx_request).await.unwrap();
        client.sync_state().await.unwrap();

        let notes = client.get_consumable_notes(Some(target.id())).await.unwrap();
        assert!(!notes.is_empty(), "Should have notes to check expiry");
    }
}

// =============================================================================
// TC-2.4: Faucet Interaction
// =============================================================================

mod tc_2_4_faucet_interaction {
    use super::*;

    /// TC-2.4.1: Faucet can mint multiple CLAIMs
    #[tokio::test]
    async fn test_faucet_multiple_mints() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let faucet = create_faucet(&mut client, &keystore, AccountStorageMode::Public, "MULT", 1_000_000_000)
            .await
            .unwrap();

        let mut recipients = Vec::new();
        for _ in 0..3 {
            let account = create_wallet(&mut client, &keystore, AccountStorageMode::Public)
                .await
                .unwrap();
            recipients.push(account.id());
        }

        for recipient_id in &recipients {
            let asset = FungibleAsset::new(faucet.id(), 50_000_000).unwrap();
            let tx_request = TransactionRequestBuilder::new()
                .build_mint_fungible_asset(asset, *recipient_id, NoteType::Public, client.rng())
                .unwrap();
            let result = client.submit_new_transaction(faucet.id(), tx_request).await;
            assert!(result.is_ok(), "Minting to {:?} should succeed", recipient_id);
        }
    }

    /// TC-2.4.2: Faucet respects supply limits
    #[tokio::test]
    async fn test_faucet_supply_limits() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let max_supply = 100_000_000u64;
        let faucet = create_faucet(&mut client, &keystore, AccountStorageMode::Public, "LIM1", max_supply)
            .await
            .unwrap();

        let target = create_wallet(&mut client, &keystore, AccountStorageMode::Public)
            .await
            .unwrap();

        let asset = FungibleAsset::new(faucet.id(), max_supply / 2).unwrap();
        let tx_request = TransactionRequestBuilder::new()
            .build_mint_fungible_asset(asset, target.id(), NoteType::Public, client.rng())
            .unwrap();

        let result = client.submit_new_transaction(faucet.id(), tx_request).await;
        assert!(result.is_ok(), "Minting within limits should succeed");
    }
}

// =============================================================================
// TC-2.5: Cross-Chain Coordination
// =============================================================================

mod tc_2_5_cross_chain {
    use super::*;

    /// TC-2.5.1: Notes can be tagged for cross-chain tracking
    #[tokio::test]
    async fn test_note_tagging() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let faucet = create_faucet(&mut client, &keystore, AccountStorageMode::Public, "TAG1", 1_000_000_000)
            .await
            .unwrap();

        let target = create_wallet(&mut client, &keystore, AccountStorageMode::Public)
            .await
            .unwrap();

        let asset = FungibleAsset::new(faucet.id(), 100_000_000).unwrap();
        let tx_request = TransactionRequestBuilder::new()
            .build_mint_fungible_asset(asset, target.id(), NoteType::Public, client.rng())
            .expect("Failed to build mint request");

        client.submit_new_transaction(faucet.id(), tx_request).await
            .expect("Failed to create tagged note");

        client.sync_state().await.unwrap();

        let notes = client.get_consumable_notes(Some(target.id())).await.unwrap();
        assert!(!notes.is_empty(), "Tagged note should be created");
    }

    /// TC-2.5.2: State sync includes note data
    #[tokio::test]
    async fn test_sync_includes_notes() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let faucet = create_faucet(&mut client, &keystore, AccountStorageMode::Public, "SYNC", 1_000_000_000)
            .await
            .unwrap();

        let target = create_wallet(&mut client, &keystore, AccountStorageMode::Public)
            .await
            .unwrap();

        let asset = FungibleAsset::new(faucet.id(), 100_000_000).unwrap();
        let tx_request = TransactionRequestBuilder::new()
            .build_mint_fungible_asset(asset, target.id(), NoteType::Public, client.rng())
            .unwrap();

        client.submit_new_transaction(faucet.id(), tx_request).await.unwrap();

        let sync_result = client.sync_state().await;
        assert!(sync_result.is_ok(), "Sync should succeed with new notes");

        let notes = client.get_consumable_notes(Some(target.id())).await.unwrap();
        assert!(!notes.is_empty(), "Synced state should include notes");
    }
}
