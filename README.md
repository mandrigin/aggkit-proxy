# miden-rpc-proxy

JSON-RPC proxy server bridging Ethereum-style RPC to Miden network.

## Prerequisites

- Rust 1.82 or later
- Docker (for integration tests)

## Building

```bash
# Debug build
make build

# Release build
make release
```

## Testing

```bash
# Run all tests
make test

# Run unit tests only (Phase 1)
make test-phase1

# Run integration tests (Phase 2)
make test-phase2

# Run ignored/slow tests (Phase 3)
make test-phase3

# Run Docker-based tests
make test-docker
```

## Development

```bash
# Format, check, lint, and run unit tests
make dev

# Format code
make fmt

# Check compilation
make check

# Run linter
make lint
```

## Clean Build

For deterministic builds from a clean checkout:

```bash
git clone <repo-url>
cd miden-rpc-proxy
make build
```

The `Cargo.lock` file is committed to ensure reproducible builds across environments.

## License

MIT
