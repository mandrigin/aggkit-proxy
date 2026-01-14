"""
Phase 2 CLAIM Notes Test Fixtures

Provides fixtures for testing CLAIM note creation, execution, and verification
against Miden node and RPC proxy.
"""

import os
import pytest
import httpx
from dataclasses import dataclass
from typing import Optional


# Endpoints from environment
MIDEN_NODE_URL = os.environ.get("MIDEN_NODE_URL", "http://localhost:57291")
PROXY_URL = os.environ.get("PROXY_URL", "http://localhost:8545")

# claimAsset function selector
CLAIM_ASSET_SELECTOR = bytes.fromhex("2cffd02e")

# CLAIM note size in Felts (567 Felts per spec)
CLAIM_NOTE_FELT_SIZE = 567


@dataclass
class GlobalIndex:
    """Decoded globalIndex bit fields."""
    mainnet_flag: bool
    rollup_index: int
    local_root_index: int
    raw: int

    @classmethod
    def encode(cls, mainnet_flag: bool, rollup_index: int, local_root_index: int) -> "GlobalIndex":
        """Encode bit fields into a global index."""
        raw = (
            (1 if mainnet_flag else 0) << 64 |
            (rollup_index & 0xFFFFFFFF) << 32 |
            (local_root_index & 0xFFFFFFFF)
        )
        return cls(mainnet_flag, rollup_index, local_root_index, raw)

    @classmethod
    def decode(cls, raw: int) -> "GlobalIndex":
        """Decode a raw global index into bit fields."""
        mainnet_flag = bool((raw >> 64) & 1)
        rollup_index = (raw >> 32) & 0xFFFFFFFF
        local_root_index = raw & 0xFFFFFFFF
        return cls(mainnet_flag, rollup_index, local_root_index, raw)


@dataclass
class ClaimAssetParams:
    """Parameters for a claimAsset call."""
    smt_proof_local: list[bytes]
    smt_proof_rollup: list[bytes]
    global_index: GlobalIndex
    mainnet_exit_root: bytes
    rollup_exit_root: bytes
    origin_network: int
    origin_token_address: str
    destination_network: int
    destination_address: str
    amount: int
    metadata: bytes = b""

    def encode_calldata(self) -> bytes:
        """Encode as ABI-encoded calldata for claimAsset call."""
        # This is a simplified encoding - real impl uses eth_abi
        # For tests, we verify the structure exists
        return CLAIM_ASSET_SELECTOR + self._encode_params()

    def _encode_params(self) -> bytes:
        """ABI encode the parameters."""
        import struct
        # Simplified - in practice use eth_abi.encode
        # Just encode enough to validate structure
        parts = []
        # 32 proof hashes for local
        for h in self.smt_proof_local[:32]:
            parts.append(h.ljust(32, b'\x00')[:32])
        # 32 proof hashes for rollup
        for h in self.smt_proof_rollup[:32]:
            parts.append(h.ljust(32, b'\x00')[:32])
        # global_index as uint256
        parts.append(self.global_index.raw.to_bytes(32, 'big'))
        # roots
        parts.append(self.mainnet_exit_root.ljust(32, b'\x00')[:32])
        parts.append(self.rollup_exit_root.ljust(32, b'\x00')[:32])
        # networks and addresses
        parts.append(self.origin_network.to_bytes(32, 'big'))
        parts.append(bytes.fromhex(self.origin_token_address[2:]).rjust(32, b'\x00'))
        parts.append(self.destination_network.to_bytes(32, 'big'))
        parts.append(bytes.fromhex(self.destination_address[2:]).rjust(32, b'\x00'))
        parts.append(self.amount.to_bytes(32, 'big'))
        # metadata offset and data
        parts.append((len(parts) * 32).to_bytes(32, 'big'))
        parts.append(len(self.metadata).to_bytes(32, 'big'))
        if self.metadata:
            parts.append(self.metadata.ljust(32, b'\x00'))
        return b''.join(parts)


class MidenRPCClient:
    """Miden node RPC client."""

    def __init__(self, base_url: str):
        self.base_url = base_url.rstrip("/")
        self._client = httpx.Client(timeout=30.0)
        self._request_id = 0

    def _next_id(self) -> int:
        self._request_id += 1
        return self._request_id

    def rpc(self, method: str, params: Optional[dict | list] = None) -> dict:
        """Make JSON-RPC call to Miden node."""
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


