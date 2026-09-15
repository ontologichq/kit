# The kit's gate: formatting, lints and tests with every feature, and a build with none.
.PHONY: check fmt test

check:
	cargo fmt --check
	cargo clippy --all-features --all-targets -- -D warnings
	cargo check --no-default-features
	cargo test --all-features

fmt:
	cargo fmt

test:
	cargo test --all-features
