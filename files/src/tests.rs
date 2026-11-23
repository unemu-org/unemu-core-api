// SPDX-License-Identifier: MIT OR Apache-2.0
//! The behaviour of the file-backed objects, on real files in a temporary
//! directory: the storage suite the in-memory storage of the contract's
//! tests follows, where it applies to files, plus what only files have
//! (locks across processes, leftovers of a crash, device names).

use crate::storage::Step;
use crate::{FileContent, FileStorage};
use std::fs;
use std::io::{BufRead, BufReader, Write};
use std::path::{Path, PathBuf};
use std::process::{Child, Command, Stdio};
use std::rc::Rc;
use std::sync::atomic::{AtomicU64, Ordering};
use std::time::Duration;
use unemu_core_api::{ContentReader, ErrorKind, Record, Storage};

/// A fresh directory under the system's temporary directory, removed when
/// dropped. Declare it before the storages and records that use it, so it
/// goes last.
struct TempDir(PathBuf);

impl TempDir {
    fn new() -> Self {
        static NEXT: AtomicU64 = AtomicU64::new(0);
        let path = std::env::temp_dir().join(format!(
            "unemu-core-files-test-{}-{}",
            std::process::id(),
            NEXT.fetch_add(1, Ordering::Relaxed)
        ));
        let _ = fs::remove_dir_all(&path);
        fs::create_dir_all(&path).expect("create a temporary directory");
        Self(path)
    }

    fn path(&self) -> &Path {
        &self.0
    }

    /// The names of the files in the directory, sorted.
    fn files(&self) -> Vec<String> {
        let mut names: Vec<String> = fs::read_dir(&self.0)
            .expect("list the directory")
            .map(|entry| {
                entry
                    .expect("an entry")
                    .file_name()
                    .into_string()
                    .expect("utf-8")
            })
            .collect();
        names.sort();
        names
    }
}

impl Drop for TempDir {
    fn drop(&mut self) {
        let _ = fs::remove_dir_all(&self.0);
    }
}

fn read_all(record: &mut dyn Record) -> Vec<u8> {
    let size = usize::try_from(record.size().expect("size")).expect("fits");
    let mut bytes = vec![0; size];
    record.read_at(0, &mut bytes).expect("read");
    bytes
}

fn kind<T, E: Into<unemu_core_api::Error>>(result: Result<T, E>) -> ErrorKind {
    match result {
        Ok(_) => panic!("expected a failure"),
        Err(error) => error.into().kind,
    }
}

// --- names and opening -----------------------------------------------------

#[test]
fn open_refuses_names_outside_the_rules() {
    let dir = TempDir::new();
    let storage = FileStorage::open(dir.path()).expect("storage");
    for name in ["", "A", "a.", "-a", "a_b", "a/b", "../a", "a\\b"] {
        assert_eq!(kind(storage.open(name)), ErrorKind::Usage, "{name:?}");
    }
    assert!(dir.files().is_empty());
}

#[test]
fn the_storage_creates_its_directory() {
    let dir = TempDir::new();
    let nested = dir.path().join("saves").join("core").join("ab");
    let storage = FileStorage::open(&nested).expect("storage");
    assert!(nested.is_dir());
    let canonical = fs::canonicalize(&nested).expect("canonical");
    assert_eq!(storage.dir(), canonical.as_path());
    let again = FileStorage::open(nested.join("..").join("ab")).expect("storage");
    assert_eq!(again.dir(), canonical.as_path(), "the same directory");
}

#[test]
fn a_missing_record_opens_empty_and_appears_on_the_first_write() {
    let dir = TempDir::new();
    let storage = FileStorage::open(dir.path()).expect("storage");
    let mut record = storage.open("cart.sav").expect("open");
    assert_eq!(record.size().expect("size"), 0);
    record
        .read_at(0, &mut [])
        .expect("an empty read at the end");
    assert_eq!(kind(record.read_at(0, &mut [0])), ErrorKind::Usage);
    record.truncate(0).expect("a cut to the same length");
    record.sync().expect("nothing to sync");
    record.append(&[]).expect("nothing to append");
    assert!(!dir.path().join("cart.sav").exists());
    record.append(b"abc").expect("append");
    assert!(dir.path().join("cart.sav").exists());
}

#[test]
fn append_read_truncate_and_sync() {
    let dir = TempDir::new();
    let storage = FileStorage::open(dir.path()).expect("storage");
    let mut record = storage.open("journal").expect("open");
    record.append(b"hello ").expect("append");
    record.append(b"world").expect("append");
    assert_eq!(record.size().expect("size"), 11);
    let mut buf = [0; 5];
    record.read_at(6, &mut buf).expect("read");
    assert_eq!(&buf, b"world");
    assert_eq!(kind(record.read_at(7, &mut buf)), ErrorKind::Usage);
    assert_eq!(kind(record.read_at(u64::MAX, &mut buf)), ErrorKind::Usage);
    assert_eq!(kind(record.truncate(12)), ErrorKind::Usage);
    record.truncate(5).expect("truncate");
    record.append(b"!").expect("append after a cut");
    record.sync().expect("sync");
    assert_eq!(read_all(&mut *record), b"hello!");
    drop(record);
    assert_eq!(
        fs::read(dir.path().join("journal")).expect("the file"),
        b"hello!"
    );

    let other = FileStorage::open(dir.path()).expect("a second storage");
    let mut record = other.open("journal").expect("open again");
    assert_eq!(read_all(&mut *record), b"hello!");
}

#[test]
fn device_names_are_stored_under_their_mapped_file_names() {
    let dir = TempDir::new();
    let storage = FileStorage::open(dir.path()).expect("storage");
    for name in ["con", "nul.bin", "com1.a.b", "lpt9"] {
        let mut record = storage.open(name).expect("open");
        record.append(name.as_bytes()).expect("append");
        record.sync().expect("sync");
    }
    for (name, file) in [
        ("con", "con_"),
        ("nul.bin", "nul_.bin"),
        ("com1.a.b", "com1_.a.b"),
        ("lpt9", "lpt9_"),
    ] {
        assert_eq!(
            fs::read(dir.path().join(file)).expect("file"),
            name.as_bytes()
        );
        let mut record = storage.open(name).expect("open again");
        assert_eq!(read_all(&mut *record), name.as_bytes());
    }
}

// --- exclusive use ---------------------------------------------------------

#[test]
fn a_record_is_held_exclusively_until_dropped() {
    let dir = TempDir::new();
    let storage = FileStorage::open(dir.path()).expect("storage");
    let record = storage.open("a").expect("open");
    let error = storage.open("a").err().expect("a second open fails");
    assert_eq!(error.kind, ErrorKind::Usage);
    let other = storage.open("b").expect("another record opens");
    drop(record);
    drop(other);
    storage.open("a").expect("open after release");
}

#[test]
fn two_storages_over_one_directory_exclude_each_other() {
    let dir = TempDir::new();
    let first = FileStorage::open(dir.path()).expect("storage");
    let second = FileStorage::open(dir.path()).expect("storage");
    let held = first.open("a").expect("open");
    assert_eq!(kind(second.open("a")), ErrorKind::Usage);
    let _b = second.open("b").expect("another record");
    assert_eq!(kind(first.open("b")), ErrorKind::Usage);
    drop(held);
    second.open("a").expect("open after release");
}

const CHILD_DIR: &str = "UNEMU_CORE_FILES_TEST_LOCK_HOLDER_DIR";

/// Run by `a_record_held_by_another_process_does_not_open` in a child
/// process: holds the record `shared` until its standard input closes.
#[test]
#[ignore = "the child process of a_record_held_by_another_process_does_not_open"]
fn lock_holder_child() {
    let Some(dir) = std::env::var_os(CHILD_DIR) else {
        return;
    };
    let storage = FileStorage::open(PathBuf::from(dir)).expect("storage");
    let _record = storage.open("shared").expect("open in the child");
    let _path_record = crate::FileRecord::open(storage.dir().join("path-record"), b"header", None)
        .expect("open path record in the child");
    let mut out = std::io::stdout();
    writeln!(out, "holding").expect("report");
    out.flush().expect("flush");
    let mut rest = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut rest);
}

/// Start a child process that holds the record `shared` of `dir`, and wait
/// until it does. Answers the child and the rest of its output.
fn spawn_lock_holder(dir: &TempDir) -> (Child, Box<dyn Iterator<Item = String>>) {
    let mut child = Command::new(std::env::current_exe().expect("the test binary"))
        .args([
            "tests::lock_holder_child",
            "--exact",
            "--include-ignored",
            "--nocapture",
            "--test-threads=1",
        ])
        .env(CHILD_DIR, dir.path())
        .stdin(Stdio::piped())
        .stdout(Stdio::piped())
        .stderr(Stdio::null())
        .spawn()
        .expect("start the child");
    let stdout = child.stdout.take().expect("the child's output");
    let mut lines = BufReader::new(stdout).lines().map_while(Result::ok);
    let holding = lines.by_ref().any(|line| line.contains("holding"));
    assert!(holding, "the child did not take the record");
    (child, Box::new(lines))
}

#[test]
fn a_record_held_by_another_process_does_not_open() {
    let dir = TempDir::new();
    let (mut child, lines) = spawn_lock_holder(&dir);
    let storage = FileStorage::open(dir.path()).expect("storage");
    assert_eq!(kind(storage.open("shared")), ErrorKind::Usage);
    assert_eq!(
        kind(crate::FileRecord::open(
            dir.path().join("path-record"),
            b"header",
            None
        )),
        ErrorKind::Usage
    );
    assert_eq!(
        kind(crate::FileRecord::inspect(dir.path().join("path-record"))),
        ErrorKind::Usage
    );
    storage.open("other").expect("another record opens");

    drop(child.stdin.take());
    // Read the child's output to its end, so its last lines never meet a
    // closed pipe.
    lines.for_each(drop);
    assert!(child.wait().expect("the child ends").success());
    storage.open("shared").expect("open after the child ended");
    crate::FileRecord::open(dir.path().join("path-record"), b"header", None)
        .expect("open path record after the child ended");
}

