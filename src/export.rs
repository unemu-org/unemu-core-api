// SPDX-License-Identifier: MIT OR Apache-2.0
//! Offline output descriptions, current media readers and streamed sinks.

use crate::{ContentReader, Error, SlotId};

/// What an export format represents. Introduced in contract 0.5.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportScope {
    /// Exactly one medium of one slot.
    Slot,
    /// A machine document and every slot medium it exports.
    Machine,
}

/// An optional offline format advertised by a platform since contract 0.5.
/// An empty platform list means export is absent; older front ends ignore it.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ExportFormat {
    /// Stable core-defined id, unique within the platform. Same syntax as a
    /// content kind id: 1..=64 ASCII lowercase letters, digits or hyphens,
    /// starting with a letter or digit.
    pub id: &'static str,
    /// Nonempty display name.
    pub name: &'static str,
    /// Suggested extension: 1..=16 ASCII lowercase letters or digits, no dot.
    pub extension: &'static str,
    pub scope: ExportScope,
}

impl ExportFormat {
    /// Describe a format in a static platform description.
    ///
    /// # Panics
    /// If any field breaks its documented rules.
    pub const fn new(
        id: &'static str,
        name: &'static str,
        extension: &'static str,
        scope: ExportScope,
    ) -> Self {
        let value = Self {
            id,
            name,
            extension,
            scope,
        };
        assert!(value.is_valid(), "invalid export format");
        value
    }

    /// Check public fields before trusting a core's description.
    pub const fn is_valid(&self) -> bool {
        crate::content::is_kind_id(self.id)
            && !self.name.is_empty()
            && crate::content::is_extension(self.extension)
    }
}

/// One slot's CURRENT medium, assembled by the front end from its read-only
/// base, layers and journal. This never grants write access to those inputs.
#[non_exhaustive]
#[derive(Clone, Copy)]
pub struct ExportMedium<'a> {
    pub slot: SlotId,
    /// Declared length of `bytes`; must also match a fixed description size.
    pub size: u64,
    /// Stable snapshot for the whole call, through the existing content-reader
    /// abstraction. The boundary verifies full SHA-256 and length as usual;
    /// bases remain identified by their own full digest, even for large media.
    pub bytes: &'a dyn ContentReader,
}

impl<'a> ExportMedium<'a> {
    pub const fn new(slot: SlotId, size: u64, bytes: &'a dyn ContentReader) -> Self {
        Self { slot, size, bytes }
    }
}

/// A selected format and current media. Introduced in contract 0.5.
#[non_exhaustive]
#[derive(Clone, Copy)]
pub struct ExportRequest<'a> {
    pub format: &'a str,
    /// Exactly one eligible slot for Slot scope; every required slot exactly
    /// once for Machine scope, as returned by [`crate::Core::export_slots`].
    /// Missing, duplicate and ineligible slots are Usage. Ordering has no
    /// meaning; reordered requests are accepted and outputs use canonical order.
    pub media: &'a [ExportMedium<'a>],
}

impl<'a> ExportRequest<'a> {
    pub const fn new(format: &'a str, media: &'a [ExportMedium<'a>]) -> Self {
        Self { format, media }
    }
}

/// Whether a plain relative output name is valid: 1..=64 bytes of ASCII
/// `a-z`, `0-9`, `-` and `.`, starting and ending with a letter or digit,
/// without `..` anywhere. No separators, absolute names, drive prefixes,
/// whitespace or non-ASCII characters. Backend-special names must be mapped
/// internally without changing the logical name used in machine documents.
pub const fn is_export_name(name: &str) -> bool {
    if !crate::is_record_name(name) {
        return false;
    }
    let bytes = name.as_bytes();
    let mut index = 1;
    while index < bytes.len() {
        if bytes[index - 1] == b'.' && bytes[index] == b'.' {
            return false;
        }
        index += 1;
    }
    true
}

/// Output creation supplied by a front end. No host path crosses this boundary.
/// On any failed export the front end removes ALL outputs it created, including
/// unfinished ones. Dropping an output is not rollback or successful finish.
/// The sink owns every output and its backend resources. After export returns,
/// the front end can close them all before cleanup, even if the core never
/// finished them. A core cannot retain a borrowed output beyond the sink borrow
/// in `Core::export`, so it cannot write after that call returns.
pub trait ExportSink {
    /// Reserve exactly `name`, validated with [`is_export_name`]. Invalid names
    /// and an already reserved/created name fail with Usage, never overwrite.
    /// All valid names must be usable, with backend mapping if necessary.
    /// Other failures are Storage; a failed create may leave an incomplete
    /// output for the front end to remove. No input medium is changed.
    fn create(&mut self, name: &str) -> Result<&mut (dyn ExportOutput + '_), Error>;
}

/// A streamed output. Object-safe and independent of host I/O.
pub trait ExportOutput {
    /// Append all bytes, in order. Only Storage is allowed on failure; a partial
    /// append is possible and invalidates the export. Never silently short-write.
    fn write(&mut self, bytes: &[u8]) -> Result<(), Error>;

    /// Complete and close the output on success or failure. Further writes or
    /// finish calls must fail with Storage, without changing bytes or receipts.
    /// Returns its exact logical name, length and full SHA-256. Only Storage
    /// is allowed on failure; the front end still cleans up incomplete outputs.
    ///
    /// This is the hashing hook: the sink/front end hashes EVERY output byte,
    /// preferably while streaming, using its own implementation or dependency.
    /// The contract stays dependency-free and never substitutes sampled hashes,
    /// absent digests or hashes of input media for output hashes. Successful
    /// finish means all writes are accepted; host durability is front-end policy.
    fn finish(&mut self) -> Result<ExportedOutput, Error>;
}

/// Receipt for one successfully finished output, including machine documents.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ExportedOutput {
    pub name: String,
    pub size: u64,
    pub sha256: [u8; 32],
}

impl ExportedOutput {
    /// A sink's receipt. Panics if the logical name is invalid.
    pub fn new(name: impl Into<String>, size: u64, sha256: [u8; 32]) -> Self {
        let name = name.into();
        assert!(is_export_name(&name), "invalid export output name");
        Self { name, size, sha256 }
    }
}

/// Complete export result. An error never returns a partial success report.
#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct ExportReport {
    /// Exactly one receipt per output created, all successfully finished;
    /// names are unique. Includes outputs referenced by machine documents.
    /// Receipts are sink-computed but core-editable: front ends compare this
    /// list with their own record of created outputs and completed receipts
    /// before accepting success.
    pub outputs: Vec<ExportedOutput>,
    /// Human-readable notes naming EVERY setting the format cannot express.
    /// Never silently discard a setting; front ends present these notes.
    pub notes: Vec<String>,
}

impl ExportReport {
    pub const fn new() -> Self {
        Self {
            outputs: Vec::new(),
            notes: Vec::new(),
        }
    }
    pub fn with_outputs(mut self, outputs: Vec<ExportedOutput>) -> Self {
        self.outputs = outputs;
        self
    }
    pub fn with_notes(mut self, notes: Vec<String>) -> Self {
        self.notes = notes;
        self
    }
}
