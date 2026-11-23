# Depending on the contract

A core and a front end name the crate the same way: by the git URL and the
exact commit of a release tag. Contract 0.5 keeps exact git pins; registry
SemVer requirements are planned for 1.0, not this release. From 0.4, see the
[0.5 migration guide](migration-0.5.md) (and, from 0.3, the
[0.4 migration index](migration-0.4.md) first).

```toml
unemu-core-api = { version = "0.5", git = "https://github.com/unemu-org/unemu-core-api", rev = "316dd2a1f429cc3ef37a2bed203e89b2160079ff" }
```

- An exact revision, never a branch and never a tag name: a tag can be
  moved.
- Never a `path` dependency in a committed manifest: it breaks CI and
  every other checkout. Never a copy of the source.
- For local work, a gitignored `.cargo/config.toml`:

  ```toml
  [patch."https://github.com/unemu-org/unemu-core-api"]
  unemu-core-api = { path = "<your checkout>" }
  # Only when you depend on it too:
  unemu-core-files = { path = "<your checkout>/files" }
  ```

- A core's or a front end's tests may enable the feature `test-support`
  (a storage and content in memory) in `[dev-dependencies]`, on the same
  URL and revision. A library never enables it for its users.

## Storage and content over host files

The contract hands storage and content to a core as objects
(`Storage`, `Record`, `ContentReader`). `unemu-core-files`, the second
crate of this repository, is those objects over host files:
`FileStorage` and `FileContent`.

- **Who links it:** a front end, and a core's own command line, which is
  a front end of one core and lives in a crate of its own. Both name it
  by the same URL and revision as the contract:

  ```toml
  unemu-core-files = { version = "0.5", git = "https://github.com/unemu-org/unemu-core-api", rev = "316dd2a1f429cc3ef37a2bed203e89b2160079ff" }
  ```

- **Who never links it:** a core library. A core takes the objects the
  front end hands to `Core::create`, so it builds and runs where there
  are no files, and an embedder with its own storage (a database, a
  browser's storage) leaves this crate out.
- A record is one file of the storage's directory, named after the
  record; the crate's documentation states the lock, the durability on
  each host and how Windows device names (`con`, `nul`) are mapped.

## One copy in every build

Cargo builds one copy of a git dependency for one URL and one revision.
Two revisions are two different crates with the same name, and a core
built against one does not implement the `Core` trait of the other. A
front end and every core it links therefore pin the same revision. When
they do not, the build fails at the line where the front end lists the
core.

At start-up a front end checks each core by value:
`CONTRACT.drives(core.description().implements)`. `implements` is a
literal the core's author writes, never the `CONTRACT` constant, because
the constant follows whichever copy Cargo resolved. A front end keeps a
test that every compiled-in core passes this check.

## Moving to a new release

1. The release is tagged here.
2. Each core re-pins, updates its `implements` literal and its code, and
   names its resulting revision.
3. The front end moves the contract's pin and every core's pin in one
   commit.

A core that has not re-pinned blocks step 3, not the other cores.
