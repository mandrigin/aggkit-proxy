"""
TC-1.6: P2ID Transfer

Tests Pay-to-ID (P2ID) transfers between accounts.
P2ID creates a note that can only be consumed by the target account.
"""

import pytest


@pytest.mark.phase1
class TestP2IDTransfer:
    """TC-1.6: P2ID token transfers."""

    def test_alice_transfers_to_bob(self, miden_client):
        """TC-1.6.1: Alice sends P2ID transfer to Bob."""
        from tests.phase1.test_tc_1_2_accounts import get_test_accounts
        from tests.phase1.test_tc_1_3_faucet import get_faucet_account

        accounts = get_test_accounts()
        faucet = get_faucet_account()

        assert "alice" in accounts, "Alice account not created"
        assert "bob" in accounts, "Bob account not created"
        assert "id" in faucet, "Faucet not created"

        # Alice sends 100 tokens to Bob
        result = miden_client.rpc("send_p2id", {
            "sender_account_id": accounts["alice"],
            "target_account_id": accounts["bob"],
            "faucet_id": faucet["id"],
            "amount": "10000000000"  # 100 tokens
        })

        assert result is not None
        assert "note_id" in result or "tx_id" in result
        print(f"Alice sent P2ID to Bob: {result}")

    def test_bob_receives_p2id_note(self, miden_client):
        """TC-1.6.2: Bob has a pending P2ID note from Alice."""
        from tests.phase1.test_tc_1_2_accounts import get_test_accounts

        accounts = get_test_accounts()
        assert "bob" in accounts, "Bob account not created"

        # Query Bob's pending notes
        result = miden_client.rpc("get_input_notes", {
            "account_id": accounts["bob"],
            "status": "pending"
        })

        notes = result.get("notes", [])
        # Bob should have a pending note (P2ID from Alice)
        assert len(notes) > 0, "Bob should have pending P2ID note"

    def test_bob_consumes_p2id(self, miden_client):
        """TC-1.6.3: Bob consumes the P2ID note."""
        from tests.phase1.test_tc_1_2_accounts import get_test_accounts

        accounts = get_test_accounts()
        assert "bob" in accounts, "Bob account not created"

        # Get Bob's pending notes
        notes_result = miden_client.rpc("get_input_notes", {
            "account_id": accounts["bob"],
            "status": "pending"
        })

        notes = notes_result.get("notes", [])
        assert len(notes) > 0, "Bob should have pending notes"

        # Consume the P2ID note
        note_id = notes[0].get("note_id") or notes[0].get("id")
        result = miden_client.rpc("consume_notes", {
            "account_id": accounts["bob"],
            "note_ids": [note_id]
        })

        assert result is not None
        print(f"Bob consumed P2ID note: {note_id}")

    def test_bob_balance_increased(self, miden_client):
        """TC-1.6.4: Bob's balance increased after P2ID consumption."""
        from tests.phase1.test_tc_1_2_accounts import get_test_accounts
        from tests.phase1.test_tc_1_3_faucet import get_faucet_account

        accounts = get_test_accounts()
        faucet = get_faucet_account()

        assert "bob" in accounts, "Bob account not created"
        assert "id" in faucet, "Faucet not created"

        result = miden_client.rpc("get_account_balance", {
            "account_id": accounts["bob"],
            "faucet_id": faucet["id"]
        })

        balance = int(result.get("balance", 0))
        # Bob should have 500 (minted) + 100 (P2ID) = 600 tokens
        assert balance >= 60000000000, f"Bob should have >= 600 tokens, got {balance}"
        print(f"Bob's final balance: {balance}")

    def test_alice_balance_decreased(self, miden_client):
        """TC-1.6.5: Alice's balance decreased after P2ID transfer."""
        from tests.phase1.test_tc_1_2_accounts import get_test_accounts
        from tests.phase1.test_tc_1_3_faucet import get_faucet_account

        accounts = get_test_accounts()
        faucet = get_faucet_account()

        assert "alice" in accounts, "Alice account not created"
        assert "id" in faucet, "Faucet not created"

        result = miden_client.rpc("get_account_balance", {
            "account_id": accounts["alice"],
            "faucet_id": faucet["id"]
        })

        balance = int(result.get("balance", 0))
        # Alice should have 1000 - 100 = 900 tokens
        assert balance <= 90000000000, f"Alice should have <= 900 tokens, got {balance}"
        print(f"Alice's final balance: {balance}")
