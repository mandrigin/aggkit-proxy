# Miden Integration Tests
# Entry point: make test-phase1

SHELL := /bin/bash
.PHONY: test test-phase1 test-phase2 test-phase3 test-all setup clean

# Default Miden node endpoint (override with MIDEN_NODE_URL)
MIDEN_NODE_URL ?= http://localhost:57291

# Test runner
PYTEST := python -m pytest
PYTEST_OPTS := -v --tb=short

# Setup Python environment
setup:
	python -m venv .venv
	.venv/bin/pip install -r requirements.txt

# Run all tests
test-all: test-phase1 test-phase2 test-phase3

# Phase 1: Miden Standalone Tests (no L1 interaction)
# TC-1.1 through TC-1.7
test-phase1:
	@echo "=== Phase 1: Miden Standalone Tests ==="
	@echo "Miden Node: $(MIDEN_NODE_URL)"
	$(PYTEST) $(PYTEST_OPTS) tests/phase1/ -m "phase1"

# Phase 2: CLAIM Notes Tests (L1 + Miden interaction)
test-phase2:
	@echo "=== Phase 2: CLAIM Notes Tests ==="
	$(PYTEST) $(PYTEST_OPTS) tests/phase2/ -m "phase2"

# Phase 3: Full Integration Tests
test-phase3:
	@echo "=== Phase 3: Full Integration Tests ==="
	$(PYTEST) $(PYTEST_OPTS) tests/phase3/ -m "phase3"

# Clean up
clean:
	rm -rf .venv __pycache__ .pytest_cache
	find . -type d -name "__pycache__" -exec rm -rf {} + 2>/dev/null || true
	find . -type f -name "*.pyc" -delete 2>/dev/null || true