#[test]
fn a_record_held_by_a_killed_process_opens_again() {
    let dir = TempDir::new();
    let (mut child, lines) = spawn_lock_holder(&dir);
    let storage = FileStorage::open(dir.path()).expect("storage");
    assert_eq!(kind(storage.open("shared")), ErrorKind::Usage);
    child.kill().expect("kill the child");
    child.wait().expect("the child ends");
    lines.for_each(drop);
    // Windows may give a dead process's lock back a moment late.
    let mut attempts = 0;
    loop {
        match storage.open("shared") {
            Ok(_) => break,
            Err(error) if error.kind == ErrorKind::Usage && attempts < 100 => {
                attempts += 1;
                std::thread::sleep(Duration::from_millis(50));
            }
            Err(error) => panic!("the dead child's lock stays: {error}"),
        }
    }
}

// --- replacement -----------------------------------------------------------

#[test]
fn a_replacement_takes_the_place_of_the_old_contents_on_commit() {
    let dir = TempDir::new();
    let storage = FileStorage::open(dir.path()).expect("storage");
    let mut record = storage.open("nvram").expect("open");
    record.append(b"old").expect("append");
    record.sync().expect("sync");

    let mut replacement = record.replace().expect("replace");
    assert_eq!(kind(record.replace()), ErrorKind::Usage);
    replacement.append(b"new ").expect("append");
    replacement.append(b"bytes").expect("append");
    assert_eq!(replacement.size().expect("size"), 9);
    let mut buf = [0; 5];
    replacement
        .read_at(4, &mut buf)
        .expect("read the replacement");
    assert_eq!(&buf, b"bytes");
    assert_eq!(kind(replacement.read_at(5, &mut buf)), ErrorKind::Usage);

    // The old contents stay readable and appendable until the commit.
    record.append(b"!").expect("append to the old contents");
    assert_eq!(read_all(&mut *record), b"old!");

    replacement.commit().expect("commit");
    assert_eq!(read_all(&mut *record), b"new bytes");
    record.append(b".").expect("append to the new contents");
    record.sync().expect("sync");
    let mut again = record.replace().expect("a new replacement after a commit");
    again.append(b"x").expect("append");
    drop(again);
    drop(record);

    assert_eq!(dir.files(), ["nvram", "nvram_lock"]);
    let mut record = storage.open("nvram").expect("open again");
    assert_eq!(read_all(&mut *record), b"new bytes.");
}

#[test]
fn a_commit_creates_a_record_that_had_no_file() {
    let dir = TempDir::new();
    let storage = FileStorage::open(dir.path()).expect("storage");
    let mut record = storage.open("kept-a").expect("open");
    let mut replacement = record.replace().expect("replace");
    replacement.append(b"header").expect("append");
    replacement.commit().expect("commit");
    assert_eq!(read_all(&mut *record), b"header");
    assert_eq!(
        fs::read(dir.path().join("kept-a")).expect("file"),
        b"header"
    );
}

#[test]
fn a_replacement_dropped_mid_write_leaves_the_old_contents() {
    let dir = TempDir::new();
    let storage = FileStorage::open(dir.path()).expect("storage");
    let mut record = storage.open("sram").expect("open");
    record.append(b"old").expect("append");
    record.sync().expect("sync");
    let mut replacement = record.replace().expect("replace");
    replacement.append(b"half of the new").expect("append");
    drop(replacement);
    assert_eq!(read_all(&mut *record), b"old");
    drop(record);
    assert_eq!(dir.files(), ["sram", "sram_lock"]);
    let mut record = storage.open("sram").expect("open again");
    assert_eq!(read_all(&mut *record), b"old");
}

#[test]
fn opening_removes_what_a_crash_left_of_the_record_s_replacements() {
    let dir = TempDir::new();
    let storage = FileStorage::open(dir.path()).expect("storage");
    let mut record = storage.open("sram").expect("open");
    record.append(b"old").expect("append");
    record.sync().expect("sync");
    drop(record);
    // What a crash in the middle of two replacements leaves, beside files
    // that are not the record's leftovers.
    for leftover in ["sram_0.tmp", "sram_17.tmp"] {
        fs::write(dir.path().join(leftover), b"partial").expect("leftover");
    }
    for keep in ["other_0.tmp", "sram_x.tmp", "sram.tmp"] {
        fs::write(dir.path().join(keep), b"keep").expect("other file");
    }
    let mut record = storage.open("sram").expect("open after the crash");
    assert_eq!(read_all(&mut *record), b"old");
    assert_eq!(
        dir.files(),
        ["other_0.tmp", "sram", "sram.tmp", "sram_lock", "sram_x.tmp"]
    );
}

#[test]
fn a_released_record_s_replacement_fails_to_commit_and_changes_nothing() {
    let dir = TempDir::new();
    let storage = FileStorage::open(dir.path()).expect("storage");
    let mut record = storage.open("sram").expect("open");
    record.append(b"old").expect("append");
    record.sync().expect("sync");
    let mut replacement = record.replace().expect("replace");
    replacement.append(b"new").expect("append");
    drop(record);
    let mut holder = storage.open("sram").expect("the record is free again");
    assert_eq!(kind(replacement.commit()), ErrorKind::Usage);
    assert_eq!(read_all(&mut *holder), b"old");
    assert_eq!(fs::read(dir.path().join("sram")).expect("file"), b"old");
    drop(holder);
    assert_eq!(dir.files(), ["sram", "sram_lock"]);
}

#[test]
fn a_released_record_s_replacement_does_not_disturb_the_next_holder() {
    let dir = TempDir::new();
    let storage = FileStorage::open(dir.path()).expect("storage");
    let mut record = storage.open("sram").expect("open");
    let mut stale = record.replace().expect("replace");
    stale.append(b"stale").expect("append");
    drop(record);
    let mut record = storage.open("sram").expect("open again");
    let mut replacement = record.replace().expect("replace");
    replacement.append(b"fresh").expect("append");
    stale
        .append(b" more")
        .expect("appending to the stale one is harmless");
    // The stale one goes first: it must not take the new holder's
    // temporary file with it.
    assert_eq!(kind(stale.commit()), ErrorKind::Usage);
    replacement.commit().expect("commit");
    assert_eq!(read_all(&mut *record), b"fresh");
    drop(record);
    assert_eq!(dir.files(), ["sram", "sram_lock"]);
}

// --- the failure rules ------------------------------------------------------

/// A record `r` holding `old`, synchronised, and its storage.
fn record_with_old(dir: &TempDir) -> (FileStorage, Box<dyn Record>) {
    let storage = FileStorage::open(dir.path()).expect("storage");
    let mut record = storage.open("r").expect("open");
    record.append(b"old").expect("append");
    record.sync().expect("sync");
    (storage, record)
}

fn assert_writes_need_reopening(record: &mut dyn Record) {
    assert_eq!(kind(record.append(b"x")), ErrorKind::Usage);
    assert_eq!(kind(record.truncate(0)), ErrorKind::Usage);
    assert_eq!(kind(record.sync()), ErrorKind::Usage);
    assert_eq!(kind(record.replace()), ErrorKind::Usage);
}

#[test]
fn a_failed_append_that_is_undone_leaves_the_record_as_it_was() {
    let dir = TempDir::new();
    let (storage, mut record) = record_with_old(&dir);
    storage.fail_next("r", Step::Write);
    assert_eq!(kind(record.append(b"new bytes")), ErrorKind::Storage);
    assert_eq!(read_all(&mut *record), b"old");
    assert_eq!(fs::read(dir.path().join("r")).expect("file"), b"old");
    record.append(b"!").expect("the record stays writable");
    assert_eq!(read_all(&mut *record), b"old!");
}

#[test]
fn a_failed_append_that_cannot_be_undone_is_uncertain() {
    let dir = TempDir::new();
    let (storage, mut record) = record_with_old(&dir);
    storage.fail_next("r", Step::Write);
    storage.fail_next("r", Step::Restore);
    assert_eq!(
        kind(record.append(b"new bytes")),
        ErrorKind::StorageUncertain
    );
    assert_eq!(
        read_all(&mut *record),
        b"oldnew ",
        "reads follow the partial tail"
    );
    assert_writes_need_reopening(&mut *record);
    drop(record);
    let mut record = storage.open("r").expect("open again");
    assert_eq!(read_all(&mut *record), b"oldnew ");
    record.append(b"!").expect("writable after reopening");
}

#[test]
fn a_failed_cut_that_kept_the_length_leaves_the_record_as_it_was() {
    let dir = TempDir::new();
    let (storage, mut record) = record_with_old(&dir);
    storage.fail_next("r", Step::SetLen);
    assert_eq!(kind(record.truncate(1)), ErrorKind::Storage);
    assert_eq!(read_all(&mut *record), b"old");
    record.truncate(1).expect("the record stays writable");
}

#[test]
fn a_failed_cut_that_changed_the_length_is_uncertain() {
    let dir = TempDir::new();
    let (storage, mut record) = record_with_old(&dir);
    storage.fail_next("r", Step::SetLenAfter);
    assert_eq!(kind(record.truncate(1)), ErrorKind::StorageUncertain);
    assert_eq!(read_all(&mut *record), b"o", "reads follow the file");
    assert_writes_need_reopening(&mut *record);
    drop(record);
    let mut record = storage.open("r").expect("open again");
    assert_eq!(read_all(&mut *record), b"o");
    record.append(b"!").expect("writable after reopening");
}

#[test]
fn a_failed_sync_is_uncertain() {
    let dir = TempDir::new();
    let (storage, mut record) = record_with_old(&dir);
    record.append(b"!").expect("append");
    storage.fail_next("r", Step::SyncFile);
    assert_eq!(kind(record.sync()), ErrorKind::StorageUncertain);
    assert_eq!(read_all(&mut *record), b"old!");
    assert_writes_need_reopening(&mut *record);
}

