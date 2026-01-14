"""
TC-3.3: Proxy Nonce Management

Tests nonce-related proxy RPC methods (eth_getTransactionCount).
"""

import pytest


@pytest.mark.phase3
class TestProxyNonce:
    """TC-3.3: Verify proxy nonce management."""

    def test_get_transaction_count(self, proxy_client, test_account):
        """TC-3.3.1: eth_getTransactionCount returns valid response."""
        nonce = proxy_client.eth_get_transaction_count(test_account["address"])
        assert nonce is not None
        assert nonce.startswith("0x")
        # Should be non-negative
        assert int(nonce, 16) >= 0

    def test_get_transaction_count_new_address(self, proxy_client):
        """TC-3.3.2: New address has nonce 0."""
        # Random address that should have no transactions
        new_address = "0x1111111111111111111111111111111111111111"
        nonce = proxy_client.eth_get_transaction_count(new_address)
        assert int(nonce, 16) == 0

    def test_get_transaction_count_case_insensitive(self, proxy_client, test_account):
        """TC-3.3.3: Address is case-insensitive."""
        upper_addr = test_account["address"].upper()
        lower_addr = test_account["address"].lower()

        nonce_upper = proxy_client.eth_get_transaction_count(upper_addr)
        nonce_lower = proxy_client.eth_get_transaction_count(lower_addr)

        assert nonce_upper == nonce_lower

    def test_get_transaction_count_pending_block(self, proxy_client, test_account):
        """TC-3.3.4: Pending block tag works."""
        nonce = proxy_client.eth_get_transaction_count(
            test_account["address"], "pending"
        )
        assert nonce is not None
        assert nonce.startswith("0x")

    def test_get_transaction_count_latest_block(self, proxy_client, test_account):
        """TC-3.3.5: Latest block tag works."""
        nonce = proxy_client.eth_get_transaction_count(
            test_account["address"], "latest"
        )
        assert nonce is not None
        assert nonce.startswith("0x")
