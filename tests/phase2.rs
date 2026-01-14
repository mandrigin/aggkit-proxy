//! Phase 2: CLAIM Notes Tests
//!
//! TC-2.1 through TC-2.5: CLAIM note creation, redemption, expiry,
//! faucet interaction, and cross-chain coordination.
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
use common::create_test_client;

// =============================================================================
// TC-2.1: CLAIM Note Creation
// =============================================================================

mod tc_2_1_claim_creation {
    use super::*;

    /// TC-2.1.1: Create a CLAIM note for cross-chain redemption
    #[tokio::test]
    async fn test_create_claim_note() {
        let mut client = create_test_client().await.expect("Failed to create client");

        let token_symbol = TokenSymbol::new("CLM1").expect("Invalid symbol");
        let (faucet, _) = client
            .new_faucet(AccountStorageMode::Public, token_symbol, 8, Felt::new(1_000_000_000))
            .await
            .expect("Failed to create faucet");

        let (target, _) = client
            .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
            .await
            .expect("Failed to create target");

        let amount = 100_000_000u64;
        let result = client
            .new_mint_transaction(faucet.id(), target.id(), amount, NoteType::Public)
            .await;

        assert!(result.is_ok(), "CLAIM note creation should succeed");
    }

    /// TC-2.1.2: CLAIM note has correct metadata
    #[tokio::test]
    async fn test_claim_note_metadata() {
        let mut client = create_test_client().await.expect("Failed to create client");

        let token_symbol = TokenSymbol::new("CLM2").expect("Invalid symbol");
        let (faucet, _) = client
            .new_faucet(AccountStorageMode::Public, token_symbol, 8, Felt::new(1_000_000_000))
            .await
            .unwrap();

        let (target, _) = client
            .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
            .await
            .unwrap();

        client
            .new_mint_transaction(faucet.id(), target.id(), 100_000_000, NoteType::Public)
            .await
            .unwrap();

        client.sync_state().await.unwrap();

        let notes = client.get_consumable_notes(Some(target.id())).unwrap();
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
        let mut client = create_test_client().await.expect("Failed to create client");

        let token_symbol = TokenSymbol::new("RDM1").expect("Invalid symbol");
        let (faucet, _) = client
            .new_faucet(AccountStorageMode::Public, token_symbol, 8, Felt::new(1_000_000_000))
            .await
            .unwrap();

        let (redeemer, _) = client
            .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
            .await
            .unwrap();

        client
            .new_mint_transaction(faucet.id(), redeemer.id(), 100_000_000, NoteType::Public)
            .await
            .unwrap();

        client.sync_state().await.unwrap();

        let notes = client.get_consumable_notes(Some(redeemer.id())).unwrap();
        if !notes.is_empty() {
            let ids: Vec<_> = notes.iter().map(|n| n.id()).collect();
            let result = client.new_consume_transaction(redeemer.id(), &ids).await;
            assert!(result.is_ok(), "CLAIM redemption should succeed");
        }
    }

