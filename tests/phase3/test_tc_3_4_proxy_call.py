"""
TC-3.4: Proxy eth_call

Tests the eth_call method for read-only state queries.
"""

import pytest


@pytest.mark.phase3
class TestProxyCall:
    """TC-3.4: Verify proxy eth_call method."""

    def test_eth_call_basic(self, proxy_client, test_account):
        """TC-3.4.1: eth_call returns valid response."""
        tx = {
            "from": test_account["address"],
            "to": "0x0000000000000000000000000000000000000001",
            "data": "0x",
        }
        result = proxy_client.eth_call(tx)
        assert result is not None
        # Should return hex string (even if empty)
        assert result.startswith("0x")

    def test_eth_call_with_data(self, proxy_client, test_account):
        """TC-3.4.2: eth_call with call data."""
        tx = {
            "from": test_account["address"],
            "to": "0x0000000000000000000000000000000000000001",
            "data": "0xdeadbeef",
        }
        result = proxy_client.eth_call(tx)
        assert result is not None
        assert result.startswith("0x")

    def test_eth_call_latest_block(self, proxy_client, test_account):
        """TC-3.4.3: eth_call with latest block tag."""
        tx = {
            "from": test_account["address"],
            "to": "0x0000000000000000000000000000000000000001",
            "data": "0x",
        }
        result = proxy_client.eth_call(tx, "latest")
        assert result is not None
        assert result.startswith("0x")

    def test_eth_call_without_from(self, proxy_client):
        """TC-3.4.4: eth_call works without from field."""
        tx = {
            "to": "0x0000000000000000000000000000000000000001",
            "data": "0x",
        }
        result = proxy_client.eth_call(tx)
        assert result is not None
        assert result.startswith("0x")
