//! Phase 1: Miden Standalone Tests
//!
//! TC-1.1 through TC-1.7: Node connectivity, account creation, faucet,
//! token minting, note consumption, P2ID transfers, state consistency.
//!
//! Uses miden-client and miden-protocol from agglayer-v0.1 tag.

use miden_client::account::component::{AccountComponent, BasicFungibleFaucet, BasicWallet};
use miden_protocol::account::auth::AuthScheme;
use miden_standards::account::auth::AuthSingleSig;
use miden_client::account::{AccountBuilder, AccountType};
use miden_client::keystore::{FilesystemKeyStore, Keystore};
use miden_client::transaction::{PaymentNoteDescription, TransactionRequestBuilder};
use miden_protocol::account::auth::AuthSecretKey;
use miden_protocol::account::AccountStorageMode;
use miden_protocol::asset::{Asset, FungibleAsset, TokenSymbol};
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
    // Generate random seed
    let mut init_seed = [0u8; 32];
    client.rng().fill_bytes(&mut init_seed);

    // Create key pair and auth component
    let key_pair = AuthSecretKey::new_falcon512_poseidon2();
    let auth_component: AccountComponent =
        AuthSingleSig::new(key_pair.public_key().to_commitment(), AuthScheme::Falcon512Poseidon2).into();

    // Build account
    let account = AccountBuilder::new(init_seed)
        .account_type(AccountType::RegularAccountUpdatableCode)
        .storage_mode(storage_mode)
        .with_auth_component(auth_component)
        .with_component(BasicWallet)
        .build()?;

    // Add account to client
    client.add_account(&account, false).await?;

    // Add key to keystore (associated with account)
    keystore.add_key(&key_pair, account.id()).await?;

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
    // Generate random seed
    let mut init_seed = [0u8; 32];
    client.rng().fill_bytes(&mut init_seed);

    // Create key pair and auth component
    let key_pair = AuthSecretKey::new_falcon512_poseidon2();
    let auth_component: AccountComponent =
        AuthSingleSig::new(key_pair.public_key().to_commitment(), AuthScheme::Falcon512Poseidon2).into();

    // Create token symbol
    let token_symbol = TokenSymbol::new(symbol)?;
    let max_supply_felt = Felt::new(max_supply);

    // Build faucet account
    let account = AccountBuilder::new(init_seed)
        .account_type(AccountType::FungibleFaucet)
        .storage_mode(storage_mode)
        .with_auth_component(auth_component)
        .with_component(BasicFungibleFaucet::new(token_symbol, 8, max_supply_felt)?)
        .build()?;

    // Add account to client
    client.add_account(&account, false).await?;

    // Add key to keystore (associated with account)
    keystore.add_key(&key_pair, account.id()).await?;

    Ok(account)
}

// =============================================================================
// TC-1.1: Node Connectivity
// =============================================================================

mod tc_1_1_connectivity {
    use super::*;

    /// TC-1.1.1: Node endpoint is reachable
    #[tokio::test]
    async fn test_node_reachable() {
        let (mut client, _keystore, _path) = create_test_client().await.expect("Failed to create client");
        let result = client.sync_state().await;
        assert!(result.is_ok(), "Node should be reachable");
    }

    /// TC-1.1.2: Can sync state from node
    #[tokio::test]
    async fn test_sync_state() {
        let (mut client, _keystore, _path) = create_test_client().await.expect("Failed to create client");
        let result = client.sync_state().await;
        assert!(result.is_ok(), "Failed to sync state: {:?}", result.err());
    }

    /// TC-1.1.3: Can get block height
    #[tokio::test]
    async fn test_get_block_height() {
        let (mut client, _keystore, _path) = create_test_client().await.expect("Failed to create client");
        client.sync_state().await.expect("Failed to sync");
        let height = client.get_sync_height().await.expect("Failed to get sync height");
        // Block height is valid if we got here without error
        let _ = height.as_u32();
    }
}

