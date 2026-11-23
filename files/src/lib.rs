// SPDX-License-Identifier: MIT OR Apache-2.0
//! The storage and content objects of `unemu-core-api` over host files
//! (core-contract.md §12.5, §13.1).
//!
//! A core touches nothing of the host: what it reads comes as
//! [`unemu_core_api::ContentReader`]s and what must outlive a run goes to
//! the [`unemu_core_api::Record`]s of a [`unemu_core_api::Storage`]. This
//! crate is those objects for a host with a file system:
//!
//! - [`FileStorage`]: a directory of records, one file per record, held
//!   exclusively through a file lock, synchronised and replaced in one
//!   atomic step.
//! - [`FileRecord`]: a record at an arbitrary path, with a stable sidecar
//!   lock, no-clobber initial publication and protected-file cleanup.
//! - [`PathLock`]: exclusive use or shared read-only inspection, also for
//!   offline tools; [`Filesystem`] and [`FileIo`] inject host operations.
//! - [`FileContent`]: verified content read from a host file.
//! - [`file_name`] and [`record_name`]: how a record name becomes a file
//!   name, the same on every host.
//!
//! Who links it: a front end, and a core's own command line, which is a
//! front end of one core. A core library never does; it takes the objects
//! the front end hands to it, so it also runs where there are no files.
//! This is the one crate of the contract's repository that touches the
//! host, and it is tested on Linux, macOS and Windows.
//!
//! A storage, its records and their replacements live on one thread, as
//! the contract's machine does.

#![forbid(unsafe_code)]

mod content;
mod host;
mod names;
mod path;
mod storage;

pub use crate::content::FileContent;
pub use crate::host::{FileIo, Filesystem, HostFilesystem};
pub use crate::names::{file_name, record_name};
pub use crate::path::{is_temporary_path, FileError, FileStage, PathLock};
pub use crate::storage::{FileRecord, FileStorage};

#[cfg(test)]
mod tests;
