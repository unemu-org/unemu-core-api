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
use std::process::{Command, Stdio};
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

fn kind<T>(result: Result<T, unemu_core_api::Error>) -> ErrorKind {
    match result {
        Ok(_) => panic!("expected a failure"),
        Err(error) => error.kind,
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
    assert_eq!(storage.dir(), nested.as_path());
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
    let mut out = std::io::stdout();
    writeln!(out, "holding").expect("report");
    out.flush().expect("flush");
    let mut rest = String::new();
    let _ = std::io::Read::read_to_string(&mut std::io::stdin(), &mut rest);
}

#[test]
fn a_record_held_by_another_process_does_not_open() {
    let dir = TempDir::new();
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

    let storage = FileStorage::open(dir.path()).expect("storage");
    assert_eq!(kind(storage.open("shared")), ErrorKind::Usage);
    storage.open("other").expect("another record opens");

    drop(child.stdin.take());
    // Read the child's output to its end, so its last lines never meet a
    // closed pipe.
    lines.for_each(drop);
    assert!(child.wait().expect("the child ends").success());
    storage.open("shared").expect("open after the child ended");
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
    assert_eq!(read_all(&mut *record), b"old", "reads still work");
    assert_writes_need_reopening(&mut *record);
    drop(record);
    let mut record = storage.open("r").expect("open again");
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
    // The name is synchronised once: a sync after it touches the file only.
    let mut record = storage.open("r").expect("open again");
    record.append(b"!").expect("append");
    storage.fail_next("r", Step::SyncDirectory);
    record
        .sync()
        .expect("an existing record's sync leaves the directory");
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
    assert_eq!(kind(replacement.commit()), ErrorKind::Usage);
    assert_eq!(read_all(&mut *record), b"old");
    assert_writes_need_reopening(&mut *record);
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
