"""
Miden Services Module

Deploys the Miden node, RPC proxy (miden-agglayer), and nginx forwarder for bridge integration.
These services replace the OP-Geth L2 in the standard kurtosis-cdk deployment.
"""

# Service port definitions
MIDEN_NODE_PORT = 57291
MIDEN_PROXY_PORT = 8546
FORWARDER_PORT = 8545  # Bridge expects L2 on 8545
WEB_UI_PORT = 80
AGGLAYER_POSTGRES_PORT = 5432

# Default images
DEFAULT_MIDEN_NODE_IMAGE = "miden-infra/miden-node:agglayer-v0.1"
DEFAULT_MIDEN_PROXY_IMAGE = "miden-infra/miden-proxy:latest"

# Docker Desktop grouping label
DOCKER_PROJECT_LABEL = "com.docker.compose.project"
MIDEN_PROJECT_GROUP = "miden"

# Postgres credentials for miden-agglayer store
AGGLAYER_DB_USER = "agglayer"
AGGLAYER_DB_PASSWORD = "agglayer"
AGGLAYER_DB_NAME = "agglayer_store"


def deploy(plan, miden_args, contract_setup_addresses, cdk_args):
    """
    Deploy all Miden services.

    Args:
        plan: Kurtosis plan object
        miden_args: Miden-specific configuration
        contract_setup_addresses: Contract addresses from L1 deployment
        cdk_args: Parsed kurtosis-cdk arguments

    Returns:
        dict: Service context with URLs and service references
    """
    deployment_suffix = cdk_args.get("deployment_suffix", "-001")

    # Get configuration
    miden_node_image = miden_args.get("miden_node_image", DEFAULT_MIDEN_NODE_IMAGE)
    miden_proxy_image = miden_args.get("miden_proxy_image", DEFAULT_MIDEN_PROXY_IMAGE)
    miden_network_id = miden_args.get("miden_network_id", 2)

    # Get bridge address from contract deployment
    bridge_address = contract_setup_addresses.get("l1_bridge_address", "")
    if not bridge_address:
        bridge_address = contract_setup_addresses.get("polygon_bridge_address", "")

    # L1 RPC URL (internal Kurtosis network)
    l1_rpc_url = cdk_args.get("l1_rpc_url", "")

    # 1. Deploy Miden node
    miden_node = _deploy_miden_node(plan, deployment_suffix, miden_node_image)

    # Wait for Miden node to be ready
    plan.wait(
        service_name="miden-node" + deployment_suffix,
        recipe=ExecRecipe(command=["nc", "-z", "localhost", str(MIDEN_NODE_PORT)]),
        field="code",
        assertion="==",
        target_value=0,
        timeout="120s",
    )

    # 2. Deploy PostgreSQL for miden-agglayer store
    _deploy_agglayer_postgres(plan, deployment_suffix)

    plan.wait(
        service_name="miden-agglayer-postgres" + deployment_suffix,
        recipe=ExecRecipe(command=["pg_isready", "-U", AGGLAYER_DB_USER, "-d", AGGLAYER_DB_NAME]),
        field="code",
        assertion="==",
        target_value=0,
        timeout="60s",
    )

    # 3. Run database migration
    _apply_agglayer_migration(plan, deployment_suffix)

    # 4. Deploy Miden proxy (miden-agglayer)
    database_url = "host=miden-agglayer-postgres{} user={} password={} dbname={}".format(
        deployment_suffix, AGGLAYER_DB_USER, AGGLAYER_DB_PASSWORD, AGGLAYER_DB_NAME,
    )
    miden_proxy = _deploy_miden_proxy(
        plan,
        deployment_suffix,
        miden_proxy_image,
        miden_network_id,
        l1_rpc_url,
        contract_setup_addresses,
        database_url,
    )

    # Wait for proxy to be ready
    plan.wait(
        service_name="miden-proxy" + deployment_suffix,
        recipe=PostHttpRequestRecipe(
            port_id="rpc",
            endpoint="/",
            content_type="application/json",
            body='{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}',
        ),
        field="code",
        assertion="==",
        target_value=200,
        timeout="120s",
    )

    # 5. Deploy nginx forwarder
    # This routes traffic from port 8545 (where bridge expects L2) to proxy on 8546
    forwarder = _deploy_l2_forwarder(plan, deployment_suffix)

    # 6. Deploy web UI for deposits
    deploy_web_ui = miden_args.get("deploy_web_ui", True)
    web_ui = None
    if deploy_web_ui:
        l1_chain_id = cdk_args.get("l1_chain_id", 271828)
        l1_rpc_url_ext = miden_args.get("l1_rpc_url_external", "")
        web_ui = _deploy_web_ui(plan, deployment_suffix, bridge_address, l1_chain_id, l1_rpc_url_ext)

    # Build context
    proxy_url = "http://miden-proxy{}:{}".format(deployment_suffix, MIDEN_PROXY_PORT)
    forwarder_url = "http://miden-l2-forwarder{}:{}".format(deployment_suffix, FORWARDER_PORT)
    node_url = "http://miden-node{}:{}".format(deployment_suffix, MIDEN_NODE_PORT)

    ctx = {
        "node_service": miden_node,
        "proxy_service": miden_proxy,
        "forwarder_service": forwarder,
        "node_url": node_url,
        "proxy_url": proxy_url,
        "forwarder_url": forwarder_url,
        "l2_rpc_url": forwarder_url,  # Bridge uses this as L2 endpoint
        "network_id": miden_network_id,
        "chain_id": miden_network_id,
    }
    if web_ui:
        ctx["web_ui_service"] = web_ui
        ctx["web_ui_url"] = "http://miden-bridge-ui{}:{}".format(deployment_suffix, WEB_UI_PORT)
    return ctx


