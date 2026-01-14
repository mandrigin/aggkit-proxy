"""
TC-1.5: Note Consumption

Tests consuming notes (claiming minted tokens).
In Miden, tokens are transferred via notes that must be consumed.
"""

import pytest


@pytest.mark.phase1
class TestNoteConsumption:
    """TC-1.5: Consume notes to claim tokens."""

    def test_alice_consumes_mint_note(self, miden_client):
        """TC-1.5.1: Alice consumes her minting note."""
        from tests.phase1.test_tc_1_2_accounts import get_test_accounts

        accounts = get_test_accounts()
        assert "alice" in accounts, "Alice account not created"

        # Get Alice's pending notes
        notes_result = miden_client.rpc("get_input_notes", {
            "account_id": accounts["alice"],
            "status": "pending"
        })

        notes = notes_result.get("notes", [])
        assert len(notes) > 0, "Alice should have pending notes"

        # Consume the first note
        note_id = notes[0].get("note_id") or notes[0].get("id")
        result = miden_client.rpc("consume_notes", {
            "account_id": accounts["alice"],
            "note_ids": [note_id]
        })

        assert result is not None
        print(f"Alice consumed note: {note_id}")

    def test_bob_consumes_mint_note(self, miden_client):
        """TC-1.5.2: Bob consumes his minting note."""
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

        # Consume the first note
        note_id = notes[0].get("note_id") or notes[0].get("id")
        result = miden_client.rpc("consume_notes", {
            "account_id": accounts["bob"],
            "note_ids": [note_id]
        })

        assert result is not None
        print(f"Bob consumed note: {note_id}")

    def test_alice_balance_updated(self, miden_client):
        """TC-1.5.3: Alice's balance reflects consumed tokens."""
        from tests.phase1.test_tc_1_2_accounts import get_test_accounts
        from tests.phase1.test_tc_1_3_faucet import get_faucet_account

        accounts = get_test_accounts()
        faucet = get_faucet_account()
        assert "alice" in accounts, "Alice account not created"
        assert "id" in faucet, "Faucet not created"

        # Query Alice's account balance
        result = miden_client.rpc("get_account_balance", {
            "account_id": accounts["alice"],
            "faucet_id": faucet["id"]
        })

        assert result is not None
        balance = int(result.get("balance", 0))
        assert balance > 0, "Alice should have a positive balance"
        print(f"Alice's balance: {balance}")

    def test_no_duplicate_consumption(self, miden_client):
        """TC-1.5.4: Cannot consume the same note twice."""
        from tests.phase1.test_tc_1_2_accounts import get_test_accounts
        from tests.conftest import MidenRPCError

        accounts = get_test_accounts()
        assert "alice" in accounts, "Alice account not created"

        # Get consumed notes
        notes_result = miden_client.rpc("get_input_notes", {
            "account_id": accounts["alice"],
            "status": "consumed"
        })

        consumed = notes_result.get("notes", [])
        if len(consumed) > 0:
            note_id = consumed[0].get("note_id") or consumed[0].get("id")

            # Try to consume again - should fail
            with pytest.raises(MidenRPCError):
                miden_client.rpc("consume_notes", {
                    "account_id": accounts["alice"],
                    "note_ids": [note_id]
                })
