# Miden Integration Tests
# Entry point: make test (runs all), make test-phase1, etc.

SHELL := /bin/bash
.PHONY: test test-phase1 test-phase2 test-phase3 build clean

# Default Miden node endpoint (override with MIDEN_NODE_URL)
export MIDEN_NODE_URL ?= http://localhost:57291

# Cargo options
CARGO := cargo
CARGO_TEST_OPTS := --release

# Build the test binaries
build:
	$(CARGO) build $(CARGO_TEST_OPTS)

# Run all tests
test: test-phase1 test-phase2 test-phase3

# Phase 1: Miden Standalone Tests (no L1 interaction)
# TC-1.1 through TC-1.7
test-phase1:
	@echo "=== Phase 1: Miden Standalone Tests ==="
	@echo "Miden Node: $(MIDEN_NODE_URL)"
	$(CARGO) test $(CARGO_TEST_OPTS) --test phase1 -- --nocapture

# Phase 2: CLAIM Notes Tests (L1 + Miden interaction)
# TC-2.1 through TC-2.5
test-phase2:
	@echo "=== Phase 2: CLAIM Notes Tests ==="
	@echo "Miden Node: $(MIDEN_NODE_URL)"
	$(CARGO) test $(CARGO_TEST_OPTS) --test phase2 -- --nocapture

# Phase 3: Full Integration Tests
# TC-3.1 through TC-3.9
test-phase3:
	@echo "=== Phase 3: Full Integration Tests ==="
	@echo "Miden Node: $(MIDEN_NODE_URL)"
	$(CARGO) test $(CARGO_TEST_OPTS) --test phase3 -- --nocapture

# Clean build artifacts
clean:
	$(CARGO) clean
	rm -rf target/
