#!/usr/bin/env bash
set -euo pipefail

usage() {
	cat <<'EOF'
Usage: scripts/remove-worktree.sh [--force] [--keep-branch] <worktree-path|branch-name>

Removes a side worktree by path or branch name.
If the local branch is merged into main, it is deleted automatically unless
--keep-branch is provided.

Examples:
  scripts/remove-worktree.sh ../starforge-wt-combat-ai
  scripts/remove-worktree.sh codex/combat-ai
  scripts/remove-worktree.sh combat-ai
EOF
}

force=0
keep_branch=0
args=()

while [[ $# -gt 0 ]]; do
	case "$1" in
		--force)
			force=1
			;;
		--keep-branch)
			keep_branch=1
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

if [[ "${#args[@]}" -ne 1 ]]; then
	usage
	exit 1
fi

root_dir="$(git rev-parse --show-toplevel 2>/dev/null || true)"
if [[ -z "$root_dir" ]]; then
	echo "Run this script from inside the repository." >&2
	exit 1
fi

cd "$root_dir"

current_path="$(pwd -P)"
target_input="${args[0]}"
target_path=""
target_branch=""

find_worktree_by_path() {
	local wanted="$1"
	local worktree_path=""
	local branch=""

	while IFS= read -r line || [[ -n "$line" ]]; do
		if [[ -z "$line" ]]; then
			if [[ -n "$worktree_path" && "$worktree_path" == "$wanted" ]]; then
				target_path="$worktree_path"
				target_branch="$branch"
				return 0
			fi
			worktree_path=""
			branch=""
			continue
		fi

		case "$line" in
			worktree\ *)
				worktree_path="$(cd "${line#worktree }" >/dev/null 2>&1 && pwd -P)"
				;;
			branch\ refs/heads/*)
				branch="${line#branch refs/heads/}"
				;;
		esac
	done < <(git worktree list --porcelain && printf '\n')

	return 1
}

find_worktree_by_branch() {
	local wanted="$1"
	local worktree_path=""
	local branch=""

	while IFS= read -r line || [[ -n "$line" ]]; do
		if [[ -z "$line" ]]; then
			if [[ -n "$worktree_path" && "$branch" == "$wanted" ]]; then
				target_path="$worktree_path"
				target_branch="$branch"
				return 0
			fi
			worktree_path=""
			branch=""
			continue
		fi

		case "$line" in
			worktree\ *)
				worktree_path="$(cd "${line#worktree }" >/dev/null 2>&1 && pwd -P)"
				;;
			branch\ refs/heads/*)
				branch="${line#branch refs/heads/}"
				;;
		esac
	done < <(git worktree list --porcelain && printf '\n')

	return 1
}

if [[ -d "$target_input" ]]; then
	resolved_input="$(cd "$target_input" >/dev/null 2>&1 && pwd -P)"
	if ! find_worktree_by_path "$resolved_input"; then
		echo "No git worktree found at path: $target_input" >&2
		exit 1
	fi
else
	if [[ "$target_input" == codex/* || "$target_input" == main ]]; then
		resolved_branch="$target_input"
	else
		resolved_branch="codex/$target_input"
	fi

	if ! find_worktree_by_branch "$resolved_branch"; then
		echo "No git worktree found for branch: $resolved_branch" >&2
		exit 1
	fi
fi

if [[ "$target_path" == "$current_path" ]]; then
	echo "Refusing to remove the current worktree: $target_path" >&2
	exit 1
fi

remove_args=("git" "worktree" "remove")
if [[ "$force" -eq 1 ]]; then
	remove_args+=("--force")
fi
remove_args+=("$target_path")

echo "Removing worktree: $target_path"
"${remove_args[@]}"
git worktree prune

echo "Removed worktree: $target_path"

if [[ "$keep_branch" -eq 1 || -z "$target_branch" || "$target_branch" == "main" ]]; then
	if [[ -n "$target_branch" ]]; then
		echo "Kept branch: $target_branch"
	fi
	exit 0
fi

if ! git show-ref --verify --quiet "refs/heads/$target_branch"; then
	echo "Local branch already absent: $target_branch"
	exit 0
fi

main_ref=""
if git show-ref --verify --quiet "refs/remotes/origin/main"; then
	main_ref="origin/main"
elif git show-ref --verify --quiet "refs/heads/main"; then
	main_ref="main"
fi

if [[ -n "$main_ref" ]] && git merge-base --is-ancestor "$target_branch" "$main_ref"; then
	git branch -d "$target_branch"
	echo "Deleted merged branch: $target_branch"
else
	echo "Kept branch: $target_branch"
	echo "  It is not merged into main yet. Delete it manually when ready."
fi
