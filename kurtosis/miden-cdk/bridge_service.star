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
                    "rollup_address": contract_addresses.get("rollup_address", l1_bridge_address),
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
    Deploy aggkit aggoracle configured for Miden L2.

    The aggoracle injects Global Exit Root updates from L1 to L2.
    This is required for deposits to become claimable.
    """
    service_name = "aggkit" + deployment_suffix

    # Get aggoracle keystore from contracts service
    aggoracle_keystore = plan.store_service_files(
        name="aggoracle-keystore" + deployment_suffix,
        service_name="contracts" + deployment_suffix,
        src="/opt/keystores/aggoracle.keystore",
    )

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
                    "l1_chain_id": cdk_args.get("l1_chain_id", 1337),
                    "l2_chain_id": cdk_args.get("l2_chain_id", 2),
                    "l1_ger_address": contract_addresses.get("l1_ger_address", ""),
                    "l2_ger_address": contract_addresses.get("l2_ger_address", ""),
                    "l1_bridge_address": contract_addresses.get("l1_bridge_address", ""),
                    "l2_bridge_address": contract_addresses.get("l2_bridge_address", ""),
                    "rollup_manager_address": contract_addresses.get("rollup_manager_address", ""),
                    "rollup_manager_block_number": contract_addresses.get("rollup_manager_block_number", "1"),
                    "rollup_address": contract_addresses.get("rollup_address", contract_addresses.get("l1_bridge_address", "")),
                    "pol_token_address": contract_addresses.get("pol_token_address", ""),
                    "agglayer_url": cdk_args.get("agglayer_grpc_url", "http://agglayer" + deployment_suffix + ":4443"),
                    "l2_keystore_password": cdk_args.get("l2_keystore_password", ""),
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
                "/etc/aggkit": Directory(
                    artifact_names=[config_artifact, aggoracle_keystore],
                ),
            },
            # Run aggoracle (GER injection) and aggsender (L2→L1 certificate submission)
            cmd=["run", "--cfg=/etc/aggkit/config.toml", "--components=aggoracle,aggsender"],
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
# Rollup address registered in rollup manager
PolygonZkEVMAddress = "{{.rollup_address}}"
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

# Top-level mandatory parameters
PathRWData = "/tmp"
L1URL = "{{.l1_rpc_url}}"
L2URL = "{{.l2_rpc_url}}"
AggLayerURL = "{{.agglayer_url}}"
AggchainProofURL = ""
SequencerPrivateKeyPath = "/etc/aggkit/aggoracle.keystore"
SequencerPrivateKeyPassword = "{{.l2_keystore_password}}"

# Block numbers (top-level, resolved by aggkit config renderer)
rollupCreationBlockNumber = "{{.rollup_manager_block_number}}"
rollupManagerCreationBlockNumber = "{{.rollup_manager_block_number}}"
genesisBlockNumber = "{{.rollup_manager_block_number}}"

[Log]
Level = "{{.log_level}}"

# L1 configuration
[L1Config]
URL = "{{.l1_rpc_url}}"
chainId = "{{.l1_chain_id}}"
polygonZkEVMGlobalExitRootAddress = "{{.l1_ger_address}}"
polygonRollupManagerAddress = "{{.rollup_manager_address}}"
polTokenAddress = "{{.pol_token_address}}"
# Rollup address from create_rollup_output.json (registered in rollup manager)
polygonZkEVMAddress = "{{.rollup_address}}"
BridgeAddr = "{{.l1_bridge_address}}"

# L2 configuration
[L2Config]
GlobalExitRootAddr = "{{.l2_ger_address}}"
BridgeAddr = "{{.l2_bridge_address}}"

# AggOracle - injects GER updates from L1 to L2
[AggOracle]
WaitPeriodNextGER = "5s"
EnableAggOracleCommittee = false

[AggOracle.EVMSender]
GlobalExitRootL2 = "{{.l2_ger_address}}"
WaitPeriodMonitorTx = "5s"

[AggOracle.EVMSender.EthTxManager]
FrequencyToMonitorTxs = "1s"
WaitTxToBeMined = "2m"
GasPriceMarginFactor = 1
MaxGasPriceLimit = 0
ForcedGas = 0

[[AggOracle.EVMSender.EthTxManager.PrivateKeys]]
Path = "/etc/aggkit/aggoracle.keystore"
Password = "{{.l2_keystore_password}}"

[AggOracle.EVMSender.EthTxManager.Etherman]
URL = "{{.l2_rpc_url}}"
L1ChainID = {{.l2_chain_id}}

# AggSender — submits certificates to AggLayer for L2→L1 bridging
[AggSender]
AggSenderPrivateKey = {Path = "/etc/aggkit/aggoracle.keystore", Password = "{{.l2_keystore_password}}"}
Mode = "PessimisticProof"
CheckStatusCertificateInterval = "1s"
TriggerCertMode = "ASAP"

[AggSender.StorageRetainCertificatesPolicy]
RetryCertAfterInError = true

[AggSender.AggkitProverClient]
UseTLS = false

[AggSender.AgglayerClient]

[[AggSender.AgglayerClient.APIRateLimits]]
MethodName = "SendCertificate"

[AggSender.AgglayerClient.APIRateLimits.RateLimit]
NumRequests = 0

[AggSender.AgglayerClient.GRPC]
URL = "{{.agglayer_url}}"
MinConnectTimeout = "5s"
RequestTimeout = "300s"
UseTLS = false

[AggSender.AgglayerClient.GRPC.Retry]
InitialBackoff = "1s"
MaxBackoff = "10s"
BackoffMultiplier = 2.0
MaxAttempts = 20

# BridgeL2Sync — syncs BridgeEvent logs from L2 proxy for AggSender deposit tree
[BridgeL2Sync]
BridgeAddr = "{{.l2_bridge_address}}"
BlockFinality = "LatestBlock"
# Disable debug_traceTransaction calls — Miden proxy synthetic txns don't have call traces
SyncFromInBridges = "false"

[ReorgDetectorL2]
FinalizedBlock = "LatestBlock"

# L1InfoTreeSync — syncs L1 info tree for certificate building
[L1InfoTreeSync]
InitialBlock = "{{.rollup_manager_block_number}}"

# L2GERSync — syncs L2 GER for AggSender
[L2GERSync]
BlockFinality = "LatestBlock"
"""
