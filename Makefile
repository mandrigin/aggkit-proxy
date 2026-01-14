# Miden Integration Tests
# Entry points: make test-phase1-py (Python), make test-phase1-rs (Rust)

SHELL := /bin/bash
.PHONY: build test test-phase1 test-phase1-py test-phase1-rs test-phase2 test-phase2-py test-phase2-rs test-phase3 test-phase3-py test-phase3-rs test-all test-docker setup clean check fmt lint dev

# Default Miden node endpoint (override with MIDEN_NODE_URL)
MIDEN_NODE_URL ?= http://localhost:57291

# Python test runner
PYTEST := python -m pytest
PYTEST_OPTS := -v --tb=short

# ============= BUILD TARGETS (Rust) =============

build:
	cargo build

release:
	cargo build --release

# ============= TEST TARGETS =============

# Run all tests (both Python and Rust)
test-all: test-phase1 test-phase2 test-phase3

test: test-all

# Phase 1: Miden Standalone Tests
test-phase1: test-phase1-py test-phase1-rs

test-phase1-py:
	@echo "=== Phase 1: Miden Standalone Tests (Python) ==="
	@echo "Miden Node: $(MIDEN_NODE_URL)"
	$(PYTEST) $(PYTEST_OPTS) tests/phase1/ -m "phase1"

test-phase1-rs:
	@echo "=== Phase 1: Miden Standalone Tests (Rust) ==="
	cargo test --lib

# Phase 2: CLAIM Notes Tests
test-phase2: test-phase2-py test-phase2-rs

test-phase2-py:
	@echo "=== Phase 2: CLAIM Notes Tests (Python) ==="
	$(PYTEST) $(PYTEST_OPTS) tests/phase2/ -m "phase2"

test-phase2-rs:
	@echo "=== Phase 2: CLAIM Notes Tests (Rust) ==="
	cargo test --test '*'

# Phase 3: Full Integration Tests
test-phase3: test-phase3-py test-phase3-rs

test-phase3-py:
	@echo "=== Phase 3: Full Integration Tests (Python) ==="
	$(PYTEST) $(PYTEST_OPTS) tests/phase3/ -m "phase3"

test-phase3-rs:
	@echo "=== Phase 3: Full Integration Tests (Rust) ==="
	cargo test --features integration -- --ignored

test-docker:
	docker compose -f docker-compose.test.yml up --build --abort-on-container-exit

# ============= SETUP & QUALITY =============

# Setup Python environment
setup:
	python -m venv .venv
	.venv/bin/pip install -r requirements.txt

check:
	cargo check

fmt:
	cargo fmt

fmt-check:
	cargo fmt -- --check

lint:
	cargo clippy -- -D warnings

# ============= UTILITY =============

clean:
	cargo clean
	rm -rf .venv __pycache__ .pytest_cache
	find . -type d -name "__pycache__" -exec rm -rf {} + 2>/dev/null || true
	find . -type f -name "*.pyc" -delete 2>/dev/null || true

dev: fmt check lint test-phase1-rs
