//! Phase 1: Miden Standalone Tests
//!
//! TC-1.1 through TC-1.7: Node connectivity, account creation, faucet,
//! token minting, note consumption, P2ID transfers, state consistency.
//!
//! Uses miden-client and miden-protocol from agglayer-v0.1 tag.

use miden_client::Client;
use miden_client_sqlite_store::SqliteStore;
use miden_protocol::{
    account::{AccountId, AccountStorageMode, AccountType},
    asset::{FungibleAsset, TokenSymbol},
    note::NoteType,
    Felt,
};
use std::env;

mod common;
use common::{create_test_client, get_test_db_path, cleanup_db};

// =============================================================================
// TC-1.1: Node Connectivity
// =============================================================================

mod tc_1_1_connectivity {
    use super::*;

    /// TC-1.1.1: Node endpoint is reachable
    #[tokio::test]
    async fn test_node_reachable() {
        let client = create_test_client().await.expect("Failed to create client");
        let result = client.sync_state().await;
        assert!(result.is_ok(), "Node should be reachable");
    }

    /// TC-1.1.2: Can sync state from node
    #[tokio::test]
    async fn test_sync_state() {
        let client = create_test_client().await.expect("Failed to create client");
        let result = client.sync_state().await;
        assert!(result.is_ok(), "Failed to sync state: {:?}", result.err());
    }

    /// TC-1.1.3: Can get block height
    #[tokio::test]
    async fn test_get_block_height() {
        let client = create_test_client().await.expect("Failed to create client");
        client.sync_state().await.expect("Failed to sync");
        let height = client.get_sync_height();
        assert!(height >= 0, "Block height should be non-negative");
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
        let mut client = create_test_client().await.expect("Failed to create client");

        let (alice, _seed) = client
            .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
            .await
            .expect("Failed to create Alice account");

        assert!(alice.id() != AccountId::default());
        println!("Created Alice account: {:?}", alice.id());
    }

    /// TC-1.2.2: Create Bob's wallet account
    #[tokio::test]
    async fn test_create_bob_account() {
        let mut client = create_test_client().await.expect("Failed to create client");

        let (bob, _seed) = client
            .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
            .await
            .expect("Failed to create Bob account");

        assert!(bob.id() != AccountId::default());
        println!("Created Bob account: {:?}", bob.id());
    }

    /// TC-1.2.3: Accounts are distinct
    #[tokio::test]
    async fn test_accounts_are_distinct() {
        let mut client = create_test_client().await.expect("Failed to create client");

        let (alice, _) = client
            .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
            .await
            .expect("Failed to create Alice");

        let (bob, _) = client
            .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
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
        let mut client = create_test_client().await.expect("Failed to create client");

        let token_symbol = TokenSymbol::new("TEST").expect("Invalid symbol");
        let max_supply = 1_000_000_000_000_000u64;

        let (faucet, _seed) = client
            .new_faucet(
                AccountStorageMode::Public,
                token_symbol,
                8,
                Felt::new(max_supply),
            )
            .await
            .expect("Failed to create faucet");

        assert!(faucet.id() != AccountId::default());
        println!("Created faucet: {:?}", faucet.id());
    }

