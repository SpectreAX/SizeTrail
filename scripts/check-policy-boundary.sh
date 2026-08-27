#!/bin/bash
set -euo pipefail

mapfile_output="$(rg -l 'allow\(clippy::disallowed_types\)' src || true)"
if [[ "$mapfile_output" != "src/policy.rs" ]]; then
  echo "clippy::disallowed_types may be allowed only in src/policy.rs" >&2
  exit 1
fi

