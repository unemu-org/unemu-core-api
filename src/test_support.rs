// SPDX-License-Identifier: MIT OR Apache-2.0
//! A scripted storage and content in memory, for the tests of cores and
//! front ends (core-contract.md §12.5). Behind the feature `test-support`.
//!
//! [`MemoryStorage`] keeps two copies of every record: the bytes as they are
//! now, and the bytes that are durable, which a [`MemoryStorage::power_cut`]
//! falls back to. Faults are planted one at a time with
//! [`MemoryStorage::fail_next`], so a test can drive a core through a failed
//! write, a failed sync and both kinds of failed commit. [`MemoryContent`]
//! fails a read on request and can stop being the verified bytes.

//! [`ScriptedMachine`] models machine calls, statuses, media and saved state.

use crate::{is_record_name, ContentReader, Error, ErrorKind, Record, Replacement, Storage};
use std::cell::{Cell, RefCell};
use std::collections::BTreeMap;
use std::rc::Rc;

/// An operation a fault can be planted on.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Operation {
    /// [`Storage::open`].
    Open,
    /// [`Record::read_at`].
    Read,
    /// [`Record::append`].
    Append,
    /// [`Record::truncate`].
    Truncate,
    /// [`Record::sync`].
    Sync,
    /// [`Record::replace`].
    Replace,
    /// [`Replacement::append`].
    ReplacementAppend,
    /// [`Replacement::commit`].
    Commit,
}

/// A [`Storage`] in memory that models durability and failures.
///
/// A clone is the same storage: a test keeps one to look inside while the
/// machine holds another as `Rc<dyn Storage>`.
#[derive(Clone, Default)]
pub struct MemoryStorage {
    state: Rc<RefCell<State>>,
}

#[derive(Default)]
struct State {
    records: BTreeMap<String, Entry>,
    faults: Vec<Fault>,
    opened: u64,
}

#[derive(Default)]
struct Entry {
    /// What a power cut leaves.
    durable: Vec<u8>,
    /// What a reader sees now.
    current: Vec<u8>,
    /// The open record object, by the number of its opening.
    holder: Option<u64>,
    /// The living replacement, by its number within the record.
    replacement: Option<u64>,
    replacements: u64,
    /// A write failed with an uncertain outcome; only reads remain until the
    /// record is opened again.
    poisoned: bool,
}

struct Fault {
    record: String,
    operation: Operation,
    error: Error,
    takes_effect: bool,
}

fn take_fault(faults: &mut Vec<Fault>, record: &str, operation: Operation) -> Option<Fault> {
    let index = faults
        .iter()
        .position(|fault| fault.record == record && fault.operation == operation)?;
    Some(faults.remove(index))
}

fn usage(message: &str) -> Error {
    Error::new(ErrorKind::Usage, message)
}

fn read_into(bytes: &[u8], offset: u64, buf: &mut [u8]) -> Result<(), Error> {
    let start = usize::try_from(offset).ok();
    let end = start.and_then(|start| start.checked_add(buf.len()));
    match (start, end) {
        (Some(start), Some(end)) if end <= bytes.len() => {
            buf.copy_from_slice(&bytes[start..end]);
            Ok(())
        }
        _ => Err(usage("read past the end")),
    }
}

impl MemoryStorage {
    /// A storage without records.
    pub fn new() -> Self {
        Self::default()
    }

    /// Put a durable record in place, as an earlier run would have left it.
    ///
    /// # Panics
    ///
    /// When `name` is not a record name or the record is open.
    pub fn insert(&self, name: &str, bytes: &[u8]) {
        assert!(is_record_name(name), "not a record name: {name:?}");
        let mut state = self.state.borrow_mut();
        let entry = state.records.entry(name.to_owned()).or_default();
        assert!(entry.holder.is_none(), "record {name} is open");
        entry.durable = bytes.to_vec();
        entry.current = bytes.to_vec();
    }

    /// The bytes of a record now, durable or not; `None` for a record that
    /// was never opened or inserted.
    pub fn contents(&self, name: &str) -> Option<Vec<u8>> {
        let state = self.state.borrow();
        state.records.get(name).map(|entry| entry.current.clone())
    }

    /// The bytes of a record that a power cut would leave.
    pub fn durable(&self, name: &str) -> Option<Vec<u8>> {
        let state = self.state.borrow();
        state.records.get(name).map(|entry| entry.durable.clone())
    }

    /// The names of every record, in byte order.
    pub fn names(&self) -> Vec<String> {
        self.state.borrow().records.keys().cloned().collect()
    }

    /// Whether a record object holds the record now.
    pub fn is_open(&self, name: &str) -> bool {
        let state = self.state.borrow();
        state
            .records
            .get(name)
            .is_some_and(|entry| entry.holder.is_some())
    }

