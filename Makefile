.PHONY: bootstrap check fmt lint test validate worktree-new worktree-remove

bootstrap:
	bash scripts/bootstrap-worktree.sh

check:
	cargo check --workspace

fmt:
	cargo fmt --all

lint:
	cargo clippy --workspace --all-targets -- -D warnings

test:
	cargo test --workspace

validate:
	cargo fmt --all --check
	cargo clippy --workspace --all-targets -- -D warnings
	cargo test --workspace

worktree-new:
	bash scripts/new-worktree.sh $(ARGS)

worktree-remove:
	bash scripts/remove-worktree.sh $(ARGS)
