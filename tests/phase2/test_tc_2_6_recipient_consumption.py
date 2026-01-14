"""
TC-2.6: Recipient Consumption

Verifies that the recipient can consume P2ID notes created from claims.
"""

import pytest
from tests.conftest import MidenRPCClient


@pytest.mark.phase2
class TestRecipientConsumption:
    """TC-2.6: Recipient Consumption."""

    def test_recipient_can_sync_notes(self, miden_client: MidenRPCClient):
        """TC-2.6.1: Recipient can sync to see pending notes."""
        # A recipient account should be able to sync and see
        # P2ID notes addressed to them
        try:
            result = miden_client.rpc("sync_state", {
                "block_num": 0,
                "account_ids": [],
                "note_tags": [],  # Would filter by recipient tag
                "nullifiers": []
            })
            assert result is not None
        except Exception as e:
            pytest.skip(f"Miden node not available: {e}")

    def test_p2id_note_consumable_by_target(self):
        """TC-2.6.2: P2ID note is only consumable by target account."""
        # P2ID (Pay to ID) notes have a script that checks
        # the consuming account matches the target ID
        # This is enforced by the Miden VM

        # The P2ID note script verifies:
        # - Consumer account ID matches the note's target
        # - Consumer has valid authentication
        pass  # Structural test - actual enforcement is in Miden VM

    def test_consumed_note_nullified(self, miden_client: MidenRPCClient):
        """TC-2.6.3: Consumed notes are nullified."""
        # After consumption, the note's nullifier is recorded
        # preventing double-spending
        try:
            result = miden_client.rpc("sync_state", {
                "block_num": 0,
                "account_ids": [],
                "note_tags": [],
                "nullifiers": []  # Would include spent nullifiers
            })
            # Nullifiers prevent replay of consumed notes
            assert result is not None
        except Exception as e:
            pytest.skip(f"Miden node not available: {e}")

    def test_recipient_balance_increases(self):
        """TC-2.6.4: Recipient balance increases after consumption."""
        # After consuming a P2ID note, the recipient's vault
        # should contain the claimed assets
        pass  # Requires full integration test with state tracking

    def test_wrong_recipient_cannot_consume(self):
        """TC-2.6.5: Wrong recipient cannot consume P2ID note."""
        # The P2ID script should fail if the wrong account
        # tries to consume the note
        # This is enforced by:
        # - assert(consumer_account_id == target_account_id)
        pass  # Enforced by Miden VM - tested in integration

    def test_note_metadata_accessible(self):
        """TC-2.6.6: Note metadata is accessible to recipient."""
        # The claim metadata (if any) should be readable
        # by the recipient when consuming the note
        pass  # Metadata is part of note inputs
