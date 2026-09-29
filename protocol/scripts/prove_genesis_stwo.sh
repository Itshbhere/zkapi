#!/usr/bin/env bash
set -euo pipefail

cd "$(dirname "$0")/../cairo"

# Proves every current executable Stwo fixture. The filename is kept for
# compatibility with earlier genesis-only checks.
prove_and_verify() {
  local executable_name="$1"
  local output
  output="$(scarb prove --execute --executable-name "$executable_name" 2>&1)"
  printf '%s\n' "$output"

  local execution_id
  execution_id="$(
    printf '%s\n' "$output" |
      sed -n 's#.*target/execute/zkapi_cairo/execution\([0-9][0-9]*\).*#\1#p' |
      tail -1
  )"

  if [[ -z "$execution_id" ]]; then
    echo "could not determine scarb execution id for $executable_name" >&2
    exit 1
  fi

  scarb verify --execution-id "$execution_id"
}

prove_and_verify request_genesis
prove_and_verify request_non_genesis
prove_and_verify withdrawal_genesis_escape
prove_and_verify withdrawal_non_genesis_escape
prove_and_verify withdrawal_mutual_close
