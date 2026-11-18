# unemu-core-api

The contract between emulator front ends and emulator cores, as one Rust
crate with no dependency of any kind.

A **core** emulates one or more platforms. It implements [`Core`]: it
describes itself as data (which platforms, which features, which options)
and creates machines. A **front end** (a desktop window, a command line, a
test harness) holds every compiled-in core as `&dyn Core` and drives each
machine as `Box<dyn Machine>`: run to a deadline, a step count or an event;
read the picture and the sound; send input; change media; reset, save and
load state, finish. Pause is the absence of a call.

The contract has no platform words (no port, interrupt line, scan code,
disk geometry or cartridge mapper), no host clock, no randomness, no
threads and no presentation policy. Time is the core's own, in virtual
ticks at an exact rate. The crate forbids unsafe code and builds where
there is no operating system.

Cores that implement it today: [unemupc](https://github.com/unemu-org/unemupc),
an IBM PC/AT-class machine. The front end that drives it lives in unemupc's
repository for now.

The normative text is the crate's documentation comments and
[`CHANGELOG.md`](CHANGELOG.md). Comments that cite `core-contract.md §N`
refer to the design record in unemupc's repository,
`backlog/docs/design/core-contract.md`; the comments themselves say what a
core and a front end must do.

## Depending on it

Pin the commit of a release tag, as an exact revision:

```toml
# The contract between front ends and cores, at the commit of release v0.1.0.
unemu-core-api = { version = "0.1", git = "https://github.com/unemu-org/unemu-core-api", rev = "<the 40 hex digits of the tag's commit>" }
```

Never a branch, never a tag name, never a `path` dependency and never a
copy of the source. To work against a local checkout, patch the URL in a
gitignored `.cargo/config.toml`:

```toml
[patch."https://github.com/unemu-org/unemu-core-api"]
unemu-core-api = { path = "<your checkout>" }
```

The patch applies to the whole dependency graph, so a pinned core that
names the same URL builds against the local checkout too.

- [`docs/versioning.md`](docs/versioning.md): what a version number means
  and when a release breaks.
- [`docs/embedding.md`](docs/embedding.md): how a core and a front end
  depend on the crate, and why there is only ever one copy in a build.
- [`docs/conformance.md`](docs/conformance.md): what a core must hold.

## Building

```sh
cargo test
cargo doc --no-deps
```

The minimum Rust version is the `rust-version` in `Cargo.toml`.

## Licence

Licensed under either of

- Apache License, Version 2.0 ([`LICENSE-APACHE`](LICENSE-APACHE)), or
- MIT license ([`LICENSE-MIT`](LICENSE-MIT))

at your option. The licence of the contract does not change the licence
of any core or front end that uses it.

Unless you explicitly state otherwise, any contribution intentionally
submitted for inclusion in the work by you, as defined in the Apache-2.0
license, shall be dual licensed as above, without any additional terms or
conditions.

[`Core`]: src/core.rs