// =============================================================================
// TC-1.2: Account Creation
// =============================================================================

mod tc_1_2_accounts {
    use super::*;

    /// TC-1.2.1: Create Alice's wallet account
    #[tokio::test]
    async fn test_create_alice_account() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let alice = create_wallet(&mut client, &keystore, AccountStorageMode::Public)
            .await
            .expect("Failed to create Alice account");

        // Verify account was created (ID should be valid)
        println!("Created Alice account: {:?}", alice.id());
    }

    /// TC-1.2.2: Create Bob's wallet account
    #[tokio::test]
    async fn test_create_bob_account() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let bob = create_wallet(&mut client, &keystore, AccountStorageMode::Public)
            .await
            .expect("Failed to create Bob account");

        println!("Created Bob account: {:?}", bob.id());
    }

    /// TC-1.2.3: Accounts are distinct
    #[tokio::test]
    async fn test_accounts_are_distinct() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let alice = create_wallet(&mut client, &keystore, AccountStorageMode::Public)
            .await
            .expect("Failed to create Alice");

        let bob = create_wallet(&mut client, &keystore, AccountStorageMode::Public)
            .await
            .expect("Failed to create Bob");

        assert_ne!(alice.id(), bob.id(), "Alice and Bob should have different IDs");
    }
}

// =============================================================================
// TC-1.3: Faucet Deployment
// =============================================================================

mod tc_1_3_faucet {
    use super::*;

    /// TC-1.3.1: Create a fungible faucet account
    #[tokio::test]
    async fn test_create_faucet() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let faucet = create_faucet(
            &mut client,
            &keystore,
            AccountStorageMode::Public,
            "TEST",
            1_000_000_000_000_000,
        )
        .await
        .expect("Failed to create faucet");

        println!("Created faucet: {:?}", faucet.id());
    }

    /// TC-1.3.2: Faucet is queryable
    #[tokio::test]
    async fn test_faucet_queryable() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let faucet = create_faucet(
            &mut client,
            &keystore,
            AccountStorageMode::Public,
            "QRY",
            1_000_000,
        )
        .await
        .expect("Failed to create faucet");

        client.sync_state().await.expect("Failed to sync");

        let accounts = client.get_account_headers().await.expect("Failed to get accounts");
        assert!(
            accounts.iter().any(|(h, _)| h.id() == faucet.id()),
            "Faucet should be in account list"
        );
    }
}

// =============================================================================
// TC-1.4: Token Minting
// =============================================================================

mod tc_1_4_minting {
    use super::*;

    /// TC-1.4.1: Mint tokens to an account
    #[tokio::test]
    async fn test_mint_tokens() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let faucet = create_faucet(
            &mut client,
            &keystore,
            AccountStorageMode::Public,
            "MINT",
            1_000_000_000,
        )
        .await
        .expect("Failed to create faucet");

        let recipient = create_wallet(&mut client, &keystore, AccountStorageMode::Public)
            .await
            .expect("Failed to create recipient");

        let amount = 100_000_000u64;
        let asset = FungibleAsset::new(faucet.id(), amount).expect("Invalid asset");

        let tx_request = TransactionRequestBuilder::new()
            .build_mint_fungible_asset(asset, recipient.id(), NoteType::Public, client.rng())
            .expect("Failed to build mint request");

        let result = client.submit_new_transaction(faucet.id(), tx_request).await;
        assert!(result.is_ok(), "Minting should succeed: {:?}", result.err());
    }
}

// =============================================================================
// TC-1.5: Note Consumption
// =============================================================================

mod tc_1_5_consumption {
    use super::*;

    /// TC-1.5.1: Consume minted tokens
    #[tokio::test]
    async fn test_consume_note() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let faucet = create_faucet(
            &mut client,
            &keystore,
            AccountStorageMode::Public,
            "CONS",
            1_000_000_000,
        )
        .await
        .expect("Failed to create faucet");

