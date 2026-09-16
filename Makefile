.PHONY: build build-release test lint fmt fmt-check clean audit coverage ci release-check release-check-full

build:
	cargo build

build-release:
	cargo build --release --locked

test:
	cargo test --locked -- --test-threads 1

lint:
	cargo clippy --locked -- -D warnings

fmt:
	cargo fmt

fmt-check:
	cargo fmt --check

audit:
	cargo audit

coverage:
	cargo tarpaulin --locked --fail-under 85 --exclude-files src/main.rs -- --test-threads 1

ci: fmt-check lint test coverage audit

release-check:
	./scripts/check-release.sh

release-check-full:
	@if [ -z "$(VERSION)" ]; then \
		echo "Usage: make release-check-full VERSION=0.1.2"; \
		exit 1; \
	fi
	./scripts/check-release.sh --full $(VERSION)
