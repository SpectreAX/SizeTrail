#!/bin/bash
set -euo pipefail

if command -v rg >/dev/null 2>&1; then
  mapfile_output="$(rg -l 'allow\(clippy::disallowed_types\)' src || true)"
else
  mapfile_output="$(grep -RIlE 'allow\(clippy::disallowed_types\)' src || true)"
fi

if [[ "$mapfile_output" != "src/policy.rs" ]]; then
  echo "clippy::disallowed_types may be allowed only in src/policy.rs" >&2
  exit 1
fi
