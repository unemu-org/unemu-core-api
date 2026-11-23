// SPDX-License-Identifier: MIT OR Apache-2.0
//! Compile public constructors from outside the library's privacy boundary.
use std::rc::Rc;
use unemu_core_files::*;

#[test]
fn public_host_and_error_constructors() {
    let _host: Rc<dyn Filesystem> = Rc::new(HostFilesystem::new());
    let _ = HostFilesystem::default();
    let error = FileError::new(FileStage::Open, std::io::Error::other("open failed"));
    assert_eq!(error.stage, FileStage::Open);
    assert_eq!(error.to_string(), "open: open failed");
}

#[test]
fn opaque_file_types_have_public_factories() {
    // Type-check without creating files or requiring a particular host path.
    let _construct = |path: &std::path::Path, file: std::fs::File| {
        let _ = FileContent::open(path, 0);
        let _ = FileContent::from_file(file, 0);
        let _ = FileStorage::open(path);
        let _ = PathLock::exclusive(path);
        let _ = PathLock::inspection(path);
        let _ = PathLock::acquire(&HostFilesystem::new(), path, false);
        let _ = FileRecord::open(path, b"", None);
        let _ = FileRecord::inspect(path);
        let _ = FileRecord::open_with(Rc::new(HostFilesystem::new()), path, None, b"", None);
    };
}
