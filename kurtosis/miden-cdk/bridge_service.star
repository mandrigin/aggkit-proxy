"""
Bridge Service Module for Miden-CDK

Deploys the zkevm-bridge-service configured to use Miden proxy as L2.
Also deploys aggkit components (aggsender, aggoracle) with Miden L2 configuration.
"""

# Port definitions
BRIDGE_RPC_PORT = 8080
BRIDGE_GRPC_PORT = 9090
BRIDGE_METRICS_PORT = 8090

# Docker Desktop grouping label
DOCKER_PROJECT_LABEL = "com.docker.compose.project"
BRIDGE_PROJECT_GROUP = "miden"


def deploy(plan, cdk_args, contract_setup_addresses, miden_context, deploy_aggkit=False):
    """
    Deploy bridge infrastructure configured for Miden L2.

    Args:
        plan: Kurtosis plan object
        cdk_args: Parsed kurtosis-cdk arguments
        contract_setup_addresses: Contract addresses from L1 deployment
        miden_context: Miden services context (contains L2 RPC URL)
        deploy_aggkit: Whether to deploy aggkit (disabled by default for Miden)

    Returns:
        dict: Bridge service context
    """
    deployment_suffix = cdk_args.get("deployment_suffix", "-001")
    l1_rpc_url = cdk_args.get("l1_rpc_url", "")
    l2_rpc_url = miden_context.get("l2_rpc_url", "")  # Miden forwarder URL

    # Deploy zkevm-bridge-service
    bridge_service = _deploy_bridge_service(
        plan,
        deployment_suffix,
        cdk_args,
        contract_setup_addresses,
        l1_rpc_url,
        l2_rpc_url,
    )

    # Deploy aggkit (aggsender + aggoracle) with Miden L2
    # NOTE: Disabled by default for Miden as it requires a traditional rollup address
    # The bridge-service can function for L1→L2 deposits without aggkit
    aggkit_service = None
    if deploy_aggkit:
        aggkit_service = _deploy_aggkit(
            plan,
            deployment_suffix,
            cdk_args,
            contract_setup_addresses,
            l1_rpc_url,
            l2_rpc_url,
        )

    bridge_url = "http://zkevm-bridge-service{}:{}".format(deployment_suffix, BRIDGE_RPC_PORT)

    return {
        "bridge_service": bridge_service,
        "aggkit_service": aggkit_service,
        "rpc_url": bridge_url,
        "grpc_url": "http://zkevm-bridge-service{}:{}".format(deployment_suffix, BRIDGE_GRPC_PORT),
    }


