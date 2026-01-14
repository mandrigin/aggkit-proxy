"""
TC-1.1: Node Connectivity

Verifies that the test environment can connect to the Miden node
and the node is responding to RPC requests.
"""

import pytest
import httpx


@pytest.mark.phase1
class TestNodeConnectivity:
    """TC-1.1: Verify Miden node connectivity."""

    def test_node_reachable(self, miden_node_url: str):
        """TC-1.1.1: Node endpoint is reachable."""
        # Simple HTTP check - node should respond
        with httpx.Client(timeout=10.0) as client:
            # Even a malformed request should get a response
            response = client.post(
                miden_node_url,
                json={},
                headers={"Content-Type": "application/json"}
            )
            # We expect some response (even an error response is OK)
            # A 4xx/5xx with JSON body means node is alive
            assert response.status_code in (200, 400, 500)

    def test_rpc_responds(self, miden_client):
        """TC-1.1.2: Node responds to JSON-RPC requests."""
        # Try to get node status/info
        # The exact method depends on Miden's RPC API
        try:
            # Try common status methods
            result = miden_client.rpc("get_block_header_by_number", [0])
            assert result is not None
        except Exception:
            # If that fails, try sync_state which should exist
            result = miden_client.rpc("sync_state", {
                "block_num": 0,
                "account_ids": [],
                "note_tags": [],
                "nullifiers": []
            })
            assert result is not None

    def test_rpc_error_handling(self, miden_client):
        """TC-1.1.3: Node returns proper errors for invalid methods."""
        from tests.conftest import MidenRPCError

        with pytest.raises(MidenRPCError) as exc_info:
            miden_client.rpc("nonexistent_method_xyz", {})

        # Should get a proper JSON-RPC error
        assert exc_info.value.code != 0
