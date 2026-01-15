# API Reference

miden-rpc-proxy implements a subset of the Ethereum JSON-RPC API for bridge operations.

## Endpoint

```
POST http://localhost:8545
Content-Type: application/json
```

## Methods

### eth_chainId

Returns the chain ID used for EIP-155 signing.

**Parameters:** None

**Returns:** `string` - Hex-encoded chain ID

**Example:**
```bash
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}'
```

**Response:**
```json
{"jsonrpc":"2.0","result":"0x4d494445","id":1}
```

---

### eth_gasPrice

Returns the current gas price. Always returns `0x0` as Miden has no gas fees.

**Parameters:** None

**Returns:** `string` - `"0x0"`

**Example:**
```bash
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_gasPrice","params":[],"id":1}'
```

**Response:**
```json
{"jsonrpc":"2.0","result":"0x0","id":1}
```

---

### eth_estimateGas

Estimates gas for a transaction. Returns fixed estimate of 21000.

**Parameters:**
1. `object` - Transaction object
   - `from`: `string` - Sender address
   - `to`: `string` - Recipient address
   - `data`: `string` - Call data
2. `string` (optional) - Block number or tag

**Returns:** `string` - `"0x5208"` (21000 in hex)

**Example:**
```bash
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc":"2.0",
    "method":"eth_estimateGas",
    "params":[{"from":"0x...","to":"0x...","data":"0x..."}],
    "id":1
  }'
```

**Response:**
```json
{"jsonrpc":"2.0","result":"0x5208","id":1}
```

---

### eth_getTransactionCount

Returns the transaction count (nonce) for an address.

**Parameters:**
1. `string` - Address
2. `string` (optional) - Block number or tag (`"latest"`, `"pending"`)

**Returns:** `string` - Hex-encoded nonce

**Example:**
```bash
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc":"2.0",
    "method":"eth_getTransactionCount",
    "params":["0x742d35Cc6634C0532925a3b844Bc454e4438f44e","latest"],
    "id":1
  }'
```

**Response:**
```json
{"jsonrpc":"2.0","result":"0x0","id":1}
```

---

### eth_sendRawTransaction

Submits a signed transaction for processing. The transaction must be a `claimAsset` call.

**Parameters:**
1. `string` - RLP-encoded signed transaction (hex with 0x prefix)

**Returns:** `string` - Transaction hash

**Example:**
```bash
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc":"2.0",
    "method":"eth_sendRawTransaction",
    "params":["0xf86c..."],
    "id":1
  }'
```

**Response:**
```json
{"jsonrpc":"2.0","result":"0x1234567890abcdef...","id":1}
```

**Errors:**
| Code | Message |
|------|---------|
| -32602 | Invalid transaction encoding |
| -32602 | Not a claimAsset transaction |
| -32000 | Claim already processed |
| -32001 | Miden node unavailable |

---

### eth_getTransactionReceipt

Returns the receipt of a transaction by hash.

**Parameters:**
1. `string` - Transaction hash

**Returns:** `object | null` - Receipt object or null if pending/unknown

**Receipt object:**
```json
{
  "transactionHash": "0x...",
  "blockNumber": "0x...",
  "blockHash": "0x...",
  "transactionIndex": "0x0",
  "from": "0x...",
  "to": null,
  "gasUsed": "0x5208",
  "cumulativeGasUsed": "0x5208",
  "status": "0x1",
  "logs": [],
  "logsBloom": "0x00...",
  "type": "0x0",
  "effectiveGasPrice": "0x0"
}
```

**Status values:**
- `"0x1"` - Success
- `"0x0"` - Failed

**Example:**
```bash
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc":"2.0",
    "method":"eth_getTransactionReceipt",
    "params":["0x1234567890abcdef..."],
    "id":1
  }'
```

**Response (confirmed):**
```json
{
  "jsonrpc":"2.0",
  "result":{
    "transactionHash":"0x1234...",
    "blockNumber":"0xa",
    "blockHash":"0x000...00a",
    "transactionIndex":"0x0",
    "from":"0x0000000000000000000000000000000000000000",
    "to":null,
    "gasUsed":"0x5208",
    "cumulativeGasUsed":"0x5208",
    "status":"0x1",
    "logs":[],
    "logsBloom":"0x00...00",
    "type":"0x0",
    "effectiveGasPrice":"0x0"
  },
  "id":1
}
```

