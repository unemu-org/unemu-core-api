# Moving to contract 0.5

This is an additive API change on top of 0.4. Package versions, CONTRACT and
release pins remain 0.4 until release preparation. Do not pin this development
branch. When 0.5 is released, coordinate exact git revisions throughout the
build as described in [embedding](embedding.md).

## Cores

A 0.4-style core only needs its release pin updated and its literal
`CoreDescription::implements` changed to `ContractVersion::new(0, 5)`.
Existing description constructors still work. All new Core methods have
default Unsupported bodies and export_formats defaults to an empty list.
No Machine methods change.

Optionally implement `describe_media` with the same config-only validation as
validate and baseline create. Return the created machine's slots, display-only
detail text, and Some(byte_length) when config fixes a medium's length.
Otherwise return None; do not inspect content or storage to discover a length.
`MediaDescription::new(slot)` starts with empty detail and unknown length.

Optionally advertise ExportFormat values with
`PlatformDescription::with_export_formats` and implement export_slots and
export. The config-only export_slots query lists eligible slots for Slot scope
and the exact required set for Machine scope, in canonical order. Validate
format and slot ids, cardinality and declared lengths before creating outputs.
Read the CURRENT bytes in ExportRequest through ContentReader; never write a
base, layer or journal. Write new outputs through ExportSink, finish each one,
and return every sink receipt in ExportReport. Machine documents reference
slot images by their exact logical sink names, relatively. Report all settings
the chosen format cannot express in notes. Keep names, bytes, output ordering
and notes deterministic; request slot order has no meaning.

Only the errors in the [conformance matrix](conformance.md#errors-and-validation)
are allowed. In particular, unknown formats/slots and bad output names are
Usage; length mismatch is Content; output failures, including finish, are
Storage. An absent operation is Unsupported before argument checks.

## Front ends

Read each platform's export_formats; empty means unavailable. Handle unknown
ExportScope variants with a fallback that leaves that format unavailable.
Call describe_media without creating a machine. Show detail verbatim and never
parse it. Read byte lengths from size, not geometry or detail text. Unsupported
allows a baseline slot listing; None means length unknown, not zero.

Supply a stable snapshot of each selected medium's current bytes through
ExportMedium and ContentReader, composing base + layers + journal behind one
interface. Verify full SHA-256 and length at the content boundary as before;
never weaken base hashing for large media. Slot formats take one slot;
machine formats take every slot from export_slots, each once. Query the set
for the chosen format; describe_media can list other configured slots. Disable
formats whose query is Unsupported instead of guessing. Keep the original base
read-only and export/commit to a new base.

Implement an ExportSink over files or other storage. Validate names using
is_export_name; reserve them exclusively and never overwrite, even when the
previous output is unfinished. Keep logical names unchanged in receipts and
machine documents; any backend mapping is internal. All valid logical names
must work, including host-special names. Track only outputs created by this
attempt, including partial failed creations, so an error can clean them up
without touching pre-existing files. Drop alone does not clean up.

Keep outputs and backend resources owned by the sink; create lends a mutable
output reference tied to the sink borrow. The core cannot retain it past export.
Close every backend handle after either result before removing failed outputs.
ExportOutput::write accepts every byte or fails with Storage; finish closes
the output on success or failure and returns ExportedOutput with its name,
length and full SHA-256 on success. Later writes/finishes fail with Storage.
Hash all bytes while streaming, using the front end's hashing implementation
or dependency. This is the required hashing hook; the contract adds no
production hashing dependency. A successful finish accepts the writes; choose
host durability policy in the front end. Present every report note. Do not
present incomplete output as a successful export. Receipts are sink-computed
but core-editable: compare the report against your own created-output record
and completed receipts, including names, counts, completion, lengths and hashes.
Reject missing, extra, duplicate, unfinished or altered entries and clean up.

For consumer tests enable test-support and use MemoryExportSink, planted
ExportOperation failures and ScriptedCore through `&dyn Core`. The scripted
core creates ScriptedMachine, describes one raw slot and exports `raw` (Slot)
or `machine` (Machine); its `text/plain` configs are `fixed` (three bytes) and
`variable` (unknown length). MemoryExportSink retains partial outputs to help
tests inspect failure cleanup.

The accepted rationale and rejected alternatives are in the
[decision record](decisions/0001-offline-media-and-export.md). PC-specific
formats and the front end's export command are separate consumer work.
