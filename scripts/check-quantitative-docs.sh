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

if rg --line-number --glob '!docs/generated/**' -- '[0-9]' "${roots[@]}"; then
  echo "public quantitative examples must be fixture-generated under docs/generated" >&2
  exit 1
fi
