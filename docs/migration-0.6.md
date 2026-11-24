# Moving to contract 0.6

This is an additive, optional API change on top of 0.5. CONTRACT, package
versions and release pins remain at 0.5 until release preparation.
Do not pin this development branch. When 0.6 is released, coordinate exact
git revisions throughout the build as described in [embedding](embedding.md).

## Cores

A 0.5-style core updates its release pin and literal CoreDescription::implements
to ContractVersion::new(0, 6). Existing constructors still work. New Core methods
binding_slots and compose default to Unsupported before checking arguments;
PlatformDescription::content_bindings defaults to false. No Machine methods
change. Optional support advertises both methods with with_content_bindings(true).

binding_slots returns one BindingSlot per document occurrence, sorted by its
unique stable config-local id. Package references name a use and an item;
ZeroDisk has no use_id; item_id is the document-declared disk id, unique among
the document's zero disks, and size is always present. A recipe's template
disk declaration must match by id and size. Role, size and digest
constraints come from the document. Discovery performs no I/O and allows only
Config/Unsupported, as describe_media does.

compose takes BindingFacts (package facts plus a ContentSource) and returns
PreparedConfig, an owned ConfigDocumentBuf plus its Validated content list.
Resolve bindings to digest/size identities, with no unresolved package syntax
or host paths. Reuse the normal config parser/lowerer and create path. Validate
accepts the result and names exactly the content successful baseline create
requests, with existing identity-set semantics. Zero disks stay document-owned.

Before opening content, reject unbound/unused uses, duplicate uses, missing or
ambiguous items, wrong kinds, conflicting sizes/digests, invalid fact names or
empty manifest digests, and roles not declared for that item by that package.
Extra unselected items in a used package are allowed. Check implicit roles too;
never infer permission from a global catalogue or another item with the same
digest. Check geometry against disk size and reject path-plus-binding conflicts
inside the core. Return useful Config errors, optionally with existing Diagnostic
paths. Open required identities through ContentSource, trust full SHA-256/size
receipts without rehashing, and check returned reader sizes. Only Config,
Unsupported, Content and Storage are allowed; see [conformance](conformance.md).
As for create, recognised content the core cannot run (including a damaged
image or unsupported cartridge mapper) is Unsupported; Content is reserved
for receipt mismatches or detected changes.
Keep prepared bytes and content lists independent of fact ordering. Neither
method creates a machine, changes state/time, retains readers or writes bytes.

## Front ends

Check content_bindings before offering recipe preparation. False means absent;
unknown BindingKind values leave that preparation unavailable. Never parse core
config to discover slots, geometry or firmware roles. Use binding_slots to inspect
a template; supply facts to compose and pass its output to ordinary create.

For each recipe use, supply BindingUse with its opaque package manifest digest
and BindingItem entries: item id, kind, byte size, full SHA-256 and role strings
faithfully taken from that package's catalogue entry or fits. Verify the package
and hash every byte before handing it to the core, however large. Manifest
digests are provenance; the core does not authenticate manifests. Do not union
roles across packages or items. Use/item ids are exact names, never list indexes.

BindingFacts::content serves immutable bytes through the existing ContentSource,
by digest and size, using Content::Bytes or Content::Reader. No new content
representation is negotiated. Keep a base read-only and sector writes separate;
commit/export writes a new base. Store PreparedConfig::config and retain its
Validated content set for replay. Recipe/use/manifest provenance may be stored
separately, but is not required by create. Later content/storage reads can fail.

For consumer tests enable test-support and use ScriptedBindingCore through
&dyn Core. Its rustdoc specifies a small line-based template/prepared syntax.
check_prepared verifies the returned identity set through validate and rejects
remaining package slots without creating a machine or doing content I/O. Also
test creation with a source serving only the listed digests, deterministic
ordering and rejection of incorrect package roles.

The accepted rationale and alternatives are in the
[decision record](decisions/0002-content-bindings.md). PC implementation is
separate consumer work.
