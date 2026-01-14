"""
TC-1.7: State Consistency

Verifies state consistency after all Phase 1 operations.
Ensures the Miden node state reflects all transactions correctly.
"""

import pytest


@pytest.mark.phase1
class TestStateConsistency:
    """TC-1.7: Verify state consistency."""

    def test_all_accounts_exist(self, miden_client):
        """TC-1.7.1: All created accounts are queryable."""
        from tests.phase1.test_tc_1_2_accounts import get_test_accounts
        from tests.phase1.test_tc_1_3_faucet import get_faucet_account

        accounts = get_test_accounts()
        faucet = get_faucet_account()

        for name, account_id in accounts.items():
            result = miden_client.rpc("get_account_details", {
                "account_id": account_id
            })
            assert result is not None, f"Account {name} not found"

        if "id" in faucet:
            result = miden_client.rpc("get_account_details", {
                "account_id": faucet["id"]
            })
            assert result is not None, "Faucet not found"

    def test_total_supply_conserved(self, miden_client):
        """TC-1.7.2: Total token supply is conserved."""
        from tests.phase1.test_tc_1_2_accounts import get_test_accounts
        from tests.phase1.test_tc_1_3_faucet import get_faucet_account

        accounts = get_test_accounts()
        faucet = get_faucet_account()

        if "id" not in faucet:
            pytest.skip("Faucet not created")

        total_minted = 150000000000  # 1000 + 500 tokens

        # Sum up all balances
        total_balance = 0
        for account_id in accounts.values():
            result = miden_client.rpc("get_account_balance", {
                "account_id": account_id,
                "faucet_id": faucet["id"]
            })
            total_balance += int(result.get("balance", 0))

        # Total should equal minted amount (conservation of tokens)
        assert total_balance == total_minted, \
            f"Token conservation failed: {total_balance} != {total_minted}"

    def test_no_pending_notes_remain(self, miden_client):
        """TC-1.7.3: No unconsumed notes remain for test accounts."""
        from tests.phase1.test_tc_1_2_accounts import get_test_accounts

        accounts = get_test_accounts()

        for name, account_id in accounts.items():
            result = miden_client.rpc("get_input_notes", {
                "account_id": account_id,
                "status": "pending"
            })
            notes = result.get("notes", [])
            assert len(notes) == 0, f"{name} has {len(notes)} pending notes"

    def test_transaction_history_complete(self, miden_client):
        """TC-1.7.4: Transaction history reflects all operations."""
        from tests.phase1.test_tc_1_2_accounts import get_test_accounts

        accounts = get_test_accounts()

        for name, account_id in accounts.items():
            result = miden_client.rpc("get_transactions", {
                "account_id": account_id
            })
            txs = result.get("transactions", [])
            # Each account should have at least one transaction
            assert len(txs) > 0, f"{name} should have transaction history"

    def test_sync_state_consistent(self, miden_client):
        """TC-1.7.5: Sync state returns consistent block data."""
        # Get current block height
        result = miden_client.rpc("sync_state", {
            "block_num": 0,
            "account_ids": [],
            "note_tags": [],
            "nullifiers": []
        })

        assert result is not None
        assert "block_header" in result or "block_num" in result

        # Query same state again - should be consistent
        result2 = miden_client.rpc("sync_state", {
            "block_num": 0,
            "account_ids": [],
            "note_tags": [],
            "nullifiers": []
        })

        # Block data should match
        assert result.get("block_num") == result2.get("block_num") or \
               result.get("block_header") == result2.get("block_header")
