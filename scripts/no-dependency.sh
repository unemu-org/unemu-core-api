#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Fail unless unemu-core-api has no dependency of any kind: normal, build or
# dev, under any feature. Nothing in the contract may force a version of
# another crate on a core or a front end.
#
# Usage: scripts/no-dependency.sh   (needs cargo and jq)
set -euo pipefail
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

metadata=$(cargo metadata --format-version 1 --locked --all-features)

# Declared dependencies of every kind, and the resolved graph: the package is
# the only node.
declared=$(jq -r '.packages[] | select(.name == "unemu-core-api") | .dependencies[] | "\(.name) (\(.kind // "normal"))"' <<< "$metadata")
packages=$(jq -r '.packages | length' <<< "$metadata")
nodes=$(jq -r '.resolve.nodes | length' <<< "$metadata")

rc=0
if [[ -n $declared ]]; then
  printf 'no-dependency: unemu-core-api declares dependencies:\n%s\n' "$declared" >&2
  rc=1
fi
if [[ $packages != 1 || $nodes != 1 ]]; then
  printf 'no-dependency: the resolved graph holds %s packages and %s nodes, not 1\n' "$packages" "$nodes" >&2
  jq -r '.packages[] | "  \(.name) \(.version)"' <<< "$metadata" >&2
  rc=1
fi
if (( rc == 0 )); then
  printf 'no-dependency: unemu-core-api has no dependency of any kind\n'
fi
exit "$rc"
