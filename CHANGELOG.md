# Changelog

One section per release, newest first. Every entry is marked **breaking**
or **added**. Every release names the commit a consumer pins
([`docs/embedding.md`](docs/embedding.md)); the version rules are in
[`docs/versioning.md`](docs/versioning.md).

## 0.6.0 — 2026-11-24 — contract 0.6

Commit to pin: `<commit of the release>`

Contract 0.6 adds explicit content bindings
([decision](docs/decisions/0002-content-bindings.md)): a config names a
package use and an item in it, the core lists the bindings and composes the
document with front-end supplied package facts into a digest-addressed
prepared config. Both operations are optional and answer Unsupported by
default; a 0.5 core only states 0.6.

- **added** Optional Core::binding_slots and Core::compose, advertised together
  by PlatformDescription::content_bindings (false by default). Both methods
  default to Unsupported before checking arguments. Package-local item facts,
  kinds and roles resolve to a host-free digest-addressed ConfigDocumentBuf
  and matching Validated list in PreparedConfig, consumed by ordinary create.
  ContentSource supplies verified full SHA-256/size receipts without core rehash.
- **added** Canonical occurrence slots, document-owned zero disks identified
  by declared disk id and size, Config diagnostics for inconsistent package
  bindings and package-local role checks.
  ScriptedBindingCore and check_prepared support front-end tests. Accepted
  [decision](docs/decisions/0002-content-bindings.md),
  [migration](docs/migration-0.6.md) and conformance rules describe the seam.

## 0.5.0 — 2026-11-23 — contract 0.5

Commit to pin: `316dd2a1f429cc3ef37a2bed203e89b2160079ff`

Contract 0.5 adds offline media description and export through the core
([decision](docs/decisions/0001-offline-media-and-export.md)): the front end
no longer needs disk geometry or file formats. Every addition is optional
and answers Unsupported by default; a 0.4 core only states 0.5. Consumers
start at the [0.5 migration guide](docs/migration-0.5.md).

- **breaking** Both packages are 0.5.0 and CONTRACT is 0.5; before 1.0 a
  front end drives only cores that state its own minor, so every core
  states 0.5.

- **added** Optional config-only Core::describe_media returns each configured
  SlotDescription, display-only detail and an optional fixed byte length.
  Core::export streams current media readers through host-independent sinks;
  Core::export_slots exposes eligible/required slots per format. All three
  methods default to Unsupported, keeping 0.4-style implementations valid.
- **added** PlatformDescription::export_formats defaults to empty. Const
  ExportFormat constructors and its list builder check fields and unique ids;
  Slot and Machine scopes keep format knowledge in the core.
- **added** Export requests, streamed outputs, safe plain relative names with
  no overwrite, sink-owned borrowed handles, closing finish, sink-provided
  full SHA-256 receipts and explicit loss notes. Front ends verify reports
  against their own created-output records and receipts.
  Machine documents use sink names relatively. Exports are deterministic;
  front ends remove created outputs on failure. The error matrix extends to
  offline methods without changing Machine statuses.
- **added** MemoryExportSink and ScriptedCore with slot/machine formats,
  external compatibility and conformance tests, an accepted
  [decision record](docs/decisions/0001-offline-media-and-export.md) and
  [migration guide](docs/migration-0.5.md). No version bump or release pin yet.

## 0.4.0 — 2026-11-23 — contract 0.4

Commit to pin: `563f3089f2ad26a189636624faa4696b2bc79d07`

Breaking 0.4 completes the contract review: terminal Finished status,
immediate media changes and honest storage guarantees. 1.0 comes later,
after compatibility review; libretro stays after 1.0, NES first. Consumers
start at the [single migration index](docs/migration-0.4.md).

- **breaking** Both packages are 0.4.0 and CONTRACT is 0.4. Coordinate core
  implements literals and exact git pins across the dependency graph.
- **breaking** Every fallible Core/Machine/content/storage method now has an
  exhaustive error/effect matrix. Unknown/duplicate/invalid run options and
  invalid display ids use Usage. Invalid input ports, event kinds for a port,
  pointer buttons and unadvertised pad controls return Usage without changing
  the input queue. Default identify rejects receipt-size mismatches with Content.
  Keep StorageUncertain distinct. Validation
  checks config only for equal platform/config and baseline options; it
  cannot promise create success. Content lists describe identity sets, ignoring
  order, duplicates, labels and repeated/cached opens.
