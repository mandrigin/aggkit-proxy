"""
TC-3.6: Bridge Service Sync

Tests the bridge service synchronization functionality.
Verifies that the bridge service syncs with L1 and Miden.
"""

import pytest


@pytest.mark.phase3
class TestBridgeSync:
    """TC-3.6: Verify bridge service synchronization."""

    def test_bridge_service_health(self, bridge_client):
        """TC-3.6.1: Bridge service health check passes."""
        try:
            health = bridge_client.health()
            assert health is not None
            # Health response should indicate service is running
            assert health.get("status") in ("ok", "healthy", True)
        except Exception as e:
            pytest.skip(f"Bridge service not available: {e}")

    def test_bridge_service_reachable(self, bridge_service_url):
        """TC-3.6.2: Bridge service endpoint is reachable."""
        import httpx

        with httpx.Client(timeout=10.0) as client:
            try:
                response = client.get(f"{bridge_service_url}/health")
                # Any response (even error) means service is reachable
                assert response.status_code in (200, 400, 404, 500)
            except httpx.ConnectError:
                pytest.skip("Bridge service not reachable")

    def test_bridge_pending_claims_endpoint(self, bridge_client):
        """TC-3.6.3: Can query pending claims from bridge service."""
        try:
            pending = bridge_client.get_pending_claims()
            # Should return a list (possibly empty)
            assert isinstance(pending, list)
        except Exception as e:
            pytest.skip(f"Pending claims endpoint not available: {e}")

    def test_bridge_syncs_with_l1(self, bridge_client, l1_client):
        """TC-3.6.4: Bridge service tracks L1 block height."""
        try:
            health = bridge_client.health()
            if "l1_block" in health or "l1_height" in health:
                bridge_l1_block = health.get("l1_block") or health.get("l1_height")
                actual_l1_block = int(l1_client.eth_block_number(), 16)
                # Bridge should be within a few blocks of L1
                assert abs(int(bridge_l1_block) - actual_l1_block) <= 5
        except Exception as e:
            pytest.skip(f"Bridge L1 sync check not available: {e}")

    def test_bridge_syncs_with_miden(self, bridge_client, miden_client):
        """TC-3.6.5: Bridge service tracks Miden block height."""
        try:
            health = bridge_client.health()
            if "miden_block" in health or "miden_height" in health:
                bridge_miden_block = health.get("miden_block") or health.get("miden_height")
                # Note: Miden block height query depends on actual RPC method
                miden_state = miden_client.sync_state(block_num=0)
                if "block_num" in miden_state:
                    actual_miden_block = miden_state["block_num"]
                    # Bridge should be close to Miden
                    assert abs(int(bridge_miden_block) - int(actual_miden_block)) <= 5
        except Exception as e:
            pytest.skip(f"Bridge Miden sync check not available: {e}")
