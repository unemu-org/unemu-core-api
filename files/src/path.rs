// SPDX-License-Identifier: MIT OR Apache-2.0
//! Paths, stable sidecar locks and conservative orphan cleanup.

use crate::host::{parent, same_file};
use crate::{Filesystem, HostFilesystem};
use std::any::Any;
use std::fs::Metadata;
use std::io::{self, Read};
use std::path::{Path, PathBuf};
use unemu_core_api::{Error, ErrorKind};

/// A failed opening/locking stage, preserved separately from the host error.
/// Contention and invalid record paths are Usage; other host failures are
/// Storage. Record and replacement errors carry their stage in the message.
#[derive(Debug)]
#[non_exhaustive]
pub struct FileError {
    /// The operation that failed.
    pub stage: FileStage,
    /// The host's original error, including its error kind and OS code.
    pub source: io::Error,
}
impl std::fmt::Display for FileError {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(f, "{}: {}", self.stage, self.source)
    }
}
impl std::error::Error for FileError {
    fn source(&self) -> Option<&(dyn std::error::Error + 'static)> {
        Some(&self.source)
    }
}
impl From<FileError> for Error {
    fn from(error: FileError) -> Self {
        let kind = if error.source.kind() == io::ErrorKind::WouldBlock
            || (error.stage == FileStage::ResolvePath
                && error.source.kind() == io::ErrorKind::InvalidInput)
        {
            ErrorKind::Usage
        } else {
            ErrorKind::Storage
        };
        Self::new(kind, error.to_string())
    }
}
/// The operation that failed while opening a path record or taking its lock.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub enum FileStage {
    ResolvePath,
    Lock,
    Open,
    Metadata,
    VerifyPath,
    Cleanup,
    DirectorySync,
}

impl std::fmt::Display for FileStage {
    fn fmt(&self, f: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        f.write_str(match self {
            Self::ResolvePath => "resolve path",
            Self::Lock => "lock",
            Self::Open => "open",
            Self::Metadata => "metadata",
            Self::VerifyPath => "verify path",
            Self::Cleanup => "cleanup",
            Self::DirectorySync => "directory sync",
        })
    }
}

impl FileError {
    /// Preserve a typed opening stage and the original host error.
    pub fn new(stage: FileStage, source: io::Error) -> Self {
        Self { stage, source }
    }
}

pub(crate) fn stage(stage: FileStage, source: io::Error) -> FileError {
    FileError::new(stage, source)
}

/// A lock on the canonical record's stable `FILE.lock` sidecar. The file is
/// never removed. Keep this token through any offline reads or mutations,
/// or transfer it to FileRecord::open_with to avoid taking the lock twice.
/// Shared tokens can only open an inspection record.
pub struct PathLock {
    pub(crate) path: PathBuf,
    pub(crate) shared: bool,
    pub(crate) guard: Option<Box<dyn Any>>,
}
impl PathLock {
    /// Take an exclusive nonblocking lock after resolving `path`.
    pub fn exclusive(path: impl AsRef<Path>) -> Result<Self, FileError> {
        Self::acquire(&HostFilesystem, path.as_ref(), false)
    }
    /// Take a shared nonblocking inspection lock. Only when the sidecar is
    /// absent and creation fails with PermissionDenied or ReadOnlyFilesystem
    /// is a token without a lock returned. An existing, unreadable or dangling
    /// sidecar, lock contention and unsupported locks are never bypassed.
    pub fn inspection(path: impl AsRef<Path>) -> Result<Self, FileError> {
        Self::acquire(&HostFilesystem, path.as_ref(), true)
    }
    /// The same protocol with injectable namespace and locking operations.
    pub fn acquire(fs: &dyn Filesystem, path: &Path, shared: bool) -> Result<Self, FileError> {
        let path = resolve(fs, path).map_err(|e| stage(crate::FileStage::ResolvePath, e))?;
        let sidecar = suffix(&path, ".lock");
        let guard = match fs.lock(&sidecar, shared) {
            Ok(guard) => Some(guard),
            Err(error)
                if shared
                    && matches!(
                        error.kind(),
                        io::ErrorKind::PermissionDenied | io::ErrorKind::ReadOnlyFilesystem
                    )
                    && matches!(fs.symlink_metadata(&sidecar), Err(e) if e.kind() == io::ErrorKind::NotFound) =>
            {
                None
            }
            Err(error) => return Err(stage(crate::FileStage::Lock, error)),
        };
        Ok(Self {
            path,
            shared,
            guard,
        })
    }
    /// Whether inspection holds a lock. A false answer means the documented
    /// read-only-folder fallback; it conveys no permission to mutate.
    pub fn is_locked(&self) -> bool {
        self.guard.is_some()
    }
    /// The canonical path used for the lock and all sibling names.
    pub fn path(&self) -> &Path {
        &self.path
    }
}

pub(crate) fn suffix(path: &Path, suffix: &str) -> PathBuf {
    let mut name = path.as_os_str().to_os_string();
    name.push(suffix);
    PathBuf::from(name)
}

