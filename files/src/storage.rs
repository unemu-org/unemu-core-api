// SPDX-License-Identifier: MIT OR Apache-2.0
//! The contract's [`Storage`], [`Record`] and [`Replacement`] over a
//! directory of host files (core-contract.md §12.5).

use crate::names::file_name;
use std::cell::RefCell;
use std::fs::{self, File, OpenOptions, TryLockError};
use std::io::{self, Read, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use unemu_core_api::{Error, ErrorKind, Record, Replacement, Storage};

/// A [`Storage`] whose records are the files of one host directory.
///
/// Each record is one file, named after the record ([`crate::file_name`]).
/// A record that does not exist opens empty, and its file appears on the
/// first write: an `append` of at least one byte, or a commit.
///
/// # Exclusive use
///
/// [`Storage::open`] takes an exclusive lock ([`File::try_lock`]) on a lock
/// file of its own beside the record, `FILE_lock`, and holds it until the
/// record object is dropped. It never waits: a record already held, by this
/// storage, by another `FileStorage` over the same directory or by another
/// program, fails to open with [`ErrorKind::Usage`]. The lock is per record
/// and not per directory, because the contract makes records, not storages,
/// exclusive: a front end may hold one record of a directory while a tool
/// reads another. Lock files stay in the directory after use, empty:
/// removing one while another program waits to lock it would let two
/// programs lock two different files of the same name. A host or file
/// system without file locks fails the open with [`ErrorKind::Storage`]; it
/// never opens without the lock.
///
/// # Durability
///
/// [`Record::sync`] calls [`File::sync_all`], which on Apple's systems is
/// `F_FULLFSYNC`, so the bytes reach the medium and not only the drive's
/// cache. When the sync follows the file's creation, the directory is
/// synchronised as well on Unix, so the record's name survives a power cut
/// with its bytes.
///
/// [`Replacement::commit`] synchronises the new contents in a temporary
/// sibling `FILE_N.tmp`, renames it over the record's file (one atomic step
/// on every supported host) and then makes the rename durable. On Unix that
/// is a sync of the directory. On Windows the standard library offers no
/// way to synchronise a directory, so the commit opens the renamed file and
/// synchronises it once more; on NTFS that flushes the file system's
/// metadata log, which holds the rename, but Windows does not document it
/// as a promise. So on Windows a commit is atomic (the record holds the old
/// or the new contents, whole) and a commit that answered `Ok` is durable
/// as far as NTFS's log is; the same holds for the name of a record created
/// by its first `sync`.
///
/// # Failures
///
/// The contract's rules ([`Record`]): a failed `append` cuts the file back
/// to its old length and answers [`ErrorKind::Storage`], or
/// [`ErrorKind::StorageUncertain`] when the cut fails too; a failed
/// `truncate` answers `Storage` when the file kept its length and
/// `StorageUncertain` otherwise; a failed `sync` is `StorageUncertain`; a
/// commit that fails before the rename is `Storage` and leaves the record
/// as it was, and one whose rename took place but could not be made
/// durable is `StorageUncertain`. After `StorageUncertain` reads still
/// work and writes fail with [`ErrorKind::Usage`] until the record is
/// opened again.
///
/// # Leftovers of a crash
///
/// A crash while a replacement is written leaves its `FILE_N.tmp`. Opening
/// the record removes every such file of that record, under the record's
/// lock, so no other program is writing one; a file it cannot remove is
/// ignored, as nothing ever reads a temporary file of an earlier opening.
/// A replacement dropped after its record was released leaves its file to
/// that cleanup too, because the record's next holder may already use the
/// same name.
pub struct FileStorage {
    dir: PathBuf,
    faults: Faults,
}

impl FileStorage {
    /// The storage in the directory `dir`, created when it does not exist
    /// (with its missing parents, each made durable in its own parent on
    /// Unix).
    ///
    /// Fails with [`ErrorKind::Storage`] when the directory cannot be
    /// created.
    pub fn open(dir: impl AsRef<Path>) -> Result<Self, Error> {
        let dir = dir.as_ref();
        create_dir_durably(dir).map_err(|error| host("cannot create the directory", &error))?;
        Ok(Self {
            dir: dir.to_path_buf(),
            faults: Faults::default(),
        })
    }

    /// The directory of the records.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Make the next `step` of the record `name` fail, once.
    #[cfg(test)]
    pub(crate) fn fail_next(&self, name: &str, step: Step) {
        self.faults.0.borrow_mut().push((name.to_owned(), step));
    }
}

impl Storage for FileStorage {
    fn open(&self, name: &str) -> Result<Box<dyn Record>, Error> {
        let file =
            file_name(name).ok_or_else(|| Error::new(ErrorKind::Usage, "not a record name"))?;
        let lock = Lock::take(&self.dir.join(format!("{file}_lock")))?;
        remove_temporaries(&self.dir, &file);
        let path = self.dir.join(&file);
        let data = match OpenOptions::new().read(true).write(true).open(&path) {
            Ok(data) => Some(data),
            Err(error) if error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(host("cannot open the record", &error)),
        };
        let len = match &data {
            Some(data) => data
                .metadata()
                .map_err(|error| host("cannot read the record's length", &error))?
                .len(),
            None => 0,
        };
        Ok(Box::new(FileRecord {
            inner: Rc::new(RefCell::new(Inner {
                name: name.to_owned(),
                dir: self.dir.clone(),
                file,
                path,
                exists: data.is_some(),
                data,
                len,
                directory_unsynced: false,
                poisoned: false,
                released: false,
                replacement: None,
                replacements: 0,
                lock: Some(lock),
                faults: self.faults.clone(),
            })),
        }))
    }
}

/// A host call the tests can make fail; outside the tests nothing fails
/// on purpose.
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub(crate) enum Step {
    /// The write of a record's `append`; half the bytes are written first.
    Write,
    /// Cutting a record or a replacement back after a failed write.
    Restore,
    /// The cut of a record's `truncate`.
    SetLen,
    /// `sync_all` of a record's file.
    SyncFile,
    /// Making a new name durable: the directory sync after a record's
    /// creation or after a commit's rename.
    SyncDirectory,
    /// The write of a replacement's `append`; half the bytes first.
    ReplacementWrite,
    /// `sync_all` of the replacement's temporary file.
    ReplacementSync,
    /// The rename of a commit.
    Rename,
}

#[derive(Clone, Default)]
struct Faults(#[cfg(test)] Rc<RefCell<Vec<(String, Step)>>>);

impl Faults {
    fn check(&self, name: &str, step: Step) -> io::Result<()> {
        #[cfg(test)]
        {
            let mut faults = self.0.borrow_mut();
            if let Some(index) = faults
                .iter()
                .position(|fault| fault.0 == name && fault.1 == step)
            {
                faults.remove(index);
                return Err(io::Error::other("a planted host failure"));
            }
        }
        let _ = (name, step);
        Ok(())
    }
}

fn host(what: &str, error: &io::Error) -> Error {
    Error::new(ErrorKind::Storage, format!("{what}: {error}"))
}

fn uncertain(what: &str, error: &io::Error) -> Error {
    Error::new(ErrorKind::StorageUncertain, format!("{what}: {error}"))
}

fn usage(message: &str) -> Error {
    Error::new(ErrorKind::Usage, message)
}

fn reopen_first() -> Error {
    usage("the record must be opened again before it is written")
}

/// Whether `len` bytes at `offset` lie within `size`.
pub(crate) fn within(offset: u64, len: usize, size: u64) -> bool {
    u64::try_from(len)
        .ok()
        .and_then(|len| offset.checked_add(len))
        .is_some_and(|end| end <= size)
}

fn read_exact_at(mut file: &File, offset: u64, buf: &mut [u8]) -> io::Result<()> {
    file.seek(SeekFrom::Start(offset))?;
    file.read_exact(buf)
}

/// Write `bytes` at `offset`; when `fail` is an error, write half of them
/// and answer it, as a host that ran out of space would.
fn write_at(mut file: &File, offset: u64, bytes: &[u8], fail: io::Result<()>) -> io::Result<()> {
    file.seek(SeekFrom::Start(offset))?;
    if let Err(error) = fail {
        file.write_all(&bytes[..bytes.len() / 2])?;
        return Err(error);
    }
    file.write_all(bytes)
}

/// An exclusive lock on a lock file, given back when dropped. Windows lets
/// a closed handle's lock linger for a while unless it is unlocked first,
/// so the lock is unlocked explicitly.
struct Lock(File);

impl Lock {
    fn take(path: &Path) -> Result<Self, Error> {
        let file = OpenOptions::new()
            .read(true)
            .write(true)
            .create(true)
            .truncate(false)
            .open(path)
            .map_err(|error| host("cannot open the record's lock", &error))?;
        match file.try_lock() {
            Ok(()) => Ok(Self(file)),
            Err(TryLockError::WouldBlock) => Err(usage(
                "the record is already open, here or in another program",
            )),
            Err(TryLockError::Error(error)) => Err(host("cannot lock the record", &error)),
        }
    }
}

impl Drop for Lock {
    fn drop(&mut self) {
        let _ = self.0.unlock();
    }
}

/// Whether `file` is a temporary file of the record whose file is
/// `record`: `RECORD_N.tmp` with `N` decimal.
fn is_temporary_of(file: &str, record: &str) -> bool {
    file.strip_prefix(record)
        .and_then(|rest| rest.strip_prefix('_'))
        .and_then(|rest| rest.strip_suffix(".tmp"))
        .is_some_and(|number| !number.is_empty() && number.bytes().all(|b| b.is_ascii_digit()))
}

/// Remove what a crash left of the record's replacements. Best effort: a
/// leftover that stays is never read.
fn remove_temporaries(dir: &Path, record: &str) {
    let Ok(entries) = fs::read_dir(dir) else {
        return;
    };
    for entry in entries.flatten() {
        if entry
            .file_name()
            .to_str()
            .is_some_and(|file| is_temporary_of(file, record))
        {
            let _ = fs::remove_file(entry.path());
        }
    }
}

fn parent(path: &Path) -> &Path {
    path.parent()
        .filter(|parent| !parent.as_os_str().is_empty())
        .unwrap_or(Path::new("."))
}

fn create_dir_durably(dir: &Path) -> io::Result<()> {
    let mut missing = Vec::new();
    let mut cursor = Some(dir);
    while let Some(path) = cursor {
        if path.as_os_str().is_empty() || path.exists() {
            break;
        }
        missing.push(path);
        cursor = path.parent();
    }
    fs::create_dir_all(dir)?;
    for path in missing {
        sync_directory(parent(path))?;
    }
    Ok(())
}

/// Make the names in `dir` durable. Windows has no directory sync in the
/// standard library; see [`FileStorage`] for what that means.
#[cfg(unix)]
fn sync_directory(dir: &Path) -> io::Result<()> {
    File::open(dir)?.sync_all()
}

#[cfg(not(unix))]
fn sync_directory(_dir: &Path) -> io::Result<()> {
    Ok(())
}

/// Make a commit's rename durable: the directory on Unix, and on Windows a
/// second sync of the renamed file, which needs write access there.
#[cfg(unix)]
fn sync_rename(dir: &Path, _path: &Path) -> io::Result<()> {
    sync_directory(dir)
}

#[cfg(not(unix))]
fn sync_rename(_dir: &Path, path: &Path) -> io::Result<()> {
    OpenOptions::new().write(true).open(path)?.sync_all()
}

/// What a record object and its replacement share.
struct Inner {
    name: String,
    dir: PathBuf,
    /// The record's host file name.
    file: String,
    path: PathBuf,
    /// Whether the record's file exists.
    exists: bool,
    /// The record's file, opened again on demand after a commit replaced
    /// it.
    data: Option<File>,
    len: u64,
    /// The file was created and its name not yet made durable.
    directory_unsynced: bool,
    /// A write ended with `StorageUncertain`; only reads remain.
    poisoned: bool,
    /// The record object was dropped.
    released: bool,
    /// The living replacement, by its number within this opening.
    replacement: Option<u64>,
    replacements: u64,
    lock: Option<Lock>,
    faults: Faults,
}

impl Inner {
    fn data(&mut self) -> io::Result<&File> {
        let file = match self.data.take() {
            Some(file) => file,
            None => OpenOptions::new().read(true).write(true).open(&self.path)?,
        };
        Ok(self.data.insert(file))
    }

    fn check_writable(&self) -> Result<(), Error> {
        if self.poisoned {
            return Err(reopen_first());
        }
        Ok(())
    }
}

/// A record held through [`FileStorage`].
struct FileRecord {
    inner: Rc<RefCell<Inner>>,
}

impl Record for FileRecord {
    fn size(&self) -> Result<u64, Error> {
        Ok(self.inner.borrow().len)
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), Error> {
        let mut inner = self.inner.borrow_mut();
        if !within(offset, buf.len(), inner.len) {
            return Err(usage("read past the end"));
        }
        if buf.is_empty() {
            return Ok(());
        }
        inner
            .data()
            .and_then(|data| read_exact_at(data, offset, buf))
            .map_err(|error| host("cannot read the record", &error))
    }

    fn append(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let mut inner = self.inner.borrow_mut();
        inner.check_writable()?;
        if bytes.is_empty() {
            return Ok(());
        }
        if !inner.exists {
            let data = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&inner.path)
                .map_err(|error| host("cannot create the record", &error))?;
            inner.data = Some(data);
            inner.exists = true;
            inner.directory_unsynced = true;
        }
        let len = inner.len;
        let fail = inner.faults.check(&inner.name, Step::Write);
        let written = inner
            .data()
            .and_then(|data| write_at(data, len, bytes, fail));
        match written {
            Ok(()) => {
                inner.len = len + bytes.len() as u64;
                Ok(())
            }
            Err(error) => {
                let restored = inner
                    .faults
                    .check(&inner.name, Step::Restore)
                    .and_then(|()| inner.data())
                    .and_then(|data| data.set_len(len));
                match restored {
                    Ok(()) => Err(host("cannot append to the record", &error)),
                    Err(_) => {
                        inner.poisoned = true;
                        Err(uncertain("cannot append to the record nor undo it", &error))
                    }
                }
            }
        }
    }

    fn truncate(&mut self, size: u64) -> Result<(), Error> {
        let mut inner = self.inner.borrow_mut();
        inner.check_writable()?;
        if size > inner.len {
            return Err(usage("a cut never lengthens a record"));
        }
        if size == inner.len {
            return Ok(());
        }
        let len = inner.len;
        let cut = inner
            .faults
            .check(&inner.name, Step::SetLen)
            .and_then(|()| inner.data())
            .and_then(|data| data.set_len(size));
        match cut {
            Ok(()) => {
                inner.len = size;
                Ok(())
            }
            Err(error) => {
                let unchanged = inner
                    .data()
                    .and_then(|data| data.metadata())
                    .is_ok_and(|metadata| metadata.len() == len);
                if unchanged {
                    Err(host("cannot cut the record", &error))
                } else {
                    inner.poisoned = true;
                    Err(uncertain("cannot cut the record", &error))
                }
            }
        }
    }

    fn sync(&mut self) -> Result<(), Error> {
        let mut inner = self.inner.borrow_mut();
        inner.check_writable()?;
        if !inner.exists {
            return Ok(());
        }
        let synced = inner
            .faults
            .check(&inner.name, Step::SyncFile)
            .and_then(|()| inner.data())
            .and_then(File::sync_all);
        if let Err(error) = synced {
            inner.poisoned = true;
            return Err(uncertain("cannot make the record durable", &error));
        }
        if inner.directory_unsynced {
            let synced = inner
                .faults
                .check(&inner.name, Step::SyncDirectory)
                .and_then(|()| sync_directory(&inner.dir));
            if let Err(error) = synced {
                inner.poisoned = true;
                return Err(uncertain("cannot make the record's name durable", &error));
            }
            inner.directory_unsynced = false;
        }
        Ok(())
    }

    fn replace(&mut self) -> Result<Box<dyn Replacement>, Error> {
        let mut inner = self.inner.borrow_mut();
        inner.check_writable()?;
        if inner.replacement.is_some() {
            return Err(usage("the record already has a replacement"));
        }
        let (temporary, file) = create_temporary(&inner.dir, &inner.file)
            .map_err(|error| host("cannot start a replacement", &error))?;
        inner.replacements += 1;
        let number = inner.replacements;
        inner.replacement = Some(number);
        Ok(Box::new(FileReplacement {
            inner: Rc::clone(&self.inner),
            number,
            temporary: Some(temporary),
            file: Some(file),
            len: 0,
        }))
    }
}

impl Drop for FileRecord {
    fn drop(&mut self) {
        if let Ok(mut inner) = self.inner.try_borrow_mut() {
            inner.released = true;
            inner.replacement = None;
            inner.data = None;
            inner.lock = None;
        }
    }
}

/// A new temporary sibling `RECORD_N.tmp`, with the first `N` that is free.
/// A name Windows still holds for a removed file that a released
/// replacement keeps open answers "permission denied", so that is skipped
/// like a name in use.
fn create_temporary(dir: &Path, record: &str) -> io::Result<(PathBuf, File)> {
    let mut last = io::Error::other("no free temporary name");
    for number in 0..64 {
        let path = dir.join(format!("{record}_{number}.tmp"));
        match OpenOptions::new()
            .read(true)
            .write(true)
            .create_new(true)
            .open(&path)
        {
            Ok(file) => return Ok((path, file)),
            Err(error)
                if matches!(
                    error.kind(),
                    io::ErrorKind::AlreadyExists | io::ErrorKind::PermissionDenied
                ) =>
            {
                last = error;
            }
            Err(error) => return Err(error),
        }
    }
    Err(last)
}

/// New contents of a [`FileStorage`] record, in a temporary sibling file.
struct FileReplacement {
    inner: Rc<RefCell<Inner>>,
    number: u64,
    /// The temporary file's path until the commit renames it.
    temporary: Option<PathBuf>,
    file: Option<File>,
    len: u64,
}

impl FileReplacement {
    fn file(&self) -> io::Result<&File> {
        self.file
            .as_ref()
            .ok_or_else(|| io::Error::other("the replacement is closed"))
    }

    fn check(&self, step: Step) -> io::Result<()> {
        let inner = self.inner.borrow();
        inner.faults.check(&inner.name, step)
    }
}

impl Replacement for FileReplacement {
    fn size(&self) -> Result<u64, Error> {
        Ok(self.len)
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), Error> {
        if !within(offset, buf.len(), self.len) {
            return Err(usage("read past the end"));
        }
        if buf.is_empty() {
            return Ok(());
        }
        self.file()
            .and_then(|file| read_exact_at(file, offset, buf))
            .map_err(|error| host("cannot read the replacement", &error))
    }

    fn append(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if bytes.is_empty() {
            return Ok(());
        }
        let fail = self.check(Step::ReplacementWrite);
        match self
            .file()
            .and_then(|file| write_at(file, self.len, bytes, fail))
        {
            Ok(()) => {
                self.len += bytes.len() as u64;
                Ok(())
            }
            Err(error) => {
                let restored = self
                    .check(Step::Restore)
                    .and_then(|()| self.file())
                    .and_then(|file| file.set_len(self.len));
                match restored {
                    Ok(()) => Err(host("cannot append to the replacement", &error)),
                    Err(_) => {
                        // Same as the in-memory storage of the contract's
                        // tests: the record waits to be opened again.
                        let mut inner = self.inner.borrow_mut();
                        if !inner.released && inner.replacement == Some(self.number) {
                            inner.poisoned = true;
                        }
                        Err(uncertain(
                            "cannot append to the replacement nor undo it",
                            &error,
                        ))
                    }
                }
            }
        }
    }

    fn commit(mut self: Box<Self>) -> Result<(), Error> {
        let mut inner = self.inner.borrow_mut();
        if inner.released || inner.replacement != Some(self.number) {
            return Err(usage("the record was released"));
        }
        inner.check_writable()?;
        inner.replacement = None;
        let (Some(temporary), Some(file)) = (self.temporary.take(), self.file.take()) else {
            return Err(usage("the replacement is closed"));
        };
        let synced = inner
            .faults
            .check(&inner.name, Step::ReplacementSync)
            .and_then(|()| file.sync_all());
        // Windows renames a closed file reliably, an open one not always.
        drop(file);
        if let Err(error) = synced {
            let _ = fs::remove_file(&temporary);
            return Err(host("cannot make the new contents durable", &error));
        }
        inner.data = None;
        let renamed = inner
            .faults
            .check(&inner.name, Step::Rename)
            .and_then(|()| fs::rename(&temporary, &inner.path));
        if let Err(error) = renamed {
            let _ = fs::remove_file(&temporary);
            return Err(host("cannot put the new contents in place", &error));
        }
        inner.exists = true;
        inner.len = self.len;
        inner.directory_unsynced = false;
        let durable = inner
            .faults
            .check(&inner.name, Step::SyncDirectory)
            .and_then(|()| sync_rename(&inner.dir, &inner.path));
        if let Err(error) = durable {
            inner.poisoned = true;
            return Err(uncertain(
                "the new contents are in place but may not survive a power cut",
                &error,
            ));
        }
        Ok(())
    }
}

impl Drop for FileReplacement {
    fn drop(&mut self) {
        self.file = None;
        let Ok(mut inner) = self.inner.try_borrow_mut() else {
            return;
        };
        if inner.replacement == Some(self.number) {
            inner.replacement = None;
        }
        // Once the record is released, the next holder may already have
        // removed this file and created its own under the same name; the
        // next opening removes the leftover instead.
        if !inner.released {
            if let Some(temporary) = self.temporary.take() {
                let _ = fs::remove_file(temporary);
            }
        }
    }
}