- **added** Content implements ContentReader to normalize Bytes/Reader without
  allocation, read-ahead or rehash. Unnegotiated sources keep those baseline
  representations; future variants need version, absence, advertisement and
  fallback rules. Full SHA-256 plus length stays at the boundary, without
  sampling. FileContent constructors check length and detect later changes
  best effort; they do not verify a digest.
- **added** External baseline core/front-end compatibility fixtures cover
  default methods, trait objects, content forms, wildcard enum handling and
  failed storage. Public test-support APIs are explicitly covered by SemVer
  from 1.0. Versioning documents future registry packaging work; publish flags
  stay false. The accepted libretro extension plan adds no API now.

- **breaking** Storage recovery now distinguishes current visibility, confirmed
  durability, ordinary reopen and power failure. Reopen after StorageUncertain
  permits writes without rollback. Failed appends can leave partial tails;
  destructive cuts can discard synchronized bytes before another sync. Cores
  recover their own formats and use replacement for whole-record preservation.
  Failed read buffers may be partially filled and must not be consumed.
- **breaking** Replacement append explicitly restores its previous bytes on
  Storage failure, or permanently breaks the replacement and poisons parent
  writes on StorageUncertain, without changing parent bytes. The test-support
  storage now models partial uncertain appends, destructive uncertain cuts,
  failed read prefixes and broken replacements deterministically.
- **breaking** Replacement commit first synchronizes the current parent and
  its name before publication, preserving its complete previous content even
  after unsynchronized appends. A failed parent barrier prevents publication
  and follows the failed-sync rules; no new durability is confirmed.
- **breaking** Hosts must choose one lock domain per file: FileStorage locks
  FILE_lock, while path records, inspection and PathLock
  lock FILE.lock. The backend does not detect mixed access. Trusted-directory
  and hard-link-alias limitations remain. Named record opens now reject
  symbolic links, including dangling links, with Usage to prevent two names
  granting two writers access to the same file. Consumer migration is in
  [`docs/migration-0.4-storage.md`](docs/migration-0.4-storage.md).
- **added** Every valid record name now works in FileStorage, including sidecar
  spellings. file_name/record_name reversibly escape these names; every existing
  usable filename mapping stays byte-identical, with no store migration.
  File records refresh their visible length after an uncertain append or a cut
  whose resulting length could not be read.

- **breaking** Successful `Machine::finish` now enters terminal
  `Status::Finished`. Cores reject execution, reset, media changes, input
  and state I/O in Finished and Failed with Usage, including repeated finish.
  Status rejections preserve status; only finalization I/O failures set Failed.
  `Machine::input` returns `Result<(), Error>`; front ends must handle it.
  Inspection and drains remain available; terminal `set_outputs` is ignored.
- **breaking** The Machine call/status matrix is explicit in its trait docs
  and `docs/conformance.md`: absolute limits, unbounded runs until an event,
  stop priority, exhaustion, reset counters/queues and Off/Halted behavior.
  Both reset kinds preserve counters and clear queued input and held controls
  (absolute pointer positions remain). Runs need no limit; front ends enforce
  their own budgets. Reject unavailable frame stops before consuming input.
  Halted/Off runs return immediately, even for unbounded requests or unsupported
  frame stops. Matrix status rejections (including Off) precede absent-capability
  Unsupported, which precedes argument Usage.
- **breaking** Media changes complete before returning; opening errors belong
  to the media call. Ordinary failures preserve insertion and catalogue;
  StorageUncertain leaves Failed. Execution content/record failures surface from
  run and normally set Failed, with the documented halt-time durability exception;
  failures of other calls follow their method-specific rules. Compatible aliases
  with the same base identity, format and kept-record name reuse the held record.
  `Features::media_changes` promises selection only. Cores and front ends
  advertise/check `media_additions` separately for
  catalogue extension; selection-only cores need not implement `add_medium`.
- **breaking** ContractVersion, Rate, FrameArea and the struct-like variants
  of InputEvent, InputKind and WriteTarget::Kept are non-exhaustive. Consumers
  use constructors/factories and add `..` to field patterns. HostFilesystem
  is non-exhaustive; use `new()` or `default()`. The scalar wrappers Ticks,
  Key, PortId, DisplayId and SlotId deliberately retain their closed widths.
