# Phase 2 CLAIM Notes Tests
# Entry point: make test-phase2

SHELL := /bin/bash
.PHONY: test test-phase2 setup clean

# Default endpoints (override with environment variables)
MIDEN_NODE_URL ?= http://localhost:57291
PROXY_URL ?= http://localhost:8545

# Test runner
PYTEST := python -m pytest
PYTEST_OPTS := -v --tb=short

# Setup Python environment
setup:
	python -m venv .venv
	.venv/bin/pip install -r requirements.txt

# Run Phase 2 tests
test-phase2:
	@echo "=== Phase 2: CLAIM Notes Tests ==="
	@echo "Miden Node: $(MIDEN_NODE_URL)"
	@echo "Proxy: $(PROXY_URL)"
	MIDEN_NODE_URL=$(MIDEN_NODE_URL) PROXY_URL=$(PROXY_URL) \
		$(PYTEST) $(PYTEST_OPTS) tests/phase2/ -m "phase2"

test: test-phase2

# Clean up
clean:
	rm -rf .venv __pycache__ .pytest_cache
	find . -type d -name "__pycache__" -exec rm -rf {} + 2>/dev/null || true
	find . -type f -name "*.pyc" -delete 2>/dev/null || true
