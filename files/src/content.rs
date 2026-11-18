// SPDX-License-Identifier: MIT OR Apache-2.0
//! The contract's [`ContentReader`] over a host file (core-contract.md
//! §12.5).

use crate::storage::within;
use std::fs::File;
use std::io::{self, Read, Seek, SeekFrom};
use std::path::Path;
use std::time::SystemTime;
use unemu_core_api::{ContentReader, Error, ErrorKind};

/// Verified content read from a host file at any offset: a disk base, a
/// cartridge, a large ROM set.
///
/// The front end verifies the file's SHA-256 and length in full and then
/// hands this object to a core. The object keeps the file open and never
/// writes it.
///
/// A change of the file behind the object is detected when it is cheap:
/// after each read the object compares the file's length and modification
/// time with those it saw when it was made, and fails the read with
/// [`ErrorKind::Content`] when either differs. A change that keeps the
/// length within the file system's time resolution, or on a host that
/// reports no modification time, goes unseen; the contract tells a core not
/// to rely on detection.
#[derive(Debug)]
pub struct FileContent {
    file: File,
    size: u64,
    modified: Option<SystemTime>,
}

impl FileContent {
    /// Open the file at `path`, whose verified length is `size`.
    ///
    /// Fails with [`ErrorKind::Storage`] when the file cannot be opened and
    /// with [`ErrorKind::Content`] when its length is not `size`.
    pub fn open(path: impl AsRef<Path>, size: u64) -> Result<Self, Error> {
        let file = File::open(path).map_err(|error| storage("cannot open the content", &error))?;
        Self::from_file(file, size)
    }

    /// Content over a file already open for reading, such as the one the
    /// front end hashed, whose verified length is `size`.
    ///
    /// Fails as [`FileContent::open`] does.
    pub fn from_file(file: File, size: u64) -> Result<Self, Error> {
        let metadata = file
            .metadata()
            .map_err(|error| storage("cannot read the content's length", &error))?;
        if metadata.len() != size {
            return Err(changed());
        }
        Ok(Self {
            file,
            size,
            modified: metadata.modified().ok(),
        })
    }

    /// Whether the file still has the length and the modification time it
    /// had when the object was made; `None` when the host cannot say.
    fn unchanged(&self) -> Option<bool> {
        let metadata = self.file.metadata().ok()?;
        let same_time = match self.modified {
            Some(modified) => metadata.modified().ok() == Some(modified),
            None => true,
        };
        Some(metadata.len() == self.size && same_time)
    }
}

fn storage(what: &str, error: &io::Error) -> Error {
    Error::new(ErrorKind::Storage, format!("{what}: {error}"))
}

fn changed() -> Error {
    Error::new(
        ErrorKind::Content,
        "the file is no longer the verified content",
    )
}

impl ContentReader for FileContent {
    fn size(&self) -> u64 {
        self.size
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), Error> {
        if !within(offset, buf.len(), self.size) {
            return Err(Error::new(ErrorKind::Usage, "read past the end"));
        }
        let mut file = &self.file;
        let read = file
            .seek(SeekFrom::Start(offset))
            .and_then(|_| file.read_exact(buf));
        // A file cut short fails the read itself, so a failed read is
        // checked for a change too before it is called a host failure.
        if self.unchanged() == Some(false) {
            return Err(changed());
        }
        read.map_err(|error| storage("cannot read the content", &error))
    }
}