#[test]
fn a_new_record_s_first_sync_also_makes_its_name_durable() {
    let dir = TempDir::new();
    let storage = FileStorage::open(dir.path()).expect("storage");
    let mut record = storage.open("r").expect("open");
    record.append(b"new").expect("append");
    storage.fail_next("r", Step::SyncDirectory);
    assert_eq!(kind(record.sync()), ErrorKind::StorageUncertain);
    assert_writes_need_reopening(&mut *record);
    drop(record);
    // The next opening cannot tell whether the name was made durable, so
    // its first sync makes it durable again; later syncs touch the file
    // only.
    let mut record = storage.open("r").expect("open again");
    record.append(b"!").expect("append");
    storage.fail_next("r", Step::SyncDirectory);
    assert_eq!(kind(record.sync()), ErrorKind::StorageUncertain);
    drop(record);
    let mut record = storage.open("r").expect("open a third time");
    record.sync().expect("sync");
    record.append(b"?").expect("append");
    storage.fail_next("r", Step::SyncDirectory);
    record
        .sync()
        .expect("a second sync of one opening leaves the directory");
}

#[test]
fn a_commit_that_fails_before_the_rename_leaves_the_record_as_it_was() {
    for step in [Step::ReplacementSync, Step::Rename] {
        let dir = TempDir::new();
        let (storage, mut record) = record_with_old(&dir);
        let mut replacement = record.replace().expect("replace");
        replacement.append(b"new").expect("append");
        storage.fail_next("r", step);
        assert_eq!(kind(replacement.commit()), ErrorKind::Storage, "{step:?}");
        assert_eq!(read_all(&mut *record), b"old");
        record.append(b"!").expect("the record stays writable");
        record.replace().expect("a new replacement");
        drop(record);
        assert_eq!(dir.files(), ["r", "r_lock"], "{step:?}");
        assert_eq!(fs::read(dir.path().join("r")).expect("file"), b"old!");
    }
}

#[test]
fn a_commit_whose_rename_cannot_be_made_durable_is_uncertain() {
    let dir = TempDir::new();
    let (storage, mut record) = record_with_old(&dir);
    let mut replacement = record.replace().expect("replace");
    replacement.append(b"new").expect("append");
    storage.fail_next("r", Step::SyncDirectory);
    assert_eq!(kind(replacement.commit()), ErrorKind::StorageUncertain);
    assert_eq!(read_all(&mut *record), b"new", "the rename took place");
    assert_writes_need_reopening(&mut *record);
    drop(record);
    let mut record = storage.open("r").expect("open again");
    assert_eq!(read_all(&mut *record), b"new");
    record.append(b"!").expect("writable after reopening");
}

#[test]
fn a_failed_replacement_append() {
    let dir = TempDir::new();
    let (storage, mut record) = record_with_old(&dir);
    let mut replacement = record.replace().expect("replace");
    replacement.append(b"ab").expect("append");
    storage.fail_next("r", Step::ReplacementWrite);
    assert_eq!(kind(replacement.append(b"cdef")), ErrorKind::Storage);
    assert_eq!(replacement.size().expect("size"), 2);
    replacement
        .append(b"c")
        .expect("the replacement stays writable");
    storage.fail_next("r", Step::ReplacementWrite);
    storage.fail_next("r", Step::Restore);
    assert_eq!(
        kind(replacement.append(b"cdef")),
        ErrorKind::StorageUncertain
    );
    assert_eq!(kind(replacement.append(b"!")), ErrorKind::Usage);
    assert_eq!(kind(replacement.append(&[])), ErrorKind::Usage);
    assert_eq!(kind(replacement.commit()), ErrorKind::Usage);
    assert_eq!(read_all(&mut *record), b"old");
    assert_writes_need_reopening(&mut *record);
    drop(record);
    let mut record = storage.open("r").unwrap();
    assert_eq!(read_all(&mut *record), b"old");
    record.append(b"!").unwrap();
}

#[test]
fn the_storage_works_as_the_contract_s_object() {
    let dir = TempDir::new();
    let storage: Rc<dyn Storage> = Rc::new(FileStorage::open(dir.path()).expect("storage"));
    let mut record = storage.open("a").expect("open");
    record.append(b"x").expect("append");
    record.sync().expect("sync");
}

// --- content ----------------------------------------------------------------

#[test]
fn content_reads_within_its_bounds() {
    let dir = TempDir::new();
    let path = dir.path().join("base.img");
    fs::write(&path, b"0123456789").expect("file");
    let content = FileContent::open(&path, 10).expect("content");
    let reader: &dyn ContentReader = &content;
    assert_eq!(reader.size(), 10);
    let mut buf = [0; 4];
    reader.read_at(3, &mut buf).expect("read");
    assert_eq!(&buf, b"3456");
    reader.read_at(6, &mut buf).expect("read to the end");
    assert_eq!(&buf, b"6789");
    reader
        .read_at(10, &mut [])
        .expect("an empty read at the end");
    assert_eq!(kind(reader.read_at(7, &mut buf)), ErrorKind::Usage);
    assert_eq!(kind(reader.read_at(u64::MAX, &mut buf)), ErrorKind::Usage);
    assert_eq!(kind(reader.read_at(11, &mut [])), ErrorKind::Usage);
}

#[test]
fn content_reads_from_two_threads_do_not_mix() {
    let dir = TempDir::new();
    let path = dir.path().join("base.img");
    let bytes: Vec<u8> = (0..1 << 16).map(|index: u32| (index % 251) as u8).collect();
    fs::write(&path, &bytes).expect("file");
    let content = FileContent::open(&path, bytes.len() as u64).expect("content");
    std::thread::scope(|scope| {
        for start in [0usize, 1 << 15] {
            let (content, bytes) = (&content, &bytes);
            scope.spawn(move || {
                let mut buf = [0; 509];
                for round in 0..4000 {
                    let offset = start + (round * 7) % ((1 << 15) - buf.len());
                    content.read_at(offset as u64, &mut buf).expect("read");
                    assert_eq!(&buf[..], &bytes[offset..offset + buf.len()], "{offset}");
                }
            });
        }
    });
}

#[test]
fn content_of_another_length_is_refused() {
    let dir = TempDir::new();
    let path = dir.path().join("base.img");
    fs::write(&path, b"0123456789").expect("file");
    assert_eq!(kind(FileContent::open(&path, 9)), ErrorKind::Content);
    assert_eq!(kind(FileContent::open(&path, 11)), ErrorKind::Content);
    let file = fs::File::open(&path).expect("open");
    FileContent::from_file(file, 10).expect("the verified length");
    assert_eq!(
        kind(FileContent::open(dir.path().join("missing"), 0)),
        ErrorKind::Storage
    );
}

#[test]
fn content_whose_length_changed_fails_with_content() {
    let dir = TempDir::new();
    let path = dir.path().join("base.img");
    fs::write(&path, b"0123456789").expect("file");
    let content = FileContent::open(&path, 10).expect("content");
    let mut buf = [0; 2];
    fs::OpenOptions::new()
        .append(true)
        .open(&path)
        .expect("open for writing")
        .write_all(b"!")
        .expect("grow the file");
    assert_eq!(kind(content.read_at(0, &mut buf)), ErrorKind::Content);
    fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("open for writing")
        .set_len(4)
        .expect("cut the file");
    assert_eq!(kind(content.read_at(8, &mut buf)), ErrorKind::Content);
}

#[test]
fn content_whose_modification_time_changed_fails_with_content() {
    let dir = TempDir::new();
    let path = dir.path().join("base.img");
    fs::write(&path, b"0123456789").expect("file");
    let content = FileContent::open(&path, 10).expect("content");
    let mut buf = [0; 2];
    content.read_at(0, &mut buf).expect("read");
    let file = fs::OpenOptions::new()
        .write(true)
        .open(&path)
        .expect("open for writing");
    let modified = file
        .metadata()
        .and_then(|metadata| metadata.modified())
        .expect("a modification time");
    // Set explicitly, because a write within the file system's clock tick
    // keeps the time.
    file.set_modified(modified + Duration::from_secs(10))
        .expect("set the time");
    assert_eq!(kind(content.read_at(0, &mut buf)), ErrorKind::Content);
}

