// SPDX-License-Identifier: MIT OR Apache-2.0
//! Explicit package bindings (since 0.6). Configuration syntax and role
//! interpretation belong to the core; front ends supply verified facts.

use crate::{ConfigDocumentBuf, ContentSource, Validated};

/// What a configuration consumes. Unknown kinds must leave preparation
/// unavailable in a front end that cannot supply their facts.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum BindingKind {
    FirmwareRom,
    Medium,
    /// A zero-filled disk declared by the document, with no package or blob.
    ZeroDisk,
}

/// One occurrence in the document, including repeated references to an item.
/// Slots are ordered by `id` using Rust string ordering; ids are nonempty,
/// unique, stable config-local identifiers chosen by the core.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingSlot {
    pub id: String,
    /// Some for package bindings; None for document-owned ZeroDisk.
    /// Ids are exact, case-sensitive, nonempty opaque names, never positions.
    pub use_id: Option<String>,
    /// The package item id, or for ZeroDisk the document-declared disk id,
    /// unique among the document's zero disks. A recipe's template disk
    /// declaration must match this id and size without parsing config.
    pub item_id: Option<String>,
    pub kind: BindingKind,
    /// The role explicitly stated by the document, interpreted by the core.
    pub role: Option<String>,
    /// Expected bytes when fixed by the document; always Some for ZeroDisk.
    pub size: Option<u64>,
    /// A digest constraint explicitly stated by the document, if any.
    pub sha256: Option<[u8; 32]>,
}

impl BindingSlot {
    /// A package reference, initially without role, size or digest constraints.
    /// `kind` must be FirmwareRom or Medium; ZeroDisk uses `zero_disk`.
    pub fn new(
        id: impl Into<String>,
        use_id: impl Into<String>,
        item_id: impl Into<String>,
        kind: BindingKind,
    ) -> Self {
        Self {
            id: id.into(),
            use_id: Some(use_id.into()),
            item_id: Some(item_id.into()),
            kind,
            role: None,
            size: None,
            sha256: None,
        }
    }

    /// A document-owned zero disk. It needs no package facts or content open.
    /// `disk_id` is the document-declared id, unique among its zero disks;
    /// a recipe's template disk declaration must match it and `size`.
    pub fn zero_disk(id: impl Into<String>, disk_id: impl Into<String>, size: u64) -> Self {
        Self {
            id: id.into(),
            use_id: None,
            item_id: Some(disk_id.into()),
            kind: BindingKind::ZeroDisk,
            role: None,
            size: Some(size),
            sha256: None,
        }
    }

    pub fn with_role(mut self, role: impl Into<String>) -> Self {
        self.role = Some(role.into());
        self
    }

    pub fn with_size(mut self, size: u64) -> Self {
        self.size = Some(size);
        self
    }

    pub fn with_sha256(mut self, sha256: [u8; 32]) -> Self {
        self.sha256 = Some(sha256);
        self
    }
}

/// An item's facts from this package's catalogue entry or package fits.
/// Roles belong only to this item in this package: never infer them from
/// another package, another item with the same digest, or a global catalogue.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingItem {
    pub id: String,
    /// FirmwareRom or Medium; ZeroDisk cannot be supplied by a package.
    pub kind: BindingKind,
    pub size: u64,
    /// Full SHA-256, verified by the front end over every byte, without sampling.
    pub sha256: [u8; 32],
    /// Exact role strings declared by the bound package for this item.
    /// The core interprets and checks them, including any role implied by config.
    pub roles: Vec<String>,
}

impl BindingItem {
    pub fn new(id: impl Into<String>, kind: BindingKind, size: u64, sha256: [u8; 32]) -> Self {
        Self {
            id: id.into(),
            kind,
            size,
            sha256,
            roles: Vec::new(),
        }
    }

    pub fn with_roles(mut self, roles: Vec<String>) -> Self {
        self.roles = roles;
        self
    }
}

/// One package pinned by a recipe's use. The front end verifies the package
/// and faithfully supplies its declarations; the core does not authenticate
/// a manifest or fetch a package. The digest is opaque provenance, not a path.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct BindingUse {
    pub id: String,
    pub manifest_digest: String,
    /// Unselected items are allowed. Item ids must be unique within this use;
    /// equal item ids in different uses are independent.
    pub items: Vec<BindingItem>,
}

impl BindingUse {
    pub fn new(
        id: impl Into<String>,
        manifest_digest: impl Into<String>,
        items: Vec<BindingItem>,
    ) -> Self {
        Self {
            id: id.into(),
            manifest_digest: manifest_digest.into(),
            items,
        }
    }
}

/// Facts and access to their immutable bytes. Uses have unique, nonempty ids
/// and nonempty manifest digests. Order of uses, items and role declarations
/// has no meaning. ContentSource serves full digest/size receipts as for create;
/// composition trusts them and never rehashes or writes the base.
#[non_exhaustive]
pub struct BindingFacts<'a> {
    pub uses: &'a [BindingUse],
    pub content: &'a dyn ContentSource,
}

impl<'a> BindingFacts<'a> {
    pub fn new(uses: &'a [BindingUse], content: &'a dyn ContentSource) -> Self {
        Self { uses, content }
    }
}

/// Prepared configuration, not a running machine. This is an owned envelope
/// around the existing document and validation result, with no retained reader.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct PreparedConfig {
    /// Digest-addressed, host-free document accepted by validate and create.
    /// No unresolved binding syntax remains. Zero disks remain document-owned.
    /// A stored image keeps this document as its config and the listed blobs;
    /// recipe/use/manifest provenance may be kept separately by the front end.
    pub config: ConfigDocumentBuf,
    /// Equal to validate(config), with its existing identity-set semantics.
    pub validated: Validated,
}

impl PreparedConfig {
    pub fn new(config: ConfigDocumentBuf, validated: Validated) -> Self {
        Self { config, validated }
    }
}
