# Simple task runner; install with `cargo install just` (optional)

# Print available recipes
_default:
	just --list

fmt:
	@echo "(placeholder) format code once Rust sources exist"

lint:
	@echo "(placeholder) lint code once Rust sources exist"

test:
	@echo "(placeholder) run tests once Rust sources exist"

ci:
	@echo "CI placeholder; real steps are in .github/workflows/ci.yml"
