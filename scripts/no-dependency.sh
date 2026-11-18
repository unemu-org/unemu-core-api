#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Fail unless unemu-core-api has no dependency of any kind: normal, build or
# dev, under any feature. Nothing in the contract may force a version of
# another crate on a core or a front end. The workspace's other member,
# unemu-core-files, depends on the contract; it is a front end's crate and
# not checked here, but nothing outside this repository may enter the
# resolved graph through it either.
#
# Usage: scripts/no-dependency.sh   (needs cargo and jq)
set -euo pipefail
cd "$(cd "$(dirname "${BASH_SOURCE[0]}")/.." && pwd)"

metadata=$(cargo metadata --format-version 1 --locked --all-features)

# Declared dependencies of every kind, the package's node in the resolved
# graph (no edges), and the graph as a whole (workspace members only).
declared=$(jq -r '.packages[] | select(.name == "unemu-core-api") | .dependencies[] | "\(.name) (\(.kind // "normal"))"' <<< "$metadata")
edges=$(jq -r '(.packages[] | select(.name == "unemu-core-api") | .id) as $id | .resolve.nodes[] | select(.id == $id) | .deps[] | .name' <<< "$metadata")
foreign=$(jq -r '.workspace_members as $members | .packages[] | select(.id as $id | $members | index($id) | not) | "  \(.name) \(.version)"' <<< "$metadata")

rc=0
if [[ -n $declared ]]; then
  printf 'no-dependency: unemu-core-api declares dependencies:\n%s\n' "$declared" >&2
  rc=1
fi
if [[ -n $edges ]]; then
  printf 'no-dependency: unemu-core-api resolves dependencies:\n%s\n' "$edges" >&2
  rc=1
fi
if [[ -n $foreign ]]; then
  printf 'no-dependency: the resolved graph holds packages from outside the workspace:\n%s\n' "$foreign" >&2
  rc=1
fi
if (( rc == 0 )); then
  printf 'no-dependency: unemu-core-api has no dependency of any kind\n'
fi
exit "$rc"