// The public seam wraps actual files, so planted partial writes, changed
// identities and post-rename failures exercise the production state machine.
#[derive(Default)]
struct HostProbe {
    events: std::cell::RefCell<Vec<String>>,
    failures: std::cell::RefCell<Vec<String>>,
    durable_record: std::cell::RefCell<Option<Vec<u8>>>,
}
impl HostProbe {
    fn fail(&self, step: &str) {
        self.failures.borrow_mut().push(step.into());
    }
    fn step(&self, step: &str) -> std::io::Result<()> {
        self.events.borrow_mut().push(step.into());
        let mut failures = self.failures.borrow_mut();
        if let Some(i) = failures.iter().position(|s| s == step) {
            failures.remove(i);
            Err(std::io::Error::other(format!("planted {step}")))
        } else {
            Ok(())
        }
    }
}
struct ProbedFile {
    file: Box<dyn crate::FileIo>,
    probe: Rc<HostProbe>,
    role: &'static str,
}
impl ProbedFile {
    fn step(&self, op: &str) -> std::io::Result<()> {
        self.probe.step(&format!("{} {op}", self.role))
    }
}
impl std::io::Read for ProbedFile {
    fn read(&mut self, bytes: &mut [u8]) -> std::io::Result<usize> {
        if let Err(error) = self.step("read") {
            let prefix = bytes.len() / 2;
            self.file.read_exact(&mut bytes[..prefix])?;
            return Err(error);
        }
        self.file.read(bytes)
    }
}
impl Write for ProbedFile {
    fn write(&mut self, bytes: &[u8]) -> std::io::Result<usize> {
        if let Err(error) = self.step("write") {
            self.file.write_all(&bytes[..bytes.len() / 2])?;
            return Err(error);
        }
        self.file.write(bytes)
    }
    fn flush(&mut self) -> std::io::Result<()> {
        self.step("flush")?;
        self.file.flush()
    }
}
impl std::io::Seek for ProbedFile {
    fn seek(&mut self, from: std::io::SeekFrom) -> std::io::Result<u64> {
        self.step("seek")?;
        self.file.seek(from)
    }
}
impl crate::FileIo for ProbedFile {
    fn metadata(&self) -> std::io::Result<fs::Metadata> {
        self.step("metadata")?;
        self.file.metadata()
    }
    fn set_len(&mut self, len: u64) -> std::io::Result<()> {
        self.step("cut")?;
        self.file.set_len(len)?;
        self.step("cut after")
    }
    fn sync_all(&mut self) -> std::io::Result<()> {
        self.step("sync")?;
        self.file.sync_all()?;
        if self.role == "record" {
            // Restore only confirmed bytes when a test simulates a power cut.
            let position = self.file.stream_position()?;
            self.file.seek(std::io::SeekFrom::Start(0))?;
            let mut bytes = Vec::new();
            self.file.read_to_end(&mut bytes)?;
            self.file.seek(std::io::SeekFrom::Start(position))?;
            *self.probe.durable_record.borrow_mut() = Some(bytes);
        }
        Ok(())
    }
    fn set_permissions(&mut self, p: fs::Permissions) -> std::io::Result<()> {
        self.step("permissions")?;
        self.file.set_permissions(p)
    }
}
#[derive(Clone, Default)]
struct ProbedFs(Rc<HostProbe>);
impl ProbedFs {
    fn wrap(&self, file: Box<dyn crate::FileIo>, path: &Path) -> Box<dyn crate::FileIo> {
        let name = path.to_string_lossy();
        let role = if name.contains(".create-") {
            "initial"
        } else if name.contains(".retain-") {
            "retention"
        } else {
            "record"
        };
        Box::new(ProbedFile {
            file,
            probe: self.0.clone(),
            role,
        })
    }
}
impl crate::Filesystem for ProbedFs {
    fn canonicalize(&self, path: &Path) -> std::io::Result<PathBuf> {
        self.0.step("canonicalize")?;
        crate::HostFilesystem.canonicalize(path)
    }
    fn symlink_metadata(&self, path: &Path) -> std::io::Result<fs::Metadata> {
        self.0.step("entry metadata")?;
        crate::HostFilesystem.symlink_metadata(path)
    }
    fn entries(&self, dir: &Path) -> std::io::Result<Vec<PathBuf>> {
        self.0.step("entries")?;
        crate::HostFilesystem.entries(dir)
    }
    fn open(&self, path: &Path, write: bool) -> std::io::Result<Box<dyn crate::FileIo>> {
        self.0.step("open")?;
        Ok(self.wrap(crate::HostFilesystem.open(path, write)?, path))
    }
    fn open_candidate(&self, path: &Path) -> std::io::Result<Box<dyn crate::FileIo>> {
        self.0.step("candidate open")?;
        crate::HostFilesystem.open_candidate(path)
    }
    fn create_new(
        &self,
        path: &Path,
        p: Option<&fs::Permissions>,
    ) -> std::io::Result<Box<dyn crate::FileIo>> {
        self.0.step("create temporary")?;
        Ok(self.wrap(crate::HostFilesystem.create_new(path, p)?, path))
    }
    fn hard_link(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        self.0.step("link")?;
        crate::HostFilesystem.hard_link(from, to)?;
        self.0.step("link after")
    }
    fn rename(&self, from: &Path, to: &Path) -> std::io::Result<()> {
        self.0.step("rename")?;
        crate::HostFilesystem.rename(from, to)?;
        self.0.step("rename after")
    }
    fn remove_file(&self, path: &Path) -> std::io::Result<()> {
        self.0.step("remove")?;
        crate::HostFilesystem.remove_file(path)
    }
    fn sync_directory(&self, path: &Path) -> std::io::Result<()> {
        self.0.step("directory sync")?;
        crate::HostFilesystem.sync_directory(path)
    }
    fn lock(&self, path: &Path, shared: bool) -> std::io::Result<Box<dyn std::any::Any>> {
        self.0.step("lock")?;
        crate::HostFilesystem.lock(path, shared)
    }
}

fn path_record(fs: &ProbedFs, path: &Path) -> crate::FileRecord {
    crate::FileRecord::open_with(Rc::new(fs.clone()), path, None, b"header", None)
        .expect("open path record")
}
fn publish(record: &mut crate::FileRecord, bytes: &[u8]) -> Result<(), unemu_core_api::Error> {
    let mut replacement = record.replace()?;
    replacement.append(bytes)?;
    replacement.commit()
}

#[test]
fn path_records_publish_only_the_expected_initial_bytes_and_keep_the_stable_lock() {
    let dir = TempDir::new();
    let path = dir.path().join("arbitrary FILE.bin");
    let mut record = crate::FileRecord::open(&path, b"header", None).expect("open");
    assert!(!record.exists());
    assert_eq!(read_all(&mut record), b"");
    assert_eq!(kind(record.append(b"x")), ErrorKind::Usage);
    assert_eq!(kind(record.append(&[])), ErrorKind::Usage);
    assert_eq!(kind(publish(&mut record, b"wrong")), ErrorKind::Usage);
    assert_eq!(kind(publish(&mut record, b"xxxxxx")), ErrorKind::Usage);
    assert!(!path.exists());
    publish(&mut record, b"header").expect("initial publication");
    assert!(record.exists());
    record.append(b"data").expect("append");
    record.truncate(8).expect("cut");
    record.sync().expect("sync");
    assert_eq!(read_all(&mut record), b"headerda");
    assert_eq!(
        kind(crate::FileRecord::open(&path, b"header", None)),
        ErrorKind::Usage
    );
    assert!(dir.path().join("arbitrary FILE.bin.lock").is_file());
    drop(record);
    let mut record = crate::FileRecord::open(&path, b"different", None)
        .expect("generic open does not interpret existing bytes");
    assert_eq!(read_all(&mut record), b"headerda");
}

#[test]
fn path_initial_publication_never_clobbers_a_competing_entry() {
    let dir = TempDir::new();
    let path = dir.path().join("record");
    let mut record = crate::FileRecord::open(&path, b"header", None).expect("open");
    fs::write(&path, b"other writer").expect("competing file");
    assert_eq!(kind(publish(&mut record, b"header")), ErrorKind::Storage);
    assert_eq!(fs::read(path).unwrap(), b"other writer");
    assert!(!dir.files().iter().any(|name| name.contains(".create-")));
}

#[test]
fn path_protocol_orders_file_barriers_publication_cleanup_and_directory_barrier() {
    let dir = TempDir::new();
    let path = dir.path().join("record");
    let fs = ProbedFs::default();
    let mut record = path_record(&fs, &path);
    fs.0.events.borrow_mut().clear();
    publish(&mut record, b"header").expect("publish");
    let events = fs.0.events.borrow();
    let positions: Vec<_> = [
        "initial write",
        "initial flush",
        "initial sync",
        "link",
        "remove",
        "canonicalize",
        "entries",
        "directory sync",
    ]
    .iter()
    .map(|event| events.iter().position(|e| e == event).expect(event))
    .collect();
    assert!(positions.windows(2).all(|p| p[0] < p[1]), "{events:?}");
    drop(events);
    fs.0.events.borrow_mut().clear();
    publish(&mut record, b"replacement").expect("retain");
    let events = fs.0.events.borrow();
    let positions: Vec<_> = [
        "retention write",
        "retention flush",
        "retention sync",
        "rename",
        "directory sync",
    ]
    .iter()
    .map(|event| events.iter().position(|e| e == event).expect(event))
    .collect();
    assert!(positions.windows(2).all(|p| p[0] < p[1]), "{events:?}");
}

#[test]
fn path_creation_failures_preserve_absence_or_poison_after_publication() {
    for (step, stage, uncertain) in [
        ("create temporary", "create temporary", false),
        ("initial write", "append", false),
        ("initial flush", "initial sync", false),
        ("initial sync", "initial sync", false),
        ("link", "publish initial", false),
        ("link after", "publish initial", true),
        ("remove", "unlink initial", true),
        ("canonicalize", "verify path", true),
        ("entries", "cleanup", true),
        ("directory sync", "directory sync", true),
    ] {
        let dir = TempDir::new();
        let path = dir.path().join("record");
        let fs = ProbedFs::default();
        let mut record = path_record(&fs, &path);
        fs.0.fail(step);
        let error = publish(&mut record, b"header").expect_err(step);
        assert_eq!(
            error.kind,
            if uncertain {
                ErrorKind::StorageUncertain
            } else {
                ErrorKind::Storage
            },
            "{step}: {error}"
        );
        assert!(error.message.contains(stage), "{step}: {error}");
        assert_eq!(path.exists(), uncertain, "{step}");
        if uncertain {
            assert_eq!(read_all(&mut record), b"header", "{step}");
            assert_eq!(kind(record.sync()), ErrorKind::Usage);
            assert_eq!(kind(record.append(&[])), ErrorKind::Usage);
        } else {
            publish(&mut record, b"header").expect("retry known failure");
        }
    }
}

#[test]
fn path_open_failures_preserve_the_exact_stage_and_release_the_lock() {
    for (step, stage) in [
        ("canonicalize", crate::FileStage::ResolvePath),
        ("lock", crate::FileStage::Lock),
        ("open", crate::FileStage::Open),
        ("record metadata", crate::FileStage::Metadata),
        ("entries", crate::FileStage::Cleanup),
        ("directory sync", crate::FileStage::DirectorySync),
    ] {
        let dir = TempDir::new();
        let path = dir.path().join("record");
        fs::write(&path, b"header").unwrap();
        let fs = ProbedFs::default();
        fs.0.fail(step);
        let error = crate::FileRecord::open_with(Rc::new(fs), &path, None, b"header", None)
            .err()
            .expect(step);
        assert_eq!(error.stage, stage);
        assert!(error.source.to_string().contains(step));
        crate::FileRecord::open(&path, b"header", None).expect("lock released on error");
    }
}

