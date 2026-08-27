#!/bin/bash
set -euo pipefail

binary="${1:?usage: check-zero-write-sandbox.sh <sizetrail-binary>}"
sandbox=/usr/bin/sandbox-exec
profile='(version 1)(allow default)(deny file-write*)'
probe_root="$(mktemp -d /tmp/sizetrail-zero-write.XXXXXX)"
trap '/bin/rm -rf -- "$probe_root"' EXIT

test -x "$sandbox"
mkdir -p \
  "$probe_root/home" \
  "$probe_root/tmp" \
  "$probe_root/xdg/cache" \
  "$probe_root/xdg/config" \
  "$probe_root/xdg/data" \
  "$probe_root/xdg/state" \
  "$probe_root/xdg/runtime"

environment=(
  "HOME=$probe_root/home"
  "CFFIXED_USER_HOME=$probe_root/home"
  "TMPDIR=$probe_root/tmp"
  "TMP=$probe_root/tmp"
  "TEMP=$probe_root/tmp"
  "XDG_CACHE_HOME=$probe_root/xdg/cache"
  "XDG_CONFIG_HOME=$probe_root/xdg/config"
  "XDG_DATA_HOME=$probe_root/xdg/data"
  "XDG_STATE_HOME=$probe_root/xdg/state"
  "XDG_RUNTIME_DIR=$probe_root/xdg/runtime"
)

set +e
/usr/bin/env "${environment[@]}" "$sandbox" -p "$profile" \
  /bin/sh -c 'printf mutation > "$HOME/.sizetrail-mutation"' \
  >"$probe_root/mutation.stdout" 2>"$probe_root/mutation.stderr"
mutation_status=$?
set -e

if [[ $mutation_status -eq 0 || -e "$probe_root/home/.sizetrail-mutation" ]]; then
  echo "deny-write sandbox did not reject its mutation probe" >&2
  exit 1
fi

/usr/bin/env "${environment[@]}" "$sandbox" -p "$profile" \
  "$binary" scan --json --root "$probe_root/home" \
  >"$probe_root/scan.json"

grep -q '"schema_version":"0.1.0-unstable"' "$probe_root/scan.json"
