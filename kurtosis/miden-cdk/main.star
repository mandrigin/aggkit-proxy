"""
Miden-CDK Kurtosis Package

Provisions Miden bridge infrastructure without post-hoc patching:
1. L1 + Agglayer + Bridge contracts (via kurtosis-cdk)
2. Miden node + proxy (replaces OP-Geth L2)
3. Bridge services configured to use Miden proxy

This eliminates the ~600 lines of shell post-provisioning in e2e-test.sh.
"""

# Import kurtosis-cdk modules for L1/agglayer infrastructure
kurtosis_cdk = import_module("github.com/0xPolygon/kurtosis-cdk/main.star")
kurtosis_cdk_input_parser = import_module(
    "github.com/0xPolygon/kurtosis-cdk/src/package_io/input_parser.star"
)
kurtosis_cdk_constants = import_module(
    "github.com/0xPolygon/kurtosis-cdk/src/package_io/constants.star"
)
kurtosis_cdk_contracts = import_module(
    "github.com/0xPolygon/kurtosis-cdk/src/contracts/util.star"
)

# Local modules
miden_services = import_module("./miden_services.star")
bridge_service = import_module("./bridge_service.star")

# Docker Desktop grouping label
DOCKER_PROJECT_LABEL = "com.docker.compose.project"
MIDEN_PROJECT_GROUP = "miden"


def get_contract_addresses(plan):
    """
    Extract contract addresses from combined.json with correct field names.
    The upstream kurtosis-cdk uses outdated field names, so we do it ourselves.
    """
    result = plan.exec(
        service_name="contracts-001",
        recipe=ExecRecipe(
            command=["/bin/sh", "-c", "cat /opt/output/combined.json"],
            extract={
                "admin_address": "fromjson | .admin",
                "rollup_manager_address": "fromjson | .polygonRollupManagerAddress",
                "l1_bridge_address": "fromjson | .polygonZkEVMBridgeAddress",
                "l1_ger_address": "fromjson | .polygonZkEVMGlobalExitRootAddress",
                "agglayer_gateway_address": "fromjson | .aggLayerGatewayAddress",
                "pol_token_address": "fromjson | .polTokenAddress",
                "rollup_manager_block_number": "fromjson | .deploymentRollupManagerBlockNumber",
                # Rollup address from create_rollup_output.json (merged into combined.json)
                "rollup_address": "fromjson | .rollupAddress",
                # L2 addresses (same as L1 for unified bridge)
                "l2_bridge_address": "fromjson | .polygonZkEVML2BridgeAddress",
                "l2_ger_address": "fromjson | .LegacyAgglayerGERL2",
            },
        ),
        description="Getting contract addresses from combined.json",
    )
    return {
        "admin_address": result["extract.admin_address"],
        "rollup_manager_address": result["extract.rollup_manager_address"],
        "l1_bridge_address": result["extract.l1_bridge_address"],
        "l1_ger_address": result["extract.l1_ger_address"],
        "agglayer_gateway_address": result["extract.agglayer_gateway_address"],
        "pol_token_address": result["extract.pol_token_address"],
        "rollup_manager_block_number": result["extract.rollup_manager_block_number"],
        "rollup_address": result["extract.rollup_address"],
        "l2_bridge_address": result["extract.l2_bridge_address"],
        "l2_ger_address": result["extract.l2_ger_address"],
    }


# Default deployment stages - skip OP Stack, deploy Miden instead
DEFAULT_DEPLOYMENT_STAGES = {
    "deploy_l1": True,
    "deploy_agglayer_contracts_on_l1": True,
    "deploy_databases": True,
    "deploy_agglayer": True,
    # Skip OP-Geth deployment - we use Miden instead
    "deploy_cdk_central_environment": False,
    "deploy_cdk_bridge_infra": False,  # We deploy our own bridge config
    "deploy_op_succinct": False,
    "deploy_l2_contracts": True,  # Deploys sovereign L2 contracts (GER, bridge proxies)
    "deploy_aggkit_node": False,
}

