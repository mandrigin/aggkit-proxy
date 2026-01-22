"""
Aggkit Proxy for Miden - Kurtosis CDK Integration Module

This module integrates the Miden RPC proxy with kurtosis-cdk infrastructure,
enabling the zkevm-bridge-service to communicate with Miden via Ethereum-compatible
JSON-RPC endpoints.

Port assignments (per kurtosis-cdk conventions):
- HTTP RPC: 8123
- WebSocket: 8133
- Metrics: 9123

Usage in kurtosis-cdk main.star:
    miden_aggkit = import_module("./aggkit-proxy-miden.star")

    if deployment_stages.get("deploy_miden_integration", False):
        miden_aggkit.run(plan, args, contract_setup_addresses)
"""

# Default port assignments matching kurtosis-cdk conventions
DEFAULT_HTTP_PORT = 8123
DEFAULT_WS_PORT = 8133
DEFAULT_METRICS_PORT = 9123

# Service naming follows kurtosis-cdk pattern: {service}-{rollup_idx}
SERVICE_NAME_TEMPLATE = "aggkit-proxy-miden-{0}"

# Default Miden node configuration
DEFAULT_MIDEN_NODE_GRPC_PORT = 443
DEFAULT_MIDEN_NETWORK_ID = 2  # Assigned by Agglayer for Miden

def run(plan, args, contract_setup_addresses=None):
    """
    Deploy the Aggkit Proxy for Miden within kurtosis-cdk.

    Args:
        plan: Kurtosis plan object
        args: Configuration from input_parser, expects:
            - aggkit_proxy_miden_image: Docker image for proxy
            - miden_node_url: URL of Miden node gRPC endpoint
            - miden_network_id: Network ID assigned by Agglayer (default: 2)
            - rollup_idx: Index for service naming (default: "001")
        contract_setup_addresses: Bridge contract addresses from CDK deployment

    Returns:
        dict: Service information including URLs and ports
    """
    rollup_idx = args.get("rollup_idx", "001")
    service_name = SERVICE_NAME_TEMPLATE.format(rollup_idx)

    # Get configuration
    proxy_image = args.get("aggkit_proxy_miden_image", "ghcr.io/0xmiden/aggkit-proxy:latest")
    miden_node_url = args.get("miden_node_url", "https://miden-node-001:443")
    miden_network_id = args.get("miden_network_id", DEFAULT_MIDEN_NETWORK_ID)
    http_port = args.get("aggkit_proxy_miden_http_port", DEFAULT_HTTP_PORT)
    ws_port = args.get("aggkit_proxy_miden_ws_port", DEFAULT_WS_PORT)
    metrics_port = args.get("aggkit_proxy_miden_metrics_port", DEFAULT_METRICS_PORT)

    # Bridge configuration
    bridge_address = ""
    if contract_setup_addresses:
        bridge_address = contract_setup_addresses.get("polygon_bridge_address", "")

    # L1 RPC URL (from kurtosis-cdk L1 deployment)
    l1_rpc_url = args.get("l1_rpc_url", "http://el-1-geth-lighthouse:8545")

    # Generate config file
    config_content = _generate_config(
        http_port=http_port,
        ws_port=ws_port,
        metrics_port=metrics_port,
        miden_node_url=miden_node_url,
        miden_network_id=miden_network_id,
        bridge_address=bridge_address,
        l1_rpc_url=l1_rpc_url,
    )

    config_artifact = plan.render_templates(
        name="{}-config".format(service_name),
        config={
            "config.toml": struct(
                template=config_content,
                data={},
            ),
        },
    )

    # Deploy the proxy service
    service = plan.add_service(
        name=service_name,
        config=ServiceConfig(
            image=proxy_image,
            ports={
                "http-rpc": PortSpec(
                    number=http_port,
                    transport_protocol="TCP",
                    application_protocol="http",
                ),
                "ws-rpc": PortSpec(
                    number=ws_port,
                    transport_protocol="TCP",
                    application_protocol="ws",
                ),
                "metrics": PortSpec(
                    number=metrics_port,
                    transport_protocol="TCP",
                    application_protocol="http",
                ),
            },
            files={
                "/app": config_artifact,
            },
            env_vars={
                "RUST_LOG": args.get("log_level", "info"),
                "CONFIG_PATH": "/app/config.toml",
            },
            cmd=["--config", "/app/config.toml"],
            min_cpu=500,
            min_memory=512,
        ),
    )

    # Wait for service to be ready
    plan.wait(
        service_name=service_name,
        recipe=PostHttpRequestRecipe(
            port_id="http-rpc",
            endpoint="/",
            content_type="application/json",
            body='{"jsonrpc":"2.0","method":"eth_chainId","params":[],"id":1}',
        ),
        field="code",
        assertion="==",
        target_value=200,
        timeout="120s",
    )

    plan.print("Aggkit Proxy for Miden deployed: {}".format(service_name))
    plan.print("  HTTP RPC: http://{}:{}".format(service_name, http_port))
    plan.print("  WebSocket: ws://{}:{}".format(service_name, ws_port))

    return {
        "service_name": service_name,
        "http_url": "http://{}:{}".format(service_name, http_port),
        "ws_url": "ws://{}:{}".format(service_name, ws_port),
        "metrics_url": "http://{}:{}".format(service_name, metrics_port),
        "network_id": miden_network_id,
    }