def _deploy_bridge_service(plan, deployment_suffix, cdk_args, contract_addresses, l1_rpc_url, l2_rpc_url):
    """Deploy zkevm-bridge-service with Miden L2 configuration."""
    service_name = "zkevm-bridge-service" + deployment_suffix

    # Get contract addresses
    l1_bridge_address = contract_addresses.get("l1_bridge_address", "")
    l1_ger_address = contract_addresses.get("l1_ger_address", "")
    rollup_manager_address = contract_addresses.get("rollup_manager_address", "")
    rollup_manager_block_number = contract_addresses.get("rollup_manager_block_number", "0")

    # For Miden, we use synthetic L2 addresses (proxy handles the mapping)
    # These addresses are reported by the proxy's eth_call responses
    l2_bridge_address = contract_addresses.get("l2_bridge_address", l1_bridge_address)
    l2_ger_address = contract_addresses.get("l2_ger_address", l1_ger_address)

    # Database config
    db_host = "postgres" + deployment_suffix
    db_user = "bridge_user"
    db_password = "redacted"
    db_name = "bridge_db"
    db_port = "5432"

    # Generate bridge config
    config_template = _get_bridge_config_template()
    config_artifact = plan.render_templates(
        name="bridge-config-artifact" + deployment_suffix,
        config={
            "bridge-config.toml": struct(
                template=config_template,
                data={
                    "log_level": cdk_args.get("log_level", "info"),
                    "environment": cdk_args.get("environment", "development"),
                    "l1_rpc_url": l1_rpc_url,
                    "l2_rpc_url": l2_rpc_url,
                    "db_host": db_host,
                    "db_user": db_user,
                    "db_password": db_password,
                    "db_name": db_name,
                    "db_port": db_port,
                    "l1_bridge_address": l1_bridge_address,
                    "l1_ger_address": l1_ger_address,
                    "rollup_manager_address": rollup_manager_address,
                    "rollup_manager_block_number": rollup_manager_block_number,
                    "l2_bridge_address": l2_bridge_address,
                    "l2_ger_address": l2_ger_address,
                    "l2_keystore_password": cdk_args.get("l2_keystore_password", ""),
                    "grpc_port": BRIDGE_GRPC_PORT,
                    "rpc_port": BRIDGE_RPC_PORT,
                    "metrics_port": BRIDGE_METRICS_PORT,
                },
            ),
        },
    )

    # Get claimsponsor keystore from contracts service (correct path: /opt/keystores/)
    claimsponsor_keystore = plan.store_service_files(
        name="claimsponsor-keystore" + deployment_suffix,
        service_name="contracts" + deployment_suffix,
        src="/opt/keystores/claimsponsor.keystore",
    )

    return plan.add_service(
        name=service_name,
        config=ServiceConfig(
            image=cdk_args.get("zkevm_bridge_service_image", "hermeznetwork/zkevm-bridge-service:v0.6.0-RC1"),
            ports={
                "rpc": PortSpec(
                    number=BRIDGE_RPC_PORT,
                    transport_protocol="TCP",
                    application_protocol="http",
                ),
                "grpc": PortSpec(
                    number=BRIDGE_GRPC_PORT,
                    transport_protocol="TCP",
                    application_protocol="grpc",
                ),
                "metrics": PortSpec(
                    number=BRIDGE_METRICS_PORT,
                    transport_protocol="TCP",
                    application_protocol="http",
                ),
            },
            files={
                "/etc/zkevm": Directory(
                    artifact_names=[config_artifact, claimsponsor_keystore],
                ),
            },
            entrypoint=["/app/zkevm-bridge"],
            cmd=["run", "--cfg", "/etc/zkevm/bridge-config.toml"],
            # Docker Desktop grouping label
            labels={
                DOCKER_PROJECT_LABEL: BRIDGE_PROJECT_GROUP,
            },
        ),
    )


def _deploy_aggkit(plan, deployment_suffix, cdk_args, contract_addresses, l1_rpc_url, l2_rpc_url):
    """
    Deploy aggkit (aggsender + aggoracle) configured for Miden L2.

    The aggoracle injects Global Exit Root updates to L2.
    The aggsender submits certificates to the agglayer.
    """
    service_name = "aggkit" + deployment_suffix

    # Generate aggkit config
    config_template = _get_aggkit_config_template()
    config_artifact = plan.render_templates(
        name="aggkit-config-artifact" + deployment_suffix,
        config={
            "config.toml": struct(
                template=config_template,
                data={
                    "log_level": cdk_args.get("log_level", "info"),
                    "l1_rpc_url": l1_rpc_url,
                    "l2_rpc_url": l2_rpc_url,
                    "l1_ger_address": contract_addresses.get("l1_ger_address", ""),
                    "l2_ger_address": contract_addresses.get("l2_ger_address", ""),
                    "rollup_manager_address": contract_addresses.get("rollup_manager_address", ""),
                    "agglayer_url": cdk_args.get("agglayer_grpc_url", "http://agglayer" + deployment_suffix + ":4443"),
                },
            ),
        },
    )

    return plan.add_service(
        name=service_name,
        config=ServiceConfig(
            image=cdk_args.get("aggkit_image", "ghcr.io/0xpolygon/aggkit:latest"),
            ports={
                "rpc": PortSpec(
                    number=5576,
                    transport_protocol="TCP",
                ),
            },
            files={
                "/etc/aggkit": config_artifact,
            },
            cmd=["run", "--cfg=/etc/aggkit/config.toml", "--components=aggsender,aggoracle"],
            # Docker Desktop grouping label
            labels={
                DOCKER_PROJECT_LABEL: BRIDGE_PROJECT_GROUP,
            },
        ),
    )