    /// Lose everything that was not made durable, as a power cut does.
    ///
    /// # Panics
    ///
    /// When a record is still open: drop the machine first, as a power cut
    /// ends it.
    pub fn power_cut(&self) {
        let mut state = self.state.borrow_mut();
        for (name, entry) in &mut state.records {
            assert!(entry.holder.is_none(), "record {name} is open");
            entry.current = entry.durable.clone();
        }
    }

    /// Make the next `operation` on the record `name` fail with `error`,
    /// once, without effect. A failure of kind
    /// [`ErrorKind::StorageUncertain`] also leaves the record needing to be
    /// opened again before it is written; for [`Operation::Commit`] the old
    /// contents stay ([`MemoryStorage::uncertain_commit`] chooses).
    ///
    /// # Panics
    ///
    /// For [`Operation::Sync`] with any kind but `StorageUncertain`: a
    /// failed sync is always uncertain ([`Record`]).
    pub fn fail_next(&self, name: &str, operation: Operation, error: Error) {
        assert!(
            operation != Operation::Sync || error.kind == ErrorKind::StorageUncertain,
            "a failed sync is StorageUncertain"
        );
        self.state.borrow_mut().faults.push(Fault {
            record: name.to_owned(),
            operation,
            error,
            takes_effect: false,
        });
    }

    /// Make the next commit of a replacement of the record `name` fail with
    /// [`ErrorKind::StorageUncertain`], once, after the new contents were
    /// put in place when `takes_effect` is true and before otherwise.
    pub fn uncertain_commit(&self, name: &str, takes_effect: bool) {
        self.state.borrow_mut().faults.push(Fault {
            record: name.to_owned(),
            operation: Operation::Commit,
            error: Error::new(
                ErrorKind::StorageUncertain,
                "the commit may or may not have taken effect",
            ),
            takes_effect,
        });
    }
}

impl Storage for MemoryStorage {
    fn open(&self, name: &str) -> Result<Box<dyn Record>, Error> {
        if !is_record_name(name) {
            return Err(usage("not a record name"));
        }
        let mut state = self.state.borrow_mut();
        if state
            .records
            .get(name)
            .is_some_and(|entry| entry.holder.is_some())
        {
            return Err(usage("the record is already open"));
        }
        if let Some(fault) = take_fault(&mut state.faults, name, Operation::Open) {
            return Err(fault.error);
        }
        state.opened += 1;
        let holder = state.opened;
        let entry = state.records.entry(name.to_owned()).or_default();
        entry.holder = Some(holder);
        entry.replacement = None;
        entry.poisoned = false;
        Ok(Box::new(MemoryRecord {
            state: Rc::clone(&self.state),
            name: name.to_owned(),
            holder,
        }))
    }
}

struct MemoryRecord {
    state: Rc<RefCell<State>>,
    name: String,
    holder: u64,
}

impl MemoryRecord {
    /// Run `action` on the record's entry after the checks every call makes:
    /// the record is still held by this object, a write is not made on a
    /// record that must be opened again, and a planted fault fires first.
    fn with_entry<R>(
        &self,
        operation: Option<Operation>,
        writes: bool,
        action: impl FnOnce(&mut Entry) -> Result<R, Error>,
    ) -> Result<R, Error> {
        let mut guard = self.state.borrow_mut();
        let state = &mut *guard;
        let entry = state
            .records
            .get_mut(&self.name)
            .filter(|entry| entry.holder == Some(self.holder))
            .ok_or_else(|| usage("the record was released"))?;
        if writes && entry.poisoned {
            return Err(usage(
                "the record must be opened again before it is written",
            ));
        }
        let fault =
            operation.and_then(|operation| take_fault(&mut state.faults, &self.name, operation));
        if let Some(fault) = fault {
            if fault.error.kind == ErrorKind::StorageUncertain {
                entry.poisoned = true;
            }
            return Err(fault.error);
        }
        action(entry)
    }
}

impl Record for MemoryRecord {
    fn size(&self) -> Result<u64, Error> {
        self.with_entry(None, false, |entry| Ok(entry.current.len() as u64))
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), Error> {
        self.with_entry(Some(Operation::Read), false, |entry| {
            read_into(&entry.current, offset, buf)
        })
    }

    fn append(&mut self, bytes: &[u8]) -> Result<(), Error> {
        self.with_entry(Some(Operation::Append), true, |entry| {
            entry.current.extend_from_slice(bytes);
            Ok(())
        })
    }

    fn truncate(&mut self, size: u64) -> Result<(), Error> {
        self.with_entry(
            Some(Operation::Truncate),
            true,
            |entry| match usize::try_from(size) {
                Ok(size) if size <= entry.current.len() => {
                    entry.current.truncate(size);
                    Ok(())
                }
                _ => Err(usage("a cut never lengthens a record")),
            },
        )
    }

    fn sync(&mut self) -> Result<(), Error> {
        self.with_entry(Some(Operation::Sync), true, |entry| {
            entry.durable = entry.current.clone();
            Ok(())
        })
    }

