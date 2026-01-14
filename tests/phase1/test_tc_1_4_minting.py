"""
TC-1.4: Token Minting

Tests minting tokens from the faucet to test accounts.
"""

import pytest


@pytest.mark.phase1
class TestTokenMinting:
    """TC-1.4: Mint tokens from faucet."""

    def test_mint_tokens_to_alice(self, miden_client):
        """TC-1.4.1: Mint tokens to Alice's account."""
        from tests.phase1.test_tc_1_2_accounts import get_test_accounts
        from tests.phase1.test_tc_1_3_faucet import get_faucet_account

        accounts = get_test_accounts()
        faucet = get_faucet_account()

        assert "alice" in accounts, "Alice account not created"
        assert "id" in faucet, "Faucet not created"

        # Mint tokens - this creates a note for Alice to consume
        result = miden_client.rpc("mint_asset", {
            "faucet_id": faucet["id"],
            "target_account_id": accounts["alice"],
            "amount": "100000000000"  # 1000 tokens with 8 decimals
        })

        assert result is not None
        # Should return note ID or transaction ID
        assert "note_id" in result or "tx_id" in result
        print(f"Minted tokens to Alice: {result}")

    def test_mint_tokens_to_bob(self, miden_client):
        """TC-1.4.2: Mint tokens to Bob's account."""
        from tests.phase1.test_tc_1_2_accounts import get_test_accounts
        from tests.phase1.test_tc_1_3_faucet import get_faucet_account

        accounts = get_test_accounts()
        faucet = get_faucet_account()

        assert "bob" in accounts, "Bob account not created"
        assert "id" in faucet, "Faucet not created"

        result = miden_client.rpc("mint_asset", {
            "faucet_id": faucet["id"],
            "target_account_id": accounts["bob"],
            "amount": "50000000000"  # 500 tokens
        })

        assert result is not None
        assert "note_id" in result or "tx_id" in result
        print(f"Minted tokens to Bob: {result}")

    def test_alice_has_pending_note(self, miden_client):
        """TC-1.4.3: Alice has a pending note to consume."""
        from tests.phase1.test_tc_1_2_accounts import get_test_accounts

        accounts = get_test_accounts()
        assert "alice" in accounts, "Alice account not created"

        # Query for notes consumable by Alice
        result = miden_client.rpc("get_input_notes", {
            "account_id": accounts["alice"],
            "status": "pending"
        })

        # Should have at least one note from minting
        assert result is not None
        notes = result.get("notes", [])
        assert len(notes) > 0, "Alice should have pending notes"
