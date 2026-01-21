# Build stage
# Miden crates require Rust 1.90+
FROM rustlang/rust:nightly-bookworm-slim AS builder

# Install build dependencies
RUN apt-get update && apt-get install -y \
    pkg-config \
    libssl-dev \
    protobuf-compiler \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Copy manifests first for better caching
COPY Cargo.toml Cargo.lock ./

# Create dummy src to build dependencies
RUN mkdir -p src src/bin && \
    echo 'fn main() { println!("dummy"); }' > src/main.rs && \
    echo 'fn main() { println!("dummy"); }' > src/bin/verify_notes.rs && \
    cargo build --release && \
    rm -rf src

# Copy actual source and rebuild
COPY src ./src
RUN touch src/main.rs src/bin/verify_notes.rs && cargo build --release

# Runtime stage
FROM debian:bookworm-slim

# Build-time argument for git commit SHA
ARG GIT_COMMIT=unknown
ENV GIT_COMMIT=$GIT_COMMIT

RUN apt-get update && apt-get install -y \
    ca-certificates \
    libssl3 \
    curl \
    && rm -rf /var/lib/apt/lists/*

WORKDIR /app

# Create data directory for miden client SQLite storage
RUN mkdir -p /app/data

COPY --from=builder /app/target/release/miden-rpc-proxy /usr/local/bin/

# Configuration file location
ENV CONFIG_PATH=/app/config.toml

# Default ports
# Proxy listens on 8546 (Ethereum-compatible RPC)
EXPOSE 8546

ENTRYPOINT ["miden-rpc-proxy"]
CMD ["--config", "/app/config.toml"]
