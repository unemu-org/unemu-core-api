// SPDX-License-Identifier: MIT OR Apache-2.0
//! Content normalization exercised through the public reader contract.
use std::cell::Cell;
use std::rc::Rc;
use unemu_core_api::{Content, ContentReader, Error, ErrorKind};

struct Reader {
    reads: Rc<Cell<usize>>,
    failure: Option<ErrorKind>,
}

impl ContentReader for Reader {
    fn size(&self) -> u64 {
        3
    }

    fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), Error> {
        self.reads.set(self.reads.get() + 1);
        if let Some(kind) = self.failure {
            buf.fill(0);
            return Err(Error::new(kind, "read failed"));
        }
        assert_eq!(offset, 1);
        buf.copy_from_slice(b"bc");
        Ok(())
    }
}

fn tail(reader: &dyn ContentReader) -> Result<[u8; 2], Error> {
    assert_eq!(reader.size(), 3);
    let mut bytes = [0; 2];
    reader.read_at(1, &mut bytes)?;
    Ok(bytes)
}

#[test]
fn bytes_and_reader_use_the_same_public_reading_path() {
    let bytes = Content::Bytes(b"abc".to_vec());
    let reader = Content::Reader(Box::new(Reader {
        reads: Rc::new(Cell::new(0)),
        failure: None,
    }));
    assert_eq!(tail(&bytes).unwrap(), *b"bc");
    assert_eq!(tail(&reader).unwrap(), *b"bc");
}

#[test]
fn byte_reads_reject_overflow_and_out_of_bounds_without_changing_the_buffer() {
    let bytes = Content::Bytes(b"abc".to_vec());
    let mut buf = [9; 2];
    for offset in [2, 4, u64::MAX] {
        assert_eq!(
            bytes.read_at(offset, &mut buf).unwrap_err().kind,
            ErrorKind::Usage
        );
        assert_eq!(buf, [9; 2]);
    }
    bytes.read_at(3, &mut []).unwrap();
    assert_eq!(
        bytes.read_at(4, &mut []).unwrap_err().kind,
        ErrorKind::Usage
    );
    assert_eq!(Content::Bytes(vec![]).size(), 0);
}

#[test]
fn normalization_does_not_read_or_rehash_and_preserves_reader_failures() {
    for kind in [ErrorKind::Usage, ErrorKind::Content, ErrorKind::Storage] {
        let reads = Rc::new(Cell::new(0));
        let content = Content::Reader(Box::new(Reader {
            reads: Rc::clone(&reads),
            failure: Some(kind),
        }));
        assert_eq!(content.size(), 3);
        assert_eq!(reads.get(), 0);
        let mut buf = [9; 2];
        assert_eq!(content.read_at(1, &mut buf).unwrap_err().kind, kind);
        assert_eq!(buf, [0; 2]);
        assert_eq!(reads.get(), 1);
    }
}