#[test]
fn path_record_seam_covers_partial_writes_cuts_and_barriers() {
    for (step, after, expected) in [
        ("record write", false, ErrorKind::Storage),
        ("record write", true, ErrorKind::StorageUncertain),
        ("record cut", false, ErrorKind::Storage),
        ("record cut after", false, ErrorKind::StorageUncertain),
        ("record flush", false, ErrorKind::StorageUncertain),
        ("record sync", false, ErrorKind::StorageUncertain),
    ] {
        let dir = TempDir::new();
        let path = dir.path().join("record");
        fs::write(&path, b"header").unwrap();
        let fs = ProbedFs::default();
        let mut record = path_record(&fs, &path);
        fs.0.fail(step);
        if after {
            fs.0.fail("record cut");
        }
        let result = if step.contains("write") {
            record.append(b"new data")
        } else if step.contains("cut") {
            record.truncate(2)
        } else {
            record.sync()
        };
        assert_eq!(kind(result), expected, "{step}");
        assert!(!read_all(&mut record).is_empty());
        if expected == ErrorKind::StorageUncertain {
            assert_eq!(kind(record.truncate(0)), ErrorKind::Usage);
            assert_eq!(kind(record.replace()), ErrorKind::Usage);
        } else {
            assert_eq!(read_all(&mut record), b"header");
        }
    }
}

#[test]
fn retention_can_borrow_an_independent_failure_seam() {
    for (step, uncertain, new) in [
        ("create temporary", false, false),
        ("retention permissions", false, false),
        ("retention write", false, false),
        ("retention flush", false, false),
        ("retention sync", false, false),
        ("rename", true, false),
        ("rename after", true, true),
        ("directory sync", true, true),
    ] {
        let dir = TempDir::new();
        let path = dir.path().join("record");
        fs::write(&path, b"old").unwrap();
        let mut record = crate::FileRecord::open(&path, b"header", None).unwrap();
        let fs = ProbedFs::default();
        fs.0.fail(step);
        let result = (|| {
            let mut replacement = record.replace_with(&fs)?;
            replacement.append(b"new contents")?;
            replacement.commit()
        })();
        let error = result.expect_err(step);
        assert_eq!(
            error.kind,
            if uncertain {
                ErrorKind::StorageUncertain
            } else {
                ErrorKind::Storage
            },
            "{step}: {error}"
        );
        assert_eq!(
            read_all(&mut record),
            if new {
                b"new contents".as_slice()
            } else {
                b"old".as_slice()
            },
            "{step}"
        );
        if uncertain {
            assert_eq!(kind(record.sync()), ErrorKind::Usage);
        } else {
            record.append(b"!").expect("record remains writable");
        }
    }
}

#[test]
fn path_shared_inspection_excludes_writers_and_refuses_every_mutation() {
    let dir = TempDir::new();
    let path = dir.path().join("record");
    fs::write(&path, b"bytes").unwrap();
    let mut first = crate::FileRecord::inspect(&path).unwrap();
    let second = crate::FileRecord::inspect(&path).unwrap();
    assert_eq!(
        kind(crate::FileRecord::open(&path, b"header", None)),
        ErrorKind::Usage
    );
    assert_eq!(read_all(&mut first), b"bytes");
    assert_eq!(kind(first.append(&[])), ErrorKind::Usage);
    assert_eq!(kind(first.truncate(5)), ErrorKind::Usage);
    assert_eq!(kind(first.sync()), ErrorKind::Usage);
    assert_eq!(kind(first.replace()), ErrorKind::Usage);
    drop(first);
    drop(second);
    let writer = crate::FileRecord::open(&path, b"header", None).unwrap();
    assert_eq!(kind(crate::FileRecord::inspect(&path)), ErrorKind::Usage);
    drop(writer);
    assert!(dir.path().join("record.lock").exists());
}

struct ReadOnlyLockFs;
impl crate::Filesystem for ReadOnlyLockFs {
    fn lock(&self, _: &Path, _: bool) -> std::io::Result<Box<dyn std::any::Any>> {
        Err(std::io::ErrorKind::PermissionDenied.into())
    }
}
#[test]
fn inspection_fallback_requires_an_absent_sidecar_and_never_grants_writes() {
    let dir = TempDir::new();
    let path = dir.path().join("record");
    fs::write(&path, b"header").unwrap();
    let lock =
        crate::PathLock::acquire(&ReadOnlyLockFs, &path, true).expect("read-only folder fallback");
    assert!(!lock.is_locked());
    let mut record =
        crate::FileRecord::open_with(Rc::new(ReadOnlyLockFs), &path, Some(lock), &[], None)
            .unwrap();
    assert_eq!(read_all(&mut record), b"header");
    assert_eq!(kind(record.append(b"x")), ErrorKind::Usage);
    assert_eq!(
        kind(
            crate::PathLock::acquire(&ReadOnlyLockFs, &path, false)
                .map_err(unemu_core_api::Error::from)
        ),
        ErrorKind::Storage
    );
    fs::write(dir.path().join("record.lock"), b"").unwrap();
    assert_eq!(
        crate::PathLock::acquire(&ReadOnlyLockFs, &path, true)
            .err()
            .unwrap()
            .stage,
        crate::FileStage::Lock
    );
}

#[test]
fn a_preacquired_path_lock_transfers_to_the_record() {
    let dir = TempDir::new();
    let path = dir.path().join("record");
    let lock = crate::PathLock::exclusive(&path).unwrap();
    let mut record = crate::FileRecord::open_with(
        Rc::new(crate::HostFilesystem),
        &path,
        Some(lock),
        b"header",
        None,
    )
    .unwrap();
    publish(&mut record, b"header").unwrap();
    let lock = crate::PathLock::exclusive(dir.path().join("another")).unwrap();
    assert_eq!(
        crate::FileRecord::open_with(
            Rc::new(crate::HostFilesystem),
            &path,
            Some(lock),
            b"header",
            None
        )
        .err()
        .unwrap()
        .stage,
        crate::FileStage::Lock
    );
    assert_eq!(
        kind(crate::FileRecord::open(&path, b"header", None)),
        ErrorKind::Usage
    );
    drop(record);
    crate::PathLock::exclusive(path).unwrap();
}

#[cfg(unix)]
#[test]
fn path_cleanup_preserves_protected_aliases_links_special_files_and_foreign_prefixes() {
    use std::os::unix::fs::symlink;
    let dir = TempDir::new();
    let path = dir.path().join("record");
    fs::write(&path, b"headerdata").unwrap();
    fs::write(dir.path().join("base"), b"he").unwrap();
    let protected = fs::metadata(dir.path().join("base")).unwrap();
    for name in ["record.create-0.tmp", "record.retain-0.tmp"] {
        fs::hard_link(dir.path().join("base"), dir.path().join(name)).unwrap();
    }
    fs::write(dir.path().join("record.create-1.tmp"), b"head").unwrap();
    fs::write(dir.path().join("record.create-2.tmp"), b"foreign").unwrap();
    fs::write(dir.path().join("record.create-3.tmp"), b"header too long").unwrap();
    fs::write(dir.path().join("record.retain-1.tmp"), b"any partial bytes").unwrap();
    symlink(
        dir.path().join("base"),
        dir.path().join("record.retain-2.tmp"),
    )
    .unwrap();
    fs::create_dir(dir.path().join("record.retain-3.tmp")).unwrap();
    fs::write(dir.path().join("record.retain-x.tmp"), b"other namespace").unwrap();
    let record = crate::FileRecord::open(&path, b"header", Some(&protected)).unwrap();
    for name in [
        "record.create-0.tmp",
        "record.retain-0.tmp",
        "record.create-2.tmp",
        "record.create-3.tmp",
        "record.retain-2.tmp",
        "record.retain-3.tmp",
        "record.retain-x.tmp",
    ] {
        assert!(
            fs::symlink_metadata(dir.path().join(name)).is_ok(),
            "{name}"
        );
    }
    assert!(!dir.path().join("record.create-1.tmp").exists());
    assert!(!dir.path().join("record.retain-1.tmp").exists());
    assert_eq!(fs::read(dir.path().join("base")).unwrap(), b"he");
    drop(record);
    assert_eq!(
        crate::FileRecord::open(dir.path().join("base"), b"header", Some(&protected))
            .err()
            .unwrap()
            .stage,
        crate::FileStage::Metadata
    );
}

#[cfg(unix)]
#[test]
fn retention_preserves_exact_unix_mode_and_the_lock_inode() {
    use std::os::unix::fs::{MetadataExt, PermissionsExt};
    let dir = TempDir::new();
    let path = dir.path().join("record");
    fs::write(&path, b"old").unwrap();
    fs::set_permissions(&path, fs::Permissions::from_mode(0o6777)).unwrap();
    let expected_mode = fs::metadata(&path).unwrap().mode() & 0o7777;
    let mut record = crate::FileRecord::open(&path, b"header", None).unwrap();
    let lock_path = dir.path().join("record.lock");
    let before = fs::metadata(&lock_path).unwrap();
    let mut replacement = record.replace().unwrap();
    assert!(dir.path().join("record.retain-0.tmp").exists());
    replacement.append(b"new").unwrap();
    replacement.commit().unwrap();
    assert_eq!(fs::metadata(&path).unwrap().mode() & 0o7777, expected_mode);
    assert_eq!(fs::metadata(lock_path).unwrap().ino(), before.ino());
}

#[cfg(unix)]
#[test]
fn path_resolution_rejects_reserved_and_dangling_links_and_unifies_parent_aliases() {
    use std::os::unix::fs::symlink;
    let dir = TempDir::new();
    let path = dir.path().join("record");
    symlink(&path, dir.path().join("dangling")).unwrap();
    assert_eq!(
        crate::FileRecord::open(dir.path().join("dangling"), b"", None)
            .err()
            .unwrap()
            .stage,
        crate::FileStage::ResolvePath
    );
    for name in ["record.create-0.tmp", "record.retain-9.tmp"] {
        assert_eq!(
            crate::FileRecord::open(dir.path().join(name), b"", None)
                .err()
                .unwrap()
                .stage,
            crate::FileStage::ResolvePath
        );
    }
    symlink(dir.path(), dir.path().join("alias")).unwrap();
    let record = crate::FileRecord::open(&path, b"header", None).unwrap();
    assert_eq!(
        kind(crate::FileRecord::open(
            dir.path().join("alias/record"),
            b"header",
            None
        )),
        ErrorKind::Usage
    );
    drop(record);
}

