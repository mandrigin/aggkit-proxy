"""
Miden Proxy Service Definitions

Defines the topology for testing the Miden RPC Proxy:
- miden-node: Miden network node (port 57291)
- l1-anvil: L1 Ethereum devnet (port 8545)
- proxy: The RPC proxy service (port 8546)
- bridge-service: AggLayer bridge interface (port 8080)
- postgres: PostgreSQL database (port 5432)
"""

# Service port definitions
POSTGRES_PORT = 5432
MIDEN_NODE_PORT = 57291
L1_ANVIL_PORT = 8545
PROXY_PORT = 8546
BRIDGE_SERVICE_PORT = 8080

# Image definitions
POSTGRES_IMAGE = "postgres:16-alpine"
MIDEN_NODE_IMAGE = "ghcr.io/0xmiden/miden-node:agglayer-v0.1"
FOUNDRY_IMAGE = "ghcr.io/foundry-rs/foundry:latest"
BRIDGE_SERVICE_IMAGE = "ghcr.io/0xpolygonmiden/bridge-service:latest"

def deploy_topology(plan):
    """
    Deploy the complete test topology.

    Returns:
        dict: Service references for all deployed services
    """
    services = {}

    # 1. Deploy PostgreSQL
    services["postgres"] = _deploy_postgres(plan)

    # 2. Deploy Miden node
    services["miden_node"] = _deploy_miden_node(plan)

    # 3. Deploy L1 Anvil
    services["l1_anvil"] = _deploy_l1_anvil(plan)

    # Wait for infrastructure to be ready
    plan.wait(
        service_name="postgres",
        recipe=ExecRecipe(command=["pg_isready", "-U", "miden"]),
        field="code",
        assertion="==",
        target_value=0,
        timeout="60s",
    )

    # 4. Deploy bridge service (depends on postgres, miden-node, l1-anvil)
    services["bridge_service"] = _deploy_bridge_service(plan, services)

    # 5. Deploy proxy (depends on all other services)
    services["proxy"] = _deploy_proxy(plan, services)

    return services


def _deploy_postgres(plan):
    """Deploy PostgreSQL database."""
    return plan.add_service(
        name="postgres",
        config=ServiceConfig(
            image=POSTGRES_IMAGE,
            ports={
                "postgres": PortSpec(
                    number=POSTGRES_PORT,
                    transport_protocol="TCP",
                ),
            },
            env_vars={
                "POSTGRES_USER": "miden",
                "POSTGRES_PASSWORD": "miden",
                "POSTGRES_DB": "miden_proxy",
            },
        ),
    )


def _deploy_miden_node(plan):
    """Deploy Miden node in devnet mode."""
    return plan.add_service(
        name="miden-node",
        config=ServiceConfig(
            image=MIDEN_NODE_IMAGE,
            cmd=["--dev"],
            ports={
                "rpc": PortSpec(
                    number=MIDEN_NODE_PORT,
                    transport_protocol="TCP",
                ),
            },
        ),
    )


def _deploy_l1_anvil(plan):
    """Deploy Anvil L1 devnet."""
    return plan.add_service(
        name="l1-anvil",
        config=ServiceConfig(
            image=FOUNDRY_IMAGE,
            entrypoint=["anvil"],
            cmd=[
                "--host", "0.0.0.0",
                "--chain-id", "31337",
                "--block-time", "1",
            ],
            ports={
                "rpc": PortSpec(
                    number=L1_ANVIL_PORT,
                    transport_protocol="TCP",
                ),
            },
        ),
    )


def _deploy_bridge_service(plan, services):
    """Deploy the bridge service."""
    postgres_url = "postgres://miden:miden@postgres:{}/miden_proxy".format(POSTGRES_PORT)
    miden_url = "http://miden-node:{}".format(MIDEN_NODE_PORT)
    l1_url = "http://l1-anvil:{}".format(L1_ANVIL_PORT)

    return plan.add_service(
        name="bridge-service",
        config=ServiceConfig(
            image=BRIDGE_SERVICE_IMAGE,
            ports={
                "http": PortSpec(
                    number=BRIDGE_SERVICE_PORT,
                    transport_protocol="TCP",
                ),
            },
            env_vars={
                "DATABASE_URL": postgres_url,
                "MIDEN_RPC_URL": miden_url,
                "L1_RPC_URL": l1_url,
            },
        ),
    )


