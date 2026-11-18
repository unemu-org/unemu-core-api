# Changelog

One section per release, newest first. Every entry is marked **breaking**
or **added**. Every release names the commit a consumer pins
([`docs/embedding.md`](docs/embedding.md)); the version rules are in
[`docs/versioning.md`](docs/versioning.md).

## 0.1.0 — contract 0.1

Commit to pin: `<filled in at tag time: the 40 hex digits of the v0.1.0 commit>`

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