#[test]
fn a_released_path_replacement_cannot_commit_or_remove_the_next_holders_temporary() {
    let dir = TempDir::new();
    let path = dir.path().join("record");
    fs::write(&path, b"old").unwrap();
    let mut record = crate::FileRecord::open(&path, b"header", None).unwrap();
    let mut old = record.replace().unwrap();
    old.append(b"old replacement").unwrap();
    drop(record);
    let mut record = crate::FileRecord::open(&path, b"header", None).unwrap();
    let mut next = record.replace().unwrap();
    next.append(b"new replacement").unwrap();
    assert_eq!(kind(old.commit()), ErrorKind::Usage);
    next.commit().unwrap();
    assert_eq!(read_all(&mut record), b"new replacement");
}

#[cfg(unix)]
struct SwappingCandidateFs {
    replacement: PathBuf,
    after_open: bool,
}
#[cfg(unix)]
impl crate::Filesystem for SwappingCandidateFs {
    fn open_candidate(&self, path: &Path) -> std::io::Result<Box<dyn crate::FileIo>> {
        if self.after_open {
            let file = crate::HostFilesystem.open_candidate(path)?;
            fs::rename(&self.replacement, path)?;
            Ok(file)
        } else {
            fs::rename(&self.replacement, path)?;
            crate::HostFilesystem.open_candidate(path)
        }
    }
}
#[cfg(unix)]
#[test]
fn cleanup_checks_both_the_opened_inode_and_the_entry_before_unlink() {
    for after_open in [false, true] {
        let dir = TempDir::new();
        let path = dir.path().join("record");
        let candidate = dir.path().join("record.retain-0.tmp");
        let replacement = dir.path().join("different-inode");
        fs::write(&path, b"header").unwrap();
        fs::write(&candidate, b"leftover").unwrap();
        fs::write(&replacement, b"preserve this file").unwrap();
        let host = SwappingCandidateFs {
            replacement,
            after_open,
        };
        let _record =
            crate::FileRecord::open_with(Rc::new(host), &path, None, b"header", None).unwrap();
        assert_eq!(
            fs::read(&candidate).unwrap(),
            b"preserve this file",
            "after_open={after_open}"
        );
    }
}

#[test]
fn inspection_does_not_cleanup_or_request_a_write_or_directory_barrier() {
    let dir = TempDir::new();
    let path = dir.path().join("record");
    let leftover = dir.path().join("record.retain-0.tmp");
    fs::write(&path, b"header").unwrap();
    fs::write(&leftover, b"leftover").unwrap();
    let fs = ProbedFs::default();
    fs.0.fail("entries");
    fs.0.fail("directory sync");
    let lock = crate::PathLock::acquire(&fs, &path, true).unwrap();
    let mut record =
        crate::FileRecord::open_with(Rc::new(fs.clone()), &path, Some(lock), b"header", None)
            .unwrap();
    assert_eq!(read_all(&mut record), b"header");
    assert!(leftover.exists());
    assert_eq!(fs.0.failures.borrow().len(), 2);
}

#[test]
fn path_temporary_collisions_are_skipped_without_truncating_the_occupant() {
    let dir = TempDir::new();
    let path = dir.path().join("record");
    let collision = dir.path().join("record.create-0.tmp");
    fs::write(&collision, b"foreign").unwrap();
    let mut record = crate::FileRecord::open(&path, b"header", None).unwrap();
    let mut replacement = record.replace().unwrap();
    assert!(dir.path().join("record.create-1.tmp").exists());
    replacement.append(b"header").unwrap();
    replacement.commit().unwrap();
    assert_eq!(fs::read(&collision).unwrap(), b"foreign");
    let collision = dir.path().join("record.retain-0.tmp");
    fs::write(&collision, b"foreign").unwrap();
    let mut replacement = record.replace().unwrap();
    assert!(dir.path().join("record.retain-1.tmp").exists());
    replacement.append(b"new").unwrap();
    replacement.commit().unwrap();
    assert_eq!(fs::read(collision).unwrap(), b"foreign");
}

#[cfg(unix)]
#[test]
fn inspection_never_bypasses_a_dangling_sidecar() {
    let dir = TempDir::new();
    let path = dir.path().join("record");
    fs::write(&path, b"header").unwrap();
    std::os::unix::fs::symlink(dir.path().join("missing"), dir.path().join("record.lock")).unwrap();
    assert_eq!(
        crate::PathLock::acquire(&ReadOnlyLockFs, &path, true)
            .err()
            .unwrap()
            .stage,
        crate::FileStage::Lock
    );
    assert_eq!(
        crate::FileRecord::inspect(path).err().unwrap().stage,
        crate::FileStage::Lock
    );
}

#[test]
fn public_file_handles_expose_read_seek_and_metadata_failures() {
    let dir = TempDir::new();
    let path = dir.path().join("record");
    fs::write(&path, b"header").unwrap();
    let fs = ProbedFs::default();
    let mut record = path_record(&fs, &path);
    for step in ["record seek", "record read"] {
        fs.0.fail(step);
        assert_eq!(kind(record.read_at(0, &mut [0])), ErrorKind::Storage);
        assert_eq!(read_all(&mut record), b"header");
    }
    fs.0.fail("record metadata");
    assert_eq!(kind(record.replace()), ErrorKind::Storage);
    assert_eq!(read_all(&mut record), b"header");
}

#[cfg(unix)]
#[test]
fn cleanup_metadata_open_read_and_unlink_failures_are_injectable() {
    struct CandidateFs(ProbedFs);
    impl crate::Filesystem for CandidateFs {
        fn open_candidate(&self, path: &Path) -> std::io::Result<Box<dyn crate::FileIo>> {
            self.0 .0.step("candidate open")?;
            Ok(Box::new(ProbedFile {
                file: crate::HostFilesystem.open_candidate(path)?,
                probe: self.0 .0.clone(),
                role: "candidate",
            }))
        }
        fn symlink_metadata(&self, path: &Path) -> std::io::Result<fs::Metadata> {
            self.0.symlink_metadata(path)
        }
        fn remove_file(&self, path: &Path) -> std::io::Result<()> {
            self.0.remove_file(path)
        }
    }
    for (step, fails_open) in [
        ("entry metadata", true),
        ("candidate open", false),
        ("candidate metadata", true),
        ("candidate read", false),
        ("remove", true),
    ] {
        let dir = TempDir::new();
        let path = dir.path().join("record");
        let candidate = dir.path().join("record.create-0.tmp");
        fs::write(&path, b"header").unwrap();
        fs::write(&candidate, b"head").unwrap();
        let fs = ProbedFs::default();
        fs.0.fail(step);
        let result =
            crate::FileRecord::open_with(Rc::new(CandidateFs(fs)), &path, None, b"header", None);
        assert_eq!(result.is_err(), fails_open, "{step}");
        if let Err(error) = result {
            assert_eq!(error.stage, crate::FileStage::Cleanup);
        }
        assert!(candidate.exists(), "{step}");
    }
}

#[cfg(target_os = "linux")]
#[test]
fn non_utf8_paths_share_the_same_publication_and_cleanup_rules() {
    use std::os::unix::ffi::OsStrExt;
    let dir = TempDir::new();
    let path = dir.path().join(std::ffi::OsStr::from_bytes(b"record-\xff"));
    let mut record = crate::FileRecord::open(&path, b"header", None).unwrap();
    publish(&mut record, b"header").unwrap();
    drop(record);
    let leftover = crate::path::suffix(&path, ".retain-0.tmp");
    fs::write(&leftover, b"leftover").unwrap();
    let _record = crate::FileRecord::open(&path, b"header", None).unwrap();
    assert!(!leftover.exists());
    assert!(crate::is_temporary_path(&crate::path::suffix(
        &path,
        ".create-nested"
    )));
}

#[cfg(unix)]
#[test]
fn reserved_and_cleanup_names_support_non_utf8_filenames() {
    use std::os::unix::ffi::OsStrExt;
    let path = Path::new(std::ffi::OsStr::from_bytes(b"record-\xff"));
    let candidate = crate::path::suffix(path, ".retain-123.tmp");
    assert!(crate::is_temporary_path(&candidate));
    assert!(crate::path::temporary_of(path, &candidate, "retain"));
    assert!(!crate::path::temporary_of(path, &candidate, "create"));
    assert!(!crate::path::temporary_of(
        path,
        &crate::path::suffix(path, ".retain-x.tmp"),
        "retain"
    ));
}

#[test]
fn a_failed_retention_restore_poisoning_is_shared_with_the_record() {
    let dir = TempDir::new();
    let path = dir.path().join("record");
    fs::write(&path, b"old").unwrap();
    let mut record = crate::FileRecord::open(&path, b"header", None).unwrap();
    let host = ProbedFs::default();
    let mut replacement = record.replace_with(&host).unwrap();
    host.0.fail("retention write");
    host.0.fail("retention cut");
    assert_eq!(
        kind(replacement.append(b"new bytes")),
        ErrorKind::StorageUncertain
    );
    assert_eq!(read_all(&mut record), b"old");
    assert_eq!(kind(record.append(&[])), ErrorKind::Usage);
    assert_eq!(kind(replacement.append(&[])), ErrorKind::Usage);
    assert_eq!(kind(replacement.commit()), ErrorKind::Usage);
}

