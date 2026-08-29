#!/bin/bash
set -euo pipefail

binary="${1:?usage: check-zero-write-sandbox.sh <sizetrail-binary>}"
sandbox=/usr/bin/sandbox-exec
# The probe root must be a physical path: /tmp is a symlink on stock macOS, and the product
# pins roots to their physical identity. A symlinked probe root would make scan bail out
# before measuring anything and leave this gate green (Q31, Q32).
probe_root="$(cd "$(mktemp -d /tmp/sizetrail-zero-write.XXXXXX)" && pwd -P)"
observer_pid=""

cleanup() {
  if [[ -n "$observer_pid" ]] && kill -0 "$observer_pid" 2>/dev/null; then
    kill "$observer_pid"
    wait "$observer_pid" 2>/dev/null || true
  fi
  /bin/rm -rf -- "$probe_root"
}
trap cleanup EXIT

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
  "SIZETRAIL_NO_XCODE_PROBE=1"
)

run_token="sizetrail-zero-write-${GITHUB_RUN_ID:-local}-$$-$RANDOM"
start_marker="$run_token-start"
scan_marker="$run_token-scan"
end_marker="$run_token-end"
events="$probe_root/sandbox.ndjson"

profile_for() {
  printf '(version 1)(allow default)(deny file-write* (with message "%s"))(allow file-write-data (literal "/dev/dtracehelper"))' "$1"
}

wait_for_marker() {
  marker="$1"
  operation="$2"
  expected_path="$3"
  escaped_path="${expected_path//\//\\/}"
  attempts=0
  while ! grep -F "$marker" "$events" | grep -F "$operation" | grep -Fq "$escaped_path"; do
    if ! kill -0 "$observer_pid" 2>/dev/null; then
      echo "sandbox violation observer exited before $marker" >&2
      exit 1
    fi
    attempts=$((attempts + 1))
    if [[ $attempts -ge 100 ]]; then
      echo "timed out waiting for sandbox marker: $marker" >&2
      exit 1
    fi
    sleep 0.1
  done
}

establish_observer() {
  marker="$1"
  target="$2"
  attempts=0
  while true; do
    set +e
    /usr/bin/env "${environment[@]}" "$sandbox" -p "$(profile_for "$marker")" \
      /bin/sh -c ': > "$1" 2>/dev/null || true; exit 0' sh "$target" \
      >"$probe_root/mutation.stdout" 2>"$probe_root/mutation.stderr"
    mutation_status=$?
    set -e

    if [[ $mutation_status -ne 0 || -e "$target" ]]; then
      echo "sandbox observer probe did not swallow a rejected write as expected" >&2
      exit 1
    fi
    if grep -F "$marker" "$events" | grep -F "file-write-create" | grep -Fq "${target//\//\\/}"; then
      return
    fi
    if ! kill -0 "$observer_pid" 2>/dev/null; then
      echo "sandbox violation observer exited before $marker" >&2
      exit 1
    fi
    attempts=$((attempts + 1))
    if [[ $attempts -ge 100 ]]; then
      echo "timed out establishing sandbox observer: $marker" >&2
      exit 1
    fi
    sleep 0.1
  done
}

/usr/bin/log stream --style ndjson --level debug \
  --predicate "eventMessage CONTAINS[c] '$run_token'" \
  >"$events" 2>"$probe_root/log.stderr" &
observer_pid=$!

# `log stream` has no ready notification and drops events emitted before subscription. Repeat a
# swallowed, denied write until its unique message is observed; this is the handshake that proves
# the observer is live before the product process runs.
establish_observer "$start_marker" "$probe_root/home/.sizetrail-mutation"

set +e
/usr/bin/env "${environment[@]}" "$sandbox" -p "$(profile_for "$scan_marker")" \
  "$binary" scan --json --root "$probe_root/home" \
  >"$probe_root/scan.json" 2>"$probe_root/scan.stderr"
scan_status=$?
set -e

if [[ $scan_status -ne 0 && $scan_status -ne 3 ]]; then
  echo "scan exited $scan_status under the deny-write sandbox" >&2
  cat "$probe_root/scan.stderr" >&2
  exit 1
fi

grep -q '"schema_version":"0.1.0-unstable"' "$probe_root/scan.json"

# A gate that only proves "nothing failed" is not a gate (§9.0). Assert the measurement
# actually ran: a regression that makes root initialization fail everywhere would otherwise
# keep this check green while proving nothing about the read path.
if ! grep -q '"id":"capacity","status":"complete"' "$probe_root/scan.json"; then
  echo "scan produced no completed capacity measurement under the sandbox" >&2
  cat "$probe_root/scan.json" >&2
  exit 1
fi

set +e
/usr/bin/env "${environment[@]}" "$sandbox" -p "$(profile_for "$end_marker")" \
  /usr/bin/touch "$probe_root/end-mutation" \
  >"$probe_root/end.stdout" 2>"$probe_root/end.stderr"
end_status=$?
set -e

if [[ $end_status -eq 0 || -e "$probe_root/end-mutation" ]]; then
  echo "deny-write sandbox did not reject its end marker" >&2
  exit 1
fi
wait_for_marker "$end_marker" "file-write-create" "$probe_root/end-mutation"

if grep -Fq "$scan_marker" "$events"; then
  grep -F "$scan_marker" "$events" >&2
  echo "scan attempted a file write" >&2
  exit 1
fi
