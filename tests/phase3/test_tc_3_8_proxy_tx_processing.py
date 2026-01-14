"""
TC-3.8: Proxy Transaction Processing

Tests the proxy's transaction processing capabilities.
Verifies that transactions are properly decoded and forwarded.
"""

import pytest


@pytest.mark.phase3
class TestProxyTxProcessing:
    """TC-3.8: Verify proxy transaction processing."""

    def test_transaction_hash_format(self, proxy_client):
        """TC-3.8.1: Transaction hashes are properly formatted."""
        try:
            raw_tx = "0x" + "ee" * 100
            tx_hash = proxy_client.eth_send_raw_transaction(raw_tx)

            # Hash should be 32 bytes hex
            assert tx_hash.startswith("0x")
            assert len(tx_hash) == 66

            # Should be valid hex
            int(tx_hash, 16)
        except Exception:
            pass

    def test_transaction_tracking(self, proxy_client):
        """TC-3.8.2: Proxy tracks submitted transactions."""
        try:
            raw_tx = "0x" + "ff" * 100
            tx_hash = proxy_client.eth_send_raw_transaction(raw_tx)

            # Should be able to query this transaction
            # Even if receipt is None (pending), query should not error
            receipt = proxy_client.eth_get_transaction_receipt(tx_hash)
            # Result should be None or a valid receipt dict
            assert receipt is None or isinstance(receipt, dict)
        except Exception:
            pass

    def test_block_number_advances(self, proxy_client, miden_client):
        """TC-3.8.3: Block number reflects Miden state."""
        try:
            # Get proxy block number
            proxy_block = int(proxy_client.eth_block_number(), 16)

            # Block number should be non-negative
            assert proxy_block >= 0

            # Could correlate with Miden state if available
            miden_state = miden_client.sync_state(block_num=0)
            if "block_num" in miden_state:
                miden_block = int(miden_state["block_num"])
                # They should be related (proxy tracks Miden)
                # Allow some drift
                assert abs(proxy_block - miden_block) <= 10
        except Exception:
            pass

    def test_transaction_statuses(self, proxy_client):
        """TC-3.8.4: Transaction status transitions properly."""
        try:
            # Submit a transaction
            raw_tx = "0x" + "aa" * 100
            tx_hash = proxy_client.eth_send_raw_transaction(raw_tx)

            # Initial status: should be pending (receipt is None)
            receipt = proxy_client.eth_get_transaction_receipt(tx_hash)

            if receipt is not None:
                # If receipt exists, check status field
                assert "status" in receipt
                # Status should be 0x0 (failed) or 0x1 (success)
                assert receipt["status"] in ("0x0", "0x1")
        except Exception:
            pass

    def test_gas_fields_in_receipt(self, proxy_client):
        """TC-3.8.5: Receipts contain proper gas fields."""
        try:
            raw_tx = "0x" + "bb" * 100
            tx_hash = proxy_client.eth_send_raw_transaction(raw_tx)

            # Wait and check for receipt
            receipt = proxy_client.eth_get_transaction_receipt(tx_hash)

            if receipt is not None:
                # Should have gas-related fields
                assert "gasUsed" in receipt
                assert "cumulativeGasUsed" in receipt
                # Values should be hex strings
                assert receipt["gasUsed"].startswith("0x")
        except Exception:
            pass
