#!/usr/bin/env bash
set -euo pipefail

if rg -n 'while i < 32' cairo/src/merkle.cairo cairo/src/request/program.cairo cairo/src/withdrawal/program.cairo; then
  echo "hard-coded Merkle depth loop found; use MERKLE_DEPTH" >&2
  exit 1
fi
