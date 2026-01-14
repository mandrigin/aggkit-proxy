"""
TC-3.2: Proxy Gas Methods

Tests gas-related proxy RPC methods (eth_gasPrice, eth_estimateGas).
"""

import pytest


@pytest.mark.phase3
class TestProxyGas:
    """TC-3.2: Verify proxy gas-related methods."""

    def test_gas_price(self, proxy_client):
        """TC-3.2.1: eth_gasPrice returns valid response."""
        gas_price = proxy_client.eth_gas_price()
        assert gas_price is not None
        assert gas_price.startswith("0x")
        # Should be zero or positive hex value
        value = int(gas_price, 16)
        assert value >= 0

    def test_gas_price_is_zero(self, proxy_client):
        """TC-3.2.2: Gas price should be 0 for Miden bridge."""
        gas_price = proxy_client.eth_gas_price()
        # Miden bridge has no gas fees
        assert int(gas_price, 16) == 0

    def test_estimate_gas_basic(self, proxy_client, test_account):
        """TC-3.2.3: eth_estimateGas returns valid response."""
        tx = {
            "from": test_account["address"],
            "to": "0x0000000000000000000000000000000000000001",
            "data": "0x",
        }
        gas_estimate = proxy_client.eth_estimate_gas(tx)
        assert gas_estimate is not None
        assert gas_estimate.startswith("0x")
        # Should be positive
        assert int(gas_estimate, 16) > 0

    def test_estimate_gas_fixed_value(self, proxy_client, test_account):
        """TC-3.2.4: Gas estimate should be fixed for bridge operations."""
        tx = {
            "from": test_account["address"],
            "to": "0x0000000000000000000000000000000000000001",
            "data": "0x",
        }
        gas_estimate = proxy_client.eth_estimate_gas(tx)
        # Should return 21000 (fixed estimate)
        assert int(gas_estimate, 16) == 21000

    def test_estimate_gas_with_data(self, proxy_client, test_account):
        """TC-3.2.5: Gas estimate with data payload."""
        tx = {
            "from": test_account["address"],
            "to": "0x0000000000000000000000000000000000000001",
            "data": "0xdeadbeef",
        }
        gas_estimate = proxy_client.eth_estimate_gas(tx)
        assert gas_estimate is not None
        # Fixed estimate regardless of data
        assert int(gas_estimate, 16) == 21000
