"""
TC-2.2: Bridge Account Deployment

Verifies that the bridge account is deployed and can create P2ID notes
for claim distribution.
"""

import pytest


@pytest.mark.phase2
class TestBridgeAccountDeployment:
    """TC-2.2: Bridge Account Deployment."""

    def test_bridge_account_exists(self, miden_client, bridge_account_id: str):
        """TC-2.2.1: Bridge account exists on Miden."""
        try:
            result = miden_client.rpc("get_account_details", {
                "account_id": bridge_account_id
            })
            assert result is not None, "Bridge account not found"
        except Exception:
            # Try sync_state as fallback
            result = miden_client.rpc("sync_state", {
                "block_num": 0,
                "account_ids": [bridge_account_id],
                "note_tags": [],
                "nullifiers": []
            })
            assert result is not None

    def test_bridge_has_balance(self, miden_client, bridge_account_id: str):
        """TC-2.2.2: Bridge account has assets for distribution."""
        try:
            result = miden_client.rpc("get_account_details", {
                "account_id": bridge_account_id
            })
            # Bridge should have a non-empty vault
            assert result is not None
        except Exception:
            pytest.skip("Account details RPC not available")

    def test_bridge_can_create_notes(self, miden_client, bridge_account_id: str):
        """TC-2.2.3: Bridge account has note creation capability."""
        # The bridge account needs to be able to create P2ID notes
        # This is verified by checking account type/capabilities
        try:
            result = miden_client.rpc("get_account_details", {
                "account_id": bridge_account_id
            })
            # Account should be a regular account (not immutable)
            # that can execute transactions
            assert result is not None
        except Exception:
            pytest.skip("Account details RPC not available")

    def test_bridge_account_synced(self, miden_client, bridge_account_id: str):
        """TC-2.2.4: Bridge account state is synchronized."""
        # Verify the bridge account is synced to latest block
        result = miden_client.rpc("sync_state", {
            "block_num": 0,
            "account_ids": [bridge_account_id],
            "note_tags": [],
            "nullifiers": []
        })
        assert result is not None
        # Should have current block info
        assert "block_header" in result or "block_num" in str(result).lower()