# Miden-specific defaults
MIDEN_DEFAULTS = {
    # Miden network ID (assigned by Agglayer)
    "miden_network_id": 2,
    "miden_chain_id": 2,
    # Miden node configuration
    "miden_node_image": "miden-infra/miden-node:agglayer-v0.1",
    "miden_node_port": 57291,
    # Miden proxy configuration (miden-agglayer)
    "miden_proxy_image": "miden-infra/miden-proxy:latest",
    "miden_proxy_port": 8546,
    "miden_proxy_external_port": 8123,
    # Aggkit deployment (enabled - required for GER injection)
    # The aggoracle component injects Global Exit Root updates from L1 to L2
    # This is required for deposits to become claimable
    "deploy_aggkit": True,
    # Standalone L2->L1 auto-claimer (sponsors claimAsset on L1). Opt-in:
    # enable with `miden: {deploy_autoclaimer: true}`.
    "deploy_autoclaimer": False,
    # Web UI for sending deposits (bridge ETH from L1 to Miden)
    "deploy_web_ui": True,
    # pgweb for DB browsing (optional)
    "deploy_pgweb": True,
    "pgweb_port": 8082,
}


def run(plan, args={}):
    """
    Main entrypoint for Miden-CDK deployment.

    Args:
        plan: Kurtosis plan object
        args: Configuration options (merged with defaults)

    Returns:
        dict: Service information including URLs and ports
    """
    # Merge user args with defaults
    deployment_stages = DEFAULT_DEPLOYMENT_STAGES | args.get("deployment_stages", {})
    miden_args = MIDEN_DEFAULTS | args.get("miden", {})

    # Prepare kurtosis-cdk args
    # Override sequencer_type: with "op-geth" (default), rollup creation is deferred
    # to deploy_cdk_central_environment (which we skip). Setting to "cdk-erigon"
    # makes 4_createRollup.ts run during deploy_agglayer_contracts_on_l1 instead.
    miden_cdk_overrides = {
        "sequencer_type": "cdk-erigon",
        # Point AggLayer's full-node-rpc to our Miden proxy (not non-existent cdk-erigon)
        "l2_fullnode_rpc_url": "http://miden-proxy{}:{}".format(
            miden_args.get("deployment_suffix", "-001"),
            miden_args.get("miden_proxy_port", 8546),
        ),
    }
    miden_cdk_overrides.update(args.get("args", {}))
    cdk_args = {
        "deployment_stages": deployment_stages,
        "args": miden_cdk_overrides,
    }

    plan.print("=== Miden-CDK Deployment ===")
    plan.print("Deployment stages: " + str(deployment_stages))

    # Step 1: Deploy L1 + Agglayer infrastructure via kurtosis-cdk
    # This gives us L1 chain, agglayer, and contract addresses
    plan.print("Step 1: Deploying L1 + Agglayer infrastructure...")

    # Get parsed args from kurtosis-cdk for contract addresses
    (parsed_stages, parsed_args, op_args) = kurtosis_cdk_input_parser.parse_args(
        plan, cdk_args
    )

    # Deploy L1 using kurtosis-cdk's L1 launcher
    l1_launcher = import_module(
        "github.com/0xPolygon/kurtosis-cdk/src/l1/launcher.star"
    )
    l1_context = None
    if deployment_stages.get("deploy_l1", False):
        plan.print("Deploying local L1...")
        l1_context = l1_launcher.launch(plan, parsed_args)
    else:
        l1_context = struct(
            chain_id=parsed_args.get("l1_chain_id"),
            rpc_url=parsed_args.get("l1_rpc_url"),
            all_participants=[],
        )

    # Deploy contracts on L1
    contract_setup_addresses = {}
    if deployment_stages.get("deploy_agglayer_contracts_on_l1", False):
        plan.print("Deploying agglayer contracts on L1...")
        agglayer_contracts = import_module(
            "github.com/0xPolygon/kurtosis-cdk/src/contracts/agglayer.star"
        )
        agglayer_contracts.run(plan, parsed_args, deployment_stages, op_args)
        # Use our own address extraction with correct field names
        contract_setup_addresses = get_contract_addresses(plan)

    # Deploy databases
    if deployment_stages.get("deploy_databases", False):
        plan.print("Deploying databases...")
        databases = import_module(
            "github.com/0xPolygon/kurtosis-cdk/src/chain/shared/databases.star"
        )
        databases.run(plan, parsed_args)

    # Deploy agglayer
    if deployment_stages.get("deploy_agglayer", False):
        plan.print("Deploying agglayer...")
        agglayer = import_module("github.com/0xPolygon/kurtosis-cdk/src/agglayer.star")
        agglayer.run(plan, deployment_stages, parsed_args, contract_setup_addresses)

    # Step 2: Deploy Miden services (node + proxy)
    plan.print("Step 2: Deploying Miden services...")
    miden_context = miden_services.deploy(
        plan,
        miden_args,
        contract_setup_addresses,
        parsed_args,
    )

    # Step 3: Deploy bridge service configured to use Miden proxy
    plan.print("Step 3: Deploying bridge services with Miden L2...")
    bridge_context = bridge_service.deploy(
        plan,
        parsed_args,
        contract_setup_addresses,
        miden_context,
        deploy_aggkit=miden_args.get("deploy_aggkit", False),
        deploy_autoclaimer=miden_args.get("deploy_autoclaimer", False),
    )

    # Step 4: Deploy optional services (pgweb)
    if miden_args.get("deploy_pgweb", False):
        plan.print("Step 4: Deploying pgweb for DB browsing...")
        _deploy_pgweb(plan, parsed_args, miden_args)

    # Print summary
    _print_summary(plan, l1_context, miden_context, bridge_context, miden_args)

    return {
        "l1": l1_context,
        "miden": miden_context,
        "bridge": bridge_context,
        "contract_addresses": contract_setup_addresses,
    }