        let recipient = create_wallet(&mut client, &keystore, AccountStorageMode::Public)
            .await
            .expect("Failed to create recipient");

        let amount = 100_000_000u64;
        let asset = FungibleAsset::new(faucet.id(), amount).expect("Invalid asset");

        let tx_request = TransactionRequestBuilder::new()
            .build_mint_fungible_asset(asset, recipient.id(), NoteType::Public, client.rng())
            .expect("Failed to build mint request");

        client
            .submit_new_transaction(faucet.id(), tx_request)
            .await
            .expect("Failed to mint");

        client.sync_state().await.expect("Failed to sync");

        let notes = client
            .get_consumable_notes(Some(recipient.id()))
            .await
            .expect("Failed to get notes");

        if !notes.is_empty() {
            let notes_to_consume: Vec<miden_protocol::note::Note> = notes.into_iter().map(|(n, _)| n.try_into().unwrap()).collect();
            let tx_request = TransactionRequestBuilder::new()
                .build_consume_notes(notes_to_consume)
                .expect("Failed to build consume request");
            let result = client.submit_new_transaction(recipient.id(), tx_request).await;
            assert!(result.is_ok(), "Note consumption should succeed");
        }
    }
}

// =============================================================================
// TC-1.6: P2ID Transfer
// =============================================================================

mod tc_1_6_p2id {
    use super::*;

    /// TC-1.6.1: P2ID transfer between accounts
    #[tokio::test]
    async fn test_p2id_transfer() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let faucet = create_faucet(
            &mut client,
            &keystore,
            AccountStorageMode::Public,
            "P2ID",
            1_000_000_000,
        )
        .await
        .expect("Failed to create faucet");

        let alice = create_wallet(&mut client, &keystore, AccountStorageMode::Public)
            .await
            .expect("Failed to create Alice");

        let bob = create_wallet(&mut client, &keystore, AccountStorageMode::Public)
            .await
            .expect("Failed to create Bob");

        // Mint to Alice
        let mint_amount = 1_000_000_000u64;
        let mint_asset = FungibleAsset::new(faucet.id(), mint_amount).expect("Invalid asset");
        let mint_request = TransactionRequestBuilder::new()
            .build_mint_fungible_asset(mint_asset, alice.id(), NoteType::Public, client.rng())
            .expect("Failed to build mint request");
        client
            .submit_new_transaction(faucet.id(), mint_request)
            .await
            .expect("Failed to mint to Alice");

        client.sync_state().await.expect("Failed to sync");

        // Alice consumes mint note
        let alice_notes = client
            .get_consumable_notes(Some(alice.id()))
            .await
            .expect("Failed to get Alice's notes");

        if !alice_notes.is_empty() {
            let notes_to_consume: Vec<miden_protocol::note::Note> = alice_notes.into_iter().map(|(n, _)| n.try_into().unwrap()).collect();
            let consume_request = TransactionRequestBuilder::new()
                .build_consume_notes(notes_to_consume)
                .expect("Failed to build consume request");
            client
                .submit_new_transaction(alice.id(), consume_request)
                .await
                .expect("Failed to consume Alice's notes");
        }

        // Alice sends P2ID to Bob
        let transfer_amount = 100_000_000u64;
        let asset = FungibleAsset::new(faucet.id(), transfer_amount).expect("Invalid asset");
        let payment_data = PaymentNoteDescription::new(
            vec![Asset::Fungible(asset)],
            alice.id(),
            bob.id(),
        );

        let p2id_request = TransactionRequestBuilder::new()
            .build_pay_to_id(payment_data, NoteType::Public, client.rng())
            .expect("Failed to build P2ID request");

