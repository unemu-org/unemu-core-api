# What a core must hold

The documentation comments say what each call must do. This list is what
they cannot say about a core as a whole. It is the checklist a new core,
and every change to a core, is reviewed against. It is the list of
contract 0.4, with additive offline operations for 0.5 and bindings for 0.6.

1. **The dependency.** The crate by the exact revision of a release tag
   ([`embedding.md`](embedding.md)), never a path, a branch or a copy; a
   gitignored patch for local work. `CoreDescription::implements` is a
   literal, such as `ContractVersion::new(0, 6)`.
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
   reads configs implements `validate`: config-only checks agree with `create`
   for equal platform/config and baseline options, without promising creation
   success. Its content list is an identity set, not an access trace (see below).
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
   Successful sync confirms durability; a later destructive cut can discard
   those bytes even before another sync. Ordinary close/reopen preserves current
   visibility without rollback; a power failure may lose unsynchronized changes.
   A core recovers partial append tails in its own format and uses replacement
   for old-or-new whole-record preservation. After `StorageUncertain` it reopens
   before writing. It never consumes a failed read buffer as valid content and
   does not rely on a `ContentReader` detecting changed bytes.
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

Finalization I/O failures leave Failed; a second finish after success returns
Usage and preserves Finished.
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

## Errors and validation

There are eight error kinds; this release adds none. Config rejects a config
document, Usage rejects invalid handles/arguments/options, Unsupported rejects
an unavailable operation or recognised content the core cannot run (including
malformed image formats). Content means a missing/mismatched receipt or a
detected post-verification change, not an image-format diagnosis. Storage is
host I/O, State rejects a saved-state document, and Guest reports emulation
failure. StorageUncertain is an uncertain write, never flattened to Storage
or a warning. Method-specific reset rules retain the existing Guest exception.

The allowed kinds below are exhaustive for 0.4 plus the optional 0.5 offline
operations and 0.6 bindings. An implementation translates its internal errors at the adapter; a core's own invalid parsing read must not
escape as caller Usage. Apply status rejections first, then absent-operation
Unsupported, then argument checks, as in the call/status matrix. Halted/Off run
short-circuits remain as specified there. No call below advances virtual time
except run; successful load restores saved time. A status rejection preserves
state, even for finish; finalization I/O failure instead sets Failed.

