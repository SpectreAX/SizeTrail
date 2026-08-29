#!/bin/bash
set -euo pipefail

mkdir -p artifacts
output="$(cargo test --locked --release --lib \
  adapters::tests::xcode_inventory_fixture_benchmark -- \
  --ignored --exact --nocapture 2>&1)"
printf '%s\n' "$output"
line="$(printf '%s\n' "$output" | grep -m 1 '^SIZETRAIL_BENCHMARK_JSON=')"
test -n "$line"
measurement="${line#SIZETRAIL_BENCHMARK_JSON=}"

# The asset is published on its own, so it must carry the runner identity rather than rely on the
# artifact name: without it a consumer can neither attribute the timing nor notice an image
# rotation, which is exactly what SPEC §9.4 forbids reading as a trend.
if [[ "${CI:-}" == "true" ]]; then
  : "${ImageOS:?published benchmark must identify the runner image (SPEC 9.4)}"
  : "${ImageVersion:?published benchmark must identify the runner image (SPEC 9.4)}"
fi

jq -n \
  --argjson measurement "$measurement" \
  --arg image_os "${ImageOS:-unidentified_local_host}" \
  --arg image_version "${ImageVersion:-unidentified_local_host}" \
  --arg arch "$(uname -m)" \
  '$measurement + {runner: {image_os: $image_os, image_version: $image_version, arch: $arch}}' \
  > artifacts/xcode-fixture-benchmark.json
