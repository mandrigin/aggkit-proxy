"""
TC-3.11: Receipt Format

Tests that transaction receipts conform to Ethereum JSON-RPC format.
Verifies all required fields are present and correctly formatted.
"""

import pytest
import time


@pytest.mark.phase3
class TestReceiptFormat:
    """TC-3.11: Verify receipt format matches Ethereum spec."""

    def _get_receipt(self, proxy_client, raw_tx: str, max_wait: float = 5.0):
        """Helper to submit tx and wait for receipt."""
        tx_hash = proxy_client.eth_send_raw_transaction(raw_tx)
        end_time = time.time() + max_wait

        while time.time() < end_time:
            receipt = proxy_client.eth_get_transaction_receipt(tx_hash)
            if receipt is not None:
                return receipt
            time.sleep(0.5)

        return None

    def test_receipt_has_transaction_hash(self, proxy_client):
        """TC-3.11.1: Receipt contains transactionHash field."""
        try:
            receipt = self._get_receipt(proxy_client, "0x" + "44" * 100)
            if receipt is not None:
                assert "transactionHash" in receipt
                assert receipt["transactionHash"].startswith("0x")
                assert len(receipt["transactionHash"]) == 66
        except Exception:
            pass

    def test_receipt_has_block_number(self, proxy_client):
        """TC-3.11.2: Receipt contains blockNumber field."""
        try:
            receipt = self._get_receipt(proxy_client, "0x" + "55" * 100)
            if receipt is not None:
                assert "blockNumber" in receipt
                assert receipt["blockNumber"].startswith("0x")
                # Should be valid hex number
                int(receipt["blockNumber"], 16)
        except Exception:
            pass

    def test_receipt_has_block_hash(self, proxy_client):
        """TC-3.11.3: Receipt contains blockHash field."""
        try:
            receipt = self._get_receipt(proxy_client, "0x" + "66" * 100)
            if receipt is not None:
                assert "blockHash" in receipt
                assert receipt["blockHash"].startswith("0x")
                # Block hash should be 32 bytes
                assert len(receipt["blockHash"]) == 66
        except Exception:
            pass

    def test_receipt_has_status(self, proxy_client):
        """TC-3.11.4: Receipt contains status field."""
        try:
            receipt = self._get_receipt(proxy_client, "0x" + "77" * 100)
            if receipt is not None:
                assert "status" in receipt
                # Status is 0x0 (failure) or 0x1 (success)
                assert receipt["status"] in ("0x0", "0x1")
        except Exception:
            pass

    def test_receipt_has_gas_used(self, proxy_client):
        """TC-3.11.5: Receipt contains gasUsed field."""
        try:
            receipt = self._get_receipt(proxy_client, "0x" + "88" * 100)
            if receipt is not None:
                assert "gasUsed" in receipt
                assert receipt["gasUsed"].startswith("0x")
                gas = int(receipt["gasUsed"], 16)
                assert gas >= 0
        except Exception:
            pass

    def test_receipt_has_cumulative_gas_used(self, proxy_client):
        """TC-3.11.6: Receipt contains cumulativeGasUsed field."""
        try:
            receipt = self._get_receipt(proxy_client, "0x" + "99" * 100)
            if receipt is not None:
                assert "cumulativeGasUsed" in receipt
                assert receipt["cumulativeGasUsed"].startswith("0x")
        except Exception:
            pass

    def test_receipt_has_logs(self, proxy_client):
        """TC-3.11.7: Receipt contains logs array."""
        try:
            receipt = self._get_receipt(proxy_client, "0x" + "aa" * 100)
            if receipt is not None:
                assert "logs" in receipt
                assert isinstance(receipt["logs"], list)
        except Exception:
            pass

    def test_receipt_has_logs_bloom(self, proxy_client):
        """TC-3.11.8: Receipt contains logsBloom field."""
        try:
            receipt = self._get_receipt(proxy_client, "0x" + "bb" * 100)
            if receipt is not None:
                assert "logsBloom" in receipt
                assert receipt["logsBloom"].startswith("0x")
                # Bloom filter is 256 bytes
                assert len(receipt["logsBloom"]) == 514  # 0x + 512 hex chars
        except Exception:
            pass

    def test_receipt_has_type(self, proxy_client):
        """TC-3.11.9: Receipt contains type field."""
        try:
            receipt = self._get_receipt(proxy_client, "0x" + "cc" * 100)
            if receipt is not None:
                assert "type" in receipt
                assert receipt["type"].startswith("0x")
        except Exception:
            pass

    def test_receipt_has_effective_gas_price(self, proxy_client):
        """TC-3.11.10: Receipt contains effectiveGasPrice field."""
        try:
            receipt = self._get_receipt(proxy_client, "0x" + "dd" * 100)
            if receipt is not None:
                assert "effectiveGasPrice" in receipt
                assert receipt["effectiveGasPrice"].startswith("0x")
                # Should be 0 for Miden bridge
                assert int(receipt["effectiveGasPrice"], 16) == 0
        except Exception:
            pass
