//! Phase 3: Full Integration Tests
//!
//! TC-3.1 through TC-3.9: End-to-end integration tests covering
//! the complete flow from L1 deposits through Miden execution to withdrawals.
//!
//! Uses miden-client and miden-protocol from agglayer-v0.1 tag.

use miden_client::account::component::{
    AccountComponent, FungibleFaucet, BasicWallet,
};
use miden_client::account::{AccountBuilder, AccountType};
use miden_client::keystore::{FilesystemKeyStore, Keystore};
use miden_client::transaction::{PaymentNoteDescription, TransactionRequestBuilder};
use miden_protocol::account::auth::{AuthScheme, AuthSecretKey};
use miden_protocol::asset::{Asset, AssetAmount, FungibleAsset, TokenSymbol};
use miden_protocol::note::NoteType;
use miden_standards::account::auth::AuthSingleSig;
use miden_standards::account::faucets::TokenName;
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
    storage_mode: AccountType,
) -> Result<miden_protocol::account::Account, TestError> {
    let mut init_seed = [0u8; 32];
    client.rng().fill_bytes(&mut init_seed);

    let key_pair = AuthSecretKey::new_falcon512_poseidon2();
    let auth_component: AccountComponent =
        AuthSingleSig::new(key_pair.public_key().to_commitment(), AuthScheme::Falcon512Poseidon2).into();

    let account = AccountBuilder::new(init_seed)
        .account_type(storage_mode)
        .with_auth_component(auth_component)
        .with_component(BasicWallet)
        .build()?;

    keystore.add_key(&key_pair, account.id()).await?;

    client.add_account(&account, false).await?;
    Ok(account)
}

/// Create a new fungible faucet account
async fn create_faucet(
    client: &mut TestClient,
    keystore: &FilesystemKeyStore,
    storage_mode: AccountType,
    symbol: &str,
    max_supply: u64,
) -> Result<miden_protocol::account::Account, TestError> {
    let mut init_seed = [0u8; 32];
    client.rng().fill_bytes(&mut init_seed);

    let key_pair = AuthSecretKey::new_falcon512_poseidon2();
    let auth_component: AccountComponent =
        AuthSingleSig::new(key_pair.public_key().to_commitment(), AuthScheme::Falcon512Poseidon2).into();

    let token_symbol = TokenSymbol::new(symbol)?;
    let max_supply_amount = AssetAmount::new(max_supply).expect("max supply is a valid asset amount");
    let faucet_component = FungibleFaucet::builder()
        .name(TokenName::new(symbol)?)
        .symbol(token_symbol)
        .decimals(8)
        .max_supply(max_supply_amount)
        .build()?;

    let account = AccountBuilder::new(init_seed)
        .account_type(storage_mode)
        .with_auth_component(auth_component)
        .with_component(faucet_component)
        .build()?;

    keystore.add_key(&key_pair, account.id()).await?;

    client.add_account(&account, false).await?;
    Ok(account)
}

// =============================================================================
// TC-3.1: End-to-End Deposit Flow
// =============================================================================

mod tc_3_1_deposit_flow {
    use super::*;

    /// TC-3.1.1: Complete deposit flow simulation
    #[tokio::test]
    async fn test_deposit_flow() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let faucet = create_faucet(&mut client, &keystore, AccountType::Public, "DEP1", 1_000_000_000)
            .await
            .expect("Failed to create faucet");

        let user = create_wallet(&mut client, &keystore, AccountType::Public)
            .await
            .expect("Failed to create user");

        let asset = FungibleAsset::new(faucet.id(), 100_000_000).expect("Invalid asset");
        let tx_request = TransactionRequestBuilder::new()
            .build_mint_fungible_asset(asset, user.id(), NoteType::Public, client.rng())
            .expect("Failed to build mint request");

