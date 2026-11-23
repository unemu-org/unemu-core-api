# What a core must hold

The documentation comments say what each call must do. This list is what
they cannot say about a core as a whole. It is the checklist a new core,
and every change to a core, is reviewed against. It is the list of
contract 0.4 (unreleased).

1. **The dependency.** The crate by the exact revision of a release tag
   ([`embedding.md`](embedding.md)), never a path, a branch or a copy; a
   gitignored patch for local work. `CoreDescription::implements` is a
   literal, such as `ContractVersion::new(0, 3)`.
2. **The description.** A core id, its platforms, their config types and
   options, its notices. A platform that a person can start from a file
   lists its `ContentKind`s with their extensions, and at least one config
   type, because `Core::identify` answers in one. A platform's `Features`
   are the most a machine of it can do; each machine states its own in its
   `MachineDescription`, which may list less, and a front end enables
   controls from the machine's.
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
   latest when the medium is inserted. A base is never written. During
   execution, a `ContentReader` or record failure reaches the front end
   from `run` (`Storage`, `StorageUncertain`, `Content`), never the guest.
   It leaves `Failed`, except for the documented halt-time durability
   exception that may leave `Halted`. Immediate media errors and failures
   of other calls follow their method-specific `Machine` rules; ordinary
   media errors preserve status, while `StorageUncertain` sets `Failed`.
   Whatever was durable stays durable. A core does not rely on a
   `ContentReader` detecting changed bytes. After
   `StorageUncertain` the record is opened again before it is written.
7. **The machine's description.** An exact clock (`Rate`), a step unit,
   its displays, sound at a rate from 8 000 to 192 000 frames per second
   (`AudioDescription::rate_in_bounds`), its input ports and slots, all
   constant for the machine's life. The ports of kind `Pad` are the
   players, first to last, in ascending `PortId`, each with exactly the
   controls it has. No key map, dead zone or turbo setting.
8. **Time and runs.** Virtual time in ticks at an exact `Rate`, steps in
   the core's own unit; deadlines and limits as `Machine::run` describes;
   `Stop::Frame` when asked and listed. Input takes effect at run boundaries
   in order; media changes complete immediately and report opening errors
   themselves; `ReleaseAll` releases every key,
   button and control and leaves an absolute pointer's position.
9. **The picture.** `frame` returns the last picture the machine
   finished, never one it is still drawing, with `aspect` for the whole
   picture, `visible` for the part a display showed for certain, and a
   serial that moves when pixels change. Read geometry, aspect and visible
   area independently on every frame. Before the first picture a core may
   return Usage or a blank frame of its initial geometry. The core hands over
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
    own command line is a separate crate, which may open files, through
    `unemu-core-files` for the contract's objects.
15. **The hand-over.** A core names to the front end the revision to pin,
    the contract revision it was built against, and the licences of
    everything it links.

Later contract versions add to this list; the `CHANGELOG.md` entry of a
release says when they do.

## Machine calls and statuses

Call/status matrix (every cell gives the result and following status).
`same` means unchanged status; `inspect` means the documented observation
or drain, with no guest execution. Apply the matrix's status rejections
first (including Off), then return `Unsupported` for an absent capability,
then check arguments (`Usage` if invalid). These rejections preserve status.
Halted and Off runs return their stop before capability or argument checks.