def _deploy_proxy(plan, services):
    """Deploy the proxy service."""
    miden_url = "http://miden-node:{}".format(MIDEN_NODE_PORT)
    bridge_url = "http://bridge-service:{}".format(BRIDGE_SERVICE_PORT)
    l1_url = "http://l1-anvil:{}".format(L1_ANVIL_PORT)
    postgres_url = "postgres://miden:miden@postgres:{}/miden_proxy".format(POSTGRES_PORT)

    # Build the proxy image from local Dockerfile
    proxy_image = plan.upload_files(
        src=".",
        name="proxy-context",
    )

    return plan.add_service(
        name="proxy",
        config=ServiceConfig(
            # Use local build - in CI this would be a published image
            image=ImageBuildSpec(
                image_name="miden-rpc-proxy",
                build_context_dir=".",
            ),
            ports={
                "rpc": PortSpec(
                    number=PROXY_PORT,
                    transport_protocol="TCP",
                ),
            },
            env_vars={
                "MIDEN_RPC_URL": miden_url,
                "BRIDGE_SERVICE_URL": bridge_url,
                "L1_RPC_URL": l1_url,
                "DATABASE_URL": postgres_url,
                "RUST_LOG": "info",
            },
        ),
    )


def run_tests(plan, services, test_phase="all"):
    """
    Run the test suite against the deployed topology.

    Args:
        plan: Kurtosis plan
        services: Dict of deployed services
        test_phase: Which phase to run ("phase1", "phase2", "phase3", or "all")
    """
    proxy_url = "http://proxy:{}".format(PROXY_PORT)
    miden_url = "http://miden-node:{}".format(MIDEN_NODE_PORT)
    bridge_url = "http://bridge-service:{}".format(BRIDGE_SERVICE_PORT)
    l1_url = "http://l1-anvil:{}".format(L1_ANVIL_PORT)
    postgres_url = "postgres://miden:miden@postgres:{}/miden_proxy".format(POSTGRES_PORT)

    # Determine test command based on phase
    if test_phase == "phase1":
        test_cmd = ["cargo", "test", "phase1", "--", "--nocapture"]
    elif test_phase == "phase2":
        test_cmd = ["cargo", "test", "phase2", "--", "--nocapture"]
    elif test_phase == "phase3":
        test_cmd = ["cargo", "test", "phase3", "--features", "integration", "--", "--ignored", "--nocapture"]
    else:  # all
        test_cmd = ["cargo", "test", "--", "--nocapture"]

    # Run the test container
    plan.add_service(
        name="test-runner",
        config=ServiceConfig(
            image=ImageBuildSpec(
                image_name="miden-rpc-proxy-tests",
                build_context_dir=".",
                target_stage="",
            ),
            cmd=test_cmd,
            env_vars={
                "PROXY_URL": proxy_url,
                "MIDEN_RPC_URL": miden_url,
                "BRIDGE_SERVICE_URL": bridge_url,
                "L1_RPC_URL": l1_url,
                "DATABASE_URL": postgres_url,
                "RUST_LOG": "debug",
            },
        ),
    )

    # Wait for tests to complete
    plan.wait(
        service_name="test-runner",
        recipe=ExecRecipe(command=["test", "-f", "/tmp/tests_complete"]),
        field="code",
        assertion="==",
        target_value=0,
        timeout="600s",  # 10 minute timeout for full test suite
    )


def get_service_urls(services):
    """
    Get the external URLs for all services.

    Returns:
        dict: Service name -> URL mapping
    """
    return {
        "proxy": "http://localhost:{}".format(PROXY_PORT),
        "miden_node": "http://localhost:{}".format(MIDEN_NODE_PORT),
        "l1_anvil": "http://localhost:{}".format(L1_ANVIL_PORT),
        "bridge_service": "http://localhost:{}".format(BRIDGE_SERVICE_PORT),
        "postgres": "postgres://miden:miden@localhost:{}/miden_proxy".format(POSTGRES_PORT),
    }