| Fallible method | Allowed error kinds | Effect on failure |
| --- | --- | --- |
| `Core::create` | Config, Usage, Unsupported, Content, Storage, StorageUncertain | No running machine; acquired records released; no half-written session mistaken as complete. Record recovery applies; no storage rollback promised. |
| `Core::identify` | Unsupported, Content, Storage | No retained machine, content or state; a mismatched offered receipt is Content. Foreign bytes return None; recognised unmodelled/damaged images use Unsupported. |
| `Core::validate` | Config, Unsupported | Config-only check; no state changes or I/O. Unsupported covers unavailable platform/type or config-only unsupported feature. |
| `Core::describe_media` | Config, Unsupported | Config-only; no I/O, machine creation, state or status changes. Same config-only checks as validate and baseline create. |
| `Core::binding_slots` | Config, Unsupported | Config-only; no I/O, machine, state, status or time changes. Canonical occurrence slots; absent operation rejected before arguments. |
| `Core::compose` | Config, Unsupported, Content, Storage | No machine, state, status or time changes; no writes or retained readers/storage. Config/fact consistency checked before content opens; only a complete prepared config is returned. Unsupported includes recognised content the core cannot run (including a damaged image), as for create; Content is reserved for receipt/change failures. |
| `Core::export_slots` | Config, Usage, Unsupported | Config-only; no I/O, machine creation, state or status changes. Usage for unknown format id; config checks as validate. |
| `Core::export` | Config, Usage, Unsupported, Content, Storage | No input, machine, status or time changes. May leave partial outputs; front end removes every output it created, never pre-existing outputs. |
| `ExportSink::create` | Usage, Storage | Invalid or existing names are Usage with no overwrite. I/O may leave a partial newly created output to clean up. |
| `ExportOutput::write` | Storage | May leave a partial append; invalidate and clean up the whole export. |
| `ExportOutput::finish` | Storage | Output closed on every result; later writes/finishes are Storage; failed output must be cleaned up. |
| `Machine::run` | Usage, Unsupported, Guest, Content, Storage, StorageUncertain | Usage/Unsupported before execution preserve state/input. Execution errors set Failed; halt-time Storage/StorageUncertain may leave Halted. |
| `Machine::frame` | Usage, Unsupported, Content, Storage | Status, guest state and last completed picture unchanged. Invalid display id is Usage when picture output exists; no initial picture may also be Usage. |
| `Machine::drain_audio` | Unsupported, Content, Storage | Status and guest state unchanged. Partial append to out and consumed pending audio are possible; discard the failed append. |
| `Machine::input` | Usage, Unsupported | Status and queue unchanged. Invalid port/control is Usage; an unavailable event operation, including an unknown variant, is Unsupported. |
| `Machine::add_medium` | Usage, Unsupported, Content, Storage, StorageUncertain | Ordinary failure preserves catalogue/insertion/status; uncertainty sets Failed. |
| `Machine::change_medium` | Usage, Unsupported, Content, Storage, StorageUncertain | Ordinary failure preserves catalogue/insertion/status; uncertainty sets Failed. |
| `Machine::reset` | Usage, Unsupported, Guest, Content, Storage, StorageUncertain | Counters and queue preserved. Status preserved except uncertainty sets Failed, or may retain Halted. |
| `Machine::save_state` | Usage, Unsupported, Content, Storage | All machine state, queues, status and records unchanged; output may contain an incomplete document, to discard. |
| `Machine::load_state` | Usage, Unsupported, State, Storage | All observable state, queues and records unchanged; reader may be consumed. Host I/O is Storage, not State. |
| `Machine::finish` | Usage, Storage, StorageUncertain | Status rejection preserves status; finalization I/O failure sets Failed, with no storage rollback promised. |
| `ContentSource::open` | Content, Storage | No handle returned, no content changed; missing or mismatched receipt is Content. |
| `ContentReader::read_at` | Usage, Content, Storage | Content unchanged; failed buffer may be partial and must not be consumed as valid. |
| `Storage::open` | Usage, Storage | No handle returned; acquired ownership released; existing record bytes unchanged. |
| `Record::size` | Storage | Bytes and durability unchanged. |
| `Record::read_at` | Usage, Storage | Bytes and durability unchanged; failed buffer may be partial. |
| `Record::append` | Usage, Storage, StorageUncertain | Usage has no effect; Storage restores previous bytes; uncertainty may leave a partial tail and poisons writes until reopen. No new durability confirmed. |
| `Record::truncate` | Usage, Storage, StorageUncertain | Usage has no effect; Storage restores previous bytes; uncertainty may discard confirmed bytes and poisons writes until reopen. No new durability confirmed. |
| `Record::sync` | Usage, StorageUncertain | Usage rejects prohibited writes; failed barrier confirms no new durability and poisons writes until reopen. |
| `Record::replace` | Usage, Storage | Parent unchanged, no replacement returned. |
| `Replacement::size` | Storage | Parent and replacement bytes unchanged. |
| `Replacement::read_at` | Usage, Storage | Parent and replacement bytes unchanged; failed buffer may be partial. |
| `Replacement::append` | Usage, Storage, StorageUncertain | Usage has no effect; Storage restores temporary bytes. Uncertainty breaks the replacement permanently and poisons parent writes without changing parent bytes. |
| `Replacement::commit` | Usage, Storage, StorageUncertain | Consumed on every result. Usage changes nothing; Storage preserves parent. Uncertain parent barrier blocks publication, confirms no new durability and poisons writes; uncertain publication leaves old/new whole and poisons writes. |

For equal platform/config and baseline (empty) run-only options, validate and
create agree only on config-only checks. Validate cannot inspect content,
storage, options or content-dependent hardware support, so success does not
promise create success. On successful creation the requested (SHA-256, length)
identity set equals Validated.content. Order, duplicate references, labels and
repeated or cached opens do not define an access trace. A failed create may
request only a subset. Front ends compare identity sets, not vectors or logs.

## Content-source compatibility

Unnegotiated sources continue supplying Bytes or Reader, including to newer
cores. Future Content variants must specify their introduction minor, absence
meaning, capability advertisement and Bytes/Reader fallback. A wildcard that
rejects a new variant does not make that variant interoperable. Existing cores
can keep matching both baseline representations; new cores can use Content as
a ContentReader without matching or copying a whole image.

