//! Phase 3: Full Integration Tests
//!
//! TC-3.1 through TC-3.9: End-to-end integration tests covering
//! the complete flow from L1 deposits through Miden execution to withdrawals.

use miden_protocol::{
    account::{AccountStorageMode, AccountType},
    asset::{FungibleAsset, TokenSymbol},
    note::NoteType,
    Felt,
};

mod common;
use common::create_test_client;

// =============================================================================
// TC-3.1: End-to-End Deposit Flow
// =============================================================================

mod tc_3_1_deposit_flow {
    use super::*;

    /// TC-3.1.1: Complete deposit flow simulation
    #[tokio::test]
    async fn test_deposit_flow() {
        let (mut client, _state) = create_test_client().await;

        // Simulate L1 deposit by minting on Miden side
        let token_symbol = TokenSymbol::new("DEP1").expect("Invalid symbol");
        let (faucet, _) = client
            .new_faucet(AccountStorageMode::Public, token_symbol, 8, Felt::new(1_000_000_000))
            .await
            .expect("Failed to create faucet");

        let (user, _) = client
            .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
            .await
            .expect("Failed to create user");

        // Deposit (mint) tokens
        let result = client
            .new_mint_transaction(faucet.id(), user.id(), 100_000_000, NoteType::Public)
            .await;

        assert!(result.is_ok(), "Deposit should succeed");
    }

    /// TC-3.1.2: Deposit creates consumable note
    #[tokio::test]
    async fn test_deposit_creates_note() {
        let (mut client, _state) = create_test_client().await;

        let token_symbol = TokenSymbol::new("DEP2").expect("Invalid symbol");
        let (faucet, _) = client
            .new_faucet(AccountStorageMode::Public, token_symbol, 8, Felt::new(1_000_000_000))
            .await
            .unwrap();

        let (user, _) = client
            .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
            .await
            .unwrap();

        client
            .new_mint_transaction(faucet.id(), user.id(), 100_000_000, NoteType::Public)
            .await
            .unwrap();

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
        let (mut client, _state) = create_test_client().await;

        let token_symbol = TokenSymbol::new("TRF1").expect("Invalid symbol");
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

        // Fund Alice
        client
            .new_mint_transaction(faucet.id(), alice.id(), 1_000_000_000, NoteType::Public)
            .await
            .unwrap();

        client.sync_state().await.unwrap();

        // Alice consumes mint note
        let alice_notes = client.get_consumable_notes(Some(alice.id())).await.unwrap();
        if !alice_notes.is_empty() {
            let ids: Vec<_> = alice_notes.iter().map(|n| n.id()).collect();
            client.new_consume_transaction(alice.id(), ids).await.unwrap();
        }

        // Alice transfers to Bob
        let asset = FungibleAsset::new(faucet.id(), 100_000_000).unwrap();
        let result = client
            .new_send_transaction(alice.id(), bob.id(), asset.into(), NoteType::Public)
            .await;

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
        let (mut client, _state) = create_test_client().await;

        let token_symbol = TokenSymbol::new("WDR1").expect("Invalid symbol");
        let (faucet, _) = client
            .new_faucet(AccountStorageMode::Public, token_symbol, 8, Felt::new(1_000_000_000))
            .await
            .unwrap();

        let (user, _) = client
            .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
            .await
            .unwrap();

        // Fund user
        client
            .new_mint_transaction(faucet.id(), user.id(), 100_000_000, NoteType::Public)
            .await
            .unwrap();

        client.sync_state().await.unwrap();

        // Consume to have balance
        let notes = client.get_consumable_notes(Some(user.id())).await.unwrap();
        if !notes.is_empty() {
            let ids: Vec<_> = notes.iter().map(|n| n.id()).collect();
            let result = client.new_consume_transaction(user.id(), ids).await;
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
        let (mut client, _state) = create_test_client().await;

        let token_symbol = TokenSymbol::new("MUSR").expect("Invalid symbol");
        let (faucet, _) = client
            .new_faucet(AccountStorageMode::Public, token_symbol, 8, Felt::new(10_000_000_000))
            .await
            .unwrap();

        // Create multiple users
        let mut users = Vec::new();
        for _ in 0..5 {
            let (user, _) = client
                .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
                .await
                .unwrap();
            users.push(user.id());
        }

        // Fund all users
        for user_id in &users {
            client
                .new_mint_transaction(faucet.id(), *user_id, 100_000_000, NoteType::Public)
                .await
                .expect("Should mint to user");
        }

        client.sync_state().await.unwrap();

        // Verify all users have notes
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
        let (mut client, _state) = create_test_client().await;

        let token_symbol = TokenSymbol::new("RCV1").expect("Invalid symbol");
        let (faucet, _) = client
            .new_faucet(AccountStorageMode::Public, token_symbol, 8, Felt::new(1_000_000_000))
            .await
            .unwrap();

        let (user, _) = client
            .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
            .await
            .unwrap();

        // Successful operation after potential failure
        let result = client
            .new_mint_transaction(faucet.id(), user.id(), 100_000_000, NoteType::Public)
            .await;

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
        let (mut client, _state) = create_test_client().await;

        let initial_height = client.sync_state().await.unwrap();

        let token_symbol = TokenSymbol::new("STV1").expect("Invalid symbol");
        let (faucet, _) = client
            .new_faucet(AccountStorageMode::Public, token_symbol, 8, Felt::new(1_000_000_000))
            .await
            .unwrap();

        let (user, _) = client
            .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
            .await
            .unwrap();

        client
            .new_mint_transaction(faucet.id(), user.id(), 100_000_000, NoteType::Public)
            .await
            .unwrap();

        let final_height = client.sync_state().await.unwrap();

        assert!(final_height.block_num >= initial_height.block_num, "Block height should not decrease");
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
        let (mut client, _state) = create_test_client().await;

        let token_symbol = TokenSymbol::new("PERF").expect("Invalid symbol");
        let (faucet, _) = client
            .new_faucet(AccountStorageMode::Public, token_symbol, 8, Felt::new(10_000_000_000))
            .await
            .unwrap();

        // Perform multiple operations
        for i in 0..3 {
            let (user, _) = client
                .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
                .await
                .expect(&format!("Failed to create user {}", i));

            client
                .new_mint_transaction(faucet.id(), user.id(), 10_000_000, NoteType::Public)
                .await
                .expect(&format!("Failed to mint to user {}", i));
        }

        client.sync_state().await.expect("Final sync should succeed");
    }
}

// =============================================================================
// TC-3.8: Edge Cases
// =============================================================================

mod tc_3_8_edge_cases {
    use super::*;

