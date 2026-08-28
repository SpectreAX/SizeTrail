#!/bin/bash
set -euo pipefail

mkdir -p artifacts
output="$(cargo test --locked --release --lib \
  adapters::tests::xcode_inventory_fixture_benchmark -- \
  --ignored --exact --nocapture 2>&1)"
printf '%s\n' "$output"
line="$(printf '%s\n' "$output" | grep -m 1 '^SIZETRAIL_BENCHMARK_JSON=')"
test -n "$line"
printf '%s\n' "${line#SIZETRAIL_BENCHMARK_JSON=}" > artifacts/xcode-fixture-benchmark.json