The boundary verifies full SHA-256 plus length before handing over content:
no sampling, including large hard-disk images, and no core rehash. Prefer hashing
and retaining the same handle. FileContent::open/from_file check length and
record metadata; neither verifies a digest. Later changes are detected best
effort, not as an immutable snapshot guarantee. Layers can remain behind Reader;
bases stay read-only, sector writes go through the core's overlay interface,
and commit/export creates a new base without rewriting the old one.

Compatibility tests in tests/ compile external Core/Machine implementations
and drive trait objects. The baseline core leaves identify/validate defaults
alone and advertises no optional machine facilities. Both content forms,
unknown-to-the-fixture enum cases via wildcards, and failed storage are covered.
Construction tests cover default fields and extensible patterns. Feature-gated
public test-support helpers are themselves part of the API from 1.0. These are
source checks in one unified Cargo build, not a stable binary ABI or a claim
that separately released consumer adapters have been migrated and tested.


## Offline media and export (0.5)

All new Core methods are optional and default to Unsupported before checking
arguments. PlatformDescription::export_formats defaults to empty (absent).
Older front ends ignore this addition; newer front ends handle unknown scopes
by leaving the format unavailable. describe_media may be probed and fall back
to baseline labels without geometry tools. No existing Core or Machine method
gains mandatory work from these additions.

1. describe_media performs only config checks, with Config/Unsupported as
   validate does. It agrees with validate and baseline create on those checks;
   success cannot promise creation/export success. It describes every configured
   slot once, with the ids and slot data the created machine reports, in a
   deterministic order. Detail defaults to empty, is display-only and is never
   parsed by front ends. size defaults to None; Some(n) is the medium's length
   in bytes when config fixes it. Unknown is distinct from an empty medium.
2. Export format ids are stable, core-defined and unique within a platform;
   fields follow ExportFormat's documented syntax. The const constructor checks
   fields and the platform list builder rejects duplicate ids. Every advertised
   format works for supported configs. Unsupported covers an absent operation,
   platform, config type or unsupported config/format capability; an unknown
   format id in a supported operation is Usage. Config rejects a document.
3. A request contains exactly one known slot for Slot scope, or every slot
   exported by the format exactly once for Machine scope. The core defines
   that set through config-only Core::export_slots, which returns unique
   configured ids in canonical order. For Slot scope it lists eligible slots;
   for Machine scope it lists the exact required set, possibly a strict subset
   of describe_media. Each advertised format supports the query; an unavailable
   query disables the format in the front end. Query errors are Config,
   Unsupported or Usage (unknown format). Reordered requests are accepted and
   produce the same canonical output order. Unknown, ineligible, missing and
   duplicate slots or wrong scope cardinality
   are Usage. Before creating outputs, check ids/cardinality and require each
   declared length to equal its reader size and any config-fixed length;
   mismatch is Content. Readers expose stable CURRENT bytes assembled by the
   front end through one base/layers/journal interface; the core only reads.
   Reader Content/Storage errors propagate; the core's own invalid parsing read
   must not leak Usage. No failed read buffer may be emitted as valid output.
4. is_export_name validates plain relative logical names: 1–64 ASCII bytes of
   a-z, 0-9, hyphen and dot, starting/ending with a letter or digit, no `..`
   anywhere. No separators, absolute/drive paths, whitespace or Unicode.
   Sink create validates names and reserves them without overwrite, even for
   unfinished outputs. Invalid/existing names are Usage. All valid names work;
   backend-special names are mapped internally without changing logical names.
5. The sink owns outputs and backend resources, lending mutable output
   references tied to its borrow. Retaining a handle beyond Core::export cannot
   compile. After success or failure the front end can close every output,
   including unfinished ones, before cleanup. Finish closes on either result;
   later writes and finishes return Storage without changing bytes or receipts.
   Outputs take streamed writes, with no silent short writes. Every created
   output is finished before success and appears exactly once in the report,
   including machine documents. Finish returns the exact logical name, length
   and full SHA-256 of all output bytes: a required sink/front-end hashing hook,
   not an optional or sampled digest. This keeps the contract dependency-free.
   Receipts are sink-computed but core-editable. Before accepting success, front
   ends compare report names, counts, completion status, lengths and digests
   against their own record of created outputs and sink receipts. Missing,
   extra, duplicate, unfinished or altered entries invalidate success and require
   cleanup as for failure; the report alone is not evidence of completeness.
   Machine documents refer to slot outputs by exactly the names created by the
   sink, relatively, never host paths. Notes name every setting the format
   cannot express; front ends show them instead of silently dropping settings.
