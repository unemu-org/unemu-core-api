// SPDX-License-Identifier: MIT OR Apache-2.0
//! The contract's [`Storage`], [`Record`] and [`Replacement`] over a
//! directory of host files or an arbitrary path (core-contract.md §12.5).

#[cfg(unix)]
use crate::host::same_file;
use crate::names::file_name;
use crate::path::{self, stage};
use crate::{FileError, FileIo, Filesystem, HostFilesystem, PathLock};
use std::cell::RefCell;
use std::fs::{self, File, Metadata, OpenOptions, TryLockError};
use std::io::{self, Seek, SeekFrom, Write};
use std::path::{Path, PathBuf};
use std::rc::Rc;
use unemu_core_api::{Error, ErrorKind, Record, Replacement, Storage};

/// A [`Storage`] whose records are the files of one host directory.
///
/// Each record is one file, named after the record ([`crate::file_name`]).
/// A record that does not exist opens empty, and its file appears on the
/// first write: an `append` of at least one byte, or a commit. Names reserved
/// for engine sidecars (see [`FileRecord`]) fail with [`ErrorKind::Usage`].
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
/// Do not use `FileStorage` and [`FileRecord::open`] on the same file:
/// path records lock `FILE.lock`, while this storage locks `FILE_lock`, so
/// the two APIs do not exclude each other.
///
/// The locks are those of the host's own file system (`flock` on Unix,
/// `LockFileEx` on Windows), meant for a directory on a local disk. On a
/// network file system they are only as good as its emulation: an NFS
/// client emulates `flock` with locks that are per process, so two
/// storages in one program may then both open a record. A program that
/// dies gives its locks back; Windows may do so a moment late, so a program
/// started again at once can meet a transient [`ErrorKind::Usage`].
///
/// # Durability
///
/// [`Record::sync`] calls [`File::sync_all`], which on Apple's systems is
/// `F_FULLFSYNC`, so the bytes reach the medium and not only the drive's
/// cache. The first sync of each opening also synchronises the directory
/// on Unix, so the record's name survives a power cut with its bytes, even
/// when an earlier opening created the file and crashed before its sync.
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
    /// (with its missing parents). On Unix each directory it created, and
    /// `dir` itself when it already existed, is made durable in its parent.
    /// The storage keeps the directory's canonical path, so a later change
    /// of the working directory or of a link on the way does not move it.
    ///
    /// Fails with [`ErrorKind::Storage`] when the directory cannot be
    /// created or found.
    pub fn open(dir: impl AsRef<Path>) -> Result<Self, Error> {
        let dir = dir.as_ref();
        create_dir_durably(dir).map_err(|error| host("cannot create the directory", &error))?;
        let dir =
            fs::canonicalize(dir).map_err(|error| host("cannot find the directory", &error))?;
        Ok(Self {
            dir,
            faults: Faults::default(),
        })
    }

    /// The directory of the records, as a canonical path.
    pub fn dir(&self) -> &Path {
        &self.dir
    }

    /// Make the next `step` of the record `name` fail, once.
    #[cfg(test)]
    pub(crate) fn fail_next(&self, name: &str, step: Step) {
        self.faults.0.borrow_mut().push((name.to_owned(), step));
    }
}

// Named records historically create replacements with 0666 filtered by the
// umask. Keep that policy separate from path records' private initial files.
struct StorageFilesystem;
impl Filesystem for StorageFilesystem {
    fn create_new(
        &self,
        path: &Path,
        _permissions: Option<&fs::Permissions>,
    ) -> io::Result<Box<dyn FileIo>> {
        Ok(Box::new(
            OpenOptions::new()
                .read(true)
                .write(true)
                .create_new(true)
                .open(path)?,
        ))
    }
}

