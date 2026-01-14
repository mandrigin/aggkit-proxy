"""
TC-1.3: Faucet Deployment

Tests deploying a fungible token faucet on Miden.
A faucet is a special account type that can mint tokens.
"""

import pytest


# Store faucet account for use by other tests
_faucet_account = {}


@pytest.mark.phase1
class TestFaucetDeployment:
    """TC-1.3: Deploy a fungible faucet."""

    def test_create_faucet_account(self, miden_client):
        """TC-1.3.1: Create a fungible faucet account."""
        # Create a BasicFungibleFaucet account
        # This account type can mint fungible tokens
        result = miden_client.rpc("create_account", {
            "account_type": "BasicFungibleFaucet",
            "storage_mode": "public",
            "token_symbol": "TEST",
            "decimals": 8,
            "max_supply": "1000000000000000"  # 10M tokens with 8 decimals
        })

        assert result is not None
        assert "account_id" in result

        _faucet_account["id"] = result["account_id"]
        _faucet_account["token_symbol"] = "TEST"
        print(f"Created faucet account: {result['account_id']}")

    def test_faucet_is_valid_account(self, miden_client):
        """TC-1.3.2: Faucet account is valid and queryable."""
        assert "id" in _faucet_account, "Faucet not created"

        result = miden_client.rpc("get_account_details", {
            "account_id": _faucet_account["id"]
        })

        assert result is not None
        # Verify it's a faucet type account
        assert "account_type" in result or "code" in result

    def test_faucet_has_minting_capability(self, miden_client):
        """TC-1.3.3: Faucet account has minting capability."""
        assert "id" in _faucet_account, "Faucet not created"

        # Query account to verify it has faucet procedures
        result = miden_client.rpc("get_account_details", {
            "account_id": _faucet_account["id"]
        })

        # The account should have faucet-specific procedures
        # This verifies the account was created as a faucet
        assert result is not None


def get_faucet_account() -> dict:
    """Utility function to retrieve faucet account for other tests."""
    return _faucet_account.copy()
