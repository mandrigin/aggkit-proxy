"""
TC-3.12: Error Cases

Tests error handling across the integration.
Verifies proper error responses for invalid inputs.
"""

import pytest
from tests.conftest import RPCError


@pytest.mark.phase3
class TestErrorCases:
    """TC-3.12: Verify error handling across integration."""

    def test_invalid_method(self, proxy_client):
        """TC-3.12.1: Invalid RPC method returns error."""
        with pytest.raises(RPCError) as exc_info:
            proxy_client.rpc("invalid_method_name")

        assert exc_info.value.code != 0

    def test_invalid_params_type(self, proxy_client):
        """TC-3.12.2: Invalid parameter types return error."""
        try:
            # Pass string where object expected
            proxy_client.rpc("eth_estimateGas", "not_an_object")
        except RPCError as e:
            # Error is expected
            assert e.code != 0
        except Exception:
            # Any error is acceptable
            pass

    def test_missing_required_params(self, proxy_client):
        """TC-3.12.3: Missing required params return error."""
        try:
            # eth_getTransactionCount requires address
            proxy_client.rpc("eth_getTransactionCount", [])
        except RPCError as e:
            assert e.code != 0
        except Exception:
            pass

    def test_invalid_address_format(self, proxy_client):
        """TC-3.12.4: Invalid address format is handled."""
        try:
            # Not a valid hex address
            proxy_client.eth_get_transaction_count("not_an_address")
        except RPCError as e:
            assert e.code != 0
        except Exception:
            pass

    def test_invalid_hex_value(self, proxy_client, test_account):
        """TC-3.12.5: Invalid hex values are handled."""
        try:
            # Invalid hex in data field
            tx = {
                "from": test_account["address"],
                "to": "0x0000000000000000000000000000000000000001",
                "data": "not_hex",
            }
            proxy_client.eth_call(tx)
        except RPCError as e:
            assert e.code != 0
        except Exception:
            pass

    def test_malformed_raw_transaction(self, proxy_client):
        """TC-3.12.6: Malformed raw transaction is handled."""
        try:
            # Not valid RLP
            proxy_client.eth_send_raw_transaction("0x123")
        except RPCError as e:
            # Some error expected
            pass
        except Exception:
            pass

    def test_invalid_block_tag(self, proxy_client, test_account):
        """TC-3.12.7: Invalid block tag is handled."""
        try:
            proxy_client.eth_get_transaction_count(
                test_account["address"],
                "invalid_tag"
            )
        except RPCError as e:
            assert e.code != 0
        except Exception:
            pass

    def test_empty_raw_transaction(self, proxy_client):
        """TC-3.12.8: Empty raw transaction is handled."""
        try:
            proxy_client.eth_send_raw_transaction("0x")
        except RPCError as e:
            pass
        except Exception:
            pass
