.PHONY: help fmt lint test ci

help:
	@grep -E '^[a-zA-Z_-]+:.*?##' Makefile | awk 'BEGIN {FS = ":.*?## "}; {printf "\033[36m%-15s\033[0m %s\n", $$1, $$2}'

fmt: ## format code (placeholder)
	@echo "fmt: waiting for Rust sources"

lint: ## lint code (placeholder)
	@echo "lint: waiting for Rust sources"

test: ## run tests (placeholder)
	@echo "test: waiting for Rust sources"

ci: ## local CI (placeholder)
	@echo "ci: see .github/workflows/ci.yml"
