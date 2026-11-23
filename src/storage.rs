// SPDX-License-Identifier: MIT OR Apache-2.0
//! Storage handed in as objects (core-contract.md §12.5).
//!
//! A core opens no file, directory or path. What it reads comes as
//! [`ContentReader`]s the front end verified in full, and what must outlive a
//! run goes to [`Record`]s of a [`Storage`] the front end owns. Behind the
//! objects may be host files, a database, a browser's storage or memory; a
//! core cannot tell.
//!
//! Layered disk images stay possible: a base is read-only content named by
//! its digest, written sectors go to a record that binds the base by digest
//! and length, and nothing here writes a base. A chain of hash-linked layers
//! can stand behind one `ContentReader` without a change to these traits.

use crate::Error;

/// Verified read-only content a core reads at any offset: a disk base, a
/// cartridge, a large ROM set.
///
/// The object stands for exactly the bytes whose SHA-256 and length the
/// front end verified in full before it handed the object over. What is
/// behind it is the front end's: a file, a buffer in memory, a
/// decompressing reader, a chain of hash-linked disk layers. A core cannot
/// tell, never writes through it and never hashes it again.
pub trait ContentReader {
    /// The length in bytes. Constant for the object's life.
    fn size(&self) -> u64;

    /// Fill `buf` with the bytes at `offset`. Reading past `size()` fails
    /// with [`crate::ErrorKind::Usage`]. Fails with
    /// [`crate::ErrorKind::Content`] when it detects that the bytes are no
    /// longer the verified ones, such as when the file behind the object
    /// changed; a core must not rely on detection. Fails with
    /// [`crate::ErrorKind::Storage`] when the host read failed. The guest
    /// sees neither: the core reports the failure from the call doing the I/O
    /// (for example [`crate::Machine::run`] or [`crate::Machine::change_medium`]).
    /// On any failure, `buf` may be partially filled; callers must not consume
    /// it as valid content.
    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), Error>;
}

/// Where a machine keeps what must outlive a run: written sectors,
/// non-volatile memory, battery-backed cartridge memory.
///
/// The front end owns the storage and decides what a record is: a file in
/// the machine's session directory, a row of a database, a key of a
/// browser's storage, a buffer in memory. The core owns the format of a
/// record's bytes. A core opens no file, directory or path.
///
/// A machine and its storage live on one thread.
///
/// # Battery-backed memory
///
/// A cartridge's battery-backed memory is one record, under a name the core
/// fixes. A core holds these rules (core-contract.md §13.2):
///
/// - The record is opened only when the content has such memory.
/// - It is written with [`Record::replace`] and [`Replacement::commit`], so
///   that it holds the old memory or the new one, whole.
/// - It is written at [`crate::Machine::finish`], and also when
///   [`crate::Machine::run`] returns with the memory changed and ten seconds
///   of virtual time gone since the last write, so that a power cut loses
///   little. The host clock is not involved, so a replay writes at the same
///   points.
/// - A machine dropped without `finish` keeps what the last write made
///   durable.
pub trait Storage {
    /// Open the record `name` for this machine's exclusive use, until the
    /// returned object is dropped. A record that does not exist yet opens
    /// empty.
    ///
    /// A name is 1 to 64 bytes of `a-z`, `0-9`, `-` and `.`, starting and
    /// ending with a letter or a digit ([`is_record_name`]). Names that
    /// start with [`KEPT_RECORD_PREFIX`] are the front end's
    /// ([`crate::WriteTarget::Kept`]); a core's own records never start with
    /// it. The core chooses the names of what its config describes, and the
    /// same config gives the same names in every release of the core,
    /// because a name is how a later run finds what an earlier one kept. A
    /// name a host file system treats specially, such as the device names
    /// `con`, `nul` or `com1` of Windows, is the file-backed
    /// implementation's to map; a core need not avoid it. Every name accepted
    /// by `is_record_name` must be usable on every storage; backend sidecar
    /// names impose no additional restriction. Fails with
    /// [`crate::ErrorKind::Usage`] when the name breaks these rules or the
    /// record is already open, here or in another program, and with
    /// [`crate::ErrorKind::Storage`] when the host failed.
    fn open(&self, name: &str) -> Result<Box<dyn Record>, Error>;
}

/// Whether `name` follows the rules of a record name: 1 to 64 bytes of
/// `a-z`, `0-9`, `-` and `.`, the first and the last a letter or a digit.
/// The last byte is not a dot or a dash because some host file systems
/// drop a trailing dot, which would make two names one.
///
/// A storage refuses any other name. The records of
/// [`crate::WriteTarget::Kept`] follow [`is_kept_record_name`] as well.
pub const fn is_record_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 {
        return false;
    }
    let first = bytes[0];
    let last = bytes[bytes.len() - 1];
    if !(first.is_ascii_lowercase() || first.is_ascii_digit()) {
        return false;
    }
    if !(last.is_ascii_lowercase() || last.is_ascii_digit()) {
        return false;
    }
    let mut index = 1;
    while index < bytes.len() {
        let byte = bytes[index];
        if !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-' || byte == b'.') {
            return false;
        }
        index += 1;
    }
    true
}

/// The prefix of every record name the front end chooses for
/// [`crate::WriteTarget::Kept`]. A core's own records never start with it,
/// so the two never collide.
pub const KEPT_RECORD_PREFIX: &str = "kept-";