    fn replace(&mut self) -> Result<Box<dyn Replacement>, Error> {
        let number = self.with_entry(Some(Operation::Replace), true, |entry| {
            if entry.replacement.is_some() {
                return Err(usage("the record already has a replacement"));
            }
            entry.replacements += 1;
            entry.replacement = Some(entry.replacements);
            Ok(entry.replacements)
        })?;
        Ok(Box::new(MemoryReplacement {
            state: Rc::clone(&self.state),
            name: self.name.clone(),
            holder: self.holder,
            number,
            bytes: Vec::new(),
        }))
    }
}

impl Drop for MemoryRecord {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.try_borrow_mut() {
            if let Some(entry) = state.records.get_mut(&self.name) {
                if entry.holder == Some(self.holder) {
                    entry.holder = None;
                    entry.replacement = None;
                }
            }
        }
    }
}

struct MemoryReplacement {
    state: Rc<RefCell<State>>,
    name: String,
    holder: u64,
    number: u64,
    bytes: Vec<u8>,
}

impl Replacement for MemoryReplacement {
    fn size(&self) -> Result<u64, Error> {
        Ok(self.bytes.len() as u64)
    }

    fn read_at(&mut self, offset: u64, buf: &mut [u8]) -> Result<(), Error> {
        read_into(&self.bytes, offset, buf)
    }

    fn append(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let mut guard = self.state.borrow_mut();
        let state = &mut *guard;
        if let Some(fault) = take_fault(&mut state.faults, &self.name, Operation::ReplacementAppend)
        {
            if fault.error.kind == ErrorKind::StorageUncertain {
                if let Some(entry) = state.records.get_mut(&self.name) {
                    if entry.holder == Some(self.holder) {
                        entry.poisoned = true;
                    }
                }
            }
            return Err(fault.error);
        }
        self.bytes.extend_from_slice(bytes);
        Ok(())
    }

    fn commit(mut self: Box<Self>) -> Result<(), Error> {
        let bytes = std::mem::take(&mut self.bytes);
        let mut guard = self.state.borrow_mut();
        let state = &mut *guard;
        let entry = state
            .records
            .get_mut(&self.name)
            .filter(|entry| {
                entry.holder == Some(self.holder) && entry.replacement == Some(self.number)
            })
            .ok_or_else(|| usage("the record was released"))?;
        if entry.poisoned {
            return Err(usage(
                "the record must be opened again before it is written",
            ));
        }
        entry.replacement = None;
        match take_fault(&mut state.faults, &self.name, Operation::Commit) {
            Some(fault) => {
                if fault.error.kind == ErrorKind::StorageUncertain {
                    entry.poisoned = true;
                }
                if fault.takes_effect {
                    entry.durable = bytes.clone();
                    entry.current = bytes;
                }
                Err(fault.error)
            }
            None => {
                entry.durable = bytes.clone();
                entry.current = bytes;
                Ok(())
            }
        }
    }
}

impl Drop for MemoryReplacement {
    fn drop(&mut self) {
        if let Ok(mut state) = self.state.try_borrow_mut() {
            if let Some(entry) = state.records.get_mut(&self.name) {
                if entry.holder == Some(self.holder) && entry.replacement == Some(self.number) {
                    entry.replacement = None;
                }
            }
        }
    }
}

/// Verified content in memory, read at any offset.
///
/// A test makes the next read fail with [`MemoryContent::fail_next`], such as
/// with [`ErrorKind::Storage`] for a host read that failed once, and makes
/// every later read fail with [`ErrorKind::Content`] with
/// [`MemoryContent::change`], as when the file behind the object changed.
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct MemoryContent {
    bytes: Vec<u8>,
    failures: RefCell<Vec<Error>>,
    changed: Cell<bool>,
}

impl MemoryContent {
    /// Content that stands for `bytes`.
    pub fn new(bytes: Vec<u8>) -> Self {
        Self {
            bytes,
            failures: RefCell::new(Vec::new()),
            changed: Cell::new(false),
        }
    }

    /// Make the next read fail with `error`, once, without effect.
    pub fn fail_next(&self, error: Error) {
        self.failures.borrow_mut().push(error);
    }

    /// The bytes behind the object are no longer the verified ones: every
    /// later read fails with [`ErrorKind::Content`].
    pub fn change(&self) {
        self.changed.set(true);
    }
}

impl ContentReader for MemoryContent {
    fn size(&self) -> u64 {
        self.bytes.len() as u64
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), Error> {
        if self.changed.get() {
            return Err(Error::new(
                ErrorKind::Content,
                "the content is no longer the verified bytes",
            ));
        }
        let mut failures = self.failures.borrow_mut();
        if !failures.is_empty() {
            return Err(failures.remove(0));
        }
        read_into(&self.bytes, offset, buf)
    }
}

mod machine;
pub use machine::ScriptedMachine;