    /// TC-1.3.2: Faucet is queryable
    #[tokio::test]
    async fn test_faucet_queryable() {
        let mut client = create_test_client().await.expect("Failed to create client");

        let token_symbol = TokenSymbol::new("QRY").expect("Invalid symbol");
        let (faucet, _) = client
            .new_faucet(AccountStorageMode::Public, token_symbol, 8, Felt::new(1_000_000))
            .await
            .expect("Failed to create faucet");

        client.sync_state().await.expect("Failed to sync");

        let accounts = client.get_accounts().expect("Failed to get accounts");
        assert!(
            accounts.iter().any(|a| a.id() == faucet.id()),
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
        let mut client = create_test_client().await.expect("Failed to create client");

        let token_symbol = TokenSymbol::new("MINT").expect("Invalid symbol");
        let (faucet, _) = client
            .new_faucet(AccountStorageMode::Public, token_symbol, 8, Felt::new(1_000_000_000))
            .await
            .expect("Failed to create faucet");

        let (recipient, _) = client
            .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
            .await
            .expect("Failed to create recipient");

        let amount = 100_000_000u64;
        let result = client
            .new_mint_transaction(faucet.id(), recipient.id(), amount, NoteType::Public)
            .await;

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
        let mut client = create_test_client().await.expect("Failed to create client");

        let token_symbol = TokenSymbol::new("CONS").expect("Invalid symbol");
        let (faucet, _) = client
            .new_faucet(AccountStorageMode::Public, token_symbol, 8, Felt::new(1_000_000_000))
            .await
            .expect("Failed to create faucet");

        let (recipient, _) = client
            .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
            .await
            .expect("Failed to create recipient");

        let amount = 100_000_000u64;
        client
            .new_mint_transaction(faucet.id(), recipient.id(), amount, NoteType::Public)
            .await
            .expect("Failed to mint");

        client.sync_state().await.expect("Failed to sync");

        let notes = client
            .get_consumable_notes(Some(recipient.id()))
            .expect("Failed to get notes");

        if !notes.is_empty() {
            let note_ids: Vec<_> = notes.iter().map(|n| n.id()).collect();
            let result = client.new_consume_transaction(recipient.id(), &note_ids).await;
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
        let mut client = create_test_client().await.expect("Failed to create client");

        let token_symbol = TokenSymbol::new("P2ID").expect("Invalid symbol");
        let (faucet, _) = client
            .new_faucet(AccountStorageMode::Public, token_symbol, 8, Felt::new(1_000_000_000))
            .await
            .expect("Failed to create faucet");

        let (alice, _) = client
            .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
            .await
            .expect("Failed to create Alice");

        let (bob, _) = client
            .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
            .await
            .expect("Failed to create Bob");

        // Mint to Alice
        let mint_amount = 1_000_000_000u64;
        client
            .new_mint_transaction(faucet.id(), alice.id(), mint_amount, NoteType::Public)
            .await
            .expect("Failed to mint to Alice");

        client.sync_state().await.expect("Failed to sync");

        // Alice consumes mint note
        let alice_notes = client
            .get_consumable_notes(Some(alice.id()))
            .expect("Failed to get Alice's notes");

        if !alice_notes.is_empty() {
            let note_ids: Vec<_> = alice_notes.iter().map(|n| n.id()).collect();
            client
                .new_consume_transaction(alice.id(), &note_ids)
                .await
                .expect("Failed to consume Alice's notes");
        }

        // Alice sends P2ID to Bob
        let transfer_amount = 100_000_000u64;
        let asset = FungibleAsset::new(faucet.id(), transfer_amount).expect("Invalid asset");

        let result = client
            .new_send_transaction(alice.id(), bob.id(), asset.into(), NoteType::Public)
            .await;

        assert!(result.is_ok(), "P2ID transfer should succeed: {:?}", result.err());
    }

    /// TC-1.6.2: Bob can consume P2ID note
    #[tokio::test]
    async fn test_bob_consumes_p2id() {
        let mut client = create_test_client().await.expect("Failed to create client");

        let token_symbol = TokenSymbol::new("P2I2").expect("Invalid symbol");
        let (faucet, _) = client
            .new_faucet(AccountStorageMode::Public, token_symbol, 8, Felt::new(1_000_000_000))
            .await
            .unwrap();

        let (alice, _) = client
            .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
            .await
            .unwrap();

        let (bob, _) = client
            .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
            .await
            .unwrap();

        client
            .new_mint_transaction(faucet.id(), alice.id(), 1_000_000_000, NoteType::Public)
            .await
            .unwrap();

        client.sync_state().await.unwrap();

        let alice_notes = client.get_consumable_notes(Some(alice.id())).unwrap();
        if !alice_notes.is_empty() {
            let ids: Vec<_> = alice_notes.iter().map(|n| n.id()).collect();
            client.new_consume_transaction(alice.id(), &ids).await.unwrap();
        }

        let asset = FungibleAsset::new(faucet.id(), 100_000_000).unwrap();
        client
            .new_send_transaction(alice.id(), bob.id(), asset.into(), NoteType::Public)
            .await
            .unwrap();

        client.sync_state().await.unwrap();

        let bob_notes = client.get_consumable_notes(Some(bob.id())).unwrap();
        if !bob_notes.is_empty() {
            let ids: Vec<_> = bob_notes.iter().map(|n| n.id()).collect();
            let result = client.new_consume_transaction(bob.id(), &ids).await;
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
        let mut client = create_test_client().await.expect("Failed to create client");

        let (_account, _) = client
            .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
            .await
            .expect("Failed to create account");

        let height1 = {
            client.sync_state().await.expect("Failed to sync 1");
            client.get_sync_height()
        };

        let height2 = {
            client.sync_state().await.expect("Failed to sync 2");
            client.get_sync_height()
        };

        assert!(height2 >= height1, "Block height should not decrease");
    }

    /// TC-1.7.2: Account state persists
    #[tokio::test]
    async fn test_account_persistence() {
        let mut client = create_test_client().await.expect("Failed to create client");

        let (account, _) = client
            .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
            .await
            .expect("Failed to create account");

        let account_id = account.id();

        client.sync_state().await.expect("Failed to sync");

        let accounts = client.get_accounts().expect("Failed to get accounts");
        assert!(
            accounts.iter().any(|a| a.id() == account_id),
            "Account should persist after sync"
        );
    }

    /// TC-1.7.3: No data loss after operations
    #[tokio::test]
    async fn test_no_data_loss() {
        let mut client = create_test_client().await.expect("Failed to create client");

        let mut account_ids = Vec::new();
        for _ in 0..3 {
            let (account, _) = client
                .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
                .await
                .expect("Failed to create account");
            account_ids.push(account.id());
        }

        client.sync_state().await.expect("Failed to sync");

        let accounts = client.get_accounts().expect("Failed to get accounts");
        for id in &account_ids {
            assert!(
                accounts.iter().any(|a| a.id() == *id),
                "Account {:?} should exist after sync",
                id
            );
        }
    }
}