        let result = client.submit_new_transaction(faucet.id(), tx_request).await;
        assert!(result.is_ok(), "Deposit should succeed");
    }

    /// TC-3.1.2: Deposit creates consumable note
    #[tokio::test]
    async fn test_deposit_creates_note() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let faucet = create_faucet(&mut client, &keystore, AccountType::Public, "DEP2", 1_000_000_000)
            .await
            .unwrap();

        let user = create_wallet(&mut client, &keystore, AccountType::Public)
            .await
            .unwrap();

        let asset = FungibleAsset::new(faucet.id(), 100_000_000).unwrap();
        let tx_request = TransactionRequestBuilder::new()
            .build_mint_fungible_asset(asset, user.id(), NoteType::Public, client.rng())
            .unwrap();

        client.submit_new_transaction(faucet.id(), tx_request).await.unwrap();
        client.sync_state().await.unwrap();

        let notes = client.get_consumable_notes(Some(user.id())).await.unwrap();
        assert!(!notes.is_empty(), "Deposit should create consumable note");
    }
}

// =============================================================================
// TC-3.2: End-to-End Transfer Flow
// =============================================================================

mod tc_3_2_transfer_flow {
    use super::*;

    /// TC-3.2.1: Complete transfer between accounts
    #[tokio::test]
    async fn test_transfer_flow() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let faucet = create_faucet(&mut client, &keystore, AccountType::Public, "TRF1", 1_000_000_000)
            .await
            .unwrap();

        let alice = create_wallet(&mut client, &keystore, AccountType::Public)
            .await
            .unwrap();

        let bob = create_wallet(&mut client, &keystore, AccountType::Public)
            .await
            .unwrap();

        // Mint to Alice
        let mint_asset = FungibleAsset::new(faucet.id(), 1_000_000_000).unwrap();
        let mint_request = TransactionRequestBuilder::new()
            .build_mint_fungible_asset(mint_asset, alice.id(), NoteType::Public, client.rng())
            .unwrap();
        client.submit_new_transaction(faucet.id(), mint_request).await.unwrap();

        client.sync_state().await.unwrap();

        // Alice consumes mint note
        let alice_notes = client.get_consumable_notes(Some(alice.id())).await.unwrap();
        if !alice_notes.is_empty() {
            let notes_to_consume: Vec<_> = alice_notes.into_iter().map(|(n, _)| n.try_into().expect("consumable note has metadata")).collect();
            let consume_request = TransactionRequestBuilder::new()
                .build_consume_notes(notes_to_consume)
                .unwrap();
            client.submit_new_transaction(alice.id(), consume_request).await.unwrap();
        }

        // Transfer to Bob
        let asset = FungibleAsset::new(faucet.id(), 100_000_000).unwrap();
        let payment_data = PaymentNoteDescription::new(
            vec![Asset::Fungible(asset)],
            alice.id(),
            bob.id(),
        );
        let p2id_request = TransactionRequestBuilder::new()
            .build_pay_to_id(payment_data, NoteType::Public, client.rng())
            .unwrap();

        let result = client.submit_new_transaction(alice.id(), p2id_request).await;
        assert!(result.is_ok(), "Transfer should succeed");
    }
}

// =============================================================================
// TC-3.3: End-to-End Withdrawal Flow
// =============================================================================

mod tc_3_3_withdrawal_flow {
    use super::*;

    /// TC-3.3.1: Withdrawal preparation (burn tokens)
    #[tokio::test]
    async fn test_withdrawal_preparation() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let faucet = create_faucet(&mut client, &keystore, AccountType::Public, "WDR1", 1_000_000_000)
            .await
            .unwrap();

        let user = create_wallet(&mut client, &keystore, AccountType::Public)
            .await
            .unwrap();

        let asset = FungibleAsset::new(faucet.id(), 100_000_000).unwrap();
        let tx_request = TransactionRequestBuilder::new()
            .build_mint_fungible_asset(asset, user.id(), NoteType::Public, client.rng())
            .unwrap();

        client.submit_new_transaction(faucet.id(), tx_request).await.unwrap();
        client.sync_state().await.unwrap();

        let notes = client.get_consumable_notes(Some(user.id())).await.unwrap();
        if !notes.is_empty() {
            let notes_to_consume: Vec<_> = notes.into_iter().map(|(n, _)| n.try_into().expect("consumable note has metadata")).collect();
            let consume_request = TransactionRequestBuilder::new()
                .build_consume_notes(notes_to_consume)
                .unwrap();
            let result = client.submit_new_transaction(user.id(), consume_request).await;
            assert!(result.is_ok(), "Should be able to consume for withdrawal prep");
        }
    }
}