impl Storage for FileStorage {
    fn open(&self, name: &str) -> Result<Box<dyn Record>, Error> {
        let file =
            file_name(name).ok_or_else(|| Error::new(ErrorKind::Usage, "not a record name"))?;
        if path::is_reserved_path(Path::new(&file)) {
            return Err(usage("reserved sidecar path"));
        }
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
                data: data.map(|file| Box::new(file) as Box<dyn FileIo>),
                host: Rc::new(StorageFilesystem),
                initial: None,
                protected: None,
                read_only: false,
                len,
                len_unknown: false,
                // An earlier opening may have created the file and crashed
                // before it made the name durable.
                directory_unsynced: true,
                poisoned: false,
                released: false,
                replacement: None,
                replacements: 0,
                lock: Some(Box::new(lock)),
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
    /// After the cut of a record's `truncate` took place: the host reports
    /// a failure anyway.
    SetLenAfter,
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

/// Fill `buf` from `offset` without the file's shared cursor, so that
/// several threads may read one file at once.
#[cfg(unix)]
pub(crate) fn read_exact_at(file: &File, offset: u64, buf: &mut [u8]) -> io::Result<()> {
    std::os::unix::fs::FileExt::read_exact_at(file, buf, offset)
}

/// Fill `buf` from `offset`. Windows has no positional read that leaves
/// the cursor alone, but a read at an explicit offset does not depend on
/// it either, so concurrent reads do not mix.
#[cfg(windows)]
pub(crate) fn read_exact_at(file: &File, mut offset: u64, mut buf: &mut [u8]) -> io::Result<()> {
    use std::os::windows::fs::FileExt;
    while !buf.is_empty() {
        match file.seek_read(buf, offset) {
            Ok(0) => return Err(io::ErrorKind::UnexpectedEof.into()),
            Ok(read) => {
                buf = &mut std::mem::take(&mut buf)[read..];
                offset += read as u64;
            }
            Err(error) if error.kind() == io::ErrorKind::Interrupted => {}
            Err(error) => return Err(error),
        }
    }
    Ok(())
}

/// Write `bytes` at `offset`; when `fail` is an error, write half of them
/// and answer it, as a host that ran out of space would.
fn write_at(
    file: &mut dyn FileIo,
    offset: u64,
    bytes: &[u8],
    fail: io::Result<()>,
) -> io::Result<()> {
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

/// Create `dir` and its missing parents, and make the name of each
/// directory it created durable; when `dir` already existed, make its own
/// name durable, as its creator may have crashed before doing so.
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
    if missing.is_empty() {
        missing.push(dir);
    }
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

/// What a record object and its replacement share.
struct Inner {
    name: String,
    dir: PathBuf,
    /// The record's host file name.
    file: String,
    path: PathBuf,
    /// Whether the record's file exists.
    exists: bool,
    /// The current descriptor, retained across successful publication.
    /// An uncertain publication may require reopening the path on demand.
    data: Option<Box<dyn FileIo>>,
    host: Rc<dyn Filesystem>,
    initial: Option<Vec<u8>>,
    protected: Option<Metadata>,
    read_only: bool,
    len: u64,
    len_unknown: bool,
    /// The file's name may not be durable yet: true from the opening until
    /// the first sync.
    directory_unsynced: bool,
    /// A write ended with `StorageUncertain`; only reads remain.
    poisoned: bool,
    /// The record object was dropped.
    released: bool,
    /// The living replacement, by its number within this opening.
    replacement: Option<u64>,
    replacements: u64,
    lock: Option<Box<dyn std::any::Any>>,
    faults: Faults,
}

impl Inner {
    fn data(&mut self) -> io::Result<&mut dyn FileIo> {
        if self.data.is_none() {
            self.data = Some(self.host.open(&self.path, !self.read_only)?);
        }
        match self.data.as_mut() {
            Some(file) => Ok(file.as_mut()),
            None => Err(io::Error::other("record is not open")),
        }
    }

    fn check_writable(&self) -> Result<(), Error> {
        if self.read_only {
            return Err(usage("an inspection record is read-only"));
        }
        if self.poisoned {
            return Err(reopen_first());
        }
        Ok(())
    }
}

/// A contract record over an arbitrary host path, or a named FileStorage
/// record. Both use the same append, rollback, truncation, poisoning and
/// replacement implementation.
///
/// Path records hold a stable `FILE.lock`. A missing record reads empty;
/// append is refused until a replacement commits exactly `initial` bytes.
/// Creation writes `FILE.create-N.tmp`, flushes and syncs it, publishes with
/// a no-clobber hard link, removes its temporary link, checks the canonical
/// path, cleans orphans and syncs the directory. A replacement of an existing
/// file uses `FILE.retain-N.tmp`, preserves its exact permissions, syncs,
/// atomically renames and syncs the directory. Numbers start at zero and skip
/// occupied names. No journal format is interpreted.
///
/// Record paths must not name engine sidecars: `.lock`, `_lock`,
/// `.create-`/`.retain-` names and `_N.tmp` replacements are reserved, also
/// through symbolic links. Opening one fails with Usage. Do not use these
/// path methods on a file also accessed through FileStorage: its `FILE_lock`
/// and this API's `FILE.lock` do not exclude each other.
///
/// Opening an existing path under an exclusive lock verifies its canonical
/// path, cleans leftovers, then syncs the directory. Creation leftovers are
/// removed only when their bytes are a prefix of `initial`; retention
/// leftovers may contain arbitrary bytes. Cleanup preserves symlinks, special
/// files, changed identities and aliases of `protected`. The directory must
/// be trusted (see Filesystem). A protected file is also refused as a writable
/// record. On hosts without inode identity, cleanup preserves all candidates
/// and protected writable opens are refused.
///
/// Failures follow FileStorage and Record: an append is rolled back when
/// possible; an unconfirmed cut or barrier poisons writes. A rename failure
/// is conservatively StorageUncertain, because it may have taken place.
/// Reads then reopen the path and see the old or new whole file. Errors name
/// the failed stage; writes after uncertainty fail with Usage until reopen.
/// Replacements cannot commit after their record is dropped and cannot
/// remove another opening's temporary file. Inspection records perform no
/// cleanup or barrier and refuse every mutation, including empty ones.
pub struct FileRecord {
    inner: Rc<RefCell<Inner>>,
}

impl Record for FileRecord {
    fn size(&self) -> Result<u64, Error> {
        let mut inner = self.inner.borrow_mut();
        if inner.len_unknown {
            let metadata = inner
                .data()
                .and_then(|file| file.metadata())
                .map_err(|e| host("metadata after uncertain publication", &e))?;
            inner.len = metadata.len();
            inner.len_unknown = false;
        }
        Ok(inner.len)
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), Error> {
        let len = self.size()?;
        let mut inner = self.inner.borrow_mut();
        if !within(offset, buf.len(), len) {
            return Err(usage("read past the end"));
        }
        if buf.is_empty() {
            return Ok(());
        }
        inner
            .data()
            .and_then(|data| {
                data.seek(SeekFrom::Start(offset))?;
                data.read_exact(buf)
            })
            .map_err(|error| host("cannot read the record", &error))
    }

    fn append(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let mut inner = self.inner.borrow_mut();
        inner.check_writable()?;
        if !inner.exists && inner.initial.is_some() {
            return Err(usage("a path record is created by a replacement"));
        }
        if bytes.is_empty() {
            return Ok(());
        }
        if inner.exists {
            inner
                .data()
                .map_err(|error| host("cannot open the record", &error))?;
        } else {
            let data = OpenOptions::new()
                .read(true)
                .write(true)
                .create(true)
                .truncate(false)
                .open(&inner.path)
                .map_err(|error| host("cannot create the record", &error))?;
            inner.data = Some(Box::new(data));
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
        inner
            .data()
            .map_err(|error| host("cannot open the record", &error))?;
        let len = inner.len;
        let fail_after = inner.faults.check(&inner.name, Step::SetLenAfter);
        let cut = inner
            .faults
            .check(&inner.name, Step::SetLen)
            .and_then(|()| inner.data())
            .and_then(|data| data.set_len(size))
            .and(fail_after);
        match cut {
            Ok(()) => {
                inner.len = size;
                Ok(())
            }
            Err(error) => {
                let actual = inner
                    .data()
                    .and_then(|data| data.metadata())
                    .map(|metadata| metadata.len());
                if matches!(actual, Ok(actual) if actual == len) {
                    return Err(host("cannot cut the record", &error));
                }
                // Reads go on over what the file holds now, when known.
                if let Ok(actual) = actual {
                    inner.len = actual;
                }
                inner.poisoned = true;
                Err(uncertain("cannot cut the record", &error))
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
            .and_then(|file| {
                file.flush()?;
                file.sync_all()
            });
        if let Err(error) = synced {
            inner.poisoned = true;
            return Err(uncertain("cannot make the record durable", &error));
        }
        if inner.directory_unsynced {
            let synced = inner
                .faults
                .check(&inner.name, Step::SyncDirectory)
                .and_then(|()| inner.host.sync_directory(&inner.path));
            if let Err(error) = synced {
                inner.poisoned = true;
                return Err(uncertain("cannot make the record's name durable", &error));
            }
            inner.directory_unsynced = false;
        }
        Ok(())
    }

    fn replace(&mut self) -> Result<Box<dyn Replacement>, Error> {
        self.start_replacement(None)
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

enum ReplacementHost<'a> {
    Owned(Rc<dyn Filesystem>),
    Borrowed(&'a dyn Filesystem),
}
impl ReplacementHost<'_> {
    fn get(&self) -> &dyn Filesystem {
        match self {
            Self::Owned(fs) => fs.as_ref(),
            Self::Borrowed(fs) => *fs,
        }
    }
}

/// New contents of a [`FileStorage`] record, in a temporary sibling file.
struct FileReplacement<'a> {
    inner: Rc<RefCell<Inner>>,
    number: u64,
    /// The temporary file's path until the commit renames it.
    temporary: Option<PathBuf>,
    file: Option<Box<dyn FileIo>>,
    host: ReplacementHost<'a>,
    creation: bool,
    broken: bool,
    len: u64,
}

impl FileReplacement<'_> {
    fn file(&mut self) -> io::Result<&mut (dyn FileIo + 'static)> {
        self.file
            .as_deref_mut()
            .ok_or_else(|| io::Error::other("the replacement is closed"))
    }

    fn check(&self, step: Step) -> io::Result<()> {
        let inner = self.inner.borrow();
        inner.faults.check(&inner.name, step)
    }
}

impl Replacement for FileReplacement<'_> {
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
            .and_then(|file| {
                file.seek(SeekFrom::Start(offset))?;
                file.read_exact(buf)
            })
            .map_err(|error| host("cannot read the replacement", &error))
    }

    fn append(&mut self, bytes: &[u8]) -> Result<(), Error> {
        if self.broken {
            return Err(usage("the replacement must be dropped"));
        }
        if bytes.is_empty() {
            return Ok(());
        }
        let len = self.len;
        let fail = self.check(Step::ReplacementWrite);
        match self
            .file()
            .and_then(|file| write_at(file, len, bytes, fail))
        {
            Ok(()) => {
                self.len += bytes.len() as u64;
                Ok(())
            }
            Err(error) => {
                let restored = self
                    .check(Step::Restore)
                    .and_then(|()| self.file())
                    .and_then(|file| file.set_len(len));
                match restored {
                    Ok(()) => Err(host("cannot append to the replacement", &error)),
                    Err(_) => {
                        // Same as the in-memory storage of the contract's
                        // tests: the record waits to be opened again.
                        self.broken = true;
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
        if self.broken {
            return Err(usage("the replacement must be dropped"));
        }
        inner.replacement = None;
        let (Some(temporary), Some(mut file)) = (self.temporary.take(), self.file.take()) else {
            return Err(usage("the replacement is closed"));
        };
        let fs = self.host.get();
        let result = (|| {
            if self.creation {
                let initial = inner.initial.as_deref().unwrap_or_default();
                if self.len != initial.len() as u64 {
                    return Err(usage("initial bytes differ"));
                }
                let mut bytes = vec![0; initial.len()];
                file.seek(SeekFrom::Start(0))
                    .and_then(|_| file.read_exact(&mut bytes))
                    .map_err(|e| host("initial read", &e))?;
                if bytes != initial {
                    return Err(usage("initial bytes differ"));
                }
            } else if inner.initial.is_some() && inner.exists {
                // Apply permissions after writes too: Unix may clear set-id bits
                // when writing a file even after an earlier chmod.
                let permissions = inner
                    .data()
                    .and_then(|file| file.metadata())
                    .map_err(|e| host("replacement metadata", &e))?
                    .permissions();
                file.set_permissions(permissions.clone())
                    .map_err(|e| host("replacement permissions", &e))?;
                let actual = file
                    .metadata()
                    .map_err(|e| host("replacement metadata", &e))?
                    .permissions();
                if actual != permissions {
                    return Err(host(
                        "replacement permissions",
                        &io::Error::other("host did not preserve exact permissions"),
                    ));
                }
            }
            inner
                .faults
                .check(&inner.name, Step::ReplacementSync)
                .and_then(|()| file.flush())
                .and_then(|()| file.sync_all())
                .map_err(|e| {
                    host(
                        if self.creation {
                            "initial sync"
                        } else {
                            "replacement sync"
                        },
                        &e,
                    )
                })?;
            inner.data = None;
            if self.creation {
                if let Err(error) = fs.hard_link(&temporary, &inner.path) {
                    if error.kind() == io::ErrorKind::AlreadyExists {
                        return Err(host("publish initial", &error));
                    }
                    // A failed host call can still have published the link.
                    // Only observed absence proves that nothing is in place.
                    match fs.symlink_metadata(&inner.path) {
                        Err(missing) if missing.kind() == io::ErrorKind::NotFound => {
                            return Err(host("publish initial", &error));
                        }
                        Ok(metadata) => {
                            inner.exists = true;
                            inner.len = metadata.len();
                        }
                        Err(_) => {
                            inner.exists = true;
                            inner.len_unknown = true;
                        }
                    }
                    inner.poisoned = true;
                    return Err(uncertain("publish initial", &error));
                }
            } else {
                inner
                    .faults
                    .check(&inner.name, Step::Rename)
                    .map_err(|e| host("replacement rename", &e))?;
                if let Err(error) = fs.rename(&temporary, &inner.path) {
                    inner.poisoned = true;
                    inner.len_unknown = true;
                    if let Ok(data) = fs.open(&inner.path, true) {
                        if let Ok(metadata) = data.metadata() {
                            inner.len = metadata.len();
                            inner.len_unknown = false;
                        }
                        inner.data = Some(data);
                    }
                    return Err(uncertain("replacement rename", &error));
                }
            }
            // Creation grants write access even when the final permissions
            // forbid reopening. Keep that access after either publication.
            inner.data = Some(file);
            inner.exists = true;
            inner.len = self.len;
            inner.directory_unsynced = true;
            let settle = (|| {
                if self.creation {
                    fs.remove_file(&temporary)
                        .map_err(|e| ("unlink initial", e))?;
                    path::verify(fs, &inner.path).map_err(|e| ("verify path", e))?;
                    path::cleanup(
                        fs,
                        &inner.path,
                        inner.initial.as_deref().unwrap_or_default(),
                        inner.protected.as_ref(),
                    )
                    .map_err(|e| ("cleanup", e))?;
                }
                inner
                    .faults
                    .check(&inner.name, Step::SyncDirectory)
                    .and_then(|()| fs.sync_directory(&inner.path))
                    .map_err(|e| ("directory sync", e))
            })();
            if let Err((step, error)) = settle {
                inner.poisoned = true;
                return Err(uncertain(step, &error));
            }
            inner.directory_unsynced = false;
            Ok(())
        })();
        let _ = fs.remove_file(&temporary);
        result
    }
}

impl Drop for FileReplacement<'_> {
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
                let _ = self.host.get().remove_file(&temporary);
            }
        }
    }
}

impl FileRecord {
    /// Open a path for writing with the host filesystem. `initial` is the
    /// exact first publication, not a format validator for existing bytes.
    /// `protected` identifies a read-only input which must not be opened for
    /// writing or removed through a temporary-name alias. No parent directory
    /// is created. Empty existing files remain valid generic records.
    pub fn open(
        path: impl AsRef<Path>,
        initial: &[u8],
        protected: Option<&Metadata>,
    ) -> Result<Self, FileError> {
        Self::open_with(
            Rc::new(HostFilesystem),
            path.as_ref(),
            None,
            initial,
            protected,
        )
    }

    /// Open for shared read-only inspection, with the absent-sidecar fallback
    /// specified by PathLock::inspection. Missing files fail with Storage.
    pub fn inspect(path: impl AsRef<Path>) -> Result<Self, FileError> {
        let fs = Rc::new(HostFilesystem);
        let lock = PathLock::acquire(fs.as_ref(), path.as_ref(), true)?;
        Self::open_with(fs, path.as_ref(), Some(lock), &[], None)
    }

    /// Open through a retained injectable filesystem and an optional lock
    /// already acquired by the caller. A supplied token must name the same
    /// canonical path; a shared token selects inspection. No token means an
    /// exclusive lock is taken. A no-lock inspection token never grants writes.
    /// The record owns the token until drop, even while replacements live.
    pub fn open_with(
        fs: Rc<dyn Filesystem>,
        path: &Path,
        lock: Option<PathLock>,
        initial: &[u8],
        protected: Option<&Metadata>,
    ) -> Result<Self, FileError> {
        let resolved = path::resolve(fs.as_ref(), path)
            .map_err(|e| stage(crate::FileStage::ResolvePath, e))?;
        let lock = match lock {
            Some(lock) if lock.path == resolved => lock,
            Some(_) => {
                return Err(stage(
                    crate::FileStage::Lock,
                    io::Error::new(io::ErrorKind::InvalidInput, "lock names another path"),
                ))
            }
            None => PathLock::acquire(fs.as_ref(), &resolved, false)?,
        };
        let read_only = lock.shared;
        let data = match fs.open(&resolved, !read_only) {
            Ok(data) => Some(data),
            Err(error) if !read_only && error.kind() == io::ErrorKind::NotFound => None,
            Err(error) => return Err(stage(crate::FileStage::Open, error)),
        };
        let metadata = data
            .as_ref()
            .map(|file| file.metadata())
            .transpose()
            .map_err(|e| stage(crate::FileStage::Metadata, e))?;
        if !read_only {
            if let Some(_protected) = protected {
                #[cfg(not(unix))]
                return Err(stage(
                    crate::FileStage::Metadata,
                    io::Error::new(
                        io::ErrorKind::Unsupported,
                        "protected identity is unavailable on this host",
                    ),
                ));
                #[cfg(unix)]
                if metadata
                    .as_ref()
                    .is_some_and(|meta| same_file(meta, _protected))
                {
                    return Err(stage(
                        crate::FileStage::Metadata,
                        io::Error::new(
                            io::ErrorKind::InvalidInput,
                            "record aliases the protected file",
                        ),
                    ));
                }
            }
        }
        if data.is_some() {
            path::verify(fs.as_ref(), &resolved)
                .map_err(|e| stage(crate::FileStage::VerifyPath, e))?;
            if !read_only {
                path::cleanup(fs.as_ref(), &resolved, initial, protected)
                    .map_err(|e| stage(crate::FileStage::Cleanup, e))?;
                fs.sync_directory(&resolved)
                    .map_err(|e| stage(crate::FileStage::DirectorySync, e))?;
            }
        }
        Ok(Self {
            inner: Rc::new(RefCell::new(Inner {
                name: String::new(),
                dir: crate::host::parent(&resolved).to_owned(),
                file: String::new(),
                path: resolved,
                exists: data.is_some(),
                len: metadata.map_or(0, |meta| meta.len()),
                len_unknown: false,
                data,
                host: fs,
                initial: Some(initial.to_vec()),
                protected: protected.cloned(),
                read_only,
                directory_unsynced: false,
                poisoned: false,
                released: false,
                replacement: None,
                replacements: 0,
                lock: Some(Box::new(lock)),
                faults: Faults::default(),
            })),
        })
    }

    /// Whether a publication exists. A missing path remains absent until its
    /// first replacement commits; an uncertain publication may already exist.
    pub fn exists(&self) -> bool {
        self.inner.borrow().exists
    }

    /// Start a replacement using a borrowed host seam, including temporary
    /// creation, writes through FileIo, permissions, barriers, rename/link and
    /// directory sync. This supports offline tools whose injected operations
    /// live for one retention only. The replacement cannot outlive `fs`.
    pub fn replace_with<'a>(
        &mut self,
        fs: &'a dyn Filesystem,
    ) -> Result<Box<dyn Replacement + 'a>, Error> {
        self.start_replacement(Some(fs))
    }

    fn start_replacement<'a>(
        &mut self,
        fs: Option<&'a dyn Filesystem>,
    ) -> Result<Box<dyn Replacement + 'a>, Error> {
        let mut inner = self.inner.borrow_mut();
        inner.check_writable()?;
        if inner.replacement.is_some() {
            return Err(usage("the record already has a replacement"));
        }
        let host = match fs {
            Some(fs) => ReplacementHost::Borrowed(fs),
            None => ReplacementHost::Owned(Rc::clone(&inner.host)),
        };
        let creation = inner.initial.is_some() && !inner.exists;
        let permissions = if inner.initial.is_some() && inner.exists {
            Some(
                inner
                    .data()
                    .and_then(|file| file.metadata())
                    .map_err(|e| crate::storage::host("replacement metadata", &e))?
                    .permissions(),
            )
        } else {
            None
        };
        let mut chosen = None;
        for number in 0..if inner.initial.is_some() {
            u64::MAX
        } else {
            64
        } {
            let temporary = if inner.initial.is_some() {
                path::suffix(
                    &inner.path,
                    &format!(
                        ".{}-{number}.tmp",
                        if creation { "create" } else { "retain" }
                    ),
                )
            } else {
                inner.dir.join(format!("{}_{number}.tmp", inner.file))
            };
            match host.get().create_new(&temporary, permissions.as_ref()) {
                Ok(mut file) => {
                    if let Some(permissions) = permissions.clone() {
                        if let Err(error) = file.set_permissions(permissions) {
                            drop(file);
                            let _ = host.get().remove_file(&temporary);
                            return Err(crate::storage::host("replacement permissions", &error));
                        }
                    }
                    chosen = Some((temporary, file));
                    break;
                }
                Err(error)
                    if error.kind() == io::ErrorKind::AlreadyExists
                        || (inner.initial.is_none()
                            && error.kind() == io::ErrorKind::PermissionDenied) => {}
                Err(error) => return Err(crate::storage::host("create temporary", &error)),
            }
        }
        let (temporary, file) = chosen.ok_or_else(|| {
            crate::storage::host(
                "create temporary",
                &io::Error::other("no free temporary name"),
            )
        })?;
        inner.replacements += 1;
        let number = inner.replacements;
        inner.replacement = Some(number);
        Ok(Box::new(FileReplacement {
            inner: Rc::clone(&self.inner),
            number,
            temporary: Some(temporary),
            file: Some(file),
            host,
            creation,
            broken: false,
            len: 0,
        }))
    }
}