| Method | Ready | Halted | Off | Failed | Finished |
| --- | --- | --- | --- | --- | --- |
| `description` | description; same | description; same | description; same | description; same | description; same |
| `now` | ticks; same | ticks; same | ticks; same | ticks; same | ticks; same |
| `steps` | total; same | total; same | total; same | total; same | total; same |
| `status` | Ready | Halted | Off | Failed | Finished |
| `frame` | inspect; same | inspect; same | inspect; same | inspect; same | inspect; same |
| `drain_audio` | inspect; same | inspect; same | inspect; same | inspect; same | inspect; same |
| `set_outputs` | set; same | set; same | set; same | ignored; Failed | ignored; Finished |
| `input` | queue; same | queue; same | Usage; Off | Usage; Failed | Usage; Finished |
| `slot_states` | inspect; same | inspect; same | inspect; same | inspect; same | inspect; same |
| `state_identity` | token/None; same | token/None; same | token/None; same | token/None; same | token/None; same |
| `drain_messages` | drain; same | drain; same | drain; same | drain; same | drain; same |
| `run` | stop; see below | Halted stop; Halted | PoweredOff stop; Off | Usage; Failed | Usage; Finished |
| `add_medium` | index; same | index; same | Usage; Off | Usage; Failed | Usage; Finished |
| `change_medium` | success; same | success; same | Usage; Off | Usage; Failed | Usage; Finished |
| `reset` | success; Ready | success; Ready | Hard: success; Ready; Soft: Usage; Off | Usage; Failed | Usage; Finished |
| `save_state` | success; same | success; same | success; same | Usage; Failed | Usage; Finished |
| `load_state` | success; restored status | success; restored status | success; restored status | Usage; Failed | Usage; Finished |
| `finish` | report; Finished | report; Finished | report; Finished | Usage; Failed | Usage; Finished |

In Ready, a run with no deadline, step total or frame stop is allowed:
it runs until an event (halt, power-off or error). A requested frame stop
also ends a run. Front ends enforce their own budgets. Unsupported frame
stops are rejected before consuming input. A valid run applies queued input
in order at the current time, even if a limit was already reached; it then
returns without executing a step for that limit.
Halted and Off runs return their stop immediately, without consuming input
or validating limits. A run does not wake a halted machine: reset does.

At a boundary with simultaneous reasons, report an execution error first,
then power-off, halt, requested frame, deadline, steps (in that order).
An event not requested as a stop does not end the run. Deadline, Steps and
Frame leave Ready; Halted leaves Halted; PoweredOff leaves Off. Execution
errors follow `run`'s failure rules below. Before a tick or step counter
would overflow, fail with `Guest` and leave Failed; never wrap or silently
saturate. Already-reached limits take precedence over attempting more work.

Both reset kinds preserve `now()` and the absolute step total and take no
virtual time. Successful reset clears queued input and held controls,
preserving absolute pointer positions; media stay inserted. A failed reset
keeps the queue and counters. Unsupported reset kinds leave status unchanged.
Reset failures otherwise follow `reset`'s documented rules.

Media additions and selections complete immediately, including required
content/record opens. The call doing the open reports its error; it must
not defer that error to `run`. Usage, Unsupported, Content and ordinary
Storage errors preserve the previous catalogue, insertion and status.
StorageUncertain leaves Failed (without promising rollback of storage).
A successful media call does not wake Halted. Selection requires
`media_changes`; extension requires `media_additions`; neither implies the
other. Slot removability and `accepts_changes` still constrain selection.
A non-removable slot cannot be extended. Compatible catalogue aliases
with the same base identity, format and kept-record name reuse the held
record when selected; they do not reopen an exclusively held record.

A failed finish leaves Failed; a second finish after success returns Usage.
Failed sessions cannot be marked complete: drop/recreate is the contract's
recovery path. Read-only inspection and drains remain available in both
terminal statuses. `set_outputs` has no error return and is ignored there.

## Numeric, output and state invariants

Clock numerator and denominator are positive; a zero denominator never
represented a valid rate. Display, input port and slot ids are unique
within their respective lists. Frame width, height and aspect components
are positive and pixel count is exactly width × height. Empty visible
areas are invalid: `FrameArea::is_within` rejects them; `with_visible`
clips and falls back to the whole picture for an empty intersection.

State tokens are opaque and compare for equality. They cover release,
effective configuration, catalogue and immutable media digest/size. Mutable
overlays and battery contents are captured in the state or their generation
is part of its token. A save preserves the machine and does not flush records.
A successful load restores time, step total, Ready/Halted/Off status, queued
input and held controls; pending audio is dropped and its position follows
restored time. Output preferences remain the caller's. Loading never rewinds
or commits external records. Incompatible external data is rejected with
State. Any failed load preserves all observable state, queues and external
records; it may consume the input reader. Neither real core currently
advertises saved state; the scripted test-support machine pins these rules.