def get_bridge_config_overrides(proxy_info, network_id=None):
    """
    Get configuration overrides for the bridge service to connect to Miden proxy.

    Use this to update the bridge-config.toml template in kurtosis-cdk.

    Args:
        proxy_info: Return value from run()
        network_id: Override network ID (default: use proxy_info value)

    Returns:
        dict: Configuration values for bridge service
    """
    net_id = network_id if network_id else proxy_info.get("network_id", DEFAULT_MIDEN_NETWORK_ID)

    return {
        "l2_rpc_url": proxy_info["http_url"],
        "l2_ws_url": proxy_info["ws_url"],
        "network_id": net_id,
        # Bridge service Etherman config
        "etherman_l2_urls": [proxy_info["http_url"]],
        # Agglayer full-node-rpcs entry
        "agglayer_rpc_entry": '{} = "{}"'.format(net_id, proxy_info["http_url"]),
    }


def get_agglayer_config_entry(proxy_info):
    """
    Get the agglayer configuration entry for the Miden network.

    Add this to templates/bridge-infra/agglayer-config.toml under [full-node-rpcs].

    Args:
        proxy_info: Return value from run()

    Returns:
        str: TOML entry for agglayer config
    """
    return '{} = "{}"'.format(
        proxy_info.get("network_id", DEFAULT_MIDEN_NETWORK_ID),
        proxy_info["http_url"],
    )


def _generate_config(http_port, ws_port, metrics_port, miden_node_url, miden_network_id, bridge_address, l1_rpc_url):
    """Generate the proxy configuration file content."""
    return """# Aggkit Proxy for Miden - Kurtosis CDK Configuration
# Auto-generated by aggkit-proxy-miden.star

[server]
# HTTP RPC endpoint (Ethereum-compatible JSON-RPC)
http_port = {http_port}
http_host = "0.0.0.0"

# WebSocket endpoint for subscriptions
ws_port = {ws_port}
ws_host = "0.0.0.0"

# Metrics endpoint (Prometheus)
metrics_port = {metrics_port}

[miden]
# Miden node gRPC endpoint
rpc_url = "{miden_node_url}"

# Network ID assigned by Agglayer
network_id = {miden_network_id}

# Chain ID to report via eth_chainId (hex-encoded network_id)
chain_id = "0x{miden_network_id:x}"

[bridge]
# Bridge contract address on L1 (PolygonZkEVMBridgeV2)
contract_address = "{bridge_address}"

# Bridge event topic for deposit detection
# BridgeEvent topic: 0x501781209a1f8899323b96b4ef08b168df93e0a90c673d1e4cce39366cb62f9b
bridge_event_topic = "0x501781209a1f8899323b96b4ef08b168df93e0a90c673d1e4cce39366cb62f9b"

[l1]
# L1 Ethereum RPC endpoint
rpc_url = "{l1_rpc_url}"

[sync]
# Polling interval for eth_blockNumber (matches bridge service SyncInterval)
poll_interval_secs = 10

# Maximum logs per eth_getLogs response
max_logs_per_request = 1000

[logging]
level = "info"
format = "json"
""".format(
        http_port=http_port,
        ws_port=ws_port,
        metrics_port=metrics_port,
        miden_node_url=miden_node_url,
        miden_network_id=miden_network_id,
        bridge_address=bridge_address,
        l1_rpc_url=l1_rpc_url,
    )


def deploy_with_miden_node(plan, args):
    """
    Deploy both Miden node and Aggkit proxy together.

    Use this for standalone testing or when Miden node is not already deployed
    in the kurtosis-cdk environment.

    Args:
        plan: Kurtosis plan object
        args: Configuration options

    Returns:
        dict: Service information for both Miden node and proxy
    """
    rollup_idx = args.get("rollup_idx", "001")
    miden_node_name = "miden-node-{}".format(rollup_idx)
    miden_node_port = args.get("miden_node_grpc_port", DEFAULT_MIDEN_NODE_GRPC_PORT)
    miden_node_image = args.get("miden_node_image", "ghcr.io/0xmiden/miden-node:latest")

    # Deploy Miden node
    miden_service = plan.add_service(
        name=miden_node_name,
        config=ServiceConfig(
            image=miden_node_image,
            ports={
                "grpc": PortSpec(
                    number=miden_node_port,
                    transport_protocol="TCP",
                ),
            },
            cmd=["--network", "testnet"],
        ),
    )

    # Wait for Miden node to be ready
    plan.wait(
        service_name=miden_node_name,
        recipe=ExecRecipe(command=["curl", "-sf", "http://localhost:{}/health".format(miden_node_port)]),
        field="code",
        assertion="==",
        target_value=0,
        timeout="120s",
    )

    # Update args with Miden node URL and deploy proxy
    updated_args = dict(args)
    updated_args["miden_node_url"] = "https://{}:{}".format(miden_node_name, miden_node_port)

    proxy_info = run(plan, updated_args)

    return {
        "miden_node": {
            "service_name": miden_node_name,
            "grpc_url": "https://{}:{}".format(miden_node_name, miden_node_port),
        },
        "proxy": proxy_info,
    }
