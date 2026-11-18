# What a core must hold

The documentation comments say what each call must do. This list is what
they cannot say about a core as a whole. It is the checklist a new core,
and every change to a core, is reviewed against. It is the list of
contract 0.2.

1. **The dependency.** The crate by the exact revision of a release tag
   ([`embedding.md`](embedding.md)), never a path, a branch or a copy; a
   gitignored patch for local work. `CoreDescription::implements` is a
   literal, such as `ContractVersion::new(0, 2)`.
2. **The description.** A core id, its platforms, their config types and
   options, its notices. A platform that a person can start from a file
   lists its `ContentKind`s with their extensions, and at least one config
   type, because `Core::identify` answers in one. `Features` list only
   what every machine of the platform can do.
3. **An honest description.** Every control a description promises works:
   no call that the core, platform or machine description promises
   answers `ErrorKind::Unsupported`. A core has a test that drives it only
   as `&dyn Core` and `Box<dyn Machine>`, with the storage in memory of the
   `test-support` feature, and checks this.
4. **Stable ids and names.** The core id, platform ids, config types,
   content kind ids, slot ids and record names are chosen once: saved
   sessions, stored images and front-end settings name them. The same
   config gives the same record names in every release of the core.
5. **`identify` and `validate`.** A core that opens content implements
   `identify`: `None` for foreign bytes, `Unsupported` with a reason for
   content it recognises and cannot run, otherwise a config that names the
   content by digest and size, never by name, with the platform's
   defaults. It reads only through the offered `ContentReader`, keeps
   nothing and answers the same for the same bytes. A core whose `create`
   reads configs implements `validate`, which fails as `create` would and
   lists exactly the content `create` asks for.
6. **Content by digest, storage as records.** `create` takes every blob
   from the `ContentSource` by the digest and size its config names, and
   keeps what outlives a run only in records of the `Storage` it is given.
   No file, no path, no environment variable. A core's own record names
   never start with `kept-`, which is the front end's for
   `WriteTarget::Kept`; `add_medium` refuses a kept name outside
   `is_kept_record_name` with `Usage`, and the record is opened at the
   latest when the medium is inserted. A base is never written. A failure
   of a `ContentReader` or a record reaches the front end from `run`
   (`Storage`, `StorageUncertain`, `Content`), never the guest, and leaves
   the machine `Failed`; whatever was durable stays durable. A core does
   not rely on a `ContentReader` detecting changed bytes. After
   `StorageUncertain` the record is opened again before it is written.
7. **The machine's description.** An exact clock (`Rate`), a step unit,
   its displays, sound at a rate from 8 000 to 192 000 frames per second
   (`AudioDescription::rate_in_bounds`), its input ports and slots, all
   constant for the machine's life. The ports of kind `Pad` are the
   players, first to last, in ascending `PortId`, each with exactly the
   controls it has. No key map, dead zone or turbo setting.
8. **Time and runs.** Virtual time in ticks at an exact `Rate`, steps in
   the core's own unit; deadlines and limits as `Machine::run` describes;
   `Stop::Frame` when asked and listed. Input and media changes take
   effect at run boundaries, in order; `ReleaseAll` releases every key,
   button and control and leaves an absolute pointer's position.
9. **The picture.** `frame` returns the last picture the machine
   finished, never one it is still drawing, with `aspect` for the whole
   picture, `visible` for the part a display showed for certain, and a
   serial that moves only when the pixels did. The core always hands over
   the whole picture; cropping is the front end's.
10. **Sound.** Frame `i` is the sound at virtual time
    `i × clock / rate` ticks from creation. At every return of `run`, once
    `drain_audio` has been called, the next frame it will hand over has
    index `floor(now() × rate / clock)`, within one frame. Frames not
    drained when sound is switched off, or before `load_state`, are
    dropped; after `load_state` the index follows the loaded `now()`. A
    generator faster than the bound is filtered and divided by a whole
    number inside the core. One channel goes in both samples of a frame.
11. **Battery-backed memory** by the rules on `Storage`: one record,
    opened only when the content has such memory, written with `replace`
    and `commit` at `finish` and after ten seconds of virtual time with the
    memory changed.
12. **Determinism.** No host clock, no random source, no thread and no
    process-wide state in the execution path. The same creation and the
    same calls give the same pictures, sound, messages and stops; a core
    keeps a test of that.
13. **Messages, not prints.** A core never writes to the standard streams;
    it reports through `Message`s and errors by `ErrorKind`.
14. **Embeddable.** The core and what it links forbid unsafe code where
    they can, and add no crate that touches the host (files, network,
    windows, audio devices) to the library a front end links. A core's
    own command line is a separate crate, which may open files.
15. **The hand-over.** A core names to the front end the revision to pin,
    the contract revision it was built against, and the licences of
    everything it links.

Later contract versions add to this list; the `CHANGELOG.md` entry of a
release says when they do.
