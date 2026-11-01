#!/bin/sh
# All worktrees publish to the main checkout's stable release location.
set -eu
source_root=$(CDPATH= cd -- "$(dirname "$0")/.." && pwd)
common_git_dir=$(git -C "$source_root" rev-parse --path-format=absolute --git-common-dir)
repo_root=$(dirname "$common_git_dir")
printf '%s/release/Token Café.app\n' "$repo_root"
