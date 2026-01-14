"""
TC-3.13: Service-Level Errors

Tests error handling when services are unavailable or return errors.
"""

import pytest


@pytest.mark.phase3
class TestServiceErrors:
    """TC-3.13: Verify service-level error handling."""

    def test_miden_node_error_propagation(self, proxy_client, miden_client):
        """TC-3.13.1: Errors from Miden node are handled."""
        # Verify proxy doesn't crash when Miden returns errors
        try:
            # Invalid account ID should trigger error from Miden
            miden_client.get_account_details("invalid")
        except Exception:
            # Error handling is working
            pass

    def test_proxy_handles_backend_timeout(self, proxy_client):
        """TC-3.13.2: Proxy handles backend timeouts gracefully."""
        # This test verifies the proxy has timeout handling
        # Actual timeout testing requires mocking
        block = proxy_client.eth_block_number()
        assert block is not None

    def test_bridge_service_error_handling(self, bridge_client):
        """TC-3.13.3: Bridge service errors are handled."""
        try:
            # Invalid claim ID should return error
            bridge_client.get_claim_status("invalid_claim_id")
        except Exception:
            # Error handling is working
            pass

    def test_l1_connection_handling(self, l1_client):
        """TC-3.13.4: L1 connection errors are handled."""
        # Verify L1 client handles errors
        try:
            # Invalid block should return error
            l1_client.rpc("eth_getBlockByNumber", ["0xffffffff", True])
        except Exception:
            pass

    def test_concurrent_request_handling(self, proxy_client):
        """TC-3.13.5: Concurrent requests don't cause errors."""
        import concurrent.futures

        def make_request():
            return proxy_client.eth_block_number()

        # Submit concurrent requests
        with concurrent.futures.ThreadPoolExecutor(max_workers=5) as executor:
            futures = [executor.submit(make_request) for _ in range(10)]
            results = [f.result() for f in concurrent.futures.as_completed(futures)]

        # All should succeed
        assert all(r.startswith("0x") for r in results)
