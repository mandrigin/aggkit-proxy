# Build stage for miden-node
FROM rust:1.82-slim-bookworm AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    libsqlite3-dev \
    protobuf-compiler \
    git \
    cmake \
    clang \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /build

# Clone miden-node at the agglayer-v0.1 tag
# Source: https://github.com/0xMiden/miden-node/tree/agglayer-v0.1
RUN git clone --depth 1 --branch agglayer-v0.1 \
    https://github.com/0xMiden/miden-node.git .

# Save the miden-node commit SHA for labeling
RUN git rev-parse HEAD > /tmp/miden-node-commit.txt && \
    echo "Miden-node commit: $(cat /tmp/miden-node-commit.txt)"

# Copy and apply agglayer faucet genesis patch
COPY patches/add-agglayer-genesis.sh /tmp/
RUN chmod +x /tmp/add-agglayer-genesis.sh && /tmp/add-agglayer-genesis.sh

# Build the node binary
RUN cargo build --release --bin miden-node

# Runtime stage
FROM debian:bookworm-slim

# Copy the miden-node commit SHA from builder
COPY --from=builder /tmp/miden-node-commit.txt /app/miden-node-commit.txt

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    libsqlite3-0 \
    curl \
    netcat-openbsd \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

COPY --from=builder /build/target/release/miden-node /usr/local/bin/

# Add label with miden-node source info
LABEL org.opencontainers.image.source="https://github.com/0xMiden/miden-node" \
      org.opencontainers.image.ref.name="agglayer-v0.1"

# Copy genesis config
COPY config/genesis.toml /app/genesis.toml

# gRPC port
EXPOSE 57291

# Create data and accounts directories
RUN mkdir -p /data /accounts

# Create entrypoint script
RUN printf '%s\n' \
    '#!/bin/bash' \
    'set -e' \
    '' \
    'DATA_DIR="${MIDEN_NODE_DATA_DIRECTORY:-/data}"' \
    'ACCOUNTS_DIR="${MIDEN_NODE_ACCOUNTS_DIRECTORY:-/accounts}"' \
    'RPC_URL="${MIDEN_NODE_RPC_URL:-http://0.0.0.0:57291}"' \
    'GENESIS_FILE="/app/genesis.toml"' \
    '' \
    '# Log miden-node version info' \
    'echo "=== Miden Node Startup ==="' \
    'if [ -f /app/miden-node-commit.txt ]; then' \
    '    echo "Miden-node commit: $(cat /app/miden-node-commit.txt)"' \
    'fi' \
    '' \
    '# Dump genesis.toml contents for debugging' \
    'echo ""' \
    'echo "=== Genesis Config ($GENESIS_FILE) ==="' \
    'cat "$GENESIS_FILE"' \
    'echo ""' \
    'echo "=== End Genesis Config ==="' \
    'echo ""' \
    '' \
    '# Bootstrap if data directory is empty (no database yet)' \
    'if [ ! -d "$DATA_DIR/db" ]; then' \
    '    echo "Bootstrapping miden-node..."' \
    '    miden-node bundled bootstrap \' \
    '        --data-directory "$DATA_DIR" \' \
    '        --accounts-directory "$ACCOUNTS_DIR" \' \
    '        --genesis-config-file "$GENESIS_FILE"' \
    '    echo "Bootstrap complete."' \
    '    echo ""' \
    '    echo "=== Accounts Created at Bootstrap ==="' \
    '    ls -la "$ACCOUNTS_DIR"' \
    '    echo "=== End Accounts List ==="' \
    '    echo ""' \
    'fi' \
    '' \
    'echo "Starting miden-node on $RPC_URL..."' \
    'exec miden-node bundled start \' \
    '    --rpc.url "$RPC_URL" \' \
    '    --data-directory "$DATA_DIR" \' \
    '    "$@"' \
    > /usr/local/bin/entrypoint.sh && \
    chmod +x /usr/local/bin/entrypoint.sh

ENTRYPOINT ["/usr/local/bin/entrypoint.sh"]
CMD []
