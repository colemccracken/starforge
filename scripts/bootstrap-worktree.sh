#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat <<'EOF'
Usage: scripts/bootstrap-worktree.sh [--no-fetch] [--target-source auto|main|local|<path>]

Bootstraps a Rust worktree by:
1) Fetching Cargo dependencies
2) Refreshing workspace metadata
3) Linking target/ from the main worktree when available

Target source behavior:
- auto (default): symlink target/ from the main worktree, otherwise create a local target/
- main: symlink only from the main worktree target/
- local: always create a local target/
- <path>: symlink target/ from an explicit directory path
EOF
}

fetch_deps=1
target_source="auto"

while [[ $# -gt 0 ]]; do
	case "$1" in
		--no-fetch)
			fetch_deps=0
			;;
		--target-source)
			if [[ $# -lt 2 ]]; then
				echo "Missing value for --target-source" >&2
				usage
				exit 1
			fi
			target_source="$2"
			shift
			;;
		-h|--help)
			usage
			exit 0
			;;
		*)
			echo "Unknown argument: $1" >&2
			usage
			exit 1
			;;
	esac
	shift
done

root_dir="$(git rev-parse --show-toplevel 2>/dev/null || true)"
if [[ -z "$root_dir" ]]; then
	echo "Run this script from inside the repository." >&2
	exit 1
fi

if ! command -v cargo >/dev/null 2>&1; then
	echo "cargo is required but was not found on PATH." >&2
	exit 1
fi

cd "$root_dir"
echo "Bootstrapping worktree at $root_dir"

current_branch="$(git symbolic-ref --short -q HEAD || true)"
if [[ -z "$current_branch" ]]; then
	echo "Warning: detached HEAD. Create a branch with: git switch -c codex/<short-name>"
fi

resolve_main_target() {
	local main_worktree
	main_worktree="$(git worktree list | awk '$0 ~ /\[main\]$/ { print $1; exit }')"
	if [[ -z "$main_worktree" ]]; then
		return 1
	fi
	if [[ "$main_worktree" == "$root_dir" ]]; then
		return 1
	fi
	if [[ ! -d "$main_worktree/target" ]]; then
		return 1
	fi
	(
		cd "$main_worktree/target" >/dev/null 2>&1
		pwd -P
	)
}

resolve_explicit_target() {
	local explicit_path="$1"
	if [[ ! -d "$explicit_path" ]]; then
		return 1
	fi
	(
		cd "$explicit_path" >/dev/null 2>&1
		pwd -P
	)
}

if [[ "$fetch_deps" -eq 1 ]]; then
	echo "Fetching Cargo dependencies..."
	cargo fetch
fi

echo "Refreshing workspace metadata..."
cargo metadata --no-deps --format-version 1 >/dev/null

target_path="$root_dir/target"
source_path=""

case "$target_source" in
	auto)
		source_path="$(resolve_main_target || true)"
		;;
	main)
		if ! source_path="$(resolve_main_target)"; then
			echo "Could not resolve target/ from the main worktree." >&2
			exit 1
		fi
		;;
	local)
		;;
	*)
		if ! source_path="$(resolve_explicit_target "$target_source")"; then
			echo "Explicit target source is not a directory: $target_source" >&2
			exit 1
		fi
		;;
esac

if [[ -L "$target_path" && ! -e "$target_path" ]]; then
	rm -f "$target_path"
fi

if [[ -e "$target_path" ]]; then
	if [[ -L "$target_path" ]]; then
		echo "target/ symlink already exists: $(readlink "$target_path")"
	else
		echo "target/ already exists; leaving it unchanged."
	fi
else
	if [[ -n "$source_path" ]]; then
		ln -s "$source_path" "$target_path"
		echo "Linked target/ to $source_path"
	else
		mkdir -p "$target_path"
		echo "Created local target/ directory"
	fi
fi

echo "Bootstrap complete."
echo "Suggested next commands:"
echo "  make check"
echo "  make test"
echo "  make validate"
echo "  cargo run -p starforge-cli -- help"
