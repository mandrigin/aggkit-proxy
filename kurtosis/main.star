"""
Miden RPC Proxy - Kurtosis Package

This package sets up the complete test topology for the Miden RPC Proxy:
- PostgreSQL database
- Miden node (devnet)
- L1 devnet (Anvil)
- Bridge service
- Proxy service
- Test runner
"""

miden_proxy = import_module("./miden_proxy.star")

def run(plan, args={}):
    """
    Main entrypoint for the Kurtosis package.

    Args:
        plan: Kurtosis plan object
        args: Optional configuration:
            - run_tests: bool - Whether to run tests after setup (default: True)
            - test_phase: str - Which test phase to run (default: "all")
    """
    run_tests = args.get("run_tests", True)
    test_phase = args.get("test_phase", "all")

    # Deploy the full topology
    services = miden_proxy.deploy_topology(plan)

    # Run tests if requested
    if run_tests:
        miden_proxy.run_tests(plan, services, test_phase)

    return services
