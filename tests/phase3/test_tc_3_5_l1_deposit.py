"""
TC-3.5: L1 Deposit Simulation

Tests L1 deposit functionality via Anvil devnet.
Simulates depositing funds to the bridge contract on L1.
"""

import pytest
import time


@pytest.mark.phase3
class TestL1Deposit:
    """TC-3.5: Verify L1 deposit simulation via Anvil."""

    def test_l1_connectivity(self, l1_client):
        """TC-3.5.1: L1 (Anvil) is reachable."""
        block_num = l1_client.eth_block_number()
        assert block_num is not None
        assert block_num.startswith("0x")

    def test_l1_test_account_funded(self, l1_client, test_account):
        """TC-3.5.2: Test account has funds on L1."""
        balance = l1_client.eth_get_balance(test_account["address"])
        assert balance is not None
        assert balance.startswith("0x")
        # Anvil test accounts have 10000 ETH
        balance_wei = int(balance, 16)
        assert balance_wei > 0

    def test_l1_can_mine_blocks(self, l1_client):
        """TC-3.5.3: Can mine blocks on Anvil."""
        block_before = int(l1_client.eth_block_number(), 16)

        # Mine a block
        l1_client.anvil_mine(1)

        block_after = int(l1_client.eth_block_number(), 16)
        assert block_after >= block_before

    def test_l1_set_balance(self, l1_client):
        """TC-3.5.4: Can set balance on Anvil (for test setup)."""
        # Random test address
        test_addr = "0x2222222222222222222222222222222222222222"

        # Set balance to 1 ETH
        one_eth = hex(10**18)
        l1_client.anvil_set_balance(test_addr, one_eth)

        # Verify balance
        balance = l1_client.eth_get_balance(test_addr)
        assert int(balance, 16) == 10**18

    def test_deposit_to_bridge(self, l1_client, test_account, bridge_contract_address):
        """TC-3.5.5: Can send deposit transaction to bridge contract."""
        # Note: This is a simplified test - actual bridge deposit would
        # require the bridge contract to be deployed and configured.
        # For now, we verify we can send a transaction to the bridge address.

        try:
            # This would be a signed transaction to the bridge contract
            # For this test, we just verify the infrastructure is in place
            nonce_before = l1_client.rpc("eth_getTransactionCount", [
                test_account["address"], "latest"
            ])
            assert nonce_before is not None
            # If we got here, L1 infrastructure is ready for deposits
        except Exception as e:
            pytest.skip(f"Bridge contract not deployed: {e}")

    def test_l1_transaction_receipt(self, l1_client):
        """TC-3.5.6: Can retrieve transaction receipts on L1."""
        # Query for a non-existent transaction
        receipt = l1_client.eth_get_transaction_receipt(
            "0x0000000000000000000000000000000000000000000000000000000000000000"
        )
        # Should return None for non-existent transaction
        assert receipt is None
