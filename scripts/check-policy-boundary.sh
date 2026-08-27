#!/bin/bash
set -euo pipefail

lint="${1:-clippy::disallowed_types}"
allowed_file="${2:-src/policy.rs}"
source_root="${3:-src}"
if [[ $# -ge 3 ]]; then
  shift 3
else
  set --
fi
aliases=("$lint" "$@")

for candidate in "${aliases[@]}"; do
  if [[ ! "$candidate" =~ ^[A-Za-z0-9_:]+$ ]]; then
    echo "invalid lint name: $candidate" >&2
    exit 2
  fi
done

suppression_count=0
suppression_files=""
last_suppression=""
while IFS= read -r -d '' file; do
  compact="$(LC_ALL=C tr -d '[:space:]' < "$file")"
  for candidate in "${aliases[@]}"; do
    if grep -Eq "(allow|expect)\\([^)]*${candidate}([,)])" <<< "$compact"; then
      suppression_count=$((suppression_count + 1))
      suppression_files+="$file"$'\n'
      last_suppression="$file"
      break
    fi
  done
done < <(find "$source_root" -type f -name '*.rs' -print0)

expected_count=1
if [[ "$allowed_file" == "-" ]]; then
  expected_count=0
fi

if [[ $suppression_count -ne $expected_count || ($expected_count -eq 1 && "$last_suppression" != "$allowed_file") ]]; then
  printf '%s' "$suppression_files"
  echo "$lint may be suppressed only in $allowed_file" >&2
  exit 1
fi
