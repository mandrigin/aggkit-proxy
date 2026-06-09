"""
TC-3.1: Proxy Connectivity

Verifies that the proxy service is reachable and responding to RPC requests.
"""

import pytest
import httpx


@pytest.mark.phase3
class TestProxyConnectivity:
    """TC-3.1: Verify proxy service connectivity."""

    def test_proxy_reachable(self, proxy_url: str):
        """TC-3.1.1: Proxy endpoint is reachable."""
        with httpx.Client(timeout=10.0) as client:
            # Even a malformed request should get a response
            response = client.post(
                proxy_url,
                json={},
                headers={"Content-Type": "application/json"}
            )
            # We expect some response (even an error response is OK)
            assert response.status_code in (200, 400, 500)

    def test_proxy_chain_id_responds(self, proxy_client):
        """TC-3.1.2: Proxy responds to eth_chainId."""
        chain_id = proxy_client.eth_chain_id()
        assert chain_id is not None
        # Chain ID should be a hex string
        assert chain_id.startswith("0x")
        # Should be valid hex
        int(chain_id, 16)

    def test_proxy_block_number_responds(self, proxy_client):
        """TC-3.1.3: Proxy responds to eth_blockNumber."""
        block_num = proxy_client.eth_block_number()
        assert block_num is not None
        assert block_num.startswith("0x")
        # Should be valid hex number
        int(block_num, 16)

    def test_proxy_error_handling(self, proxy_client):
        """TC-3.1.4: Proxy returns proper errors for invalid methods."""
        # NB: catch by JSON-RPC `.code` rather than the RPCError class. pytest
        # loads conftest.py as the top-level `conftest` module, so the fixture
        # raises `conftest.MidenRPCError`, while `from tests.conftest import
        # RPCError` would be a *different* class object — `pytest.raises` on it
        # wouldn't match. Asserting the `.code` attribute is import-path-agnostic.
        with pytest.raises(Exception) as exc_info:
            proxy_client.rpc("nonexistent_method_xyz", {})

        # Should get a proper JSON-RPC error (-32601 = method not found)
        assert getattr(exc_info.value, "code", 0) != 0
