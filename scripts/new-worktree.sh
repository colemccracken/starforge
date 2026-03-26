#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat <<'EOF'
Usage: scripts/new-worktree.sh [--no-bootstrap] <branch-name> [worktree-path]

Creates a new worktree from origin/main on branch codex/<branch-name>.
If branch-name already starts with codex/, it is used as-is.

Examples:
  scripts/new-worktree.sh combat-ai ../starforge-wt-combat-ai
  scripts/new-worktree.sh codex/api-cleanup
EOF
}

bootstrap=1
args=()

while [[ $# -gt 0 ]]; do
	case "$1" in
		--no-bootstrap)
			bootstrap=0
			;;
		-h|--help)
			usage
			exit 0
			;;
		*)
			args+=("$1")
			;;
	esac
	shift
done

if [[ "${#args[@]}" -lt 1 ]]; then
	usage
	exit 1
fi

raw_branch="${args[0]}"
if [[ "$raw_branch" == codex/* ]]; then
	branch="$raw_branch"
else
	branch="codex/$raw_branch"
fi

root_dir="$(git rev-parse --show-toplevel 2>/dev/null || true)"
if [[ -z "$root_dir" ]]; then
	echo "Run this script from inside the repository." >&2
	exit 1
fi

repo_name="$(basename "$root_dir")"
suffix="${branch#codex/}"
suffix="${suffix//\//-}"

if [[ "${#args[@]}" -ge 2 ]]; then
	worktree_path="${args[1]}"
else
	worktree_path="$(dirname "$root_dir")/${repo_name}-wt-${suffix}"
fi

if [[ -e "$worktree_path" ]]; then
	echo "Target path already exists: $worktree_path" >&2
	exit 1
fi

if git show-ref --verify --quiet "refs/heads/$branch"; then
	echo "Branch already exists locally: $branch" >&2
	echo "Use a new branch name or attach an existing branch manually." >&2
	exit 1
fi

if git show-ref --verify --quiet "refs/remotes/origin/$branch"; then
	echo "Branch already exists on origin: $branch" >&2
	echo "Use a new branch name or create a worktree from the existing branch manually." >&2
	exit 1
fi

cd "$root_dir"
echo "Fetching origin/main..."
git fetch origin main

echo "Creating worktree $worktree_path on $branch..."
git worktree add -b "$branch" "$worktree_path" origin/main

if [[ "$bootstrap" -eq 1 ]]; then
	echo "Bootstrapping new worktree..."
	bash "$worktree_path/scripts/bootstrap-worktree.sh"
fi

echo "Worktree ready: $worktree_path"
echo "Branch: $branch"
echo "Lifecycle:"
echo "  (cd \"$worktree_path\" && make check)"
echo "  (cd \"$worktree_path\" && make test)"
echo "  (cd \"$root_dir\" && make worktree-remove ARGS=\"$branch\")"