def _deploy_miden_node(plan, deployment_suffix, image):
    """Deploy Miden node service."""
    service_name = "miden-node" + deployment_suffix

    return plan.add_service(
        name=service_name,
        config=ServiceConfig(
            image=image,
            ports={
                "rpc": PortSpec(
                    number=MIDEN_NODE_PORT,
                    transport_protocol="TCP",
                ),
            },
            # Miden node runs in devnet mode for testing
            # AGGLAYER_GENESIS=0: proxy creates bridge/faucet accounts (not node genesis)
            env_vars={
                "RUST_LOG": "info,miden_node_ntx_builder=debug,miden_tx=debug",
                "AGGLAYER_GENESIS": "1",
            },
            # Memory: miden-node needs ~4GB for claims + bridge-out
            min_memory=4096,
            # Docker Desktop grouping label
            labels={
                DOCKER_PROJECT_LABEL: MIDEN_PROJECT_GROUP,
            },
        ),
    )


def _deploy_agglayer_postgres(plan, deployment_suffix):
    """Deploy PostgreSQL for miden-agglayer store."""
    service_name = "miden-agglayer-postgres" + deployment_suffix

    return plan.add_service(
        name=service_name,
        config=ServiceConfig(
            image="postgres:16-alpine",
            ports={
                "postgres": PortSpec(
                    number=AGGLAYER_POSTGRES_PORT,
                    transport_protocol="TCP",
                ),
            },
            env_vars={
                "POSTGRES_USER": AGGLAYER_DB_USER,
                "POSTGRES_PASSWORD": AGGLAYER_DB_PASSWORD,
                "POSTGRES_DB": AGGLAYER_DB_NAME,
            },
            labels={
                DOCKER_PROJECT_LABEL: MIDEN_PROJECT_GROUP,
            },
        ),
    )


