"""
Miden Integration Test Fixtures

Provides fixtures for connecting to:
- Miden Node (native Miden RPC)
- Miden Proxy (Ethereum-compatible JSON-RPC) - Phase 3
- Bridge Service (AggLayer bridge interface) - Phase 3
- L1 Anvil (Ethereum devnet) - Phase 3
"""

import os
import pytest
import httpx


# Service endpoints from environment
MIDEN_NODE_URL = os.environ.get("MIDEN_NODE_URL", "http://localhost:57291")
PROXY_URL = os.environ.get("PROXY_URL", "http://localhost:8546")
BRIDGE_SERVICE_URL = os.environ.get("BRIDGE_SERVICE_URL", "http://localhost:8080")
L1_RPC_URL = os.environ.get("L1_RPC_URL", "http://localhost:8545")


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


# Alias for compatibility with Phase 3 tests
RPCError = MidenRPCError


class RPCClient:
    """Generic JSON-RPC client for testing."""

    def __init__(self, base_url: str, timeout: float = 30.0):
        self.base_url = base_url.rstrip("/")
        self._client = httpx.Client(timeout=timeout)
        self._request_id = 0

    def _next_id(self) -> int:
        self._request_id += 1
        return self._request_id

    def rpc(self, method: str, params: dict | list | None = None) -> dict:
        """Make a JSON-RPC call."""
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
            raise RPCError(result["error"])

        return result.get("result")

    def close(self):
        self._client.close()


class ProxyClient(RPCClient):
    """Client for the Miden Proxy (Ethereum JSON-RPC interface)."""

    def eth_chain_id(self) -> str:
        """Returns the chain ID."""
        return self.rpc("eth_chainId")

    def eth_gas_price(self) -> str:
        """Returns the current gas price."""
        return self.rpc("eth_gasPrice")

    def eth_estimate_gas(self, tx: dict, block: str = "latest") -> str:
        """Returns gas estimate for transaction."""
        return self.rpc("eth_estimateGas", [tx, block])

    def eth_get_transaction_count(self, address: str, block: str = "latest") -> str:
        """Returns the nonce for an address."""
        return self.rpc("eth_getTransactionCount", [address, block])

    def eth_send_raw_transaction(self, signed_tx: str) -> str:
        """Submit a raw signed transaction."""
        return self.rpc("eth_sendRawTransaction", [signed_tx])

    def eth_get_transaction_receipt(self, tx_hash: str) -> dict | None:
        """Get the receipt for a transaction."""
        return self.rpc("eth_getTransactionReceipt", [tx_hash])

    def eth_call(self, tx: dict, block: str = "latest") -> str:
        """Execute a read-only call."""
        return self.rpc("eth_call", [tx, block])

    def eth_block_number(self) -> str:
        """Returns the current block number."""
        return self.rpc("eth_blockNumber")


class BridgeServiceClient:
    """Client for the Bridge Service REST API."""

    def __init__(self, base_url: str, timeout: float = 30.0):
        self.base_url = base_url.rstrip("/")
        self._client = httpx.Client(timeout=timeout)

    def health(self) -> dict:
        """Check bridge service health."""
        response = self._client.get(f"{self.base_url}/health")
        response.raise_for_status()
        return response.json()

    def get_pending_claims(self) -> list:
        """Get list of pending claims."""
        response = self._client.get(f"{self.base_url}/claims/pending")
        response.raise_for_status()
        return response.json()

    def get_claim_status(self, claim_id: str) -> dict:
        """Get status of a specific claim."""
        response = self._client.get(f"{self.base_url}/claims/{claim_id}")
        response.raise_for_status()
        return response.json()

    def close(self):
        self._client.close()


class L1Client(RPCClient):
    """Client for L1 Anvil (Ethereum devnet)."""

    def eth_block_number(self) -> str:
        """Returns the current L1 block number."""
        return self.rpc("eth_blockNumber")

    def eth_get_balance(self, address: str, block: str = "latest") -> str:
        """Get ETH balance of an address."""
        return self.rpc("eth_getBalance", [address, block])

    def eth_send_raw_transaction(self, signed_tx: str) -> str:
        """Submit a raw signed transaction to L1."""
        return self.rpc("eth_sendRawTransaction", [signed_tx])

    def eth_get_transaction_receipt(self, tx_hash: str) -> dict | None:
        """Get the receipt for an L1 transaction."""
        return self.rpc("eth_getTransactionReceipt", [tx_hash])

    def anvil_set_balance(self, address: str, balance: str) -> None:
        """Set the balance for an address (Anvil-specific)."""
        self.rpc("anvil_setBalance", [address, balance])

    def anvil_mine(self, blocks: int = 1) -> None:
        """Mine blocks (Anvil-specific)."""
        self.rpc("anvil_mine", [blocks])


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


# =============================================================================
# Phase 3 Fixtures - Full Integration Tests
# =============================================================================

@pytest.fixture(scope="session")
def proxy_url() -> str:
    """Returns the proxy URL from environment."""
    return PROXY_URL


@pytest.fixture(scope="session")
def bridge_service_url() -> str:
    """Returns the bridge service URL from environment."""
    return BRIDGE_SERVICE_URL


@pytest.fixture(scope="session")
def l1_rpc_url() -> str:
    """Returns the L1 RPC URL from environment."""
    return L1_RPC_URL


@pytest.fixture(scope="session")
def proxy_client(proxy_url: str) -> ProxyClient:
    """Provides a proxy client for the test session."""
    client = ProxyClient(proxy_url)
    yield client
    client.close()


@pytest.fixture(scope="session")
def bridge_client(bridge_service_url: str) -> BridgeServiceClient:
    """Provides a bridge service client for the test session."""
    client = BridgeServiceClient(bridge_service_url)
    yield client
    client.close()


@pytest.fixture(scope="session")
def l1_client(l1_rpc_url: str) -> L1Client:
    """Provides an L1 client for the test session."""
    client = L1Client(l1_rpc_url)
    yield client
    client.close()


@pytest.fixture(scope="session")
def test_account() -> dict:
    """
    Provides a test account for transactions.
    Uses a well-known Anvil test account.
    """
    # Anvil's first test account (0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266)
    return {
        "address": "0xf39Fd6e51aad88F6F4ce6aB8827279cffFb92266",
        "private_key": "0xac0974bec39a17e36ba4a6b4d238ff944bacb478cbed5efcae784d7bf4f2ff80",
    }


@pytest.fixture(scope="session")
def bridge_contract_address() -> str:
    """
    Returns the bridge contract address on L1.
    In test environment, this is deployed by the bridge service.
    """
    # Placeholder - actual address would be retrieved from bridge service
    return "0x5FbDB2315678afecb367f032d93F642f64180aa3"
