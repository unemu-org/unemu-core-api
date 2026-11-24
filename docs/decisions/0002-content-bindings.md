# Explicit content bindings

Status: accepted, 2026-11-24. Intended contract: 0.6; CONTRACT, package release
versions and pins remain at 0.5 until release preparation.

## Context

A recipe pins packages under use ids such as `bios`, `vga` and `dos`. A
machine template needs to select an item in one of those packages without
naming a host path or relying on list order. The front end pulls packages,
hashes bytes and supplies package facts. It must not interpret PC config,
geometry, placement or firmware roles. A video ROM cannot become a system
ROM merely because another catalogue entry associates its digest with that
role: the bound package must declare the role for that very item.

The existing contract already has digest-addressed config documents,
config-only validation and ContentSource/ContentReader. The PC lowerer
resolves logical image keys through ImageReader. Its source-position walk
records provenance rather than resolving content. The PC Core adapter
currently decodes/checks launch documents for validation, opens content and
lowers the same document for creation, and delegates offline export to its
export adapter. A binding seam must feed that existing path rather than
require a parallel assembler or a front-end config parser.

## Decision

Add optional, object-safe Core operations to describe binding slots and
compose template bytes with package facts. PlatformDescription advertises
both together with `content_bindings`, false by default. Absent operations
return Unsupported before examining arguments. Older implementations retain
all baseline behavior. Front ends leave recipe preparation unavailable when
the capability is absent and handle unknown binding kinds conservatively.
No existing Core or Machine method acquires mandatory work.

The output is PreparedConfig, a small owned envelope containing the existing
ConfigDocumentBuf and Validated types. It is not a new machine interface:
`create` consumes its document and the existing ContentSource as before.
The returned document is digest-addressed and host-free, with no unresolved
package binding syntax. Core-owned logical keys may remain as mappings to
(SHA-256, size) entries. A stored image keeps that document as its config
and retains the listed content. The front end may keep the recipe, use ids
and manifest digests separately for provenance; replay never needs package
resolution or package facts.

This supports a cartridge bound by digest just as it supports firmware or
media in a PC. BindingKind::Medium includes immutable cartridge content;
mutability and placement remain properties of the core's config. No PC
hardware model, role vocabulary or config syntax enters the API.

## API shape

```rust,ignore
Core::binding_slots(platform, config) -> Result<Vec<BindingSlot>, Error>
Core::compose(platform, config, BindingFacts) -> Result<PreparedConfig, Error>

BindingFacts { uses: &[BindingUse], content: &dyn ContentSource }
BindingUse { id, manifest_digest, items: Vec<BindingItem> }
BindingItem { id, kind, size, sha256, roles: Vec<String> }
BindingSlot { id, use_id, item_id, kind, role, size, sha256 }
PreparedConfig { config: ConfigDocumentBuf, validated: Validated }
```

BindingSlot has a stable config-local occurrence id, distinct from use/item
ids, so several document occurrences can refer to the same item. Slots are
sorted lexicographically by that id, using Rust string ordering. Package
slots carry Some(use_id) and Some(item_id). Document-owned ZeroDisk slots
carry no use_id; item_id is the document-declared disk id, unique among the
document's zero disks. A recipe's template disk declaration must match by id
and size. Zero disks have a fixed size and consume no package fact or content.
FirmwareRom and Medium are package item kinds; ZeroDisk is never a package
item kind. The enum is non-exhaustive. Slot role, size and digest describe
constraints explicitly stated by the document; implicit role requirements
are still checked by the core. All extensible structs have constructors.

Facts contain opaque, case-sensitive ids and an opaque manifest digest.
Roles come faithfully from the bound package's catalogue entry or package
fits. The core interprets role strings; it neither downloads packages nor
authenticates manifests. A manifest digest records provenance, not proof
that a role was declared. Package verification and faithful fact extraction
remain the front end's duty. Unselected items in a used package are allowed.

Bytes use ContentSource with the same full SHA-256/size receipt as create,
and Content implements ContentReader for either baseline representation.
This avoids a second reader convention and allows large read-only bases,
including layered media, without copying or rehashing in the core. Composition
opens every required identity before success and checks reader sizes; it may
read content for core-specific checks but never rehashes it. Later reads can
still fail. Mutable sectors remain separate from immutable bases; commit or
export creates a new base, never rewrites an old one. This seam adds no
mutable-media or overlay interface.

## Rules

- binding_slots is config-only, deterministic and performs no I/O. It allows
  only Config or Unsupported, mirroring describe_media. It reports every
  binding occurrence, including zero disks, in canonical order. A prepared
  document has no package slots, but may still describe its zero disks.
- compose first checks config and facts consistency. Config rejects unbound
  uses, unused supplied uses, duplicate use ids, missing or ambiguous item ids,
  wrong kinds, size/digest conflicts and undeclared roles. Item ids are unique
  within a use; equal ids in different uses are independent. Empty fact names
  or manifest digests are Config errors. Messages identify the relevant use,
  item or slot; the existing Error/Diagnostic types suffice without another
  diagnostic hierarchy.
- A required role, including one implied by the config position, must be
  declared for that item by that bound package. Roles cannot be borrowed from
  a global catalogue or another item/package sharing the digest. For example,
  selecting a video ROM for a PC system-firmware slot fails unless that
  package declares the required system role for that ROM.
- Geometry versus declared disk size, and path-plus-binding conflicts, are
  config checks owned by each core. The front end must not implement them.
  Config-only checks agree across discovery, composition and baseline create
  on the resolved document. An unresolved template may require composition
  before validate can return a content identity set.
- All consistency checks precede content opens. Missing/mismatched receipts,
  changed content or a returned reader of the wrong size are Content; read
  I/O is Storage. Internal parsing reads cannot escape as Usage. Unsupported
  covers unavailable operation/platform/config type, unsupported config
  capability or recognised content the core cannot run (including a damaged
  image), exactly as create does. Correctly hashed content with an unsupported
  cartridge mapper is Unsupported, not Content. There are no other compose
  error kinds, no writes, no acquired storage and no machine/status/time
  changes on either result.
- Equal config and facts produce identical prepared bytes and content lists
  regardless of use/item/role order. validate accepts the output and returns
  its Validated identity set; baseline create requests only that set. Success
  does not promise later content/storage access or creation will succeed.

## Rejected alternatives

Host paths and positional package selection are not portable identities.
Front-end parsing duplicates core rules and lets recipes silently reinterpret
hardware. Inferring roles from digests loses the package's declaration boundary.
Keeping unresolved bindings in stored images would add package resolution to
create and replay. A new prepared-machine trait would duplicate create and its
lifecycle. Per-item readers would introduce another receipt/reader interface
and obscure the existing digest-addressed content cache. A universal config
or role schema would impose one platform's model on unrelated cores.

## Consequences

Cores own syntax, role enforcement, deterministic rewriting and validation.
Package bindings map onto the PC core's existing digest-addressed launch
content rows and ImageReader lowering. A document-owned zero disk has no
content row there today: the PC core needs a native zero-disk form in its
prepared representation. Persisting the prepared document is a PC policy
change on its side. Composition must preserve package-local role evidence.
This repository does not implement that PC work.

Front ends supply verified receipts and faithful package declarations, retain
prepared config/content, and use the normal create path. ScriptedBindingCore
and check_prepared support consumer tests. No dependency, host I/O, hardware
implementation or package release is added here.