- **breaking** Rate::new rejects zero denominators (always invalid), and
  MachineDescription::new rejects a zero machine clock (always invalid).
  Frame::new validates positive geometry/aspect and exact pixel count;
  description list builders reject duplicate ids. Empty visible areas are
  invalid; with_visible falls back to the whole picture for empty intersections.
  A pixel serial does not cover geometry, aspect or visible-area changes:
  front ends read that metadata every time. Before the first picture they
  handle either Usage or an initial blank frame.
- **breaking** Saved-state identity covers effective configuration, catalogue,
  base media and mutable data dependencies. Mutable data is captured or its
  generation is in the token. Failed loads preserve all observable state,
  queues and records; successful loads restore counters, status and input,
  discard pending audio and never rewind or commit external records.
- **breaking** FileError::stage is the non-exhaustive FileStage enum rather
  than a string. Match its variants with a wildcard; Display retains the
  existing diagnostic text and the original host error remains available.
- **added** Const FrameArea::new, checked Rate::checked_new and Rate::is_clock,
  const InputEvent/InputKind factories (including pads with empty vectors),
  const WriteTarget::kept, FileError::new and HostFilesystem::new/Default. RunLimit::at_steps is the clearer alias for
  steps(n), which continues to mean an absolute total, not n more steps.
- **added** The test-support ScriptedMachine models the lifecycle, immediate
  media calls, planted failures, output metadata and atomic saved-state loads
  for core/front-end conformance tests. Downstream integration tests construct
  public data through its supported factories. The source migration list is
  in `docs/migration-0.4-shapes.md`.

## 0.3.1 — 2026-11-21 — contract 0.3

Commit to pin: `afba6e6b217946eb57b9556e79a30bbe53c90fb4` (tag `v0.3.1`)

- **added** Correct first-writer behavior under a restrictive umask:
  `FileRecord` and `FileStorage` retain the synchronized creation descriptor
  after publication so appends do not require reopening the published path.
  Path records preserve the protected file's permissions when supplied;
  named records keep 0666 filtered by the creation umask. Replacements also
  retain their writable descriptor. No contract or public API changes.

## 0.3.0 — 2026-11-20 — contract 0.3

Commit to pin: `0d490fcb0b97320ac785b12f84a5c49e72f28b1d` (tag `v0.3.0`)

- **breaking** CONTRACT is 0.3 and the package version 0.3.0. A front end
  of 0.3 refuses a core that states 0.2 (`ContractVersion::drives`); a core
  moves by stating 0.3, nothing else of the contract changed.
- **added** `unemu-core-files::FileRecord::{open, open_with, inspect}`:
  a contract record at an arbitrary path, sharing the record and replacement
  implementation with `FileStorage`. The format stays with the consumer:
  expected initial bytes and optional protected-file metadata are the inputs.
  `PathLock` holds the stable `FILE.lock`, supports shared inspection and an
  explicit absent-lock fallback in a read-only directory. Initial publication
  uses `FILE.create-N.tmp`, a file barrier and a no-clobber hard link;
  replacement uses `FILE.retain-N.tmp`, preserves permissions, synchronizes
  and atomically renames. Cleanup checks opened and current inode identities,
  preserves protected aliases and only removes creation prefixes that match.
  `Filesystem` and `FileIo` expose the host operations, including cleanup and
  borrowed retention injection (`FileRecord::replace_with`). `FileError`
  preserves opening stages and host errors; uncertain writes poison further
  mutations while reads remain available.

## 0.2.0 — 2026-11-18 — contract 0.2

Commit to pin: `56c4640146339e967b69925b724bbec0cf878df8` (tag `v0.2.0`)

Storage handed in as objects, and what a second platform needs. The release
as a whole is **breaking**: a core or a front end written for 0.1 must be
edited, and a front end of 0.2 drives only cores that state 0.2. After it
the contract names no file system or path type.

- **breaking** `CONTRACT` is 0.2 and the package version 0.2.0. A front end
  of 0.2 refuses a core that states 0.1 (`ContractVersion::drives`).
- **breaking** `Content::File(std::fs::File)` is gone. Large content is
  `Content::Reader(Box<dyn ContentReader>)`: verified read-only bytes read
  at any offset, failing with `Usage` past the end, `Content` when it
  detects that the bytes are no longer the verified ones (a core must not
  rely on detection), and `Storage` when the host read failed.
  `Content` keeps `Debug` by hand; a reader shows its size.
- **breaking** `CreateRequest::session: &Path` is gone. The machine's
  storage is `CreateRequest::storage: Rc<dyn Storage>`, and
  `CreateRequest::new` takes it in place of the session.