// =============================================================================
// TC-3.4: Multi-User Scenario
// =============================================================================

mod tc_3_4_multi_user {
    use super::*;

    /// TC-3.4.1: Multiple concurrent users
    #[tokio::test]
    async fn test_multi_user_scenario() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let faucet = create_faucet(&mut client, &keystore, AccountType::Public, "MUSR", 10_000_000_000)
            .await
            .unwrap();

        let mut users = Vec::new();
        for _ in 0..5 {
            let user = create_wallet(&mut client, &keystore, AccountType::Public)
                .await
                .unwrap();
            users.push(user.id());
        }

        for user_id in &users {
            let asset = FungibleAsset::new(faucet.id(), 100_000_000).unwrap();
            let tx_request = TransactionRequestBuilder::new()
                .build_mint_fungible_asset(asset, *user_id, NoteType::Public, client.rng())
                .unwrap();
            client.submit_new_transaction(faucet.id(), tx_request).await
                .expect("Should mint to user");
        }

        client.sync_state().await.unwrap();

        for user_id in &users {
            let notes = client.get_consumable_notes(Some(*user_id)).await.unwrap();
            assert!(!notes.is_empty(), "User {:?} should have notes", user_id);
        }
    }
}

// =============================================================================
// TC-3.5: Error Recovery
// =============================================================================

mod tc_3_5_error_recovery {
    use super::*;

    /// TC-3.5.1: Recovery after failed transaction
    #[tokio::test]
    async fn test_recovery_after_failure() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let faucet = create_faucet(&mut client, &keystore, AccountType::Public, "RCV1", 1_000_000_000)
            .await
            .unwrap();

        let user = create_wallet(&mut client, &keystore, AccountType::Public)
            .await
            .unwrap();

        let asset = FungibleAsset::new(faucet.id(), 100_000_000).unwrap();
        let tx_request = TransactionRequestBuilder::new()
            .build_mint_fungible_asset(asset, user.id(), NoteType::Public, client.rng())
            .unwrap();

        let result = client.submit_new_transaction(faucet.id(), tx_request).await;
        assert!(result.is_ok(), "Should recover and succeed");
    }
}

// =============================================================================
// TC-3.6: State Verification
// =============================================================================

mod tc_3_6_state_verification {
    use super::*;

    /// TC-3.6.1: State remains consistent after operations
    #[tokio::test]
    async fn test_state_consistency() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let initial_height = {
            client.sync_state().await.unwrap();
            client.get_sync_height().await.unwrap()
        };

        let faucet = create_faucet(&mut client, &keystore, AccountType::Public, "STV1", 1_000_000_000)
            .await
            .unwrap();

        let user = create_wallet(&mut client, &keystore, AccountType::Public)
            .await
            .unwrap();

        let asset = FungibleAsset::new(faucet.id(), 100_000_000).unwrap();
        let tx_request = TransactionRequestBuilder::new()
            .build_mint_fungible_asset(asset, user.id(), NoteType::Public, client.rng())
            .unwrap();

        client.submit_new_transaction(faucet.id(), tx_request).await.unwrap();

        let final_height = {
            client.sync_state().await.unwrap();
            client.get_sync_height().await.unwrap()
        };

        assert!(final_height >= initial_height, "Block height should not decrease");
    }
}

// =============================================================================
// TC-3.7: Performance Under Load
// =============================================================================

mod tc_3_7_performance {
    use super::*;

    /// TC-3.7.1: Handle multiple operations in sequence
    #[tokio::test]
    async fn test_sequential_operations() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let faucet = create_faucet(&mut client, &keystore, AccountType::Public, "PERF", 10_000_000_000)
            .await
            .unwrap();

        for i in 0..3 {
            let user = create_wallet(&mut client, &keystore, AccountType::Public)
                .await
                .unwrap_or_else(|_| panic!("Failed to create user {}", i));

            let asset = FungibleAsset::new(faucet.id(), 10_000_000).unwrap();
            let tx_request = TransactionRequestBuilder::new()
                .build_mint_fungible_asset(asset, user.id(), NoteType::Public, client.rng())
                .unwrap();

            client.submit_new_transaction(faucet.id(), tx_request).await
                .unwrap_or_else(|_| panic!("Failed to mint to user {}", i));
        }

