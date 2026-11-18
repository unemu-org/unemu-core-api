# Depending on the contract

A core and a front end name the crate the same way: by the git URL and the
exact commit of a release tag.

```toml
unemu-core-api = { version = "0.2", git = "https://github.com/unemu-org/unemu-core-api", rev = "<the 40 hex digits of the tag's commit>" }
```

- An exact revision, never a branch and never a tag name: a tag can be
  moved.
- Never a `path` dependency in a committed manifest: it breaks CI and
  every other checkout. Never a copy of the source.
- For local work, a gitignored `.cargo/config.toml`:

  ```toml
  [patch."https://github.com/unemu-org/unemu-core-api"]
  unemu-core-api = { path = "<your checkout>" }
  ```

- A core's or a front end's tests may enable the feature `test-support`
  (a storage and content in memory) in `[dev-dependencies]`, on the same
  URL and revision. A library never enables it for its users.

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
