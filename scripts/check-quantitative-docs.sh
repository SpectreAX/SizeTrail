#!/bin/bash
set -euo pipefail

roots=()
for candidate in README.md docs site; do
  if [[ -e "$candidate" ]]; then
    roots+=("$candidate")
  fi
done

if [[ ${#roots[@]} -eq 0 ]]; then
  exit 0
fi

if command -v rg >/dev/null 2>&1; then
  matches="$(rg --line-number --glob '!docs/generated/**' -- '[0-9]' "${roots[@]}" || true)"
else
  files=()
  while IFS= read -r -d '' file; do
    files+=("$file")
  done < <(find "${roots[@]}" -type f ! -path 'docs/generated/*' -print0)

  matches=""
  if [[ ${#files[@]} -gt 0 ]]; then
    matches="$(grep -nE -- '[0-9]' "${files[@]}" || true)"
  fi
fi

if [[ -n "$matches" ]]; then
  echo "$matches"
  echo "public quantitative examples must be fixture-generated under docs/generated" >&2
  exit 1
fi