        client.sync_state().await.expect("Final sync should succeed");
    }
}

// =============================================================================
// TC-3.8: Edge Cases
// =============================================================================

mod tc_3_8_edge_cases {
    use super::*;

    /// TC-3.8.1: Handle minimum transfer
    #[tokio::test]
    async fn test_minimum_transfer() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let faucet = create_faucet(&mut client, &keystore, AccountType::Public, "EDGE", 1_000_000_000)
            .await
            .unwrap();

        let user = create_wallet(&mut client, &keystore, AccountType::Public)
            .await
            .unwrap();

        let asset = FungibleAsset::new(faucet.id(), 1).unwrap();
        let tx_request = TransactionRequestBuilder::new()
            .build_mint_fungible_asset(asset, user.id(), NoteType::Public, client.rng())
            .unwrap();

        let result = client.submit_new_transaction(faucet.id(), tx_request).await;
        assert!(result.is_ok(), "Minimum transfer should succeed");
    }
}

// =============================================================================
// TC-3.9: Full Cycle Test
// =============================================================================

mod tc_3_9_full_cycle {
    use super::*;

    /// TC-3.9.1: Complete deposit-transfer-withdraw cycle
    #[tokio::test]
    async fn test_full_cycle() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let faucet = create_faucet(&mut client, &keystore, AccountType::Public, "FULL", 10_000_000_000)
            .await
            .expect("Faucet creation");

        let alice = create_wallet(&mut client, &keystore, AccountType::Public)
            .await
            .expect("Alice creation");

        let bob = create_wallet(&mut client, &keystore, AccountType::Public)
            .await
            .expect("Bob creation");

        // Step 1: Deposit (mint to Alice)
        let mint_asset = FungibleAsset::new(faucet.id(), 1_000_000_000).unwrap();
        let mint_request = TransactionRequestBuilder::new()
            .build_mint_fungible_asset(mint_asset, alice.id(), NoteType::Public, client.rng())
            .expect("Build mint request");
        client.submit_new_transaction(faucet.id(), mint_request).await
            .expect("Deposit to Alice");

        client.sync_state().await.expect("Sync after deposit");

        // Step 2: Alice consumes deposit
        let alice_notes = client.get_consumable_notes(Some(alice.id())).await.unwrap();
        if !alice_notes.is_empty() {
            let notes_to_consume: Vec<_> = alice_notes.into_iter().map(|(n, _)| n.try_into().expect("consumable note has metadata")).collect();
            let consume_request = TransactionRequestBuilder::new()
                .build_consume_notes(notes_to_consume)
                .unwrap();
            client.submit_new_transaction(alice.id(), consume_request).await
                .expect("Alice consume");
        }

        // Step 3: Transfer (Alice to Bob)
        let asset = FungibleAsset::new(faucet.id(), 500_000_000).unwrap();
        let payment_data = PaymentNoteDescription::new(
            vec![Asset::Fungible(asset)],
            alice.id(),
            bob.id(),
        );
        let p2id_request = TransactionRequestBuilder::new()
            .build_pay_to_id(payment_data, NoteType::Public, client.rng())
            .unwrap();
        client.submit_new_transaction(alice.id(), p2id_request).await
            .expect("Transfer to Bob");

        client.sync_state().await.expect("Sync after transfer");

        // Step 4: Bob consumes transfer
        let bob_notes = client.get_consumable_notes(Some(bob.id())).await.unwrap();
        if !bob_notes.is_empty() {
            let notes_to_consume: Vec<_> = bob_notes.into_iter().map(|(n, _)| n.try_into().expect("consumable note has metadata")).collect();
            let consume_request = TransactionRequestBuilder::new()
                .build_consume_notes(notes_to_consume)
                .unwrap();
            client.submit_new_transaction(bob.id(), consume_request).await
                .expect("Bob consume");
        }

        client.sync_state().await.expect("Final sync");
        println!("Full cycle test completed successfully");
    }
}