class ProxyRPCClient:
    """RPC proxy client (Ethereum-style JSON-RPC)."""

    def __init__(self, base_url: str):
        self.base_url = base_url.rstrip("/")
        self._client = httpx.Client(timeout=30.0)
        self._request_id = 0

    def _next_id(self) -> int:
        self._request_id += 1
        return self._request_id

    def eth_rpc(self, method: str, params: Optional[list] = None) -> dict:
        """Make Ethereum-style JSON-RPC call."""
        payload = {
            "jsonrpc": "2.0",
            "id": self._next_id(),
            "method": method,
            "params": params or [],
        }

        response = self._client.post(
            self.base_url,
            json=payload,
            headers={"Content-Type": "application/json"}
        )
        response.raise_for_status()
        result = response.json()

        if "error" in result:
            raise ProxyRPCError(result["error"])

        return result.get("result")

    def eth_send_raw_transaction(self, signed_tx: bytes) -> str:
        """Send raw transaction (claim submission)."""
        return self.eth_rpc("eth_sendRawTransaction", ["0x" + signed_tx.hex()])

    def eth_get_transaction_receipt(self, tx_hash: str) -> Optional[dict]:
        """Get transaction receipt."""
        return self.eth_rpc("eth_getTransactionReceipt", [tx_hash])

    def eth_chain_id(self) -> int:
        """Get chain ID."""
        result = self.eth_rpc("eth_chainId", [])
        return int(result, 16)

    def close(self):
        self._client.close()


class MidenRPCError(Exception):
    """Raised when Miden RPC returns an error."""

    def __init__(self, error: dict):
        self.code = error.get("code", -1)
        self.message = error.get("message", "Unknown error")
        self.data = error.get("data")
        super().__init__(f"Miden RPC Error {self.code}: {self.message}")


class ProxyRPCError(Exception):
    """Raised when Proxy RPC returns an error."""

    def __init__(self, error: dict):
        self.code = error.get("code", -1)
        self.message = error.get("message", "Unknown error")
        self.data = error.get("data")
        super().__init__(f"Proxy RPC Error {self.code}: {self.message}")


# Fixtures

@pytest.fixture(scope="session")
def miden_node_url() -> str:
    """Miden node URL."""
    return MIDEN_NODE_URL


@pytest.fixture(scope="session")
def proxy_url() -> str:
    """RPC proxy URL."""
    return PROXY_URL


@pytest.fixture(scope="session")
def miden_client(miden_node_url: str) -> MidenRPCClient:
    """Miden node RPC client."""
    client = MidenRPCClient(miden_node_url)
    yield client
    client.close()


@pytest.fixture(scope="session")
def proxy_client(proxy_url: str) -> ProxyRPCClient:
    """RPC proxy client."""
    client = ProxyRPCClient(proxy_url)
    yield client
    client.close()


@pytest.fixture
def sample_claim_params() -> ClaimAssetParams:
    """Sample claim parameters for testing."""
    return ClaimAssetParams(
        smt_proof_local=[os.urandom(32) for _ in range(32)],
        smt_proof_rollup=[os.urandom(32) for _ in range(32)],
        global_index=GlobalIndex.encode(
            mainnet_flag=True,
            rollup_index=0,
            local_root_index=42,
        ),
        mainnet_exit_root=os.urandom(32),
        rollup_exit_root=os.urandom(32),
        origin_network=0,
        origin_token_address="0x0000000000000000000000000000000000000000",
        destination_network=1,
        destination_address="0x1234567890123456789012345678901234567890",
        amount=1000000000000000000,  # 1 ETH in wei
        metadata=b"",
    )


@pytest.fixture
def agglayer_faucet_id() -> str:
    """AggLayer faucet account ID for testing."""
    # This should be configured based on test environment
    return os.environ.get("AGGLAYER_FAUCET_ID", "0x" + "00" * 15 + "01")


@pytest.fixture
def bridge_account_id() -> str:
    """Bridge account ID for testing."""
    return os.environ.get("BRIDGE_ACCOUNT_ID", "0x" + "00" * 15 + "02")
