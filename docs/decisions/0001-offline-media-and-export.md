# Offline media descriptions and export

Status: accepted, 2026-11-23. Intended contract: 0.5; release versions and
pins are unchanged until release preparation.

## Context

A front end must not know disk geometry or file formats. It needs to list
configured media and export their current contents without creating or running
a machine. The core owns the interpretation of configuration and formats;
the front end owns content assembly, destinations and host I/O.

## Decision

Offline operations go through the core, like operations on running machines.
All additions are optional and source-compatible with a 0.4-style core:
new Core methods default to Unsupported, platform export lists default to
empty, and media details default to empty with unknown length. The methods
are object-safe. A consumer moving to 0.5 changes its implements literal and
release pin; support for these operations is optional.

A core reads a snapshot of each medium's CURRENT bytes supplied by the front
end through ContentReader. The front end composes the read-only base, layers
and journal through one reader interface; the core neither opens nor rewrites
them. Bases keep their full SHA-256 identity, including large disks. Export
creates new outputs, preserving future chains of hash-linked layers.

## API shape

- `Core::describe_media(platform, config)` returns one MediaDescription per
  configured slot: the existing SlotDescription, a display-only detail string
  and an optional medium byte length fixed by config. Front ends show detail
  and never parse it. Validation is config-only, agreeing with validate and
  baseline create; it performs no I/O and cannot promise creation success.
- `PlatformDescription::export_formats` lists non-exhaustive ExportFormat
  values: stable core-defined id, name, extension and Slot or Machine scope.
  Const constructors validate fields; the list builder rejects duplicate ids.
- `Core::export_slots(platform, config, format_id)` returns unique configured
  slot ids in canonical order: eligible slots for Slot scope (select one), or
  the exact required set for Machine scope (supply all). Each format can export
  a different subset of describe_media's slots. This query is config-only,
  object-safe and defaults to Unsupported; front ends disable formats whose
  slot set cannot be queried instead of guessing. Unknown formats are Usage.
- `Core::export(platform, config, request, sink)` takes a format id and current
  media readers with slot ids and declared lengths. Slot scope requires one
  slot; Machine scope requires every slot exported by that format, each once.
  The core defines that set and rejects missing, duplicate or unknown slots.
- `ExportSink::create(name)` reserves an exact logical output name. The
  contract's `is_export_name` accepts 1–64 ASCII bytes from `a-z`, `0-9`, `-`
  and `.`, starting and ending in a letter or digit, with no `..` anywhere.
  Thus separators, absolute/drive paths, whitespace and Unicode are excluded.
  An existing name fails, including an unfinished output: no overwrite.
  Backends map special host names internally while preserving logical names.
- The sink owns outputs and backend handles, lending `&mut dyn ExportOutput`
  from its mutable borrow. The core cannot retain an output beyond export:
  storing that borrow for later use cannot compile. After either result the
  front end can close every backend handle before cleanup, including outputs
  the core left unfinished, so an open file cannot block removal on Windows.
  Finish closes on success or failure; later writes and finishes return Storage.
- ExportOutput accepts streamed writes and a closing finish. The sink/front
  end hashes every output byte with full SHA-256 and returns its exact name,
  byte length and digest from finish. This required hook leaves the contract
  dependency-free; production sinks can use their own hashing dependency.
  Test-support provides a reference SHA-256 implementation in memory.
- ExportReport includes a receipt for every completed output, including any
  machine document, and notes naming all settings the format cannot express.
  Settings are never silently dropped; front ends display the notes. Receipts
  are sink-computed but core-editable. Before accepting success, front ends
  compare report names, counts, completion status, lengths and digests against
  their own record of created outputs and sink receipts. They reject missing,
  extra, duplicate, unfinished or altered entries and clean up as for failure.

A machine-scope document refers to its slot outputs using exactly the names
created by the sink, relatively, never host paths. The same inputs give the
same output names, order, bytes, receipts and notes, independent of request
slot order. Reordered requests are accepted. No host clock, randomness or
threads enter the execution path.

A failed export returns an error, never a success report. Outputs may be
partial, even after failed create or finish. The front end tracks and removes
all outputs it created on failure; drop is not rollback. It must preserve
pre-existing outputs. Host durability is front-end policy. The exhaustive
error matrix and unchanged Machine status rules are in
[conformance](../conformance.md).

## Rejected alternatives

Having the front end implement formats would duplicate core knowledge of
geometry, hardware and configuration. Having a core write host files directly
would couple it to paths and host I/O, defeating embedding and neutral front
ends. Neither is part of this design.

## Consequences

Core implementations own format correctness, deterministic output and honest
loss notes. Front ends implement safe no-overwrite sinks, streaming hashing,
cleanup and current-byte snapshots; they need no geometry or format parsers.
Unsupported operations remain usable through baseline behavior. PC format
implementation and the front end's neutral export command follow in their
own repositories; neither is implemented here. Existing raw-byte front-end
export remains separate from format-aware export through the core.
