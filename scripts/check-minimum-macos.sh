#!/bin/bash
set -euo pipefail

binary="${1:?usage: check-minimum-macos.sh <binary> <architecture>}"
expected_architecture="${2:?usage: check-minimum-macos.sh <binary> <architecture>}"
actual_architecture="$(lipo -archs "$binary")"

if [[ "$actual_architecture" != "$expected_architecture" ]]; then
  echo "$binary has architecture $actual_architecture, expected $expected_architecture" >&2
  exit 1
fi

minimum_versions="$(otool -l "$binary" | awk '
  $1 == "cmd" && $2 == "LC_BUILD_VERSION" { mode = "build"; next }
  $1 == "cmd" && $2 == "LC_VERSION_MIN_MACOSX" { mode = "legacy"; next }
  mode == "build" && $1 == "minos" { print $2; mode = ""; next }
  mode == "legacy" && $1 == "version" { print $2; mode = ""; next }
')"

if [[ -z "$minimum_versions" ]]; then
  echo "$binary has no macOS minimum-version load command" >&2
  exit 1
fi

while IFS= read -r version; do
  if [[ "$version" != "13.0" && "$version" != "13.0.0" ]]; then
    echo "$binary targets macOS $version, expected 13.0" >&2
    exit 1
  fi
done <<< "$minimum_versions"