def _get_bridge_config_template():
    """Return the bridge config template for Miden integration."""
    return """
# Bridge Service Configuration for Miden L2
# Auto-generated by miden-cdk

[Log]
Level = "{{.log_level}}"
Environment = "{{.environment}}"
Outputs = ["stderr"]

[SyncDB]
Database = "postgres"
    [SyncDB.PgStorage]
    User = "{{.db_user}}"
    Name = "{{.db_name}}"
    Password = "{{.db_password}}"
    Host = "{{.db_host}}"
    Port = "{{.db_port}}"
    MaxConns = 20

[Etherman]
l1URL = "{{.l1_rpc_url}}"
L2URLs = ["{{.l2_rpc_url}}"]

[Synchronizer]
SyncInterval = "5s"
SyncChunkSize = 100
ForceL2SyncChunk = true

[BridgeController]
Height = 32

[BridgeServer]
GRPCPort = "{{.grpc_port}}"
HTTPPort = "{{.rpc_port}}"
DefaultPageLimit = 25
MaxPageLimit = 1000
FinalizedGEREnabled = true
    [BridgeServer.DB]
    Database = "postgres"
        [BridgeServer.DB.PgStorage]
        User = "{{.db_user}}"
        Name = "{{.db_name}}"
        Password = "{{.db_password}}"
        Host = "{{.db_host}}"
        Port = "{{.db_port}}"
        MaxConns = 20

[NetworkConfig]
L1GenBlockNumber = "{{.rollup_manager_block_number}}"
L2GenBlockNumbers = [0]
PolygonBridgeAddress = "{{.l1_bridge_address}}"
PolygonZkEVMGlobalExitRootAddress = "{{.l1_ger_address}}"
PolygonRollupManagerAddress = "{{.rollup_manager_address}}"
# Miden integration: sovereign chain addresses
PolygonZkEVMAddress = "{{.l1_bridge_address}}"
L2PolygonBridgeAddresses = ["{{.l2_bridge_address}}"]
RequireSovereignChainSmcs = [true]
L2PolygonZkEVMGlobalExitRootAddresses = ["{{.l2_ger_address}}"]

[ClaimTxManager]
Enabled = true
FrequencyToMonitorTxs = "5s"
PrivateKey = {Path = "/etc/zkevm/claimsponsor.keystore", Password = "{{.l2_keystore_password}}"}
RetryInterval = "1s"
RetryNumber = 10

[Metrics]
Enabled = true
Host = "0.0.0.0"
Port = "{{.metrics_port}}"
"""


def _get_aggkit_config_template():
    """Return the aggkit config template for Miden integration."""
    return """
# Aggkit Configuration for Miden L2
# Auto-generated by miden-cdk

[Log]
Level = "{{.log_level}}"

# L2 connection - points to Miden proxy via forwarder
[L2]
L2URL = "{{.l2_rpc_url}}"
RPCURL = "{{.l2_rpc_url}}"
# TargetChainType must be EVM for aggoracle to work
TargetChainType = "EVM"

# L1 connection
[L1]
L1URL = "{{.l1_rpc_url}}"
RPCURL = "{{.l1_rpc_url}}"

# Agglayer connection
[Agglayer]
URL = "{{.agglayer_url}}"

# Global Exit Root configuration
[AggOracle]
L1GERAddress = "{{.l1_ger_address}}"
L2GERAddress = "{{.l2_ger_address}}"

[AggSender]
RollupManagerAddress = "{{.rollup_manager_address}}"
"""
