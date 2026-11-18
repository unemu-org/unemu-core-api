#!/usr/bin/env bash
# SPDX-License-Identifier: MIT OR Apache-2.0
# Fail when the contract names a host interface of the standard library:
# since contract 0.2 a core reads content and keeps records through objects
# the front end hands in, and it never had a host clock, threads, processes,
# the environment or the network. Nothing under src/ may name the modules
# std::fs, std::path, std::os, std::env, std::process, std::net or
# std::thread, nor std::time::Instant or std::time::SystemTime. This is the
# contract's part of the host-interface check; the wasm32 build checks the
# rest.
#
# Usage: scripts/no-host.sh [--self-test]
#
# It scans every .rs file under src/, comments and tests included, for the
# names written directly (std::fs, ::std::path, std :: env,
# std::time::Instant) and for the grouped form of a use statement
# (use std::{io, fs::File};, use std::{fs as f};,
# use std::time::{Duration, Instant};), also when the group spans several
# lines. Needs only bash 3.2, find, sort and awk, so it runs the same on
# macOS and Linux.
#
# --self-test runs the check over small fixture trees, one clean and one per
# form, and fails unless the clean one passes and every other fails.
set -euo pipefail
export LC_ALL=C

check_path="$(cd "$(dirname "${BASH_SOURCE[0]}")" && pwd)/$(basename "${BASH_SOURCE[0]}")"

if [[ ${1:-} == --self-test ]]; then
  work=$(mktemp -d)
  trap 'rm -rf "$work"' EXIT
  failures=0

  # Build a fixture tree holding the check and one source file, then run the
  # check there. The forbidden texts are assembled from pieces so that this
  # file does not hold them whole.
  run_fixture() {
    local name="$1" text="$2" tree="$work/$1"
    mkdir -p "$tree/scripts" "$tree/src"
    cp "$check_path" "$tree/scripts/no-host.sh"
    printf '%s\n' "$text" > "$tree/src/lib.rs"
    bash "$tree/scripts/no-host.sh" > "$work/$name.log" 2>&1
  }

  expect_pass() {
    if run_fixture "$@"; then
      printf 'ok    %s passes\n' "$1"
    else
      printf 'FAIL  %s should pass:\n' "$1"; sed 's/^/      /' "$work/$1.log"
      failures=$((failures + 1))
    fi
  }

  expect_fail() {
    if run_fixture "$@"; then
      printf 'FAIL  %s should fail\n' "$1"
      failures=$((failures + 1))
    elif ! grep -qF 'no-host: ' "$work/$1.log"; then
      printf 'FAIL  %s should report a hit:\n' "$1"; sed 's/^/      /' "$work/$1.log"
      failures=$((failures + 1))
    else
      printf 'ok    %s fails\n' "$1"
    fi
  }

  s='std'
  expect_pass clean "use ${s}::io::{Read, Write};
use ${s}::{
    rc::Rc,
    io::{self, Read as _},
};
// A record is not a filesystem path; fs_name and path_like are fine.
fn path(fs: u8) -> u8 { let fsx = fs; fsx }
const PATH: &str = \"${s}::fsx\";
use ${s}::time::Duration;
use ${s}::{sync::Arc, time::{Duration as D}};
// No threads, no host clock; os and env are words here.
fn env(os: u8) -> u8 { os }"
  expect_fail direct "fn open() -> ${s}::""fs::File { todo!() }"
  expect_fail use-path "use ${s}::""path::Path;"
  expect_fail absolute "use ::${s}::""fs;"
  expect_fail spaced "type P = ${s} :: ""path::PathBuf;"
  expect_fail grouped-one-line "use ${s}::{io::Read, ""fs::File};"
  expect_fail grouped-lines "use ${s}::{
    io::{Read, Write},
    ""path::Path,
};"
  expect_fail grouped-module "use ${s}::{""fs, io};"
  expect_fail comment "/// Replaces ${s}::""fs::File of contract 0.1."
  expect_fail grouped-rename "use ${s}::{io, ""fs as f};"
  expect_fail os "use ${s}::""os::unix::ffi::OsStrExt;"
  expect_fail env "let home = ${s}::""env::var(\"HOME\");"
  expect_fail process "${s}::""process::exit(1);"
  expect_fail net "use ${s}::""net::TcpStream;"
  expect_fail thread "${s}::""thread::spawn(|| ());"
  expect_fail grouped-thread "use ${s}::{sync::Arc, ""thread};"
  expect_fail instant "let t = ${s}::time::""Instant::now();"
  expect_fail grouped-time "use ${s}::time::{Duration, ""SystemTime};"
  expect_fail nested-time "use ${s}::{io, time::{""Instant}};"

  if (( failures )); then
    printf 'no-host self-test: %d failure(s)\n' "$failures" >&2
    exit 1
  fi
  printf 'no-host self-test: all passed\n'
  exit 0
fi

cd "$(dirname "$check_path")/.."

files=()
while IFS= read -r -d '' file; do
  files+=("$file")
done < <(find src -type f -name '*.rs' -print0 | sort -z)

if (( ${#files[@]} == 0 )); then
  printf 'no-host: no source under src/\n' >&2
  exit 1
fi

# Print "file:line: text" for each hit. A line names a module or type
# directly, or it lies inside a grouped use of std (std::{ or
# std::time::{, on one line or several) and names a module as a member or
# one of the two clock types.
hits=$(awk '
  FNR == 1 { grouped = 0 }
  {
    line = $0
    hit = line ~ /(^|[^[:alnum:]_])std[[:space:]]*::[[:space:]]*(fs|path|os|env|process|net|thread)([^[:alnum:]_]|$)/
    if (line ~ /(^|[^[:alnum:]_])std[[:space:]]*::[[:space:]]*time[[:space:]]*::[[:space:]]*(Instant|SystemTime)([^[:alnum:]_]|$)/) hit = 1
    rest = ""
    if (!grouped && match(line, /(^|[^[:alnum:]_])std[[:space:]]*(::[[:space:]]*[[:alnum:]_]+[[:space:]]*)*::[[:space:]]*\{/)) {
      grouped = 1
      rest = substr(line, RSTART + RLENGTH)
    } else if (grouped) {
      rest = line
    }
    if (grouped) {
      if (rest ~ /(^|[^[:alnum:]_:])(fs|path|os|env|process|net|thread)([[:space:]]*(::|,|\}|;)|[[:space:]]+as([^[:alnum:]_]|$)|[[:space:]]*$)/) hit = 1
      if (rest ~ /(^|[^[:alnum:]_])(Instant|SystemTime)([^[:alnum:]_]|$)/) hit = 1
      if (rest ~ /;/) grouped = 0
    }
    if (hit) print FILENAME ":" FNR ": " line
  }
' "${files[@]}")

if [[ -n $hits ]]; then
  printf '%s\n' "$hits"
  printf 'no-host: the contract names a host interface; hand content and storage in as objects\n' >&2
  exit 1
fi
printf 'no-host: no host interface named under src/\n'