6. The same platform/config/format/current slot bytes produce identical output
   names, order, bytes, receipts and notes, independent of request slot order.
   No wall clock, randomness or threads. Inputs, bases, layers and journals
   stay read-only. Large bases retain full SHA-256 verification; exporting or
   committing makes a new base, never rewrites an old base in place.
7. Sink I/O failures (create, write, finish) and reader I/O are Storage. Export
   never returns StorageUncertain: these are new outputs, not recoverable
   records. Failure returns an error, never a partial success report; outputs
   may be incomplete and front ends remove every output created by the attempt,
   including failed creations, without removing pre-existing outputs. Dropping
   an output is not rollback. Successful finish accepts all writes; host
   durability policy belongs to the front end. Offline calls never change a
   running machine's status or virtual time, on either success or failure.

Consumer steps and the accepted rationale are in
[migration-0.5.md](migration-0.5.md) and the
[decision record](decisions/0001-offline-media-and-export.md).


## Explicit content bindings (0.6)

PlatformDescription::content_bindings defaults to false. True promises both
binding_slots and compose for supported configurations. Both methods are
object-safe and default to Unsupported before arguments. Front ends leave
preparation unavailable for absent support or unknown kinds. Existing methods
retain baseline behavior; unresolved templates may need compose before validate.

1. binding_slots performs no I/O and returns every document binding occurrence
   with a stable unique config-local id, sorted by Rust string ordering. It
   reports exact use/item ids, kind and explicitly declared role/size/digest
   constraints. Repeated references have separate occurrence ids. ZeroDisk is
   document-owned: use_id is None, item_id is the document-declared disk id,
   unique among the document's zero disks. A recipe's template disk declaration
   must match by id and size. Size is fixed; no content open or supplied item.
   Prepared documents have no package slots; zero slots may remain discoverable.
2. compose rejects unbound/unused uses, duplicate uses, missing/ambiguous item
   ids, wrong kinds, size/digest conflicts and undeclared roles with Config and
   useful use/item/slot messages. Names and manifest digests must be nonempty;
   ids are case-sensitive and item ids unique within a use. Unselected package
   items are allowed. Roles, including implicit config requirements, must be
   declared for this item by this bound package, never inferred from a global
   catalogue or another item/package sharing the digest. The front end verifies
   packages and faithfully extracts declarations; the core does not authenticate
   the opaque manifest digest. Geometry versus declared size and path-plus-binding
   conflicts are core-specific Config checks, never front-end responsibilities.
3. All consistency checks precede content opens. Full SHA-256 and length are
   front-end receipts, with no sampling or core rehash. compose opens every
   required identity, checks reader size and uses ContentSource/ContentReader
   conventions. Missing/mismatched receipts or detected changes are Content;
   correctly hashed content the core cannot run (such as an unsupported
   cartridge mapper) is Unsupported. Recognised malformed content is also
   Unsupported, exactly as create's damaged-image rule specifies, never Content.
   I/O is Storage. Internal parsing reads never leak Usage. No base is written,
   no storage is acquired and no partial result is returned on failure.
4. Equal config and facts produce identical prepared bytes/content lists
   independent of use/item/role order. PreparedConfig wraps existing document
   and validation types; the host-free document contains no unresolved package
   bindings. validate returns the matching identity set; successful baseline
   create requests exactly that set through ContentSource. Store this config
   and its content for replay; provenance may be stored separately. Success
   cannot promise later content/storage or creation success. Cartridge content
   fits Medium just as disk media does; roles and config syntax stay core-owned.
5. Consumer conformance tests exercise advertised methods through &dyn Core,
   canonical slots, all Config rejection cases, receipt/I/O failures,
   content-dependent Unsupported, package-local roles and creation using only
   the listed identities. test-support supplies
   ScriptedBindingCore and check_prepared. The helper compares validate's identity
   set with the result and checks no package slots remain, without creating a
   machine or doing content I/O. Consumer tests still verify actual create opens
   and deterministic composition.

See [migration-0.6.md](migration-0.6.md) and the
[decision record](decisions/0002-content-bindings.md).