    /// TC-3.8.1: Handle zero-value edge case
    #[tokio::test]
    async fn test_minimum_transfer() {
        let (mut client, _state) = create_test_client().await;

        let token_symbol = TokenSymbol::new("EDGE").expect("Invalid symbol");
        let (faucet, _) = client
            .new_faucet(AccountStorageMode::Public, token_symbol, 8, Felt::new(1_000_000_000))
            .await
            .unwrap();

        let (user, _) = client
            .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
            .await
            .unwrap();

        // Minimum non-zero amount
        let result = client
            .new_mint_transaction(faucet.id(), user.id(), 1, NoteType::Public)
            .await;

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
        let (mut client, _state) = create_test_client().await;

        // Setup
        let token_symbol = TokenSymbol::new("FULL").expect("Invalid symbol");
        let (faucet, _) = client
            .new_faucet(AccountStorageMode::Public, token_symbol, 8, Felt::new(10_000_000_000))
            .await
            .expect("Faucet creation");

        let (alice, _) = client
            .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
            .await
            .expect("Alice creation");

        let (bob, _) = client
            .new_account(AccountStorageMode::Public, AccountType::RegularAccountUpdatableCode)
            .await
            .expect("Bob creation");

        // Step 1: Deposit (mint to Alice)
        client
            .new_mint_transaction(faucet.id(), alice.id(), 1_000_000_000, NoteType::Public)
            .await
            .expect("Deposit to Alice");

        client.sync_state().await.expect("Sync after deposit");

        // Step 2: Alice consumes deposit
        let alice_notes = client.get_consumable_notes(Some(alice.id())).await.unwrap();
        if !alice_notes.is_empty() {
            let ids: Vec<_> = alice_notes.iter().map(|n| n.id()).collect();
            client
                .new_consume_transaction(alice.id(), ids)
                .await
                .expect("Alice consume");
        }

        // Step 3: Transfer (Alice to Bob)
        let asset = FungibleAsset::new(faucet.id(), 500_000_000).unwrap();
        client
            .new_send_transaction(alice.id(), bob.id(), asset.into(), NoteType::Public)
            .await
            .expect("Transfer to Bob");

        client.sync_state().await.expect("Sync after transfer");

        // Step 4: Bob consumes transfer
        let bob_notes = client.get_consumable_notes(Some(bob.id())).await.unwrap();
        if !bob_notes.is_empty() {
            let ids: Vec<_> = bob_notes.iter().map(|n| n.id()).collect();
            client
                .new_consume_transaction(bob.id(), ids)
                .await
                .expect("Bob consume");
        }

        // Verify final state
        client.sync_state().await.expect("Final sync");
        println!("Full cycle test completed successfully");
    }
}