def _apply_agglayer_migration(plan, deployment_suffix):
    """Run the miden-agglayer database migration."""
    service_name = "miden-agglayer-postgres" + deployment_suffix

    # Migration SQL from miden-agglayer/migrations/001_initial.sql
    migration_sql = """
CREATE TABLE service_state (
    id                  INT PRIMARY KEY DEFAULT 1 CHECK (id = 1),
    latest_block_number BIGINT NOT NULL DEFAULT 0,
    log_counter         BIGINT NOT NULL DEFAULT 0,
    hash_chain_value    BYTEA NOT NULL DEFAULT '\\x0000000000000000000000000000000000000000000000000000000000000000',
    deposit_counter     INT NOT NULL DEFAULT 0,
    updated_at          TIMESTAMPTZ NOT NULL DEFAULT now()
);
INSERT INTO service_state (id) VALUES (1);

CREATE TABLE synthetic_logs (
    id                BIGSERIAL PRIMARY KEY,
    log_index         BIGINT NOT NULL,
    address           TEXT NOT NULL,
    topics            TEXT[] NOT NULL,
    data              TEXT NOT NULL,
    block_number      BIGINT NOT NULL,
    block_hash        BYTEA NOT NULL,
    transaction_hash  TEXT NOT NULL,
    transaction_index BIGINT NOT NULL DEFAULT 0,
    removed           BOOLEAN NOT NULL DEFAULT FALSE,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_logs_block_range_address ON synthetic_logs (block_number, lower(address));
CREATE INDEX idx_logs_tx_hash ON synthetic_logs (lower(transaction_hash));

CREATE TABLE ger_entries (
    ger_hash          BYTEA PRIMARY KEY,
    mainnet_exit_root BYTEA,
    rollup_exit_root  BYTEA,
    block_number      BIGINT NOT NULL,
    timestamp         BIGINT NOT NULL,
    is_injected       BOOLEAN NOT NULL DEFAULT FALSE,
    created_at        TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE transactions (
    tx_hash         TEXT PRIMARY KEY,
    miden_tx_id     TEXT,
    envelope_bytes  BYTEA NOT NULL,
    signer          TEXT NOT NULL,
    expires_at      BIGINT,
    status          TEXT NOT NULL DEFAULT 'pending',
    error_message   TEXT,
    block_number    BIGINT NOT NULL DEFAULT 0,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now(),
    updated_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE INDEX idx_txns_status ON transactions (status);
CREATE INDEX idx_txns_miden_id ON transactions (miden_tx_id) WHERE miden_tx_id IS NOT NULL;

CREATE TABLE transaction_logs (
    id        BIGSERIAL PRIMARY KEY,
    tx_hash   TEXT NOT NULL REFERENCES transactions(tx_hash) ON DELETE CASCADE,
    topics    BYTEA[] NOT NULL,
    data      BYTEA NOT NULL
);
CREATE INDEX idx_txn_logs_tx_hash ON transaction_logs (tx_hash);

CREATE TABLE nonces (
    address    TEXT PRIMARY KEY,
    nonce      BIGINT NOT NULL DEFAULT 0
);

CREATE TABLE claimed_indices (
    global_index TEXT PRIMARY KEY,
    created_at   TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE address_mappings (
    eth_address   TEXT PRIMARY KEY,
    miden_account TEXT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE bridge_out_processed (
    note_id       TEXT PRIMARY KEY,
    deposit_count INT NOT NULL,
    created_at    TIMESTAMPTZ NOT NULL DEFAULT now()
);

CREATE TABLE IF NOT EXISTS faucet_registry (
    faucet_id       TEXT PRIMARY KEY,
    origin_address  BYTEA NOT NULL,
    origin_network  INT NOT NULL,
    symbol          TEXT NOT NULL,
    origin_decimals SMALLINT NOT NULL,
    miden_decimals  SMALLINT NOT NULL,
    scale           SMALLINT NOT NULL,
    created_at      TIMESTAMPTZ NOT NULL DEFAULT now()
);
CREATE UNIQUE INDEX IF NOT EXISTS idx_faucet_origin
    ON faucet_registry (origin_address, origin_network);
"""

    plan.exec(
        service_name=service_name,
        recipe=ExecRecipe(
            command=[
                "/bin/sh", "-c",
                "cat << 'EOSQL' | psql -U {} -d {}\n{}\nEOSQL".format(
                    AGGLAYER_DB_USER,
                    AGGLAYER_DB_NAME,
                    migration_sql,
                ),
            ],
        ),
        description="Running miden-agglayer database migration",
    )


