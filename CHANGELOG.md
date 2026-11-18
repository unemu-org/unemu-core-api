# Changelog

One section per release, newest first. Every entry is marked **breaking**
or **added**. Every release names the commit a consumer pins
([`docs/embedding.md`](docs/embedding.md)); the version rules are in
[`docs/versioning.md`](docs/versioning.md).

## 0.2.0 — 2026-11-18 — contract 0.2

Commit to pin: set when the release is tagged.

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
  inserted, and a failure to open it fails the next run.
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
