# What a core must hold

The documentation comments say what each call must do. This list is what
they cannot say about a core as a whole. It is the checklist a new core,
and every change to a core, is reviewed against.

1. **The dependency.** The crate by the exact revision of a release tag
   ([`embedding.md`](embedding.md)), never a path, a branch or a copy.
   `CoreDescription::implements` is a literal, such as
   `ContractVersion::new(0, 1)`.
2. **An honest description.** Every control a description promises works:
   no call that the core, platform or machine description promises
   answers `ErrorKind::Unsupported`. A core has a test that drives it only
   as `&dyn Core` and `Box<dyn Machine>` and checks this.
3. **Stable ids.** The core id, platform ids, config types and slot ids
   are chosen once: saved sessions and front-end settings name them.
4. **Content by digest.** `create` takes every blob from the
   `ContentSource` by the digest and size its config names, and writes
   only below the session it is given.
5. **Time and runs.** Virtual time in ticks at an exact `Rate`, steps in
   the core's own unit; deadlines and limits as `Machine::run` describes.
   Input and media changes take effect at run boundaries, in order.
6. **Determinism.** No host clock, no random source, no thread and no
   process-wide state in the execution path. The same creation and the
   same calls give the same pictures, sound, messages and stops; a core
   keeps a test of that.
7. **Messages, not prints.** A core never writes to the standard streams;
   it reports through `Message`s and errors by `ErrorKind`.
8. **Embeddable.** The core and what it links forbid unsafe code where
   they can, and add no crate that touches the host (files, network,
   windows, audio devices) to the library a front end links. A core's
   own command line is a separate crate.
9. **The hand-over.** A core names to the front end the revision to pin,
   the contract revision it was built against, and the licences of
   everything it links.

Later contract versions add to this list; the `CHANGELOG.md` entry of a
release says when they do.
