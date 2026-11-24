# Versioning

The package version is the contract's: release `0.6.0` is
`CONTRACT` 0.6, and `MAJOR.MINOR` of every release equals the
`ContractVersion` it defines. Both workspace packages release together.

## Before 1.0

Every minor may break. A front end drives only cores that state its own
minor (`ContractVersion::drives`), so every minor is a meeting of all
cores and the front end: each core moves to it, then the front end does.

A **minor** release is any change to what a core implements or to what a
front end may call or read: a method, a field, a variant, or a rule in a
documentation comment that a core must hold. Public test-support additions
also belong in a minor.

A **patch** release changes none of that: corrections to documentation,
tests, compatible implementation fixes (including test-support helpers),
or a named constant for a value that already existed. Feature gating does
not permit breaking a helper's public API in a patch.

## From 1.0

A minor only adds compatible API; a major breaks. The freeze covers public
signatures, constructors, field types, implemented traits, bounds, units,
error classification, lifecycle, ownership and storage guarantees. It also
covers every public API behind `test-support`, including scripted helpers.
No existing helper is exempt because it is used only in tests.

New fields must have backward-compatible defaults without changing existing
constructors or implemented traits. Optional trait methods remain object-safe
and have safe defaults (Unsupported for a fallible capability); existing
methods gain no mandatory duties in a minor. New variants require useful
fallback behavior, not just non-exhaustive syntax. Every new capability states
its introduction minor, absence meaning, advertisement and fallback. In
particular, unnegotiated content sources still supply Bytes or Reader.

A front end drives every core of its major, treating additions newer than
the core's implements.minor as absent. ContractVersion::drives also accepts a
newer core minor: older front ends ignore unfamiliar optional capabilities,
use baseline behavior and handle unknown enum cases with wildcards. This is
Rust source compatibility within a unified Cargo build, not a stable binary
ABI. Keep Core: Send + Sync, non-Send machines and Rc<dyn Storage>; a concrete
shareable reader does not make all ContentReader implementations Send/Sync.

Two cores of different platforms are necessary evidence for 1.0, not automatic
permission to freeze. Release review must also settle the documented semantics
and run source compatibility/conformance checks. The libretro wrapper remains
[after 1.0, NES first](libretro.md).

## Packaging at 1.0

0.4 keeps both publish flags false and consumers on exact git revision pins.
No publication is part of this release preparation. Before publishing 1.0:

- Update both package versions and CONTRACT together; remove `publish = false`
  or allow crates.io in both manifests. Keep the contract dependency-free.
- Set the companion's dependency requirement to the released registry API
  version. A local path may remain for workspace development, but the version
  must resolve from crates.io in the packaged manifest. Publish the API before
  the companion, then verify resolution without local patches or git sources.
- Inspect `cargo package --list` and the built package archives for both crates:
  licence texts, README, normative docs, changelog, metadata and manifest
  dependency requirements must be present and correct. Include companion-local
  documentation/licences or adjust packaging if files outside its package root
  are omitted; a repository checkout is not evidence of packaged contents.
- Build/test the packaged crates and the external compatibility fixtures;
  check the minimum Rust version and core/host separation. Complete registry
  metadata, ownership and release requirements before actual publication.
- Update embedding examples to supported registry SemVer requirements and
  coordinate every consumer. Git and registry sources have different type
  identities: a compatible version requirement alone cannot unify them.
  Choose one Cargo source and resolved API version throughout a build.

## Releases

- A release is a signed tag `vMAJOR.MINOR.PATCH` on `main`.
- `main` between two tags may hold half of the next minor; nobody pins it.
- Every release has a section in `CHANGELOG.md`, every entry marked
  **breaking** or **added**, and the section names the commit to pin.
- The `rust-version` in `Cargo.toml` starts at the version the first
  consumer builds with and is lowered when a consumer needs it, never
  raised above the lowest consumer's.
