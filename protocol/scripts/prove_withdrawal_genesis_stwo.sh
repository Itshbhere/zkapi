#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/../cairo"

output="$(scarb prove --execute --executable-name withdrawal_genesis_escape 2>&1)"
printf '%s\n' "$output"

execution_id="$(
  printf '%s\n' "$output" |
    sed -n 's#.*target/execute/zkapi_cairo/execution\([0-9][0-9]*\).*#\1#p' |
    tail -1
)"

if [[ -z "$execution_id" ]]; then
  echo "could not determine scarb execution id" >&2
  exit 1
fi

scarb verify --execution-id "$execution_id"
