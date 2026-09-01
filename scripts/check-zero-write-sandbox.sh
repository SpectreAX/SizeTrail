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
  "$probe_root/home/Library/Group Containers/HUAQ24HBR6.dev.orbstack/data" \
  "$probe_root/home/Library/Caches/go-build" \
  "$probe_root/tmp" \
  "$probe_root/xdg/cache" \
  "$probe_root/xdg/config" \
  "$probe_root/xdg/data" \
  "$probe_root/xdg/state" \
  "$probe_root/xdg/runtime"
: >"$probe_root/home/Library/Group Containers/HUAQ24HBR6.dev.orbstack/data/data.img.raw"
: >"$probe_root/home/Library/Caches/go-build/cached-object"

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
  "SIZETRAIL_NO_DOCKER_PROBE=1"
  "SIZETRAIL_NO_GO_PROBE=1"
)

run_token="sizetrail-zero-write-${GITHUB_RUN_ID:-local}-$$-$RANDOM"
start_marker="$run_token-start"
end_marker="$run_token-end"
events="$probe_root/sandbox.ndjson"

# Every subcommand ships in the released binary, so every subcommand must be observed. Proving
# `scan` alone and describing the result as a property of the product is the over-claim this gate
# exists to prevent (Q49).
product_markers=()
last_status=0

run_product() {
  local name="$1"
  shift
  local marker="$run_token-$name"
  product_markers+=("$marker")
  set +e
  /usr/bin/env "${environment[@]}" "$sandbox" -p "$(profile_for "$marker")" \
    "$binary" "$@" \
    >"$probe_root/$name.stdout" 2>"$probe_root/$name.stderr"
  last_status=$?
  set -e
}

require_status() {
  local name="$1"
  shift
  local candidate
  for candidate in "$@"; do
    if [[ $last_status -eq $candidate ]]; then
      return
    fi
  done
  echo "$name exited $last_status under the deny-write sandbox" >&2
  cat "$probe_root/$name.stderr" >&2
  exit 1
}

# A gate that only proves "nothing failed" is not a gate (§9.0): each command must show it did its
# work, or a regression that turns everything into an early error would keep this check green.
require_output() {
  local name="$1"
  local needle="$2"
  if ! grep -Fq "$needle" "$probe_root/$name.stdout"; then
    echo "$name produced no evidence of doing its work under the sandbox" >&2
    cat "$probe_root/$name.stdout" >&2
    exit 1
  fi
}

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

run_product scan scan --json --root "$probe_root/home"
require_status scan 0 3
require_output scan '"schema_version":"1.0.0"'
require_output scan '"id":"capacity","status":"complete"'
require_output scan '"rule_id":"docker.virtual_disk"'
require_output scan '"rule_id":"go.build_cache"'

# The probe kill-switch leaves the Xcode region applicable but unmeasured, which is exit 3 by
# Q21 — semantically distinct from the `--no-xcode` exclusion that exits 0.
run_product doctor doctor --json
require_status doctor 0 3
require_output doctor '"side_effect_policy"'
require_output doctor '"status":"readable"'

run_product rules rules --json
require_status rules 0
require_output rules '"evidence"'

run_product completion completion zsh
require_status completion 0
require_output completion 'sizetrail'

# The report is the one this run just produced, so the absent-finding path is reached only after the
# file was opened, parsed and schema-checked. A failure to open would report a different error, so
# this distinguishes "read the report" from "never got that far".
run_product explain explain --from "$probe_root/scan.stdout" 'f1:xcode:0000000000000000'
require_status explain 1
if ! grep -Fq 'absent from the supplied report' "$probe_root/explain.stderr"; then
  echo "explain did not reach the supplied report under the sandbox" >&2
  cat "$probe_root/explain.stderr" >&2
  exit 1
fi

run_product version --version
require_status version 0
require_output version 'sizetrail'

run_product help
require_status help 0
require_output help 'completion'

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

for marker in "${product_markers[@]}"; do
  if grep -Fq "$marker" "$events"; then
    grep -F "$marker" "$events" >&2
    echo "${marker#"$run_token-"} attempted a file write" >&2
    exit 1
  fi
done
