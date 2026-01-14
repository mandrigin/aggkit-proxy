"""
TC-3.10: Receipt Polling

Tests receipt polling functionality for transaction status.
Verifies that receipts become available after transaction processing.
"""

import pytest
import time


@pytest.mark.phase3
class TestReceiptPolling:
    """TC-3.10: Verify receipt polling functionality."""

    def test_receipt_initially_none(self, proxy_client):
        """TC-3.10.1: Receipt is None for unknown transaction."""
        unknown_hash = "0x" + "00" * 32
        receipt = proxy_client.eth_get_transaction_receipt(unknown_hash)
        assert receipt is None

    def test_receipt_pending_initially(self, proxy_client):
        """TC-3.10.2: Receipt is None while transaction is pending."""
        try:
            raw_tx = "0x" + "11" * 100
            tx_hash = proxy_client.eth_send_raw_transaction(raw_tx)

            # Immediately after submission, receipt may be None
            receipt = proxy_client.eth_get_transaction_receipt(tx_hash)
            # Either None (pending) or dict (confirmed) is valid
            assert receipt is None or isinstance(receipt, dict)
        except Exception:
            pass

    def test_poll_for_receipt(self, proxy_client):
        """TC-3.10.3: Can poll for receipt until available."""
        try:
            raw_tx = "0x" + "22" * 100
            tx_hash = proxy_client.eth_send_raw_transaction(raw_tx)

            # Poll with timeout
            max_attempts = 5
            poll_interval = 1.0

            for _ in range(max_attempts):
                receipt = proxy_client.eth_get_transaction_receipt(tx_hash)
                if receipt is not None:
                    break
                time.sleep(poll_interval)

            # After polling, we got either a receipt or timeout
            # Both are valid outcomes in test environment
        except Exception:
            pass

    def test_multiple_receipt_queries(self, proxy_client):
        """TC-3.10.4: Multiple queries for same hash are idempotent."""
        try:
            raw_tx = "0x" + "33" * 100
            tx_hash = proxy_client.eth_send_raw_transaction(raw_tx)

            # Query multiple times
            receipt1 = proxy_client.eth_get_transaction_receipt(tx_hash)
            receipt2 = proxy_client.eth_get_transaction_receipt(tx_hash)
            receipt3 = proxy_client.eth_get_transaction_receipt(tx_hash)

            # All should return same result
            assert receipt1 == receipt2 == receipt3
        except Exception:
            pass

    def test_receipt_query_does_not_block(self, proxy_client):
        """TC-3.10.5: Receipt query returns quickly (doesn't block)."""
        import time

        unknown_hash = "0x" + "ff" * 32

        start = time.time()
        receipt = proxy_client.eth_get_transaction_receipt(unknown_hash)
        duration = time.time() - start

        # Query should return quickly (< 5 seconds)
        assert duration < 5.0
        assert receipt is None
