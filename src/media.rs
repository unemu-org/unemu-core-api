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
//! to a [`WriteTarget`] below the session directory and never into the base,
//! and nothing here rewrites a base in place.

use crate::Content;
use std::path::Path;

/// One slot of a machine.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct SlotDescription {
    pub id: SlotId,
    pub kind: SlotKind,
    /// The slot's name for people, in the platform's words: `A:`, `C:`,
    /// `Cartridge`.
    pub label: String,
    /// Whether the catalogue and the inserted medium can change at all.
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
    /// Guest writes are kept in the session, separate from the base.
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
    /// Keep written sectors in this file below the session directory. The
    /// core owns its format; the base is never changed.
    Kept {
        file: &'a Path,
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
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct SlotId(pub u8);