def _deploy_pgweb(plan, args, miden_args):
    """Deploy pgweb for browsing bridge database."""
    deployment_suffix = args.get("deployment_suffix", "-001")

    plan.add_service(
        name="pgweb" + deployment_suffix,
        config=ServiceConfig(
            image="sosedoff/pgweb",
            ports={
                "http": PortSpec(
                    number=8081,
                    transport_protocol="TCP",
                    application_protocol="http",
                ),
            },
            cmd=[
                "--bind=0.0.0.0",
                "--listen=8081",
                "--host=postgres" + deployment_suffix,
                "--user=bridge_user",
                "--pass=redacted",
                "--db=bridge_db",
                "--ssl=disable",
            ],
            # Docker Desktop grouping label
            labels={
                DOCKER_PROJECT_LABEL: MIDEN_PROJECT_GROUP,
            },
        ),
    )


def _print_summary(plan, l1_context, miden_context, bridge_context, miden_args):
    """Print deployment summary."""
    plan.print("")
    plan.print("=== Miden-CDK Deployment Complete ===")
    plan.print("")
    plan.print("Services:")
    plan.print("  L1 RPC: " + str(l1_context.rpc_url if l1_context else "N/A"))
    plan.print("  Miden Node: " + str(miden_context.get("node_url", "N/A")))
    plan.print("  Miden Proxy: " + str(miden_context.get("proxy_url", "N/A")))
    plan.print("  Bridge Service: " + str(bridge_context.get("rpc_url", "N/A")))
    if miden_context.get("web_ui_url"):
        plan.print("  Bridge Web UI: " + str(miden_context.get("web_ui_url")))
    if miden_args.get("deploy_pgweb", False):
        plan.print(
            "  pgweb (DB): http://localhost:" + str(miden_args.get("pgweb_port", 8082))
        )
    plan.print("")
    plan.print("Test proxy:")
    plan.print(
        "  curl -X POST "
        + str(miden_context.get("proxy_url", ""))
        + ' -H "Content-Type: application/json" -d \'{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}\''
    )
    plan.print("")
