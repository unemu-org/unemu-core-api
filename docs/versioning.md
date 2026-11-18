# Versioning

The package version is the contract's: release `0.2.0` is
`CONTRACT` 0.2, and `MAJOR.MINOR` of every release equals the
`ContractVersion` it defines.

## Before 1.0

Every minor may break. A front end drives only cores that state its own
minor (`ContractVersion::drives`), so every minor is a meeting of all
cores and the front end: each core moves to it, then the front end does.

A **minor** release is any change to what a core implements or to what a
front end may call or read: a method, a field, a variant, or a rule in a
documentation comment that a core must hold.

A **patch** release changes none of that: documentation, tests, a
helper behind the `test-support` feature, a named constant for a value
that already existed.

## From 1.0

A minor only adds (a field, a variant, a trait method with a default body
that answers `ErrorKind::Unsupported`); a major breaks. A front end then
drives every core of its major and treats additions newer than the
core's `implements.minor` as absent. From 1.0 the crate is published to
crates.io and consumers may use SemVer requirements.

1.0 comes when two cores of different platforms both run on the contract.

## Releases

- A release is a signed tag `vMAJOR.MINOR.PATCH` on `main`.
- `main` between two tags may hold half of the next minor; nobody pins it.
- Every release has a section in `CHANGELOG.md`, every entry marked
  **breaking** or **added**, and the section names the commit to pin.
- The `rust-version` in `Cargo.toml` starts at the version the first
  consumer builds with and is lowered when a consumer needs it, never
  raised above the lowest consumer's.
