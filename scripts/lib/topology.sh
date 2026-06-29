#!/usr/bin/env bash
# Shared topology resolver for aggkit-proxy manual-ops scripts.
#
# Two local stacks are supported; every value is overridable via env vars:
#   - kurtosis   miden-cdk enclave              (network id 2, miden-*-001 services)
#   - compose    miden-agglayer docker-compose  (network id 1, miden-agglayer-* containers)
#
# Force a profile with TOPOLOGY=kurtosis|compose; otherwise it is auto-detected
# from the running containers (compose wins when miden-agglayer-* is up).
#
# Source this file, then use the exported vars + helpers:
#   TOPOLOGY MIDEN_NETWORK_ID L1_RPC L2_RPC BRIDGE_SERVICE_URL
#   MIDEN_NODE_CONTAINER MIDEN_NODE_DATA_DIR BRIDGE_PG_CONTAINER AGGLAYER_PROXY_CONTAINER
#   topology_cid <name>     -> resolves a service name/substring to a running container ID
#   topology_node_db        -> path to the miden-node sqlite store inside the node container

_topo_has() { docker ps --format '{{.Names}}' 2>/dev/null | grep -qE "$1"; }

if [[ -z "${TOPOLOGY:-}" || "${TOPOLOGY}" == "auto" ]]; then
    if _topo_has '^miden-agglayer-'; then TOPOLOGY=compose
    elif _topo_has 'miden-node-001';  then TOPOLOGY=kurtosis
    else TOPOLOGY=kurtosis; fi
fi

case "$TOPOLOGY" in
  compose)
    : "${MIDEN_NETWORK_ID:=1}"
    : "${L1_RPC:=http://localhost:8545}"
    : "${L2_RPC:=http://localhost:8546}"
    : "${BRIDGE_SERVICE_URL:=http://localhost:18080}"
    : "${MIDEN_NODE_CONTAINER:=miden-agglayer-miden-node-1}"
    : "${MIDEN_NODE_DATA_DIR:=/data/node}"
    : "${BRIDGE_PG_CONTAINER:=miden-agglayer-postgres-1}"
    : "${AGGLAYER_PROXY_CONTAINER:=miden-agglayer-miden-agglayer-1}"
    ;;
  kurtosis)
    : "${MIDEN_NETWORK_ID:=2}"
    : "${L1_RPC:=}"   # dynamic — resolve via: kurtosis port print miden-cdk el-1-geth-lighthouse rpc
    : "${L2_RPC:=http://localhost:8546}"
    : "${BRIDGE_SERVICE_URL:=http://localhost:5579}"
    : "${MIDEN_NODE_CONTAINER:=miden-node-001}"
    : "${MIDEN_NODE_DATA_DIR:=/app/data}"
    : "${BRIDGE_PG_CONTAINER:=postgres-001}"
    : "${AGGLAYER_PROXY_CONTAINER:=miden-proxy-001}"
    ;;
  *) echo "topology.sh: unknown TOPOLOGY='$TOPOLOGY' (use kurtosis|compose|auto)" >&2
     return 1 2>/dev/null || exit 1 ;;
esac

export TOPOLOGY MIDEN_NETWORK_ID L1_RPC L2_RPC BRIDGE_SERVICE_URL \
       MIDEN_NODE_CONTAINER MIDEN_NODE_DATA_DIR BRIDGE_PG_CONTAINER AGGLAYER_PROXY_CONTAINER

# Resolve a service name/substring to a single running container ID.
# Works for both the compose exact names and kurtosis' uuid-suffixed names.
topology_cid() { docker ps --filter "name=$1" --format '{{.ID}}' | head -1; }

# Path to the miden-node sqlite store inside the node container (override: MIDEN_NODE_DB).
# Discovered at runtime so it survives node-image layout changes between versions.
topology_node_db() {
    if [[ -n "${MIDEN_NODE_DB:-}" ]]; then echo "$MIDEN_NODE_DB"; return; fi
    local cid; cid=$(topology_cid "$MIDEN_NODE_CONTAINER")
    [[ -z "$cid" ]] && return 1
    docker exec "$cid" sh -c \
        "find '${MIDEN_NODE_DATA_DIR}' /app/data /data -name 'miden-store.sqlite3' 2>/dev/null | head -1"
}