/// Whether a filename contains the reserved `.create-` or `.retain-`
/// marker. All suffixes are reserved to keep nested temporary namespaces
/// from overlapping, including filenames that are not UTF-8.
pub fn is_temporary_path(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        let name = name.as_encoded_bytes();
        [b".create-".as_slice(), b".retain-"]
            .iter()
            .any(|marker| name.windows(marker.len()).any(|part| part == *marker))
    })
}
// Sidecars must never become records: replacing a lock changes its inode
// and lets another opener bypass the lock held on the original inode.
pub(crate) fn is_reserved_path(path: &Path) -> bool {
    path.file_name().is_some_and(|name| {
        // Case variants also alias sidecars on case-insensitive filesystems.
        let name = name.as_encoded_bytes().to_ascii_lowercase();
        [b".create-".as_slice(), b".retain-"]
            .iter()
            .any(|marker| name.windows(marker.len()).any(|part| part == *marker))
            || name.ends_with(b".lock")
            || name.ends_with(b"_lock")
            || name.strip_suffix(b".tmp").is_some_and(|stem| {
                stem.rsplit(|b| *b == b'_')
                    .next()
                    .is_some_and(|n| !n.is_empty() && n.iter().all(u8::is_ascii_digit))
                    && stem.contains(&b'_')
            })
    })
}
fn decimal_tmp(end: &str) -> bool {
    end.strip_suffix(".tmp")
        .is_some_and(|n| !n.is_empty() && n.bytes().all(|b| b.is_ascii_digit()))
}
pub(crate) fn temporary_of(path: &Path, candidate: &Path, marker: &str) -> bool {
    if parent(path) != parent(candidate) {
        return false;
    }
    let prefix = suffix(path, &format!(".{marker}-"));
    // OsStr preserves filenames that are not UTF-8; only the generated suffix
    // must be ASCII.
    let name = candidate.as_os_str().as_encoded_bytes();
    let prefix = prefix.as_os_str().as_encoded_bytes();
    name.strip_prefix(prefix)
        .and_then(|tail| std::str::from_utf8(tail).ok())
        .is_some_and(decimal_tmp)
}

pub(crate) fn resolve(fs: &dyn Filesystem, path: &Path) -> io::Result<PathBuf> {
    if is_reserved_path(path) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "reserved sidecar path",
        ));
    }
    let resolved = match fs.canonicalize(path) {
        Ok(path) => path,
        Err(error) if error.kind() == io::ErrorKind::NotFound => {
            match fs.symlink_metadata(path) {
                Ok(meta) if meta.file_type().is_symlink() => {
                    return Err(io::Error::new(
                        io::ErrorKind::InvalidInput,
                        "dangling symbolic link",
                    ))
                }
                Err(e) if e.kind() != io::ErrorKind::NotFound => return Err(e),
                _ => (),
            }
            let name = path.file_name().ok_or_else(|| {
                io::Error::new(io::ErrorKind::InvalidInput, "record path needs a filename")
            })?;
            fs.canonicalize(parent(path))?.join(name)
        }
        Err(error) => return Err(error),
    };
    if is_reserved_path(&resolved) {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "reserved sidecar path",
        ));
    }
    Ok(resolved)
}

pub(crate) fn verify(fs: &dyn Filesystem, path: &Path) -> io::Result<()> {
    if fs.canonicalize(path)? != path {
        return Err(io::Error::new(
            io::ErrorKind::InvalidInput,
            "canonical path changed after locking",
        ));
    }
    Ok(())
}

pub(crate) fn cleanup(
    fs: &dyn Filesystem,
    path: &Path,
    initial: &[u8],
    protected: Option<&Metadata>,
) -> io::Result<()> {
    for candidate in fs.entries(parent(path))? {
        let creation = temporary_of(path, &candidate, "create");
        if !creation && !temporary_of(path, &candidate, "retain") {
            continue;
        }
        let before = match fs.symlink_metadata(&candidate) {
            Ok(meta) => meta,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        };
        if !before.is_file() || (creation && before.len() > initial.len() as u64) {
            continue;
        }
        let Ok(mut file) = fs.open_candidate(&candidate) else {
            continue;
        };
        let opened = file.metadata()?;
        if !same_file(&before, &opened) || protected.is_some_and(|p| same_file(p, &opened)) {
            continue;
        }
        if creation {
            let mut bytes = Vec::new();
            if file
                .by_ref()
                .take(initial.len() as u64 + 1)
                .read_to_end(&mut bytes)
                .is_err()
                || !initial.starts_with(&bytes)
            {
                continue;
            }
        }
        match fs.symlink_metadata(&candidate) {
            Ok(current) if same_file(&opened, &current) => (),
            Ok(_) => continue,
            Err(e) if e.kind() == io::ErrorKind::NotFound => continue,
            Err(e) => return Err(e),
        }
        match fs.remove_file(&candidate) {
            Ok(()) => (),
            Err(e) if e.kind() == io::ErrorKind::NotFound => (),
            Err(e) => return Err(e),
        }
    }
    Ok(())
}
