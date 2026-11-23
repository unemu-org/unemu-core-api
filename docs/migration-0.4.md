# Migrating to contract 0.4

This is the consumer migration index for 0.4.0. Move every core and front end
in a build together, using one exact release revision. The release's commit
to pin remains `<commit of the release>` until the maintainer tags it.

- [API shapes, lifecycle, media and state](migration-0.4-shapes.md): factories,
  extensible patterns, Finished, fallible input, immediate media changes,
  separate selection/addition features, limits and saved-state atomicity.
- [Storage guarantees, names and locks](migration-0.4-storage.md): honest
  recovery after uncertainty, partial reads/tails, destructive cuts, replacement
  barriers, portable names and separate named/path lock domains.
- [Error/validation matrix](conformance.md#errors-and-validation): allowed kinds
  and effects for every fallible method. Unknown/duplicate/invalid run options
  use Usage, not Config. Invalid display ids use Usage, not Unsupported, when
  picture output is available. Preserve status/capability/argument precedence
  and StorageUncertain as a distinct failure. Reject invalid input ports,
  mismatched event kinds, pointer buttons and unadvertised pad controls with
  Usage before queuing, preserving the queue. Receipt-size mismatches are
  Content even with default identify. A repeated finish preserves Finished;
  only finalization I/O failures set Failed.
- [Content compatibility](conformance.md#content-source-compatibility): sources
  keep supplying Bytes/Reader without negotiation. Content now implements
  ContentReader for common offset reads; adopting the helper is optional.
  Verification remains full SHA-256 plus length at the boundary, no sampling
  or core rehash. FileContent constructors only check length and record metadata
  for best-effort change detection; they do not verify a digest.
- Validation checks config only for equal platform/config and baseline options;
  success does not promise create success. Treat content lists as identity sets
  of (SHA-256, length), ignoring order, duplicate references, labels and repeated
  or cached opens. Keep content-dependent checks in identify/create.
- Re-pin both packages to the same release commit with version requirements
  `0.4`; change core implements literals to ContractVersion::new(0, 4), and
  update front-end version/refusal tests. Keep exact git pins for 0.4 as in
  [embedding](embedding.md). No registry publication or libretro API is added;
  [the wrapper plan](libretro.md) is after 1.0, NES first. Public test-support
  APIs are covered by the [1.0 SemVer freeze](versioning.md).

CONSUMER MIGRATION — READ, source snapshots only; no downstream edits or builds:

- PC: `crates/pc-core/src/core.rs:80` already uses Usage for unknown/duplicate
  options; keep it. `crates/pc-core/src/machine.rs:695` currently returns
  Unsupported for an invalid display: change that to Usage. Update the literal
  in `crates/pc-core/src/core.rs` and the workspace dependency pins. In
  `crates/pc-core/src/mount.rs:136`, Bytes/Reader with a wildcard remains valid;
  optionally replace variant dispatch with ContentReader calls.
- NES: `crates/nes-core/src/lib.rs:120` currently returns Config for unknown
  options: change it to Usage. `crates/nes-core/src/machine.rs:237` already
  returns Usage for an invalid display. Keep damaged/unsupported cartridge
  formats as Unsupported (`lib.rs:156`), distinct from receipt integrity.
  Keep validate config-only (`lib.rs:109`) and Bytes/Reader fallback (`:177`).
  Re-pin `crates/nes-core/Cargo.toml` and update its implements literal.
- Front end in `unemu`: `Cargo.toml:61,64` pins both packages to 0.3;
  `crates/core-launch/src/tests.rs:57` asserts 0.3. Update pins/refusal tests
  together with both production cores. Keep receipt hashing on the retained
  handle (`crates/core-launch/src/launch.rs:425`) and Bytes/Reader sourcing
  (`:639`). Audit error branches in `launch.rs:345` and
  `crates/machine-view/src/session.rs:397,417` against the matrix.
- Front end in `unemu-host`: `Cargo.toml:16` pins the API to 0.3. Update that
  pin and `crates/test-core/src/lib.rs:159`'s implements literal. Its unknown
  option branch (`:203`) and the equivalent test core in `unemu` (`:204`)
  currently use Config: move both to Usage. Audit their validators (`:265`
  and `:266`) as config-only, and keep enum wildcards/content fallbacks.
  State/read failure tests in `crates/machine-view/src/session/tests.rs:351`
  must keep StorageUncertain distinct from Storage.

The two linked migration documents contain further source sites for the first
two parts. Their findings and this list need measured consumer conformance
runs after migration. They do not claim the consumers already implement 0.4.
