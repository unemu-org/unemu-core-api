// SPDX-License-Identifier: MIT OR Apache-2.0
//! Media slots and catalogues of a running machine (core-contract.md §3.8).
//!
//! A slot is a place a medium goes: a floppy drive, a fixed disk, a
//! cartridge port, a tape deck. A slot holds an ordered catalogue of media
//! and at most one of them is inserted. The front end changes what is
//! inserted; the core tells the guest in the way its hardware does.
//!
//! The catalogue of a stored image, pending inserts and the library of a
//! media picker are store data on the front end's side; they never pass
//! through the core.
//!
//! Layered disk images stay possible: a base is read-only [`crate::Content`]
//! named by its digest and hashed in full by the source, written sectors go
//! to a [`WriteTarget`], a record of the machine's [`crate::Storage`], and
//! never into the base, and nothing here rewrites a base in place.

use crate::Content;

/// One slot of a machine.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotDescription {
    pub id: SlotId,
    pub kind: SlotKind,
    /// The slot's name for people, in the platform's words: `A:`, `C:`,
    /// `Cartridge`.
    pub label: String,
    /// Whether the inserted medium can change. Catalogue extension also
    /// requires [`crate::Features::media_additions`].
    pub removable: bool,
    /// Format ids of the media the slot takes, in the vocabulary of the
    /// platform's media formats. The front end offers only these.
    pub formats: Vec<String>,
}

impl SlotDescription {
    /// A slot that takes no format yet.
    pub fn new(id: SlotId, kind: SlotKind, label: impl Into<String>, removable: bool) -> Self {
        Self {
            id,
            kind,
            label: label.into(),
            removable,
            formats: Vec::new(),
        }
    }

    pub fn with_formats(mut self, formats: Vec<String>) -> Self {
        self.formats = formats;
        self
    }
}

/// What kind of place a slot is.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum SlotKind {
    Floppy,
    FixedDisk,
    Optical,
    Cartridge,
    Tape,
}

/// A slot's state now.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotState {
    pub id: SlotId,
    pub catalogue: Vec<MediumInfo>,
    /// Index into `catalogue` of the inserted medium.
    pub inserted: Option<usize>,
    /// Whether the slot was busy at any boundary since the last call.
    pub active: bool,
    /// Whether [`crate::Machine::change_medium`] is accepted now.
    pub accepts_changes: bool,
}

impl SlotState {
    /// An idle slot with an empty catalogue that takes no change.
    pub fn new(id: SlotId) -> Self {
        Self {
            id,
            catalogue: Vec::new(),
            inserted: None,
            active: false,
            accepts_changes: false,
        }
    }

    pub fn with_catalogue(mut self, catalogue: Vec<MediumInfo>) -> Self {
        self.catalogue = catalogue;
        self
    }

    pub fn with_inserted(mut self, inserted: Option<usize>) -> Self {
        self.inserted = inserted;
        self
    }

    pub fn with_active(mut self, active: bool) -> Self {
        self.active = active;
        self
    }

    pub fn with_accepts_changes(mut self, accepts_changes: bool) -> Self {
        self.accepts_changes = accepts_changes;
        self
    }
}

/// One medium of a slot's catalogue.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediumInfo {
    /// A name for people; never a host path.
    pub name: String,
    pub sha256: [u8; 32],
    pub size: u64,
    pub format: String,
    pub writes: WriteKind,
}

impl MediumInfo {
    pub fn new(
        name: impl Into<String>,
        sha256: [u8; 32],
        size: u64,
        format: impl Into<String>,
        writes: WriteKind,
    ) -> Self {
        Self {
            name: name.into(),
            sha256,
            size,
            format: format.into(),
            writes,
        }
    }
}

/// What happens to a guest's writes to a medium.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteKind {
    /// The guest sees a write-protected medium.
    ReadOnly,
    /// Guest writes are kept in memory and lost with the session.
    Discarded,
    /// Guest writes are kept in the machine's storage, separate from the
    /// base.
    Kept,
}

/// A medium the front end hands to a running machine.
#[non_exhaustive]
#[derive(Debug)]
pub struct NewMedium<'a> {
    pub name: &'a str,
    /// The read-only base, verified in full by the front end.
    pub content: Content,
    pub sha256: [u8; 32],
    pub size: u64,
    pub format: &'a str,
    pub writes: WriteTarget<'a>,
}

impl<'a> NewMedium<'a> {
    pub fn new(
        name: &'a str,
        content: Content,
        sha256: [u8; 32],
        size: u64,
        format: &'a str,
        writes: WriteTarget<'a>,
    ) -> Self {
        Self {
            name,
            content,
            sha256,
            size,
            format,
            writes,
        }
    }
}

/// Where a guest's writes to a new medium go.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum WriteTarget<'a> {
    ReadOnly,
    Discarded,
    /// Keep written sectors in this record of the machine's storage, named
    /// by the front end under the rules of [`crate::is_kept_record_name`]:
    /// a record name that starts with [`crate::KEPT_RECORD_PREFIX`]. The
    /// core owns the record's format; the base is never changed. Replaces
    /// the file path of contract 0.1.
    ///
    /// [`crate::Machine::add_medium`] fails with
    /// [`crate::ErrorKind::Usage`] for a name that breaks those rules. The
    /// core opens the record at the latest during insertion; the call that
    /// opens it reports failure immediately, never a later run.
    #[non_exhaustive]
    Kept {
        record: &'a str,
    },
}

/// A change of what a slot holds.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MediaAction {
    Eject,
    /// Insert a catalogue entry.
    Insert(usize),
    /// Insert the next entry, wrapping to the first; an empty slot takes
    /// the first.
    Next,
}

/// A slot of a machine, by the number its description gives it.
/// Deliberately closed scalar representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SlotId(pub u8);

impl<'a> WriteTarget<'a> {
    /// Keep writes in a record; `Machine::add_medium` validates its name.
    pub const fn kept(record: &'a str) -> Self {
        Self::Kept { record }
    }
}

/// One configured slot described offline, without opening content (since 0.5).
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct MediaDescription {
    pub slot: SlotDescription,
    /// Display-only text in the core's vocabulary. Front ends show it verbatim
    /// and never parse it for geometry, lengths or format decisions.
    pub detail: String,
    /// Medium byte length when fixed by the config, otherwise unknown. Never
    /// derive a length from detail; unknown is distinct from an empty medium.
    pub size: Option<u64>,
}

impl MediaDescription {
    /// A slot with empty detail and unknown medium length.
    pub fn new(slot: SlotDescription) -> Self {
        Self {
            slot,
            detail: String::new(),
            size: None,
        }
    }
    pub fn with_detail(mut self, detail: impl Into<String>) -> Self {
        self.detail = detail.into();
        self
    }
    pub const fn with_size(mut self, size: Option<u64>) -> Self {
        self.size = size;
        self
    }
}
