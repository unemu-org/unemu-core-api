# unemu-core-api

The contract between emulator front ends and emulator cores, as one Rust
crate with no dependency of any kind, and beside it `unemu-core-files`,
the contract's storage and content objects over host files.

A **core** emulates one or more platforms. It implements [`Core`]: it
describes itself as data (which platforms, which features, which options)
and creates machines. A **front end** (a desktop window, a command line, a
test harness) holds every compiled-in core as `&dyn Core` and drives each
machine as `Box<dyn Machine>`: run to a deadline, a step count or an event;
read the picture and the sound; send input; change media; reset, save and
load state, finish. Pause is the absence of a call.

The contract has no platform words (no port, interrupt line, scan code,
disk geometry or cartridge mapper), no host clock, no randomness, no
threads, no files and no presentation policy. Time is the core's own, in
virtual ticks at an exact rate. Content and storage are objects the front
end hands in: verified content read at any offset (`ContentReader`) and
records that outlive a run (`Storage`, `Record`). A file a person opens
reaches a core through `Core::identify`, which writes the config
`Core::create` reads. The crate forbids unsafe code and builds where there
is no operating system.

Cores that implement it: [unemupc](https://github.com/unemu-org/unemupc),
an IBM PC/AT-class machine, and [unemunes](https://github.com/unemu-org/unemunes),
an NES, are migrating to contract 0.4; the [migration index](docs/migration-0.4.md)
lists the coordinated source and pin changes.
The front end lives in unemupc's repository for now.

The normative text is the crate's documentation comments and
[`CHANGELOG.md`](CHANGELOG.md). Comments that cite `core-contract.md §N`
refer to the design record in unemupc's repository,
`backlog/docs/design/core-contract.md`; the comments themselves say what a
core and a front end must do.

## Depending on it

Pin the commit of a release tag, as an exact revision:

```toml
# The contract between front ends and cores, at the commit of release v0.5.0.
unemu-core-api = { version = "0.5", git = "https://github.com/unemu-org/unemu-core-api", rev = "<commit of the release>" }
```

Never a branch, never a tag name, never a `path` dependency and never a
copy of the source. To work against a local checkout, patch the URL in a
gitignored `.cargo/config.toml`:

```toml
[patch."https://github.com/unemu-org/unemu-core-api"]
unemu-core-api = { path = "<your checkout>" }
# Only when you depend on it too:
unemu-core-files = { path = "<your checkout>/files" }
```

The patch applies to the whole dependency graph, so a pinned core that
names the same URL builds against the local checkout too.

The feature `test-support` adds `unemu_core_api::test_support`: a scripted
storage in memory that models durability and planted failures, and content
in memory. Enable it for a core's or a front end's tests only:

```toml
[dev-dependencies]
unemu-core-api = { version = "0.5", git = "https://github.com/unemu-org/unemu-core-api", rev = "<commit of the release>", features = ["test-support"] }
```

## Storage and content over host files

`unemu-core-files`, in [`files/`](files/), is the contract's objects over
host files: `FileStorage`, a directory of records, one file each, held
through a file lock, synchronised and replaced in one atomic step, and
`FileContent`, content read from a file at any offset after boundary verification.
Its constructors check length, not a digest; later change detection is best
effort. A front end links it, and so does a core's own command line, which is a front end
of one core. A core library never does: it takes the objects a front end
hands in, so it also runs where there are no files. It is the one crate of
this repository that touches the host, and its tests run on Linux, macOS
and Windows. It is versioned with the contract and pinned at the same
revision:

```toml
unemu-core-files = { version = "0.5", git = "https://github.com/unemu-org/unemu-core-api", rev = "<commit of the release>" }
```

- [`docs/versioning.md`](docs/versioning.md): what a version number means
  and when a release breaks.
- [`docs/embedding.md`](docs/embedding.md): how a core and a front end
  depend on the crate, and why there is only ever one copy in a build.
- [`docs/conformance.md`](docs/conformance.md): what a core must hold.
- [`docs/migration-0.4.md`](docs/migration-0.4.md): the consumer migration index.
- [`docs/migration-0.5.md`](docs/migration-0.5.md): additive offline media and export (unreleased).
- [`docs/libretro.md`](docs/libretro.md): the accepted extension plan after 1.0.

## Building

```sh
cargo test --workspace
cargo test --workspace --all-features
cargo doc --workspace --no-deps --all-features
bash scripts/no-dependency.sh
bash scripts/no-host.sh
```

`no-dependency.sh` and `no-host.sh` check the contract's package; the
crate in `files/` touches the host by design.

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
