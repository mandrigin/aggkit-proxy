.PHONY: build test test-phase1 test-phase2 test-phase3 test-unit test-docker test-kurtosis clean check fmt lint docker-build docker-up docker-down

# ==============================================================================
# Build targets
# ==============================================================================

build:
	cargo build

release:
	cargo build --release

docker-build:
	docker compose build

# ==============================================================================
# Test targets - Primary interface
# ==============================================================================

# Run all tests (the required single command)
test: test-docker

# Run unit tests only (no Docker required)
test-unit:
	cargo test --lib

# Run tests by phase in Docker
test-phase1: docker-up
	docker compose run --rm test-runner cargo test phase1 -- --nocapture
	@$(MAKE) docker-down

test-phase2: docker-up
	docker compose run --rm test-runner cargo test phase2 -- --nocapture
	@$(MAKE) docker-down

test-phase3: docker-up
	docker compose run --rm test-runner cargo test phase3 --features integration -- --ignored --nocapture
	@$(MAKE) docker-down

# ==============================================================================
# Docker Compose targets
# ==============================================================================

# Run full test suite in Docker (default for `make test`)
test-docker: docker-build
	docker compose --profile test up --build --abort-on-container-exit --exit-code-from test-runner
	@$(MAKE) docker-down

# Start services (without test runner)
docker-up:
	docker compose up -d --build --wait
	@echo "Services ready:"
	@echo "  Proxy:          http://localhost:8546"
	@echo "  Miden Node:     http://localhost:57291"
	@echo "  L1 Anvil:       http://localhost:8545"
	@echo "  Bridge Service: http://localhost:8080"
	@echo "  PostgreSQL:     localhost:5432"

docker-down:
	docker compose down -v --remove-orphans

docker-logs:
	docker compose logs -f

docker-ps:
	docker compose ps

# ==============================================================================
# Kurtosis targets (for CI/reproducible environments)
# ==============================================================================

test-kurtosis:
	kurtosis run ./kurtosis

test-kurtosis-phase1:
	kurtosis run ./kurtosis '{"test_phase": "phase1"}'

test-kurtosis-phase2:
	kurtosis run ./kurtosis '{"test_phase": "phase2"}'

test-kurtosis-phase3:
	kurtosis run ./kurtosis '{"test_phase": "phase3"}'

kurtosis-clean:
	kurtosis enclave stop --all
	kurtosis enclave rm --all

# ==============================================================================
# Quality targets
# ==============================================================================

check:
	cargo check

fmt:
	cargo fmt

fmt-check:
	cargo fmt -- --check

lint:
	cargo clippy -- -D warnings

# ==============================================================================
# Utility targets
# ==============================================================================

clean:
	cargo clean
	docker compose down -v --remove-orphans 2>/dev/null || true
	kurtosis enclave rm --all 2>/dev/null || true

# Development helpers
dev: fmt check lint test-unit

# CI target - runs full validation
ci: fmt-check lint test-kurtosis