/// Whether `name` may name the record of a [`crate::WriteTarget::Kept`]: a
/// record name ([`is_record_name`]) that starts with
/// [`KEPT_RECORD_PREFIX`].
pub const fn is_kept_record_name(name: &str) -> bool {
    let bytes = name.as_bytes();
    let prefix = KEPT_RECORD_PREFIX.as_bytes();
    if !is_record_name(name) || bytes.len() <= prefix.len() {
        return false;
    }
    let mut index = 0;
    while index < prefix.len() {
        if bytes[index] != prefix[index] {
            return false;
        }
        index += 1;
    }
    true
}

/// One persistent sequence of bytes, held exclusively.
///
/// Reads and `size` describe the currently visible bytes. Successful `append`
/// and `truncate` change visibility immediately; their durability is confirmed
/// only by a successful `sync`, as far as the host can confirm it. Closing and
/// reopening is not a durability barrier and does not undo visible changes.
/// A power failure can lose unsynchronized changes or leave a partial append.
/// Confirmed bytes remain durable unless a later destructive operation removes
/// them: `truncate` can discard even previously synchronized bytes before its
/// own `sync`. Use [`Replacement`] when the old or new record must stay whole.
///
/// A [`crate::ErrorKind::Storage`] failure of `append` or `truncate` leaves
/// the visible record as it was. An implementation that cannot undo a partial
/// write answers [`crate::ErrorKind::StorageUncertain`]. A failed `sync` is
/// `StorageUncertain` and does not confirm any new durability. After
/// `StorageUncertain` from any call, reads remain available (and may themselves
/// fail), while every write (`append`, `truncate`, `sync`, `replace`, and a
/// pending replacement's `commit`) fails with [`crate::ErrorKind::Usage`]
/// until the record is opened again. Reopening clears that prohibition; it
/// does not roll back to the last successful sync. A failed append may leave
/// a partial tail; recovery belongs to the core's record format. An uncertain
/// replacement publication instead leaves the old or new contents, whole.
/// Failure of commit's preliminary parent sync follows the failed-sync rules.
///
/// Dropping the object releases the record without a durability barrier.
pub trait Record {
    /// The length in bytes now.
    fn size(&self) -> Result<u64, Error>;

    /// Fill `buf` with the bytes at `offset`. Reading past the end fails
    /// with [`crate::ErrorKind::Usage`]. On any failure, `buf` may be partially
    /// filled; callers must not consume it as valid content.
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), Error>;

    /// Add `bytes` at the end. Durability is confirmed only after `sync`.
    fn append(&mut self, bytes: &[u8]) -> Result<(), Error>;

    /// Cut the record to its first `size` bytes. Durability is confirmed only
    /// after `sync`; the cut can discard synchronized bytes even before that.
    /// A `size` beyond the end fails with [`crate::ErrorKind::Usage`]: a cut
    /// never lengthens a record.
    fn truncate(&mut self, size: u64) -> Result<(), Error>;

    /// Make everything appended and cut so far durable. When this returns
    /// `Ok`, the host has confirmed the bytes as far as it confirms
    /// anything. A host that cannot confirm fails; it never pretends. A
    /// failure is always [`crate::ErrorKind::StorageUncertain`].
    fn sync(&mut self) -> Result<(), Error>;

    /// Start new contents for this record beside the old ones. The record
    /// keeps its old bytes, readable and appendable, until the replacement
    /// is committed. One replacement exists at a time: a second one while
    /// the first lives fails with [`crate::ErrorKind::Usage`].
    fn replace(&mut self) -> Result<Box<dyn Replacement>, Error>;
}

/// New contents of a record, while they are written.
pub trait Replacement {
    /// The length of the new contents now.
    fn size(&self) -> Result<u64, Error>;

    /// Fill `buf` with the new contents at `offset`, to check them before
    /// they are committed. Reading past the end fails with
    /// [`crate::ErrorKind::Usage`]. On any failure, `buf` may be partially
    /// filled; callers must not consume it as valid content.
    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), Error>;

    /// Add `bytes` at the end of the new contents.
    /// A [`crate::ErrorKind::Storage`] failure restores the replacement's
    /// previous bytes and leaves it appendable and committable. If rollback
    /// fails, return [`crate::ErrorKind::StorageUncertain`]: the replacement
    /// must be dropped, and further append (including empty append) and commit
    /// return [`crate::ErrorKind::Usage`]. The parent's bytes remain unchanged,
    /// but its writes and pending commits are prohibited until reopen, just
    /// as for any uncertain record write. Reads remain available. Reopening
    /// the parent does not make this broken replacement usable.
    fn append(&mut self, bytes: &[u8]) -> Result<(), Error>;

    /// First synchronize the parent's current contents, including mutations
    /// made while this replacement was being built. If that barrier fails,
    /// return [`crate::ErrorKind::StorageUncertain`] without publication;
    /// the usual failed-sync rules apply, with no new durability confirmed.
    /// Once it succeeds, make the new contents durable and put them in place
    /// in one step: after a crash the record holds the previous complete
    /// contents or the new ones, never a mixture. On `Ok` the record this
    /// came from reads and appends to the new contents.
    ///
    /// After the parent barrier, a failure before anything was put in place is
    /// [`crate::ErrorKind::Storage`] and leaves the record as it was. When
    /// the host cannot say which contents the record holds now, the failure
    /// is [`crate::ErrorKind::StorageUncertain`], and the record must be
    /// opened again before it is written; opened again, it holds the old or
    /// the new contents, whole. A replacement whose record was released (its
    /// object dropped) fails with [`crate::ErrorKind::Usage`] and changes
    /// nothing. Dropping a replacement without `commit` discards it.
    fn commit(self: Box<Self>) -> Result<(), Error>;
}
