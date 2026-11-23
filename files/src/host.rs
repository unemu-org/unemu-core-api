// SPDX-License-Identifier: MIT OR Apache-2.0
//! Injectable host operations for file records and their replacements.

use std::any::Any;
use std::fs::{self, File, Metadata, OpenOptions, Permissions, TryLockError};
use std::io::{self, Read, Seek, Write};
use std::path::{Path, PathBuf};

/// An open record or temporary inode. Implementations must report partial
/// writes and failed barriers honestly; a failed write may have changed bytes.
/// `metadata` describes this handle, never a later occupant of its path.
pub trait FileIo: Read + Write + Seek {
    /// Inspect this inode, including its length, permissions and identity.
    fn metadata(&self) -> io::Result<Metadata>;
    /// Cut or extend this inode. An error may follow a partial change.
    fn set_len(&mut self, len: u64) -> io::Result<()>;
    /// Confirm all bytes and inode metadata on stable storage.
    fn sync_all(&mut self) -> io::Result<()>;
    /// Set the exact permissions, without applying the creation umask again.
    fn set_permissions(&mut self, permissions: Permissions) -> io::Result<()>;
}

impl FileIo for File {
    fn metadata(&self) -> io::Result<Metadata> {
        File::metadata(self)
    }
    fn set_len(&mut self, len: u64) -> io::Result<()> {
        File::set_len(self, len)
    }
    fn sync_all(&mut self) -> io::Result<()> {
        File::sync_all(self)
    }
    fn set_permissions(&mut self, permissions: Permissions) -> io::Result<()> {
        File::set_permissions(self, permissions)
    }
}

/// Host operations used by a record, including retention replacements and
/// orphan cleanup. Defaults operate on real local files. Override any method
/// to model failures; the same implementation is retained for the record's
/// lifetime. No method may silently weaken a requested lock or barrier.
///
/// Directories must be trusted: identity checks prevent accidental alias
/// removal, but cannot exclude a hostile rename between the last check and
/// unlink. Locks coordinate users of the same canonical path, not arbitrary
/// hard-link aliases. Callers must not open one record by hard-link aliases.
pub trait Filesystem {
    /// Resolve an existing path, following links.
    fn canonicalize(&self, path: &Path) -> io::Result<PathBuf> {
        path.canonicalize()
    }
    /// Inspect an entry without following its final symbolic link.
    fn symlink_metadata(&self, path: &Path) -> io::Result<Metadata> {
        fs::symlink_metadata(path)
    }
    /// List immediate children; no traversal is performed by cleanup.
    fn entries(&self, dir: &Path) -> io::Result<Vec<PathBuf>> {
        fs::read_dir(dir)?
            .map(|entry| entry.map(|entry| entry.path()))
            .collect()
    }
    /// Open an existing regular file; `write` requests write access.
    /// A missing entry is `NotFound`, never an empty handle.
    fn open(&self, path: &Path, write: bool) -> io::Result<Box<dyn FileIo>> {
        let mut options = OpenOptions::new();
        options.read(true).write(write);
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            options.custom_flags(candidate_flags()?);
        }
        let file = options.open(path)?;
        if !file.metadata()?.is_file() {
            return Err(io::Error::other("record must be a regular file"));
        }
        Ok(Box::new(file))
    }
    /// Open a cleanup candidate without following links or blocking on a
    /// special file substituted after entry inspection. Unsupported hosts
    /// may refuse: cleanup then preserves the candidate.
    fn open_candidate(&self, path: &Path) -> io::Result<Box<dyn FileIo>> {
        #[cfg(unix)]
        {
            use std::os::unix::fs::OpenOptionsExt;
            let mut options = OpenOptions::new();
            options.read(true).custom_flags(candidate_flags()?);
            Ok(Box::new(options.open(path)?))
        }
        #[cfg(not(unix))]
        {
            let _ = path;
            Err(io::ErrorKind::Unsupported.into())
        }
    }

    /// Create a new read/write file exclusively. Initial permissions on Unix
    /// are 0600 unless supplied; exact replacement permissions are set again
    /// before publication to undo the creation umask.
    fn create_new(
        &self,
        path: &Path,
        permissions: Option<&Permissions>,
    ) -> io::Result<Box<dyn FileIo>> {
        let mut options = OpenOptions::new();
        options.read(true).write(true).create_new(true);
        #[cfg(unix)]
        {
            use std::os::unix::fs::{OpenOptionsExt, PermissionsExt};
            options.mode(permissions.map_or(0o600, |p| p.mode() & 0o7777));
        }
        #[cfg(not(unix))]
        let _ = permissions;
        Ok(Box::new(options.open(path)?))
    }
    /// Publish a new name without replacing any existing entry.
    fn hard_link(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::hard_link(from, to)
    }
    /// Atomically replace `to` with `from`. An error may have occurred after
    /// replacement; records therefore treat it as uncertain.
    fn rename(&self, from: &Path, to: &Path) -> io::Result<()> {
        fs::rename(from, to)
    }
    /// Remove exactly this directory entry; never follow a symbolic link.
    fn remove_file(&self, path: &Path) -> io::Result<()> {
        fs::remove_file(path)
    }
    /// Make a publication/removal durable. On Unix sync the parent directory;
    /// on other hosts sync the published file (the limitation in FileStorage).
    fn sync_directory(&self, path: &Path) -> io::Result<()> {
        #[cfg(unix)]
        {
            File::open(parent(path))?.sync_all()
        }
        #[cfg(not(unix))]
        {
            OpenOptions::new().write(true).open(path)?.sync_all()
        }
    }
    /// Open/create the stable sidecar and take a nonblocking shared or
    /// exclusive lock. Return `WouldBlock` for contention. The returned guard
    /// releases the lock on drop and must never unlink the sidecar.
    fn lock(&self, path: &Path, shared: bool) -> io::Result<Box<dyn Any>> {
        let file = if shared {
            match File::open(path) {
                Ok(file) => file,
                Err(error) if error.kind() == io::ErrorKind::NotFound => {
                    match OpenOptions::new()
                        .read(true)
                        .write(true)
                        .create_new(true)
                        .open(path)
                    {
                        Ok(file) => file,
                        Err(error) if error.kind() == io::ErrorKind::AlreadyExists => {
                            File::open(path)?
                        }
                        Err(error) => return Err(error),
                    }
                }
                Err(error) => return Err(error),
            }
        } else {
            OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(path)?
        };
        let locked = if shared {
            file.try_lock_shared()
        } else {
            file.try_lock()
        };
        locked.map_err(|error| match error {
            TryLockError::WouldBlock => {
                io::Error::new(io::ErrorKind::WouldBlock, "record is already locked")
            }
            TryLockError::Error(error) => error,
        })?;
        Ok(Box::new(HostLock(file)))
    }
}