- **breaking** `WriteTarget::Kept { file: &Path }` is
  `WriteTarget::Kept { record: &str }`: a record of the machine's storage,
  named by the front end with the reserved prefix `kept-`
  (`KEPT_RECORD_PREFIX`, `is_kept_record_name`); a core's own records never
  start with it. `add_medium` fails with `Usage` for a name outside the
  rules; the core opens the record at the latest when the medium is
  inserted. From 0.4, the opening call reports failure immediately (see
  0.4.0 above).
- **added** `Storage`, `Record` and `Replacement`: records a machine holds
  exclusively, appended to, cut and synchronised, and replaced whole in one
  step (`Record::replace`, `Replacement::commit`). `is_record_name` states
  the rule of a record name (1 to 64 bytes of `a-z`, `0-9`, `-` and `.`, the
  first and the last a letter or a digit); a storage refuses other names
  with `Usage`. Names a host file system treats specially (`con`, `nul`,
  `com1`) are the file-backed implementation's to map. Failure rules: a
  `Storage` failure of `append` or `truncate` leaves the record as it was
  (an implementation that cannot undo a partial write answers
  `StorageUncertain`); a failed `sync` is `StorageUncertain`; after
  `StorageUncertain` reads work and writes fail with `Usage` until the
  record is opened again, and an uncertain commit leaves the old or the new
  contents, whole. A replacement whose record was released fails to commit
  with `Usage`. A cut beyond the end and a second replacement fail with
  `Usage`.
- **added** `ErrorKind::StorageUncertain`: a host write whose outcome is
  unknown; the record is opened again before it is written. At a run
  boundary it fails the machine as `Storage` does. `Usage` now also covers
  a call that is not valid for the object it was made on.
- **breaking** (a rule) What a run may fail with: `Guest`, `Storage`,
  `StorageUncertain` and `Content`, each leaving `Status::Failed`; a
  `Storage` or `StorageUncertain` failure while the guest halts may leave
  `Halted`. What was durable stays durable, and the front end retries by
  dropping the machine and creating it again. A `StorageUncertain` failure
  of `reset` leaves `Failed` (`Halted` for a halted machine); a failed
  `finish` leaves `Failed`.
- **added** The feature `test-support` and the module `test_support`:
  `MemoryStorage`, a storage in memory with durable and current bytes, a
  power cut, and planted failures (`fail_next` for every record operation
  and `Replacement::append`, `uncertain_commit`), and `MemoryContent`,
  content in memory that fails a read on request (`fail_next`) or every
  read after `change` with `Content`. The crate still has no dependency of
  any kind.
- **added** Content a person opens: `ContentKind` (an id, a name for
  people, file name extensions; `ContentKind::new` refuses ids and
  extensions outside their rules, at compile time in a `static`),
  `PlatformDescription::content` and `with_content`. `ContentKind::is_valid`
  checks a kind read from a description. A front end compares a file's
  extension after lowering its ASCII letters.
- **added** `Core::identify` with `OfferedContent` and `Identified`, and
  `ConfigDocumentBuf` for the config it writes. The default answers
  `Ok(None)`.
- **added** `Core::validate` with `Validated` and `ContentRef`. The default
  answers `Unsupported`.
- **added** An absolute pointer: `InputEvent::PointerPosition` (a place on
  the whole picture in fractions of 65 536, or `None` off the picture),
  `InputKind::AbsolutePointer`, and `pointer_pixel` for the pixel a
  position means. `ReleaseAll` leaves a pointer's position.
- **added** The picture: `Frame::visible`, a `FrameArea` within the
  picture, set by `Frame::with_visible` and whole by `Frame::new`;
  `FrameArea::is_within` for a front end that checks it.
- **breaking** (a rule, no signature change) `Machine::frame` returns the
  last picture the machine finished, never one it is still drawing.
- **breaking** (a rule, no signature change) `AudioDescription::rate` is the
  machine's own rate, from 8 000 to 192 000 frames per second; a faster
  generator is divided by a whole number inside the core; one channel goes
  in both samples. A front end converts to whatever it plays.
  `AudioDescription::MIN_RATE`, `MAX_RATE` and `rate_in_bounds` are
  **added**.
- **breaking** (a rule, no signature change) Sound keeps its place in
  virtual time: frame `i` is the sound at virtual time `i × clock / rate`
  ticks from creation. At every return of `run`, once `drain_audio` has
  been called, the next frame it will hand over has index
  `floor(now() × rate / clock)`, within one frame. Frames not drained when
  sound is switched off, or before `load_state`, are dropped; after
  `load_state` the index follows the loaded `now()`. This is stated as a
  position, not as a count since creation, which `set_outputs` and
  `load_state` would break.
