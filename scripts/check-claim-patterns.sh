#!/bin/bash
set -euo pipefail

roots=()
for candidate in src README.md docs site; do
  if [[ -e "$candidate" ]]; then
    roots+=("$candidate")
  fi
done

if [[ ${#roots[@]} -eq 0 ]]; then
  exit 0
fi

pattern='释放[[:space:]]*[0-9]|可释放空间|解释全部[[:space:]]*System Data|解释了[[:space:]]*System Data|共享字节|全球无商标|free(s|d|ing)? up[[:space:]]+[0-9]|reclaimable[[:space:]]+[0-9]|explain(s|ed|ing)? all[[:space:]]+System Data|globally trademark[- ]free'
if command -v rg >/dev/null 2>&1; then
  matches="$(rg --ignore-case --line-number -- "$pattern" "${roots[@]}" || true)"
else
  matches="$(grep -RinE -- "$pattern" "${roots[@]}" || true)"
fi

if [[ -n "$matches" ]]; then
  echo "$matches"
  echo "public copy contains a forbidden truth-contract claim" >&2
  exit 1
fi
