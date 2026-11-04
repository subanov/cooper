#!/usr/bin/env bash
# Guards the forward-only migration rule (AGENTS.md §5):
#   1. every file in migrations/ is named YYYYMMDDHHMMSS_description.sql (lower snake case);
#   2. no migration that exists on the base branch is modified, renamed or deleted;
#   3. new migrations sort after every migration on the base branch.
#
# Usage: scripts/check-migrations.sh [base-ref]   (default: origin/main)
set -euo pipefail

base="${1:-origin/main}"
dir="migrations"
status=0

for file in "$dir"/*; do
  name="$(basename "$file")"
  if [[ ! "$name" =~ ^[0-9]{14}_[a-z0-9_]+\.sql$ ]]; then
    echo "::error file=$file::migration name must match YYYYMMDDHHMMSS_description.sql"
    status=1
  fi
done

if ! git rev-parse --verify --quiet "$base^{commit}" >/dev/null; then
  echo "base ref $base not found; skipping history checks"
  exit "$status"
fi

merge_base="$(git merge-base "$base" HEAD)"

while IFS=$'\t' read -r change path _; do
  [[ -z "${change:-}" ]] && continue
  echo "::error file=$path::merged migration was changed ($change). Migrations are forward-only: add a new migration instead."
  status=1
done < <(git diff --name-status --diff-filter=MDRT "$merge_base" HEAD -- "$dir")

latest_on_base="$(git ls-tree --name-only "$merge_base" -- "$dir/" | xargs -r -n1 basename | sort | tail -n1)"
if [[ -n "$latest_on_base" ]]; then
  while read -r path; do
    [[ -z "$path" ]] && continue
    name="$(basename "$path")"
    if [[ "$name" < "$latest_on_base" ]]; then
      echo "::error file=$path::new migration sorts before $latest_on_base on $base; use a newer timestamp."
      status=1
    fi
  done < <(git diff --name-only --diff-filter=A "$merge_base" HEAD -- "$dir")
fi

if [[ "$status" -eq 0 ]]; then
  echo "migrations OK"
fi
exit "$status"
