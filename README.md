# miden-rpc-proxy

JSON-RPC proxy server bridging Ethereum-style RPC to Miden network. Enables AggLayer bridge claim processing by translating `eth_sendRawTransaction` calls containing `claimAsset` transactions into Miden P2ID note distributions.

## Architecture

```
                                    +------------------+
                                    |   Miden Node     |
                                    |  (gRPC :57291)   |
                                    +--------^---------+
                                             |
+-------------+    JSON-RPC    +-------------+-------------+
|   Wallet    | ------------> |      miden-rpc-proxy      |
| (MetaMask)  |   :8545       |                           |
+-------------+               | +-------+ +-------------+ |
                              | |Decoder| |Address      | |
                              | |       | |Mapper       | |
                              | +-------+ +-------------+ |
                              | +-------+ +-------------+ |
                              | |Bridge | |Miden Client | |
                              | |State  | |Wrapper      | |
                              | +-------+ +-------------+ |
                              +---------------------------+
```

**Flow:**
1. User submits `claimAsset` transaction via Ethereum wallet
2. Proxy decodes RLP transaction and extracts claim parameters
3. Address mapper resolves Ethereum address to Miden AccountId
4. Miden client creates P2ID note for the recipient
5. Transaction submitted to Miden network
6. Receipt returned in Ethereum format

## Prerequisites

- Rust 1.82 or later
- Docker (for integration tests)
- A running Miden node (for full functionality)

## Building

```bash
# Clone the repository
git clone <repo-url>
cd miden-rpc-proxy

# Debug build
cargo build

# Release build (optimized)
cargo build --release

# Or use Make
make build      # debug
make release    # release
```

## Configuration

Create a `config.toml` file:

```toml
# Port for JSON-RPC server (default: 8545)
listen_port = 8545

# Miden node RPC endpoint
miden_rpc_url = "http://localhost:57291"

# Chain ID returned by eth_chainId (default: 1)
chain_id = 1296123973  # "MIDE" in hex

# Bridge faucet account ID (hex string)
bridge_account_id = "0x..."
```

### Configuration Options

| Option | Type | Default | Description |
|--------|------|---------|-------------|
| `listen_port` | u16 | 8545 | HTTP port for JSON-RPC server |
| `miden_rpc_url` | string | `http://localhost:57291` | Miden node gRPC endpoint |
| `chain_id` | u64 | 1 | EIP-155 chain ID for signing |
| `bridge_account_id` | string | (required) | Miden account holding bridged assets |

### Environment Variables

- `RUST_LOG` - Logging level (e.g., `info`, `debug`, `miden_rpc=debug`)

## Running

```bash
# Start with default config
./target/release/miden-rpc-proxy

# Start with custom config
./target/release/miden-rpc-proxy --config /path/to/config.toml

# With debug logging
RUST_LOG=debug ./target/release/miden-rpc-proxy
```

The server listens on `http://127.0.0.1:8545` by default.

## Quick Demo

One-command test to verify the proxy is working:

```bash
# Build and run unit tests
make dev

# Or run the full test suite
make test
```

### Manual RPC Test

```bash
# Start the proxy (in one terminal)
cargo run

# Test eth_chainId (in another terminal)
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}'

# Expected response:
# {"jsonrpc":"2.0","result":"0x4d494445","id":1}

# Test eth_blockNumber
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}'

# Test eth_gasPrice (always returns 0x0 - no gas on Miden)
curl -X POST http://localhost:8545 \
  -H "Content-Type: application/json" \
  -d '{"jsonrpc":"2.0","method":"eth_gasPrice","params":[],"id":1}'
```

## Testing

```bash
# Run all tests
make test

# Run unit tests only (fast)
make test-phase1

# Run integration tests
make test-phase2

# Run slow/ignored tests
make test-phase3

# Development workflow (format + check + lint + unit tests)
make dev
```

## Project Structure

```
src/
├── main.rs          # RPC server and Ethereum API implementation
├── lib.rs           # Library root
├── config.rs        # Configuration loading (TOML)
├── client.rs        # Miden client wrapper (P2ID notes, transactions)
├── address_mapper.rs # Ethereum <-> Miden address mapping
├── storage.rs       # SQLite persistence for address mappings
├── decode.rs        # Transaction/claimAsset calldata decoder
├── types.rs         # Core types (ClaimAssetParams)
└── error.rs         # Error types with JSON-RPC codes
```

## License

MIT