#[test]
fn inspection_fallback_accepts_only_permission_or_read_only_errors() {
    struct FailedLock(std::io::ErrorKind);
    impl crate::Filesystem for FailedLock {
        fn lock(&self, _: &Path, _: bool) -> std::io::Result<Box<dyn std::any::Any>> {
            Err(self.0.into())
        }
    }
    let dir = TempDir::new();
    let path = dir.path().join("record");
    for error in [
        std::io::ErrorKind::PermissionDenied,
        std::io::ErrorKind::ReadOnlyFilesystem,
    ] {
        assert!(!crate::PathLock::acquire(&FailedLock(error), &path, true)
            .unwrap()
            .is_locked());
    }
    for error in [
        std::io::ErrorKind::Unsupported,
        std::io::ErrorKind::WouldBlock,
        std::io::ErrorKind::Other,
    ] {
        let failure = crate::PathLock::acquire(&FailedLock(error), &path, true)
            .err()
            .unwrap();
        assert_eq!(failure.stage, crate::FileStage::Lock);
        assert_eq!(failure.source.kind(), error);
    }
}

#[test]
fn post_open_canonical_verification_cannot_be_skipped() {
    struct RedirectedFs(std::cell::Cell<u32>);
    impl crate::Filesystem for RedirectedFs {
        fn canonicalize(&self, path: &Path) -> std::io::Result<PathBuf> {
            let n = self.0.get() + 1;
            self.0.set(n);
            if n == 3 {
                Ok(path.with_file_name("changed"))
            } else {
                path.canonicalize()
            }
        }
    }
    let dir = TempDir::new();
    let path = dir.path().join("record");
    fs::write(&path, b"header").unwrap();
    let error = crate::FileRecord::open_with(
        Rc::new(RedirectedFs(std::cell::Cell::new(0))),
        &path,
        None,
        b"header",
        None,
    )
    .err()
    .unwrap();
    assert_eq!(error.stage, crate::FileStage::VerifyPath);
    crate::FileRecord::open(path, b"header", None).expect("failed verification released lock");
}

#[test]
fn record_paths_reserve_every_engine_sidecar() {
    let dir = TempDir::new();
    for name in [
        "record.lock",
        "record_lock",
        "record.create-0.tmp",
        "record.retain-9.tmp",
        "record_0.tmp",
        "record.LOCK",
    ] {
        let path = dir.path().join(name);
        for exists in [false, true] {
            if exists {
                fs::write(&path, b"sidecar").unwrap();
            }
            assert_eq!(
                kind(crate::FileRecord::open(&path, b"", None)),
                ErrorKind::Usage,
                "{name}, exists={exists}"
            );
            assert_eq!(kind(crate::FileRecord::inspect(&path)), ErrorKind::Usage);
            assert_eq!(kind(crate::PathLock::exclusive(&path)), ErrorKind::Usage);
        }
    }
}

#[test]
fn a_record_cannot_replace_another_records_sidecar_lock() {
    let dir = TempDir::new();
    let path = dir.path().join("record");
    let _held = crate::FileRecord::open(&path, b"", None).unwrap();
    assert_eq!(
        kind(crate::FileRecord::open(
            dir.path().join("record.lock"),
            b"",
            None
        )),
        ErrorKind::Usage
    );
    let storage = FileStorage::open(dir.path()).unwrap();
    let _escaped = storage.open("record.lock").unwrap();
    assert_eq!(
        kind(crate::FileRecord::open(&path, b"", None)),
        ErrorKind::Usage
    );
}

#[test]
fn named_records_escape_path_sidecars_and_preserve_ordinary_mappings() {
    let dir = TempDir::new();
    let storage = FileStorage::open(dir.path()).unwrap();
    for (name, file) in [
        ("record.lock", "_record_lock_"),
        ("record.create-0.tmp", "_record_create-0_tmp_"),
        ("record.retain-9.tmp", "_record_retain-9_tmp_"),
        ("con.lock", "_con_lock_"),
        ("a.retain-x.nested.lock", "_a_retain-x_nested_lock_"),
        ("record", "record"),
        ("cart.sav", "cart.sav"),
        ("con", "con_"),
        ("nul.bin", "nul_.bin"),
    ] {
        assert_eq!(crate::file_name(name).as_deref(), Some(file));
        assert_eq!(crate::record_name(file).as_deref(), Some(name));
        let mut record = storage.open(name).unwrap();
        record.append(b"old").unwrap();
        record.sync().unwrap();
        let mut replacement = record.replace().unwrap();
        replacement.append(b"new").unwrap();
        replacement.commit().unwrap();
        drop(record);
        assert_eq!(fs::read(dir.path().join(file)).unwrap(), b"new");
        assert_eq!(read_all(&mut *storage.open(name).unwrap()), b"new");
    }
    for sidecar in [
        "record.lock",
        "record.create-0.tmp",
        "record.retain-9.tmp",
        "_record_lock__lock",
        "_record_lock__0.tmp",
    ] {
        assert_eq!(crate::record_name(sidecar), None);
    }
}

#[test]
fn named_and_path_records_have_disjoint_lock_domains() {
    let dir = TempDir::new();
    let storage = FileStorage::open(dir.path()).unwrap();
    let mut named = storage.open("record").unwrap();
    named.append(b"data").unwrap();
    named.sync().unwrap();
    // Demonstrate the host precondition without mutating through both holders.
    let mut path = crate::FileRecord::open(dir.path().join("record"), b"", None).unwrap();
    assert_eq!(read_all(&mut path), b"data");
    assert!(dir.path().join("record_lock").exists());
    assert!(dir.path().join("record.lock").exists());
    assert_eq!(kind(storage.open("record")), ErrorKind::Usage);
    assert_eq!(
        kind(crate::FileRecord::open(
            dir.path().join("record"),
            b"",
            None
        )),
        ErrorKind::Usage
    );
}

#[cfg(unix)]
#[test]
fn record_sidecar_reservations_cover_symlinks_and_non_utf8_names() {
    use std::os::unix::ffi::OsStrExt;
    use std::os::unix::fs::symlink;
    let dir = TempDir::new();
    let sidecar = dir.path().join("record.lock");
    let ordinary = dir.path().join("ordinary");
    fs::write(&sidecar, b"lock").unwrap();
    fs::write(&ordinary, b"data").unwrap();
    symlink(&sidecar, dir.path().join("alias")).unwrap();
    symlink(&ordinary, dir.path().join("alias.lock")).unwrap();
    for name in ["alias", "alias.lock"] {
        assert_eq!(
            kind(crate::FileRecord::open(dir.path().join(name), b"", None)),
            ErrorKind::Usage
        );
    }
    for name in [
        b"record-\xff.lock".as_slice(),
        b"record-\xff_lock",
        b"record-\xff_0.tmp",
    ] {
        assert_eq!(
            kind(crate::FileRecord::open(
                dir.path().join(std::ffi::OsStr::from_bytes(name)),
                b"",
                None
            )),
            ErrorKind::Usage
        );
    }
}

