#!/bin/bash
set -euo pipefail

git diff --exit-code -- docs/generated
if [[ -e README.md ]]; then
  python3 scripts/sync-generated-fragments.py
fi