- **added** (rules, no signature change) Pads: the ports of kind `Pad` are
  the players in ascending `PortId`; the input ports are constant for a
  machine's life; no core carries a key map. Battery-backed memory: one
  record, written with `replace` and `commit` at `finish` and after ten
  seconds of virtual time with the memory changed (on `Storage`).
- **added** `Identified`, `Validated`, `ConfigDocumentBuf`, `Error` and
  `Message` are `Send`, checked at compile time.
- **added** `scripts/no-host.sh` with its self-test, in CI's "No host" job:
  `src/` names none of `std::fs`, `std::path`, `std::os`, `std::env`,
  `std::process`, `std::net`, `std::thread`, `std::time::Instant` and
  `std::time::SystemTime`, directly or in a grouped `use`, renamed or not.
  The job also builds for wasm32 with all features.
- **added** The crate `unemu-core-files` (in `files/`, the workspace's
  second member, version 0.2.0, depending only on the contract): the
  contract's objects over host files, for front ends and a core's own
  command line, never for a core library. `FileStorage::open(dir)` is a
  directory of records, one file per record, the name mapped by
  `file_name` and back by `record_name` (Windows device names such as
  `con` and `nul.bin` gain `_` after the device part: `con_`,
  `nul_.bin`). A record is held through an exclusive lock on its own lock
  file, across storages and programs, and a held record fails to open
  with `Usage`. `sync` is `sync_all` (`F_FULLFSYNC` on Apple's systems)
  and, on the first sync of each opening, also syncs the directory on
  Unix; a commit writes a temporary sibling, syncs it, renames it over the
  record and makes the rename durable (a directory sync on Unix; on
  Windows, which has no directory sync in the standard library, a second
  sync of the renamed file). The failure rules of `Record` are held; temporary files
  a crash left are removed when the record is opened. `FileContent` is a
  `ContentReader` over a file of verified length that fails a read with
  `Content` when the file's length or modification time changed; its
  reads are positional, so threads may share one. `FileStorage` keeps the
  directory's canonical path. The locks are the local file system's
  (`flock`, `LockFileEx`), not a network file system's emulation. The
  crate's tests run on Linux, macOS and Windows; `no-dependency.sh` and
  `no-host.sh` keep checking the contract's package only.
- `docs/conformance.md` is the checklist of contract 0.2.

## 0.1.0 — contract 0.1

Commit to pin: `4e2dd8325b54ce652bf4884fd85da602b14ae726` (tag `v0.1.0`)

The contract as unemupc's PC core and desktop front end use it, moved
unchanged into its own repository.

- **added** `ContractVersion`, the constant `CONTRACT` (0.1) and
  `ContractVersion::drives`: during major 0 a front end drives only cores
  that state its own minor.
- **added** `Core`, a stateless factory for machines, described as data by
  `CoreDescription`, `PlatformDescription`, `Features`,
  `OptionDescription` and `Notice`.
- **added** Creating a machine: `CreateRequest` with a `ConfigDocument`,
  a session directory and a `ContentSource` that hands out content by
  SHA-256 digest and size.
- **added** `Machine`: `run` to a `RunLimit` (a deadline in `Ticks` at an
  exact `Rate`, a step count, a stop on a finished frame) answering a
  `Stop`; `now`, `steps` and `Status`.
- **added** Picture and sound out: `Frame`, `DisplayDescription`,
  `AudioDescription` and `Outputs`.
- **added** Input: `InputEvent` with keys as USB HID usages (`Key`),
  pointer events and pad controls, described per port by
  `InputPortDescription`.
- **added** Media: slots (`SlotDescription`, `SlotState`) with catalogues
  of media (`NewMedium`, `MediumInfo`, `MediaAction`, `WriteTarget`).
- **added** Control: `reset` by `ResetKind`, saved state with
  `state_identity`, `finish` answering `Finished`.
- **added** Errors and messages: one `Error` with an `ErrorKind`,
  `Diagnostic`, and `Message`s drained by the front end instead of
  printed by the core.
- **added** The crate's rules: no dependency of any kind, object-safe
  traits without generics, every struct and enum `#[non_exhaustive]`,
  no unsafe code.
- Licensed `MIT OR Apache-2.0`.
