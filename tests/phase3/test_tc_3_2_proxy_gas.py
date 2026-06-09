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

    def test_gas_price_fixed(self, proxy_client):
        """TC-3.2.2: gas price is a fixed compatibility value.

        Miden has no L2 gas market, but the miden-agglayer proxy returns a
        fixed non-zero `eth_gasPrice` (0x3b9aca00 = 1 gwei, see
        `service.rs::eth_gasPrice`) so EVM wallets/tooling that reject a zero
        gas price still work. (The old gutted miden-rpc-proxy returned 0.)
        """
        gas_price = proxy_client.eth_gas_price()
        assert int(gas_price, 16) == 0x3B9ACA00

    def test_estimate_gas_basic(self, proxy_client, test_account):
        """TC-3.2.3: eth_estimateGas returns a valid response."""
        tx = {
            "from": test_account["address"],
            "to": "0x0000000000000000000000000000000000000001",
            "data": "0x",
        }
        gas_estimate = proxy_client.eth_estimate_gas(tx)
        assert gas_estimate is not None
        assert gas_estimate.startswith("0x")

    def test_estimate_gas_is_zero(self, proxy_client, test_account):
        """TC-3.2.4: estimateGas is 0 — Miden has no gas, so the proxy
        returns 0x0 (see `service.rs::eth_estimateGas`)."""
        tx = {
            "from": test_account["address"],
            "to": "0x0000000000000000000000000000000000000001",
            "data": "0x",
        }
        gas_estimate = proxy_client.eth_estimate_gas(tx)
        assert int(gas_estimate, 16) == 0

    def test_estimate_gas_with_data(self, proxy_client, test_account):
        """TC-3.2.5: estimateGas is 0 regardless of the data payload."""
        tx = {
            "from": test_account["address"],
            "to": "0x0000000000000000000000000000000000000001",
            "data": "0xdeadbeef",
        }
        gas_estimate = proxy_client.eth_estimate_gas(tx)
        assert gas_estimate is not None
        assert int(gas_estimate, 16) == 0
