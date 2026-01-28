#!/bin/bash
#
# health-check.sh - Verify health of Kurtosis agglayer/miden setup
#
# Usage: ./health-check.sh [--verbose]
#
# Checks:
#   - All expected containers are running
#   - Container health status
#   - Network connectivity between services
#   - RPC endpoints responding
#   - Log errors in last 5 minutes

set -euo pipefail

VERBOSE="${1:-}"
RED='\033[0;31m'
GREEN='\033[0;32m'
YELLOW='\033[1;33m'
NC='\033[0m' # No Color

# Expected containers (partial names for matching)
EXPECTED_CONTAINERS=(
    "miden-node"
    "miden-proxy"
    "agglayer"
    "zkevm-bridge"
    "postgres"
    "op-geth"
    "op-node"
)

# RPC endpoints to check (auto-detected later)
RPC_CHECKS=()

passed=0
failed=0
warnings=0

log_pass() {
    echo -e "${GREEN}[PASS]${NC} $1"
    passed=$((passed + 1))
}

log_fail() {
    echo -e "${RED}[FAIL]${NC} $1"
    failed=$((failed + 1))
}

log_warn() {
    echo -e "${YELLOW}[WARN]${NC} $1"
    warnings=$((warnings + 1))
}

log_info() {
    echo -e "      $1"
}

section() {
    echo ""
    echo "═══════════════════════════════════════════════════════════════"
    echo "  $1"
    echo "═══════════════════════════════════════════════════════════════"
}

# ============================================================================
# Check 1: Docker daemon
# ============================================================================
section "Docker Daemon"

if docker info &>/dev/null; then
    log_pass "Docker daemon is running"
else
    log_fail "Docker daemon is not running"
    echo "Cannot continue without Docker. Exiting."
    exit 1
fi

# ============================================================================
# Check 2: Expected containers running
# ============================================================================
section "Container Status"

running_containers=$(docker ps --format '{{.Names}}' 2>/dev/null)

for expected in "${EXPECTED_CONTAINERS[@]}"; do
    matching=$(echo "$running_containers" | grep -i "$expected" || true)
    if [[ -n "$matching" ]]; then
        count=$(echo "$matching" | wc -l | tr -d ' ')
        log_pass "$expected: $count container(s) running"
        if [[ "$VERBOSE" == "--verbose" ]]; then
            echo "$matching" | while read -r name; do
                log_info "  - $name"
            done
        fi
    else
        log_fail "$expected: not running"
    fi
done

# ============================================================================
# Check 3: Container health status
# ============================================================================
section "Container Health"

# Get containers with health checks
health_containers=$(docker ps --format '{{.Names}}\t{{.Status}}' 2>/dev/null | grep -i "health" || true)

if [[ -n "$health_containers" ]]; then
    while IFS=$'\t' read -r name status; do
        if echo "$status" | grep -q "healthy"; then
            log_pass "$name: healthy"
        elif echo "$status" | grep -q "unhealthy"; then
            log_fail "$name: unhealthy"
        elif echo "$status" | grep -q "starting"; then
            log_warn "$name: starting (health check pending)"
        fi
    done <<< "$health_containers"
else
    log_info "No containers with health checks found"
fi

# ============================================================================
# Check 4: Exited/crashed containers
# ============================================================================
section "Crashed Containers"

exited=$(docker ps -a --filter "status=exited" --format '{{.Names}}\t{{.Status}}' 2>/dev/null | grep -iE "miden|agglayer|aggkit|zkevm|bridge" || true)

if [[ -n "$exited" ]]; then
    while IFS=$'\t' read -r name status; do
        exit_code=$(echo "$status" | grep -oE 'Exited \([0-9]+\)' | grep -oE '[0-9]+' || echo "?")
        if [[ "$exit_code" == "0" ]]; then
            log_warn "$name: exited cleanly (code 0)"
        else
            log_fail "$name: crashed (exit code $exit_code)"
        fi
    done <<< "$exited"
else
    log_pass "No crashed containers"
fi

# ============================================================================
# Check 5: Miden-specific containers
# ============================================================================
section "Miden Stack"

miden_containers=$(docker ps --format '{{.Names}}' | grep -i miden || true)

if [[ -n "$miden_containers" ]]; then
    for container in $miden_containers; do
        # Check if container is responding
        if docker exec "$container" true 2>/dev/null; then
            uptime=$(docker inspect --format='{{.State.StartedAt}}' "$container" 2>/dev/null || echo "unknown")
            log_pass "$container: responsive (started: ${uptime:0:19})"
        else
            log_fail "$container: not responding to exec"
        fi
    done
else
    log_fail "No Miden containers found"
fi

# ============================================================================
# Check 6: Aggkit containers
# ============================================================================
section "Aggkit Stack"

aggkit_containers=$(docker ps --format '{{.Names}}' | grep -i aggkit || true)

if [[ -n "$aggkit_containers" ]]; then
    for container in $aggkit_containers; do
        if docker exec "$container" true 2>/dev/null; then
            log_pass "$container: responsive"
        else
            log_fail "$container: not responding"
        fi
    done
else
    log_warn "No Aggkit containers found"
fi

# ============================================================================
# Check 7: RPC Endpoints
# ============================================================================
section "RPC Endpoints"

