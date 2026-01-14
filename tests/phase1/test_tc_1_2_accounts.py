"""
TC-1.2: Account Creation

Tests account creation for Alice and Bob test accounts.
Miden accounts are created locally and then registered with the network.
"""

import pytest


# Store created accounts for use by other tests
_created_accounts = {}


@pytest.mark.phase1
class TestAccountCreation:
    """TC-1.2: Create test accounts (Alice, Bob)."""

    def test_create_alice_account(self, miden_client):
        """TC-1.2.1: Create Alice's account."""
        # In Miden, accounts are created via the client
        # The account ID is derived from the account code and storage
        # For testing, we create a basic wallet account

        # Request new account creation
        # Account types: BasicWallet, BasicFungibleFaucet, etc.
        result = miden_client.rpc("create_account", {
            "account_type": "BasicWallet",
            "storage_mode": "public"  # Public for easier testing
        })

        assert result is not None
        assert "account_id" in result

        _created_accounts["alice"] = result["account_id"]
        print(f"Created Alice account: {result['account_id']}")

    def test_create_bob_account(self, miden_client):
        """TC-1.2.2: Create Bob's account."""
        result = miden_client.rpc("create_account", {
            "account_type": "BasicWallet",
            "storage_mode": "public"
        })

        assert result is not None
        assert "account_id" in result

        _created_accounts["bob"] = result["account_id"]
        print(f"Created Bob account: {result['account_id']}")

    def test_accounts_are_distinct(self, miden_client):
        """TC-1.2.3: Alice and Bob have different account IDs."""
        assert "alice" in _created_accounts, "Alice account not created"
        assert "bob" in _created_accounts, "Bob account not created"
        assert _created_accounts["alice"] != _created_accounts["bob"]

    def test_alice_account_queryable(self, miden_client):
        """TC-1.2.4: Alice's account can be queried from network."""
        assert "alice" in _created_accounts, "Alice account not created"

        # Query account state from the node
        result = miden_client.rpc("get_account_details", {
            "account_id": _created_accounts["alice"]
        })

        assert result is not None

    def test_bob_account_queryable(self, miden_client):
        """TC-1.2.5: Bob's account can be queried from network."""
        assert "bob" in _created_accounts, "Bob account not created"

        result = miden_client.rpc("get_account_details", {
            "account_id": _created_accounts["bob"]
        })

        assert result is not None


def get_test_accounts() -> dict:
    """Utility function to retrieve created accounts for other tests."""
    return _created_accounts.copy()
