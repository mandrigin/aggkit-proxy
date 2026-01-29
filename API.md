# API Reference

miden-rpc-proxy implements a subset of the Ethereum JSON-RPC API for bridge operations.

## Endpoint

```
POST http://localhost:8546
Content-Type: application/json
```

## Methods

### eth_chainId

Returns the chain ID used for EIP-155 signing.

**Parameters:** None

**Returns:** `string` - Hex-encoded chain ID

**Example:**
```bash
curl -X POST http://localhost:8546 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}'
```

**Response:**
```json
{"jsonrpc":"2.0","result":"0x2","id":1}
```

---

### eth_gasPrice

Returns the current gas price. Always returns `0x0` as Miden has no gas fees.

**Parameters:** None

**Returns:** `string` - `"0x0"`

**Example:**
```bash
curl -X POST http://localhost:8546 \
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
curl -X POST http://localhost:8546 \
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
curl -X POST http://localhost:8546 \
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
curl -X POST http://localhost:8546 \
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
curl -X POST http://localhost:8546 \
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
curl -X POST http://localhost:8546 \
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

**Parameters:** None

**Returns:** `string` - Hex-encoded block number

**Example:**
```bash
curl -X POST http://localhost:8546 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'
```

**Response:**
```json
{"jsonrpc":"2.0","result":"0x10","id":1}
```

---

### eth_getLogs

Returns logs matching a filter object.

**Parameters:**
1. `object` - Filter object
   - `fromBlock`: `string` (optional) - Block number or tag
   - `toBlock`: `string` (optional) - Block number or tag
   - `address`: `string | string[]` (optional) - Contract address(es)
   - `topics`: `array` (optional) - Topic filters (up to 4 positions)

**Returns:** `array` - Array of log objects

**Example:**
```bash
curl -X POST http://localhost:8546 \
  -H "Content-Type: application/json" \
  -d '{
    "jsonrpc":"2.0",
    "method":"eth_getLogs",
    "params":[{"fromBlock":"0x0","toBlock":"latest"}],
    "id":1
  }'
```

**Response:**
```json
{"jsonrpc":"2.0","result":[],"id":1}
```

---

### eth_getBlockByNumber

Returns block information by number.

**Parameters:**
1. `string` - Block number (hex) or tag (`"latest"`, `"pending"`, `"earliest"`)
2. `boolean` - If `true`, returns full transaction objects; if `false`, only hashes

**Returns:** `object | null` - Block object or null

**Example:**
```bash
curl -X POST http://localhost:8546 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_getBlockByNumber","params":["latest",false],"id":1}'
```

---

### eth_getBlockByHash

Returns block information by hash.

**Parameters:**
1. `string` - Block hash (32 bytes, hex)
2. `boolean` - If `true`, returns full transaction objects; if `false`, only hashes

**Returns:** `object | null` - Block object or null

**Example:**
```bash
curl -X POST http://localhost:8546 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_getBlockByHash","params":["0x...",false],"id":1}'
```

---

### eth_getTransactionByHash

Returns transaction information by hash.

**Parameters:**
1. `string` - Transaction hash

**Returns:** `object | null` - Transaction object or null

**Example:**
```bash
curl -X POST http://localhost:8546 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_getTransactionByHash","params":["0x..."],"id":1}'
```

---

### net_version

Returns the current network ID (decimal string).

**Parameters:** None

**Returns:** `string` - Network ID (e.g., `"2"`)

**Example:**
```bash
curl -X POST http://localhost:8546 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"net_version","params":[],"id":1}'
```

**Response:**
```json
{"jsonrpc":"2.0","result":"2","id":1}
```

---

### eth_getBalance

Returns the balance of an address. Always returns `0x0` (Miden has no ETH balances).

**Parameters:**
1. `string` - Address
2. `string` (optional) - Block number or tag

**Returns:** `string` - `"0x0"`

**Example:**
```bash
curl -X POST http://localhost:8546 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_getBalance","params":["0x...","latest"],"id":1}'
```

---

### eth_getCode

Returns code at an address. Returns `0x00` (STOP opcode) to signal a contract exists.

**Parameters:**
1. `string` - Address
2. `string` (optional) - Block number or tag

**Returns:** `string` - `"0x00"`

**Example:**
```bash
curl -X POST http://localhost:8546 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_getCode","params":["0x...","latest"],"id":1}'
```

---

### eth_getStorageAt

Returns the value of a storage slot. Always returns zero (no EVM storage on Miden).

**Parameters:**
1. `string` - Address
2. `string` - Storage position (hex)
3. `string` (optional) - Block number or tag

**Returns:** `string` - 32 zero bytes

**Example:**
```bash
curl -X POST http://localhost:8546 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_getStorageAt","params":["0x...","0x0","latest"],"id":1}'
```

---

### eth_getBlockTransactionCountByNumber

Returns the number of transactions in a block by number.

**Parameters:**
1. `string` - Block number (hex) or tag

**Returns:** `string` - Transaction count (hex)

**Example:**
```bash
curl -X POST http://localhost:8546 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_getBlockTransactionCountByNumber","params":["latest"],"id":1}'
```

**Response:**
```json
{"jsonrpc":"2.0","result":"0x0","id":1}
```

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

**Function selector:** `0xccaa2d11`

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
| RPC URL | http://localhost:8546 |
| Chain ID | 2 (0x2) |
| Currency Symbol | MIDEN |

### ethers.js Example

```javascript
const { ethers } = require('ethers');

const provider = new ethers.JsonRpcProvider('http://localhost:8546');

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
