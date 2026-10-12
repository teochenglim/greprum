.DEFAULT_GOAL := help
BINARY := greprum

# Single source of truth for the version (no external tooling required).
VERSION_CURRENT := $(shell cat VERSION 2>/dev/null || echo 0.0.0)
GREPTIME_VERSION ?= v1.2.1
GREPTIME_NAME := greprum-db

.PHONY: help
help: ## Show this menu
	@echo "GrepRUM $(VERSION_CURRENT) - available targets:"
	@grep -E '^[a-zA-Z0-9_-]+:.*?## .*$$' $(MAKEFILE_LIST) | sort | awk 'BEGIN {FS = ":.*?## "}; {printf "  \033[36m%-18s\033[0m %s\n", $$1, $$2}'
	@echo ""
	@echo "Release cycle:"
	@echo "  make release VERSION=x.y.z   # bump VERSION, push HEAD, tag, push tag -> CI"

## --- develop -----------------------------------------------------------

.PHONY: tracker
tracker: ## Build + minify tracker/dist/t.js (embedded into the binary)
	node tracker/build.mjs

.PHONY: build
build: tracker ## Build the release binary into ./bin
	GREPRUM_VERSION=v$(VERSION_CURRENT) cargo build --release --locked
	@mkdir -p bin && cp target/release/$(BINARY) bin/$(BINARY)

.PHONY: run
run: tracker ## Run the gateway locally with ./greprum.toml
	cargo run -- serve --config greprum.toml

.PHONY: test
test: tracker ## Run the Rust test suite (unit + AC tests, no DB needed)
	cargo test --locked

.PHONY: test-db
test-db: tracker ## Integration tests against a live GreptimeDB (run `make db-up` first)
	GREPRUM_TEST_DB=mysql://root@127.0.0.1:4002/public cargo test --locked -- --ignored --test-threads=1

.PHONY: tracker-test
tracker-test: ## Tracker behaviour tests (node --test, jsdom)
	cd tracker && npm install --no-audit --no-fund --silent && node --test test/

.PHONY: tracker-size
tracker-size: tracker ## Enforce tracker gzip size budget (core 3 KB, total 5 KB)
	node tracker/size.mjs

.PHONY: ac-map
ac-map: ## Traceability gate: every requirement -> AC -> >=2 tests
	./scripts/ac-map.sh

.PHONY: fmt
fmt: ## Format Rust source
	cargo fmt --all

.PHONY: fmt-check
fmt-check: ## Check formatting
	cargo fmt --all -- --check

.PHONY: clippy
clippy: tracker ## Lint (warnings are errors)
	cargo clippy --all-targets --locked -- -D warnings

.PHONY: check
check: fmt-check clippy ac-map test tracker-test tracker-size version-check ## Everything CI runs

.PHONY: spike
spike: build ## Phase-0 spike: generate N events (N=1000000) and run the query gates
	BIN=./bin/$(BINARY) ./scripts/spike.sh $(or $(N),1000000)

.PHONY: release-gate
release-gate: check test-db ## v1.0.0 gate: everything incl. ignored release-gate tests
	GREPRUM_TEST_DB=mysql://root@127.0.0.1:4002/public cargo test --locked --test ac_launch -- --ignored

.PHONY: clean
clean: ## Remove local build artifacts
	cargo clean
	rm -rf bin dist tracker/dist

.PHONY: smoke
smoke: build ## End-to-end smoke: gateway up -> /healthz -> /t.js -> /collect 204 (no DB needed)
	./scripts/smoke.sh ./bin/$(BINARY)

## --- container -------------------------------------------------------------

IMAGE ?= ghcr.io/teochenglim/greprum

.PHONY: docker-build
docker-build: ## Build the container image (tag :$(VERSION_CURRENT))
	docker build --build-arg VERSION=v$(VERSION_CURRENT) -t $(IMAGE):$(VERSION_CURRENT) -t $(IMAGE):latest .

.PHONY: docker-run
docker-run: ## Run the image against a local config (./greprum.toml)
	docker run --rm -p 127.0.0.1:8080:8080 -v greprum-data:/data -v $(PWD)/greprum.toml:/etc/greprum/greprum.toml:ro $(IMAGE):$(VERSION_CURRENT)

.PHONY: docker-push
docker-push: ## Manual push to GHCR (CI does this on tags). Needs `docker login ghcr.io` first.
	docker push $(IMAGE):$(VERSION_CURRENT)
	docker push $(IMAGE):latest

## --- local GreptimeDB ----------------------------------------------------

.PHONY: db-up
db-up: ## Start the pinned GreptimeDB in docker (HTTP 4000, MySQL 4002)
	-@docker rm -f $(GREPTIME_NAME) >/dev/null 2>&1
	docker run -d --name $(GREPTIME_NAME) -p 127.0.0.1:4000-4003:4000-4003 greptime/greptimedb:$(GREPTIME_VERSION) \
	  standalone start --http-addr 0.0.0.0:4000 --rpc-bind-addr 0.0.0.0:4001 \
	  --mysql-addr 0.0.0.0:4002 --postgres-addr 0.0.0.0:4003

.PHONY: db-down
db-down: ## Stop and remove the local GreptimeDB
	-docker rm -f $(GREPTIME_NAME)

## --- supply-chain hardening ----------------------------------------------

.PHONY: github-action-bump
github-action-bump: ## Pin .github/workflows/*.yml actions to latest release, full commit SHA (uses pinact)
	go run github.com/suzuki-shunsuke/pinact/cmd/pinact@latest run --update
	go run github.com/suzuki-shunsuke/pinact/cmd/pinact@latest run --verify
	@echo "Actions bumped and verified. Review the diff, then run 'make check' before committing."

.PHONY: audit
audit: ## cargo-audit + cargo-deny (install separately)
	cargo audit
	cargo deny check

## --- release --------------------------------------------------------------

.PHONY: version
version: ## Print the version currently in VERSION
	@echo $(VERSION_CURRENT)

.PHONY: version-check
version-check: ## VERSION must equal the Cargo.toml package version
	@test "$$(grep -m1 '^version' Cargo.toml | sed 's/.*"\(.*\)"/\1/')" = "$(VERSION_CURRENT)" \
	  || { echo "VERSION ($(VERSION_CURRENT)) != Cargo.toml version"; exit 1; }

.PHONY: bump
bump: ## Rewrite VERSION and Cargo.toml with the new version (VERSION=x.y.z required)
	@if [ -z "$(VERSION)" ]; then echo "Usage: make bump VERSION=x.y.z"; exit 1; fi
	@echo "$(VERSION)" > VERSION
	@perl -0pi -e 's/^version = ".*?"/version = "$(VERSION)"/m' Cargo.toml
	@cargo update -w >/dev/null 2>&1 || true
	@echo "VERSION -> $(VERSION)"

.PHONY: release
release: ## Bump VERSION, push HEAD, tag, push the tag - triggers GitHub Actions (VERSION=x.y.z required)
	@if [ -z "$(VERSION)" ]; then echo "Usage: make release VERSION=x.y.z"; exit 1; fi
	$(MAKE) bump VERSION=$(VERSION)
	git add VERSION Cargo.toml Cargo.lock
	git commit --amend --no-edit
	git push origin HEAD
	git tag v$(VERSION)
	git push origin v$(VERSION)
	@echo "Released v$(VERSION) - GitHub Actions will build and publish."