#[cfg(unix)]
#[test]
fn creation_under_restrictive_umask_keeps_the_writable_descriptor() {
    const CHILD: &str = "UNEMU_CORE_FILES_TEST_RESTRICTIVE_UMASK_CHILD";
    if std::env::var_os(CHILD).is_none() {
        // A child keeps the process-wide umask away from parallel tests.
        let output = Command::new("/bin/sh")
            .args(["-c", "umask 0222; exec \"$1\" --exact tests::creation_under_restrictive_umask_keeps_the_writable_descriptor --nocapture", "sh"])
            .arg(std::env::current_exe().unwrap())
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new();
    // Directories need write access even though newly created files do not.
    fs::set_permissions(dir.path(), fs::Permissions::from_mode(0o700)).unwrap();
    let storage = FileStorage::open(dir.path()).unwrap();
    let mut failures = Vec::new();
    for (name, named, protected_mode, expected_mode) in [
        ("path", false, None, 0o400),
        ("protected-path", false, Some(0o444), 0o400),
        ("protected-writable-path", false, Some(0o640), 0o400),
        ("named-append", true, None, 0o444),
        ("named-commit", true, None, 0o444),
    ] {
        let path = dir.path().join(name);
        let protected = protected_mode.map(|mode| {
            let base = dir.path().join(format!("{name}-base"));
            fs::write(&base, b"base").unwrap();
            fs::set_permissions(&base, fs::Permissions::from_mode(mode)).unwrap();
            fs::metadata(base).unwrap()
        });
        let mut record: Box<dyn Record> = if named {
            storage.open(name).unwrap()
        } else {
            Box::new(crate::FileRecord::open(&path, b"header", protected.as_ref()).unwrap())
        };
        for publication in 0..2 {
            if name == "named-append" && publication == 0 {
                record.append(b"header").unwrap();
            } else {
                let mut replacement = record.replace().unwrap();
                replacement.append(b"header").unwrap();
                replacement.commit().unwrap();
            }
            if let Err(error) = record.append(b"tail") {
                failures.push(format!("{name}: {error}"));
                break;
            }
            record.sync().unwrap();
            assert_eq!(read_all(record.as_mut()), b"headertail", "{name}");
            assert_eq!(fs::read(&path).unwrap(), b"headertail", "{name}");
            assert_eq!(
                fs::metadata(&path).unwrap().permissions().mode() & 0o777,
                expected_mode,
                "{name}"
            );
        }
        if let Some(mode) = protected_mode {
            let base = dir.path().join(format!("{name}-base"));
            assert_eq!(fs::read(&base).unwrap(), b"base");
            assert_eq!(
                fs::metadata(base).unwrap().permissions().mode() & 0o777,
                mode
            );
        }
    }
    assert!(failures.is_empty(), "{}", failures.join("\n"));
}

#[cfg(unix)]
#[test]
fn file_storage_replacements_use_the_creation_umask() {
    const CHILD: &str = "UNEMU_CORE_FILES_TEST_UMASK_CHILD";
    if std::env::var_os(CHILD).is_none() {
        // Set the umask in a separate process so parallel tests keep theirs.
        let output = Command::new("/bin/sh")
            .args(["-c", "umask 022; exec \"$1\" --exact tests::file_storage_replacements_use_the_creation_umask --nocapture", "sh"])
            .arg(std::env::current_exe().unwrap())
            .env(CHILD, "1")
            .output()
            .unwrap();
        assert!(
            output.status.success(),
            "{}{}",
            String::from_utf8_lossy(&output.stdout),
            String::from_utf8_lossy(&output.stderr)
        );
        return;
    }
    use std::os::unix::fs::PermissionsExt;
    let dir = TempDir::new();
    let storage = FileStorage::open(dir.path()).unwrap();
    let path = dir.path().join("record");
    let mut record = storage.open("record").unwrap();
    for exists in [false, true] {
        if exists {
            fs::set_permissions(&path, fs::Permissions::from_mode(0o600)).unwrap();
        }
        let mut replacement = record.replace().unwrap();
        assert_eq!(
            fs::metadata(dir.path().join("record_0.tmp"))
                .unwrap()
                .permissions()
                .mode()
                & 0o777,
            0o644
        );
        replacement.append(b"replacement").unwrap();
        replacement.commit().unwrap();
        assert_eq!(
            fs::metadata(&path).unwrap().permissions().mode() & 0o777,
            0o644
        );
    }
}

#[test]
fn typed_file_stages_preserve_display_source_and_error_classification() {
    use crate::{FileError, FileStage};
    for (stage, text) in [
        (FileStage::ResolvePath, "resolve path"),
        (FileStage::Lock, "lock"),
        (FileStage::Open, "open"),
        (FileStage::Metadata, "metadata"),
        (FileStage::VerifyPath, "verify path"),
        (FileStage::Cleanup, "cleanup"),
        (FileStage::DirectorySync, "directory sync"),
    ] {
        let error = FileError::new(stage, std::io::Error::from_raw_os_error(13));
        assert_eq!(error.stage, stage);
        assert_eq!(stage.to_string(), text);
        assert_eq!(error.source.raw_os_error(), Some(13));
        assert!(std::error::Error::source(&error).is_some());
        assert_eq!(unemu_core_api::Error::from(error).kind, ErrorKind::Storage);
    }
    for (stage, host_kind, expected) in [
        (
            FileStage::ResolvePath,
            std::io::ErrorKind::InvalidInput,
            ErrorKind::Usage,
        ),
        (
            FileStage::Lock,
            std::io::ErrorKind::WouldBlock,
            ErrorKind::Usage,
        ),
        (
            FileStage::Open,
            std::io::ErrorKind::InvalidInput,
            ErrorKind::Storage,
        ),
    ] {
        let error = FileError::new(stage, std::io::Error::new(host_kind, "host failure"));
        assert_eq!(unemu_core_api::Error::from(error).kind, expected);
    }
}

#[test]
fn failed_reads_of_records_and_replacements_can_fill_a_prefix() {
    let dir = TempDir::new();
    let path = dir.path().join("record");
    fs::write(&path, b"data").unwrap();
    let host = ProbedFs::default();
    let mut record =
        crate::FileRecord::open_with(Rc::new(host.clone()), &path, None, b"", None).unwrap();
    host.0.fail("record read");
    let mut bytes = *b"????";
    assert_eq!(kind(record.read_at(0, &mut bytes)), ErrorKind::Storage);
    assert_eq!(&bytes, b"da??");
    record.read_at(0, &mut bytes).unwrap();
    assert_eq!(&bytes, b"data");
    let mut replacement = record.replace().unwrap();
    replacement.append(b"next").unwrap();
    host.0.fail("retention read");
    bytes = *b"????";
    assert_eq!(kind(replacement.read_at(0, &mut bytes)), ErrorKind::Storage);
    assert_eq!(&bytes, b"ne??");
    assert_eq!(read_all(&mut record), b"data");
    replacement.commit().unwrap();
    assert_eq!(read_all(&mut record), b"next");
}

#[test]
fn failed_publication_preserves_an_unsynchronized_parent_tail_after_power_cut() {
    let dir = TempDir::new();
    let path = dir.path().join("record");
    fs::write(&path, b"old").unwrap();
    let fs = ProbedFs::default();
    let mut record = path_record(&fs, &path);
    record.sync().unwrap();
    let mut replacement = record.replace().unwrap();
    record.append(b"tail").unwrap();
    assert_eq!(
        fs.0.durable_record.borrow().as_deref(),
        Some(b"old".as_slice())
    );
    replacement.append(b"new").unwrap();
    fs.0.events.borrow_mut().clear();
    fs.0.fail("rename");
    assert_eq!(kind(replacement.commit()), ErrorKind::StorageUncertain);
    assert_eq!(read_all(&mut record), b"oldtail");
    drop(record);
    // Model loss of writes made after the last successful file barrier.
    fs::write(&path, fs.0.durable_record.borrow().as_ref().unwrap()).unwrap();
    let mut record = path_record(&fs, &path);
    assert_eq!(read_all(&mut record), b"oldtail");
    let events = fs.0.events.borrow();
    let flush = events.iter().position(|e| e == "record flush").unwrap();
    let sync = events.iter().position(|e| e == "record sync").unwrap();
    let rename = events.iter().position(|e| e == "rename").unwrap();
    assert!(flush < sync && sync < rename, "{events:?}");
}

#[test]
fn a_failed_parent_barrier_blocks_file_publication() {
    for step in ["record flush", "record sync"] {
        let dir = TempDir::new();
        let path = dir.path().join("record");
        fs::write(&path, b"old").unwrap();
        let fs = ProbedFs::default();
        let mut record = path_record(&fs, &path);
        record.append(b"tail").unwrap();
        let mut replacement = record.replace().unwrap();
        replacement.append(b"new").unwrap();
        fs.0.events.borrow_mut().clear();
        fs.0.fail(step);
        assert_eq!(
            kind(replacement.commit()),
            ErrorKind::StorageUncertain,
            "{step}"
        );
        assert_eq!(read_all(&mut record), b"oldtail");
        assert_eq!(kind(record.append(b"!")), ErrorKind::Usage);
        assert!(!fs.0.events.borrow().iter().any(|e| e == "rename"));
        assert!(!dir.files().iter().any(|name| name.contains(".retain-")));
    }
}

#[test]
fn a_failed_parent_name_barrier_blocks_named_publication() {
    let dir = TempDir::new();
    fs::write(dir.path().join("record"), b"old").unwrap();
    let storage = FileStorage::open(dir.path()).unwrap();
    let mut record = storage.open("record").unwrap();
    record.append(b"tail").unwrap();
    let mut replacement = record.replace().unwrap();
    replacement.append(b"new").unwrap();
    storage.fail_next("record", Step::SyncDirectory);
    assert_eq!(kind(replacement.commit()), ErrorKind::StorageUncertain);
    assert_eq!(read_all(&mut *record), b"oldtail");
    assert_eq!(kind(record.append(b"!")), ErrorKind::Usage);
    assert_eq!(fs::read(dir.path().join("record")).unwrap(), b"oldtail");
}

#[cfg(unix)]
#[test]
fn named_record_symlink_aliases_cannot_open_a_second_writer() {
    let dir = TempDir::new();
    fs::write(dir.path().join("record"), b"old").unwrap();
    std::os::unix::fs::symlink("record", dir.path().join("alias")).unwrap();
    let storage = FileStorage::open(dir.path()).unwrap();
    let mut record = storage.open("record").unwrap();
    assert_eq!(kind(storage.open("alias")), ErrorKind::Usage);
    record.append(b"tail").unwrap();
    assert_eq!(read_all(&mut *record), b"oldtail");
    drop(record);
    assert_eq!(kind(storage.open("alias")), ErrorKind::Usage);
    assert!(fs::symlink_metadata(dir.path().join("alias"))
        .unwrap()
        .is_symlink());
}

#[cfg(unix)]
#[test]
fn named_record_dangling_symlinks_are_rejected_before_append() {
    let dir = TempDir::new();
    std::os::unix::fs::symlink("record", dir.path().join("alias")).unwrap();
    let storage = FileStorage::open(dir.path()).unwrap();
    let mut record = storage.open("record").unwrap();
    assert_eq!(kind(storage.open("alias")), ErrorKind::Usage);
    assert!(!dir.path().join("record").exists());
    record.append(b"new").unwrap();
    assert_eq!(kind(storage.open("alias")), ErrorKind::Usage);
    assert_eq!(fs::read(dir.path().join("record")).unwrap(), b"new");
}

#[test]
fn ordinary_reopen_keeps_unsynchronized_appends_and_destructive_cuts() {
    let dir = TempDir::new();
    let (storage, mut record) = record_with_old(&dir);
    record.append(b"tail").unwrap();
    drop(record);
    let mut record = storage.open("r").unwrap();
    assert_eq!(read_all(&mut *record), b"oldtail");
    record.truncate(1).unwrap();
    drop(record);
    let mut record = storage.open("r").unwrap();
    assert_eq!(read_all(&mut *record), b"o");
}

#[test]
fn an_uncertain_cut_retries_an_unknown_length_before_reading() {
    let dir = TempDir::new();
    let path = dir.path().join("record");
    fs::write(&path, b"data").unwrap();
    let host = ProbedFs::default();
    let mut record =
        crate::FileRecord::open_with(Rc::new(host.clone()), &path, None, b"", None).unwrap();
    host.0.fail("record cut after");
    host.0.fail("record metadata");
    assert_eq!(kind(record.truncate(1)), ErrorKind::StorageUncertain);
    assert_eq!(read_all(&mut record), b"d");
    assert_writes_need_reopening(&mut record);
}

#[test]
fn a_rolled_back_replacement_append_can_commit_whole_bytes() {
    let dir = TempDir::new();
    let (storage, mut record) = record_with_old(&dir);
    let mut replacement = record.replace().unwrap();
    replacement.append(b"ab").unwrap();
    storage.fail_next("r", Step::ReplacementWrite);
    assert_eq!(kind(replacement.append(b"cdef")), ErrorKind::Storage);
    let mut bytes = [0; 2];
    replacement.read_at(0, &mut bytes).unwrap();
    assert_eq!(&bytes, b"ab");
    replacement.append(b"!").unwrap();
    replacement.commit().unwrap();
    assert_eq!(read_all(&mut *record), b"ab!");
    drop(record);
    assert_eq!(read_all(&mut *storage.open("r").unwrap()), b"ab!");
}
