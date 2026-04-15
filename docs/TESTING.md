# Manual Testing Setup

## Prerequisites

- Docker and docker-compose

## Quick Start

```bash
./scripts/start-all.sh --clean   # Start all services
./scripts/test-lumia-claims.sh   # Run test transactions
```

## Scripts

### scripts/start-all.sh

Starts miden-node in background and waits for health checks.

```bash
./scripts/start-all.sh           # Start services in background
./scripts/start-all.sh --clean   # Remove volumes first for fresh start
```

### scripts/start-miden-node.sh

Starts only miden-node (foreground mode).

```bash
./scripts/start-miden-node.sh           # Normal start
./scripts/start-miden-node.sh --clean   # Clean volumes first
```

### scripts/start-proxy.sh

Builds and runs the proxy locally, connecting to miden-node.

```bash
./scripts/start-proxy.sh
```

### scripts/test-lumia-claims.sh

Sends real Lumia claimAsset transactions for testing.

```bash
./scripts/test-lumia-claims.sh         # Run all test vectors
./scripts/test-lumia-claims.sh 1 2 3   # Run specific vectors only
```

Shows transaction results and logs.

## Docker Setup

### Dockerfile.miden-node

Builds miden-node from source at the `v0.14.6` tag. First build takes several minutes.

Features:
- Clones from https://github.com/0xMiden/miden-node
- Includes grpc_health_probe for health checks
- Auto-bootstraps genesis on first run

### config/genesis.toml

Genesis configuration with:
- MIDEN native faucet (100M supply)
- WHAT fungible faucet (100M supply)
- Test wallets with initial balances

## Ports

| Service | Port | Description |
|---------|------|-------------|
| miden-node | 57291 | gRPC endpoint |
| proxy | 8546 | Ethereum JSON-RPC endpoint |

## Debugging Tools

| Script | Purpose |
|--------|---------|
| `scripts/list-notes.sh` | List all notes tracked by the proxy |
| `scripts/list-unclaimed-notes.sh` | List notes that have not been claimed yet |
| `scripts/health-check.sh` | Check proxy and miden-node health status |

```bash
# List all tracked notes
./scripts/list-notes.sh

# Show only unclaimed notes (useful for debugging stuck claims)
./scripts/list-unclaimed-notes.sh

# Verify services are healthy before running tests
./scripts/health-check.sh
```

## Stopping Services

```bash
docker compose down      # Stop services
docker compose down -v   # Stop and remove volumes (full reset)
```

## Viewing Logs

```bash
docker compose logs -f              # All services
docker compose logs -f miden-node   # Just miden-node
```
