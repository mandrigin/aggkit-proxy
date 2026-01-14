"""
Miden Integration Test Fixtures

Provides fixtures for connecting to Miden node and managing test state.
"""

import os
import pytest
import httpx


# Miden node RPC endpoint
MIDEN_NODE_URL = os.environ.get("MIDEN_NODE_URL", "http://localhost:57291")


class MidenClient:
    """Simple Miden RPC client for testing."""

    def __init__(self, base_url: str):
        self.base_url = base_url.rstrip("/")
        self._client = httpx.Client(timeout=30.0)
        self._request_id = 0

    def _next_id(self) -> int:
        self._request_id += 1
        return self._request_id

    def rpc(self, method: str, params: dict | list | None = None) -> dict:
        """Make a JSON-RPC call to the Miden node."""
        payload = {
            "jsonrpc": "2.0",
            "id": self._next_id(),
            "method": method,
        }
        if params is not None:
            payload["params"] = params

        response = self._client.post(
            self.base_url,
            json=payload,
            headers={"Content-Type": "application/json"}
        )
        response.raise_for_status()
        result = response.json()

        if "error" in result:
            raise MidenRPCError(result["error"])

        return result.get("result")

    def close(self):
        self._client.close()


class MidenRPCError(Exception):
    """Raised when Miden RPC returns an error."""

    def __init__(self, error: dict):
        self.code = error.get("code", -1)
        self.message = error.get("message", "Unknown error")
        self.data = error.get("data")
        super().__init__(f"Miden RPC Error {self.code}: {self.message}")


@pytest.fixture(scope="session")
def miden_node_url() -> str:
    """Returns the Miden node URL from environment."""
    return MIDEN_NODE_URL


@pytest.fixture(scope="session")
def miden_client(miden_node_url: str) -> MidenClient:
    """Provides a Miden RPC client for the test session."""
    client = MidenClient(miden_node_url)
    yield client
    client.close()


@pytest.fixture
def alice_account(miden_client: MidenClient) -> dict:
    """Creates or retrieves Alice's test account."""
    # Account creation handled in TC-1.2
    # This fixture is populated after account creation
    return {"name": "alice", "id": None}


@pytest.fixture
def bob_account(miden_client: MidenClient) -> dict:
    """Creates or retrieves Bob's test account."""
    return {"name": "bob", "id": None}
