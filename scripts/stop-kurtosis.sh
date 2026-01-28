#!/usr/bin/env bash
#
# Stop Miden-CDK Kurtosis enclave
#
set -euo pipefail

ENCLAVE_NAME="${1:-miden-cdk}"

echo "Stopping enclave: $ENCLAVE_NAME"
kurtosis enclave rm "$ENCLAVE_NAME" --force
echo "Done."