/// The standard library implementation of every host operation.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default)]
pub struct HostFilesystem;

impl HostFilesystem {
    /// The standard host filesystem implementation.
    pub const fn new() -> Self {
        Self
    }
}
impl Filesystem for HostFilesystem {}

struct HostLock(File);
impl Drop for HostLock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

pub(crate) fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|p| !p.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

pub(crate) fn same_file(a: &Metadata, b: &Metadata) -> bool {
    #[cfg(unix)]
    {
        use std::os::unix::fs::MetadataExt;
        a.is_file() && b.is_file() && a.dev() == b.dev() && a.ino() == b.ino()
    }

    #[cfg(not(unix))]
    {
        let _ = (a, b);
        false
    }
}

#[cfg(unix)]
fn candidate_flags() -> io::Result<i32> {
    // These host ABI flags are O_NOFOLLOW | O_NONBLOCK. Other ABIs keep
    // leftovers rather than risk following a link or blocking on a FIFO.
    #[cfg(all(
        any(target_os = "linux", target_os = "android"),
        any(
            target_arch = "x86",
            target_arch = "x86_64",
            target_arch = "aarch64",
            target_arch = "arm",
            target_arch = "riscv32",
            target_arch = "riscv64"
        )
    ))]
    {
        Ok(0x20000 | 0x800)
    }
    #[cfg(any(
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    ))]
    {
        Ok(0x100 | 0x4)
    }
    #[cfg(not(any(
        all(
            any(target_os = "linux", target_os = "android"),
            any(
                target_arch = "x86",
                target_arch = "x86_64",
                target_arch = "aarch64",
                target_arch = "arm",
                target_arch = "riscv32",
                target_arch = "riscv64"
            )
        ),
        target_os = "macos",
        target_os = "ios",
        target_os = "freebsd",
        target_os = "openbsd",
        target_os = "netbsd",
        target_os = "dragonfly"
    )))]
    {
        Err(io::ErrorKind::Unsupported.into())
    }
}
