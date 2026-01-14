.PHONY: build test test-phase1 test-phase2 test-phase3 test-docker clean check fmt lint

# Build targets
build:
	cargo build

release:
	cargo build --release

# Test targets
test: test-phase1 test-phase2 test-phase3

test-phase1:
	cargo test --lib

test-phase2:
	cargo test --test '*'

test-phase3:
	cargo test --features integration -- --ignored

test-docker:
	docker compose -f docker-compose.test.yml up --build --abort-on-container-exit

# Quality targets
check:
	cargo check

fmt:
	cargo fmt

fmt-check:
	cargo fmt -- --check

lint:
	cargo clippy -- -D warnings

# Utility targets
clean:
	cargo clean

# Development helpers
dev: fmt check lint test-phase1