def _deploy_miden_proxy(plan, deployment_suffix, image, network_id, l1_rpc_url, contract_addresses, database_url):
    """Deploy miden-agglayer RPC proxy service."""
    service_name = "miden-proxy" + deployment_suffix
    miden_node_url = "http://miden-node{}:{}".format(deployment_suffix, MIDEN_NODE_PORT)

    env_vars = {
        # Rollup network ID from RollupManager (first rollup = 1)
        # This is used by the bridge's networkID() call, NOT the same as chain ID
        "NETWORK_ID": "1",
        "RUST_LOG": "info",
        "DATABASE_URL": database_url,
    }

    # Bridge and L1 contract addresses
    bridge_address = contract_addresses.get("l1_bridge_address", "")
    if not bridge_address:
        bridge_address = contract_addresses.get("polygon_bridge_address", "")
    if bridge_address:
        env_vars["BRIDGE_ADDRESS"] = bridge_address

    if l1_rpc_url:
        env_vars["L1_RPC_URL"] = l1_rpc_url

    l1_ger_address = contract_addresses.get("l1_ger_address", "")
    if l1_ger_address:
        env_vars["L1_GER_ADDRESS"] = l1_ger_address

    rollup_manager_address = contract_addresses.get("rollup_manager_address", "")
    if rollup_manager_address:
        env_vars["ROLLUP_MANAGER_ADDRESS"] = rollup_manager_address

    rollup_address = contract_addresses.get("rollup_address", "")
    if rollup_address:
        env_vars["ROLLUP_ADDRESS"] = rollup_address

    return plan.add_service(
        name=service_name,
        config=ServiceConfig(
            image=image,
            ports={
                "rpc": PortSpec(
                    number=MIDEN_PROXY_PORT,
                    transport_protocol="TCP",
                    application_protocol="http",
                ),
            },
            cmd=[
                "--chain-id={}".format(network_id),
                "--miden-node={}".format(miden_node_url),
                "--miden-store-dir=/var/lib/miden-agglayer-service",
                "--port={}".format(MIDEN_PROXY_PORT),
            ],
            env_vars=env_vars,
            # Docker Desktop grouping label
            labels={
                DOCKER_PROJECT_LABEL: MIDEN_PROJECT_GROUP,
            },
        ),
    )


def _deploy_l2_forwarder(plan, deployment_suffix):
    """
    Deploy nginx TCP forwarder to route L2 traffic to Miden proxy.

    The bridge services expect L2 on port 8545 (standard Ethereum RPC).
    This forwarder routes that traffic to the Miden proxy on 8546.
    """
    service_name = "miden-l2-forwarder" + deployment_suffix
    proxy_host = "miden-proxy" + deployment_suffix

    # Generate nginx config for TCP stream proxy
    nginx_config = """
events {{
    worker_connections 1024;
}}
stream {{
    upstream miden_proxy {{
        server {proxy_host}:{proxy_port};
    }}
    server {{
        listen {forwarder_port};
        proxy_pass miden_proxy;
    }}
}}
""".format(
        proxy_host=proxy_host,
        proxy_port=MIDEN_PROXY_PORT,
        forwarder_port=FORWARDER_PORT,
    )

    # Create config artifact
    config_artifact = plan.render_templates(
        name="nginx-forwarder-config" + deployment_suffix,
        config={
            "nginx.conf": struct(
                template=nginx_config,
                data={},
            ),
        },
    )

    return plan.add_service(
        name=service_name,
        config=ServiceConfig(
            image="nginx:alpine",
            ports={
                "l2-rpc": PortSpec(
                    number=FORWARDER_PORT,
                    transport_protocol="TCP",
                    application_protocol="http",
                ),
            },
            files={
                "/etc/nginx": config_artifact,
            },
            # Docker Desktop grouping label
            labels={
                DOCKER_PROJECT_LABEL: MIDEN_PROJECT_GROUP,
            },
        ),
    )


def _deploy_web_ui(plan, deployment_suffix, bridge_address, l1_chain_id, l1_rpc_url):
    """
    Deploy the Miden Bridge web UI.

    A simple single-page dApp that lets users send agglayer deposits
    (bridge ETH from L1 to Miden) via a browser wallet.
    """
    service_name = "miden-bridge-ui" + deployment_suffix

    env = {
        "BRIDGE_ADDRESS": bridge_address,
        "L1_CHAIN_ID": str(l1_chain_id),
    }
    if l1_rpc_url:
        env["L1_RPC_URL"] = l1_rpc_url

    return plan.add_service(
        name=service_name,
        config=ServiceConfig(
            image=ImageBuildSpec(
                image_name="miden-bridge-ui",
                build_context_dir="./web-ui",
            ),
            ports={
                "http": PortSpec(
                    number=WEB_UI_PORT,
                    transport_protocol="TCP",
                    application_protocol="http",
                ),
            },
            env_vars=env,
            labels={
                DOCKER_PROJECT_LABEL: MIDEN_PROJECT_GROUP,
            },
        ),
    )


def get_l2_rpc_url(deployment_suffix):
    """Get the L2 RPC URL for bridge configuration."""
    return "http://miden-l2-forwarder{}:{}".format(deployment_suffix, FORWARDER_PORT)
