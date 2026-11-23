# Migrating the 0.4 storage guarantees

This is the storage part of the breaking 0.4.0 release, alongside
[`migration-0.4-shapes.md`](migration-0.4-shapes.md); start at the
[single migration index](migration-0.4.md). Both packages and CONTRACT
now state 0.4; exact release pins move together after tagging. The storage traits and their
signatures stay unchanged; consumers must update recovery assumptions.

| Operation or outcome | Required consumer behavior |
| --- | --- |
| Visibility | Successful append and truncate change the current bytes immediately. Reads and size inspect current bytes, not a snapshot of the last sync. |
| Confirmed durability | Only sync returning Ok confirms prior mutations, as far as the host can confirm. A failed sync confirms nothing new. |
| Ordinary close/reopen | No barrier and no rollback. Unsynchronized bytes, including an uncertain append's partial tail or a destructive cut, may remain visible. |
| Power failure | Unsynchronized changes may be lost or partial. Previously synchronized bytes are protected only until a later destructive operation removes them. Reopen cannot reconstruct removed bytes. |
| Truncate | Can discard synchronized bytes before another sync, even when truncate returns StorageUncertain. Use replacement when the whole old or new record must survive. |
| Uncertain append | Recover the core's own format on reopen: scan complete records, discard an invalid tail, then sync before resuming. Never treat reopening as rollback to the last confirmed offset. |
| StorageUncertain | Reads remain available, but parent append, truncate, sync, replace and pending commits return Usage. Drop/reopen before writing, then recover the format. |
| Replacement append | Storage restores the previous temporary bytes and permits retry. StorageUncertain permanently breaks the replacement and poisons parent writes; parent bytes remain unchanged. Drop the replacement, reopen the parent and recover before retrying. Even an empty append to the broken replacement returns Usage. |
| Replacement commit | First synchronizes the current parent bytes and name, including mutations made while the replacement was built. If that barrier fails, publication is blocked with StorageUncertain and no new durability confirmed. After it succeeds, atomic publication preserves the previous complete content or the replacement across a crash. Ordinary failure preserves the parent; uncertain publication requires reopening. Dropping an uncommitted replacement discards it. |
| Failed read | ContentReader, Record and Replacement buffers may be partially filled. Propagate the error; never consume that buffer as valid content. |
| MemoryStorage tests | Uncertain append/replacement-append writes half the supplied bytes, rounded down; uncertain truncate applies the cut. Cuts also shorten the scripted power-failure bytes. Failed record reads fill half a valid requested range. Ordinary write failures remain without effect. Power-cut simulation is one deterministic possible outcome, not every host outcome. |

## Names and locks

Every name accepted by `is_record_name` is usable on every Storage, including
`record.lock`, `record.create-0.tmp` and `record.retain-9.tmp`. The file backend
escapes sidecar conflicts: replace dots with underscores and surround the
result with underscores (`record.lock` becomes `_record_lock_`). This takes
precedence over Windows device mapping (`con.lock` becomes `_con_lock_`).
`file_name` and `record_name` are the authoritative reversible mapping.
Existing usable ordinary names and device-name mappings are byte-identical;
existing stores need no migration. Raw sidecar filenames are not record names
returned by `record_name`, and path constructors still refuse sidecars.

The host must never access one file through both FileStorage names and path
records, PathLock or path inspection, even through symbolic links or different
directories. Named records lock `FILE_lock`; path access locks `FILE.lock`.
These domains do not exclude each other, and this host precondition is not
checked by the backend. Keep a single access domain for each file across all
programs and tools. Named opens now reject record symbolic links, including
dangling links, with Usage: separate names would otherwise permit two writers
through separate locks. Replace existing symbolic aliases with ordinary record
files or use only path access, which canonicalizes symbolic aliases. Arbitrary
hard-link aliases remain outside coordination. Trusted directories and avoiding
hard-link aliases remain required; hosts must not swap entries for aliases
while records are open.

## Consumer migration

READ — local consumer source snapshots were inspected read-only; no consumer
was edited or built for this part.

- PC `crates/pc-core/src/mount.rs:327` and `:352`: TrackedRecord and
  TrackedReplacement already forward errors. Keep StorageUncertain distinct,
  including replacement append failures, and never retry a poisoned record.
  Add adapter tests for partial tails, failed read buffers and destructive
  cuts using the updated MemoryStorage model.
- PC `crates/journal/src/sink.rs:222,243`: recovery already scans and truncates
  to the safe offset, then syncs. Preserve that logic on ordinary reopen as
  well as crash recovery; add uncertain-append and uncertain-cut regressions.
  Retention must continue using replacement, not truncate-and-rewrite.
  `crates/journal/src/store.rs:25,64` already propagates failed reads before
  advancing its cursor; retain that behavior with partially filled buffers.
  `crates/assembly/src/mount.rs:261` passes records through without a signature
  change; keep immutable bases and sector overlays separate.
- PC `crates/journal-files/src/journal_file.rs:65,107` and
  `crates/journal-files/src/sink.rs:281,503`: retain path-record locking for
  journals and their offline tools. Do not also expose those files through
  FileStorage. SharedRecord needs no signature change. Extend
  `crates/journal-files/tests/journal_storage.rs:236,404` beyond sync/commit
  failures to partial append and destructive-cut outcomes; ordinary reopen
  and power-cut tests must be separate.
- NES `crates/nes-core/src/lib.rs:8` and `src/machine.rs:358`: battery
  persistence is currently absent, so this part requires no storage source
  migration. When implemented, use replacement for whole battery memory and
  discard every failed read buffer.
- Front end `crates/core-launch/src/launch.rs:665,671,720`: Watched wraps
  FileStorage. Keep using file_name for mapping and collision checks; do not
  duplicate escaping. Its assumption that Usage for a valid name means a
  held record is now consistent with sidecar names being usable. Recovery UI
  must explain that recreate reopens current bytes, without promising rollback.
- Front end `crates/pc-image/src/launch.rs:386,389`: the launch table supplies
  JournalFile path records. Keep those paths outside named-store access to
  the same files, including tools in another process. Audit other custom
  Storage adapters for the portable-name rule and deliberate launch scoping.
- Front end `crates/test-core/tests/contract/records.rs:94,210`: retain
  replacement battery tests and add replacement-append poisoning and partial
  read coverage. Do not infer general rollback from a chosen power_cut case.

Coordinate dependency pins and implements declarations as described in the
migration index. These are source-review findings; downstream
conformance after migration must be measured in each consumer repository.