**Response (pending):**
```json
{"jsonrpc":"2.0","result":null,"id":1}
```

---

### eth_call

Executes a call without creating a transaction (read-only).

**Parameters:**
1. `object` - Transaction object
   - `to`: `string` - Contract address
   - `data`: `string` - Call data
2. `string` (optional) - Block number or tag

**Returns:** `string` - Return data (hex-encoded)

**Example:**
```bash
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc":"2.0",
    "method":"eth_call",
    "params":[{"to":"0x...","data":"0x..."},"latest"],
    "id":1
  }'
```

**Response:**
```json
{"jsonrpc":"2.0","result":"0x","id":1}
```

---

### eth_blockNumber

Returns the current block number (Miden block height).

When `MIDEN_RPC_URL` is configured, the proxy periodically syncs with the Miden network
to fetch the latest block height. Without configuration, returns `0x0`.

**Parameters:** None

**Returns:** `string` - Hex-encoded block number

**Example:**
```bash
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'
```

**Response:**
```json
{"jsonrpc":"2.0","result":"0x10","id":1}
```

---

## Environment Variables

| Variable | Description | Default |
|----------|-------------|---------|
| `MIDEN_RPC_URL` | Miden node gRPC endpoint (e.g., `http://localhost:57291`). Required for block sync. | None (block height = 0) |
| `MIDEN_DATA_DIR` | Directory for SQLite store and keystore | `./data` |
| `MIDEN_SYNC_INTERVAL_SECS` | Seconds between block sync operations | `10` |

---

## Error Codes

| Code | Message | Description |
|------|---------|-------------|
| -32700 | Parse error | Invalid JSON |
| -32600 | Invalid request | Missing required fields |
| -32601 | Method not found | Unknown RPC method |
| -32602 | Invalid params | Invalid transaction or parameters |
| -32603 | Internal error | Configuration or internal failure |
| -32000 | Already claimed | Duplicate claim attempt |
| -32001 | Miden RPC error | Cannot reach Miden node |
| -32002 | Transaction error | Failed to create/submit Miden tx |
| -32003 | Account resolution | Cannot map Ethereum to Miden address |
| -32004 | Receipt not found | Transaction unknown |

---

## claimAsset Transaction Format

The proxy expects `eth_sendRawTransaction` to contain a call to:

```solidity
function claimAsset(
    bytes32[32] smtProofLocalExitRoot,
    bytes32[32] smtProofRollupExitRoot,
    uint256 globalIndex,
    bytes32 mainnetExitRoot,
    bytes32 rollupExitRoot,
    uint32 originNetwork,
    address originTokenAddress,
    uint32 destinationNetwork,
    address destinationAddress,
    uint256 amount,
    bytes metadata
) external;
```

**Function selector:** `0x2cffd02e`

**globalIndex encoding:**
```
Bit 64      : mainnetFlag (1 = from mainnet, 0 = from rollup)
Bits 32-63  : rollupIndex (source rollup ID)
Bits 0-31   : localRootIndex (deposit counter)
```

---

## Wallet Integration

### MetaMask Configuration

Add network with these settings:

| Setting | Value |
|---------|-------|
| Network Name | Miden Bridge |
| RPC URL | http://localhost:8545 |
| Chain ID | 1296123973 (0x4D494445) |
| Currency Symbol | MIDEN |

### ethers.js Example

```javascript
const { ethers } = require('ethers');

const provider = new ethers.JsonRpcProvider('http://localhost:8545');

// Check chain ID
const chainId = await provider.send('eth_chainId', []);
console.log('Chain ID:', chainId);

// Get block number
const blockNum = await provider.getBlockNumber();
console.log('Block:', blockNum);

// Send claim transaction
const tx = await wallet.sendTransaction({
  to: BRIDGE_CONTRACT,
  data: claimAssetCalldata,
});
console.log('TX Hash:', tx.hash);

// Wait for receipt
const receipt = await tx.wait();
console.log('Status:', receipt.status);
```