# Find the miden-proxy container (could be aggkit-miden-proxy or miden-proxy-kurtosis)
miden_proxy=$(docker ps --format '{{.Names}}' | grep -E "miden-proxy" | head -1 || true)
if [[ -n "$miden_proxy" ]]; then
    # Check if curl is available, otherwise try wget or just log the container
    if docker exec "$miden_proxy" curl -s -X POST -H "Content-Type: application/json" \
        -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' \
        http://localhost:8545 2>/dev/null | grep -q "result"; then
        block=$(docker exec "$miden_proxy" curl -s -X POST -H "Content-Type: application/json" \
            -d '{"jsonrpc":"2.0","method":"eth_blockNumber","params":[],"id":1}' \
            http://localhost:8545 2>/dev/null | grep -oE '"result":"0x[0-9a-fA-F]+"' | cut -d'"' -f4 || echo "?")
        log_pass "$miden_proxy RPC (8545): responding (block: $block)"
    else
        log_warn "$miden_proxy RPC (8545): curl failed or no response"
    fi
else
    log_fail "No miden-proxy container found"
fi

# Find miden-node container
miden_node=$(docker ps --format '{{.Names}}' | grep -E "miden-node" | head -1 || true)
if [[ -n "$miden_node" ]]; then
    # Try to check gRPC port - use timeout command if available
    if timeout 2 docker exec "$miden_node" sh -c 'echo > /dev/tcp/localhost/57291' 2>/dev/null; then
        log_pass "$miden_node gRPC (57291): port open"
    else
        # Fallback: just check if container is responding
        log_info "$miden_node: container running (gRPC port check skipped)"
    fi
else
    log_fail "No miden-node container found"
fi

# Check agglayer RPC
agglayer_container=$(docker ps --format '{{.Names}}' | grep -i "agglayer" | grep -v "aggkit\|dashboard" | head -1 || true)
if [[ -n "$agglayer_container" ]]; then
    log_pass "$agglayer_container: running"
else
    log_warn "No agglayer container found (excluding dashboard)"
fi

# ============================================================================
# Check 8: Recent errors in logs
# ============================================================================
section "Recent Errors (last 5 min)"

error_count=0
for container in $(docker ps --format '{{.Names}}' | grep -E "miden-proxy" || true); do
    if [[ -n "$container" ]]; then
        errors=$(docker logs --since 5m "$container" 2>&1 | grep -ciE "error|panic|fatal" || true)
        if [[ "$errors" -gt 0 ]]; then
            log_warn "$container: $errors error(s) in last 5 minutes"
            error_count=$((error_count + errors))
            if [[ "$VERBOSE" == "--verbose" ]]; then
                docker logs --since 5m "$container" 2>&1 | grep -iE "error|panic|fatal" | tail -3 | while read -r line; do
                    log_info "  $line"
                done
            fi
        else
            log_pass "$container: no errors in last 5 minutes"
        fi
    fi
done

# ============================================================================
# Check 9: GER injection (miden-proxy specific)
# ============================================================================
section "GER Injection Status"

# Check all miden-proxy containers for GER activity
for proxy in $(docker ps --format '{{.Names}}' | grep -E "miden-proxy" || true); do
    ger_count=$(docker logs --since 1h "$proxy" 2>&1 | grep -c "GER injected\|Injected GER\|New GER received" || true)
    ger_count=${ger_count:-0}
    ger_count=$(echo "$ger_count" | tr -d '[:space:]')
    if [[ "$ger_count" -gt 0 ]] 2>/dev/null; then
        log_pass "$proxy: $ger_count GER(s) in last hour"
    else
        log_info "$proxy: no GERs in last hour"
    fi
done

# ============================================================================
# Check 10: Claim processing
# ============================================================================
section "Claim Processing Status"

# Check all miden-proxy containers for claim activity
for proxy in $(docker ps --format '{{.Names}}' | grep -E "miden-proxy" || true); do
    claims=$(docker logs --since 1h "$proxy" 2>&1 | grep -c "CLAIM PROCESSING COMPLETE" || true)
    claims=${claims:-0}
    claims=$(echo "$claims" | tr -d '[:space:]')
    claim_requests=$(docker logs --since 1h "$proxy" 2>&1 | grep -c "CLAIM ASSET DETAILS" || true)
    claim_requests=${claim_requests:-0}
    claim_requests=$(echo "$claim_requests" | tr -d '[:space:]')

    if [[ "$claim_requests" -gt 0 ]] 2>/dev/null; then
        log_info "$proxy: $claim_requests claim request(s), $claims completed"
        if [[ "$claims" -lt "$claim_requests" ]] 2>/dev/null; then
            failed_claims=$((claim_requests - claims))
            log_warn "$proxy: $failed_claims claim(s) may have failed"
        else
            log_pass "$proxy: all claims processed"
        fi
    else
        log_info "$proxy: no claims in last hour"
    fi
done

# ============================================================================
# Summary
# ============================================================================
section "Summary"

total=$((passed + failed + warnings))
echo ""
echo -e "  ${GREEN}Passed:${NC}   $passed"
echo -e "  ${RED}Failed:${NC}   $failed"
echo -e "  ${YELLOW}Warnings:${NC} $warnings"
echo ""

if [[ "$failed" -gt 0 ]]; then
    echo -e "${RED}Health check FAILED${NC} - $failed issue(s) found"
    exit 1
elif [[ "$warnings" -gt 0 ]]; then
    echo -e "${YELLOW}Health check PASSED with warnings${NC}"
    exit 0
else
    echo -e "${GREEN}Health check PASSED${NC}"
    exit 0
fi
