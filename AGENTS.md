# AGENTS.md

This repository is a Rust workspace. Prefer the repo scripts and `make` targets
below instead of ad hoc worktree commands.

## Development Commands

```bash
make bootstrap                     # Fetch deps, refresh metadata, set up target/
make check                         # cargo check --workspace
make lint                          # cargo clippy --workspace --all-targets -- -D warnings
make test                          # cargo test --workspace
make fmt                           # cargo fmt --all
make validate                      # fmt check + lint + test
cargo run -p starforge-cli -- help
```

Cargo aliases are also available through `.cargo/config.toml`:
`cargo check-all`, `cargo fmt-check`, `cargo lint`, `cargo test-all`.

## Worktrees

```bash
make worktree-new ARGS="<short-name> [path]"
make worktree-remove ARGS="<path-or-branch>"
```

- `make worktree-new` creates `codex/<short-name>` from `origin/main` and runs
  `make bootstrap` in the new worktree by default.
- `make bootstrap` will reuse the `main` worktree's `target/` directory when it
  can, so Rust builds do not need to start from a cold cache in every worktree.
- `make worktree-remove` removes a side worktree and deletes its local branch
  only when that branch is already merged into `main`.

## Recommended Flow

1. Keep one dedicated worktree on `main`.
2. Create side worktrees with `make worktree-new ARGS="<short-name>"`.
3. Build, test, and commit on `codex/...` branches.
4. Rebase onto `origin/main` before merging.
5. Fast-forward merge back into `main`.
6. Remove the side worktree with `make worktree-remove ARGS="codex/<short-name>"`.