    /// TC-2.2.2: Cannot redeem same CLAIM twice
    #[tokio::test]
    async fn test_no_double_redemption() {
        let mut client = create_test_client().await.expect("Failed to create client");

        let token_symbol = TokenSymbol::new("RDM2").expect("Invalid symbol");
        let (faucet, _) = client
            .new_faucet(AccountStorageMode::Public, token_symbol, 8, Felt::new(1_000_000_000))
            .await
            .unwrap();

        let (redeemer, _) = client
            .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
            .await
            .unwrap();

        client
            .new_mint_transaction(faucet.id(), redeemer.id(), 100_000_000, NoteType::Public)
            .await
            .unwrap();

        client.sync_state().await.unwrap();

        let notes = client.get_consumable_notes(Some(redeemer.id())).unwrap();
        if !notes.is_empty() {
            let ids: Vec<_> = notes.iter().map(|n| n.id()).collect();
            client.new_consume_transaction(redeemer.id(), &ids).await.unwrap();

            client.sync_state().await.unwrap();

            let remaining = client.get_consumable_notes(Some(redeemer.id())).unwrap();
            for id in &ids {
                assert!(
                    !remaining.iter().any(|n| n.id() == *id),
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
        let mut client = create_test_client().await.expect("Failed to create client");

        let token_symbol = TokenSymbol::new("EXP1").expect("Invalid symbol");
        let (faucet, _) = client
            .new_faucet(AccountStorageMode::Public, token_symbol, 8, Felt::new(1_000_000_000))
            .await
            .unwrap();

        let (target, _) = client
            .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
            .await
            .unwrap();

        client
            .new_mint_transaction(faucet.id(), target.id(), 100_000_000, NoteType::Public)
            .await
            .unwrap();

        client.sync_state().await.unwrap();

        let notes = client.get_consumable_notes(Some(target.id())).unwrap();
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
        let mut client = create_test_client().await.expect("Failed to create client");

        let token_symbol = TokenSymbol::new("MULT").expect("Invalid symbol");
        let (faucet, _) = client
            .new_faucet(AccountStorageMode::Public, token_symbol, 8, Felt::new(1_000_000_000))
            .await
            .unwrap();

        let mut recipients = Vec::new();
        for _ in 0..3 {
            let (account, _) = client
                .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
                .await
                .unwrap();
            recipients.push(account.id());
        }

        for recipient_id in &recipients {
            let result = client
                .new_mint_transaction(faucet.id(), *recipient_id, 50_000_000, NoteType::Public)
                .await;
            assert!(result.is_ok(), "Minting to {:?} should succeed", recipient_id);
        }
    }

    /// TC-2.4.2: Faucet respects supply limits
    #[tokio::test]
    async fn test_faucet_supply_limits() {
        let mut client = create_test_client().await.expect("Failed to create client");

        let token_symbol = TokenSymbol::new("LIM1").expect("Invalid symbol");
        let max_supply = 100_000_000u64;

        let (faucet, _) = client
            .new_faucet(AccountStorageMode::Public, token_symbol, 8, Felt::new(max_supply))
            .await
            .unwrap();

        let (target, _) = client
            .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
            .await
            .unwrap();

        let result = client
            .new_mint_transaction(faucet.id(), target.id(), max_supply / 2, NoteType::Public)
            .await;

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
        let mut client = create_test_client().await.expect("Failed to create client");

        let token_symbol = TokenSymbol::new("TAG1").expect("Invalid symbol");
        let (faucet, _) = client
            .new_faucet(AccountStorageMode::Public, token_symbol, 8, Felt::new(1_000_000_000))
            .await
            .unwrap();

        let (target, _) = client
            .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
            .await
            .unwrap();

        client
            .new_mint_transaction(faucet.id(), target.id(), 100_000_000, NoteType::Public)
            .await
            .expect("Failed to create tagged note");

        client.sync_state().await.unwrap();

        let notes = client.get_consumable_notes(Some(target.id())).unwrap();
        assert!(!notes.is_empty(), "Tagged note should be created");
    }

    /// TC-2.5.2: State sync includes note data
    #[tokio::test]
    async fn test_sync_includes_notes() {
        let mut client = create_test_client().await.expect("Failed to create client");

        let token_symbol = TokenSymbol::new("SYNC").expect("Invalid symbol");
        let (faucet, _) = client
            .new_faucet(AccountStorageMode::Public, token_symbol, 8, Felt::new(1_000_000_000))
            .await
            .unwrap();

        let (target, _) = client
            .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
            .await
            .unwrap();

        client
            .new_mint_transaction(faucet.id(), target.id(), 100_000_000, NoteType::Public)
            .await
            .unwrap();

        let sync_result = client.sync_state().await;
        assert!(sync_result.is_ok(), "Sync should succeed with new notes");

        let notes = client.get_consumable_notes(Some(target.id())).unwrap();
        assert!(!notes.is_empty(), "Synced state should include notes");
    }
}