        let result = client.submit_new_transaction(alice.id(), p2id_request).await;
        assert!(result.is_ok(), "P2ID transfer should succeed: {:?}", result.err());
    }

    /// TC-1.6.2: Bob can consume P2ID note
    #[tokio::test]
    async fn test_bob_consumes_p2id() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let faucet = create_faucet(
            &mut client,
            &keystore,
            AccountStorageMode::Public,
            "P2I2",
            1_000_000_000,
        )
        .await
        .unwrap();

        let alice = create_wallet(&mut client, &keystore, AccountStorageMode::Public)
            .await
            .unwrap();

        let bob = create_wallet(&mut client, &keystore, AccountStorageMode::Public)
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
            let notes_to_consume: Vec<miden_protocol::note::Note> = alice_notes.into_iter().map(|(n, _)| n.try_into().unwrap()).collect();
            let consume_request = TransactionRequestBuilder::new()
                .build_consume_notes(notes_to_consume)
                .unwrap();
            client.submit_new_transaction(alice.id(), consume_request).await.unwrap();
        }

        // Alice sends P2ID to Bob
        let asset = FungibleAsset::new(faucet.id(), 100_000_000).unwrap();
        let payment_data = PaymentNoteDescription::new(
            vec![Asset::Fungible(asset)],
            alice.id(),
            bob.id(),
        );
        let p2id_request = TransactionRequestBuilder::new()
            .build_pay_to_id(payment_data, NoteType::Public, client.rng())
            .unwrap();
        client.submit_new_transaction(alice.id(), p2id_request).await.unwrap();

        client.sync_state().await.unwrap();

        // Bob consumes P2ID note
        let bob_notes = client.get_consumable_notes(Some(bob.id())).await.unwrap();
        if !bob_notes.is_empty() {
            let notes_to_consume: Vec<miden_protocol::note::Note> = bob_notes.into_iter().map(|(n, _)| n.try_into().unwrap()).collect();
            let consume_request = TransactionRequestBuilder::new()
                .build_consume_notes(notes_to_consume)
                .unwrap();
            let result = client.submit_new_transaction(bob.id(), consume_request).await;
            assert!(result.is_ok(), "Bob should consume P2ID note");
        }
    }
}

// =============================================================================
// TC-1.7: State Consistency
// =============================================================================

mod tc_1_7_state {
    use super::*;

    /// TC-1.7.1: State is consistent after sync
    #[tokio::test]
    async fn test_state_consistency() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let _account = create_wallet(&mut client, &keystore, AccountStorageMode::Public)
            .await
            .expect("Failed to create account");

        let height1 = {
            client.sync_state().await.expect("Failed to sync 1");
            client.get_sync_height().await.expect("Failed to get height 1")
        };

        let height2 = {
            client.sync_state().await.expect("Failed to sync 2");
            client.get_sync_height().await.expect("Failed to get height 2")
        };

        assert!(height2 >= height1, "Block height should not decrease");
    }

    /// TC-1.7.2: Account state persists
    #[tokio::test]
    async fn test_account_persistence() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let account = create_wallet(&mut client, &keystore, AccountStorageMode::Public)
            .await
            .expect("Failed to create account");

        let account_id = account.id();

        client.sync_state().await.expect("Failed to sync");

        let accounts = client.get_account_headers().await.expect("Failed to get accounts");
        assert!(
            accounts.iter().any(|(h, _)| h.id() == account_id),
            "Account should persist after sync"
        );
    }

    /// TC-1.7.3: No data loss after operations
    #[tokio::test]
    async fn test_no_data_loss() {
        let (mut client, keystore, _path) = create_test_client().await.expect("Failed to create client");

        let mut account_ids = Vec::new();
        for _ in 0..3 {
            let account = create_wallet(&mut client, &keystore, AccountStorageMode::Public)
                .await
                .expect("Failed to create account");
            account_ids.push(account.id());
        }

        client.sync_state().await.expect("Failed to sync");

        let accounts = client.get_account_headers().await.expect("Failed to get accounts");
        for id in &account_ids {
            assert!(
                accounts.iter().any(|(h, _)| h.id() == *id),
                "Account {:?} should exist after sync",
                id
            );
        }
    }
}
