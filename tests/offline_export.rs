// SPDX-License-Identifier: MIT OR Apache-2.0
//! Consumer checks for offline operations through trait objects.
#![cfg(feature = "test-support")]

use unemu_core_api::test_support::{
    ExportOperation, MemoryContent, MemoryExportSink, ScriptedCore,
};
use unemu_core_api::*;

fn config(bytes: &[u8]) -> ConfigDocument<'_> {
    ConfigDocument::new("text/plain", bytes)
}
fn kind<T>(result: Result<T, Error>) -> ErrorKind {
    result.err().expect("expected failure").kind
}
fn export(format: &str, bytes: &[u8]) -> (ExportReport, MemoryExportSink) {
    let core: &dyn Core = &ScriptedCore;
    let reader = MemoryContent::new(bytes.to_vec());
    let slots = [ExportMedium::new(SlotId(0), bytes.len() as u64, &reader)];
    let mut sink = MemoryExportSink::new();
    let report = core
        .export(
            "scripted",
            config(b"variable"),
            ExportRequest::new(format, &slots),
            &mut sink,
        )
        .unwrap();
    (report, sink)
}

#[test]
fn descriptions_are_config_only_and_new_fields_have_empty_defaults() {
    let slot = SlotDescription::new(SlotId(0), SlotKind::Floppy, "Media", true);
    let media = MediaDescription::new(slot.clone());
    assert_eq!(media.slot, slot);
    assert_eq!(media.detail, "");
    assert_eq!(media.size, None);
    assert!(PlatformDescription::new("old", "Old")
        .export_formats
        .is_empty());
    let core: &dyn Core = &ScriptedCore;
    for (document, size) in [
        (b"fixed".as_slice(), Some(3)),
        (b"variable".as_slice(), None),
    ] {
        core.validate("scripted", config(document)).unwrap();
        let descriptions = core.describe_media("scripted", config(document)).unwrap();
        assert_eq!(descriptions.len(), 1);
        assert_eq!(
            descriptions[0].slot,
            slot.clone().with_formats(vec!["raw".into()])
        );
        assert_eq!(descriptions[0].size, size);
        assert!(!descriptions[0].detail.is_empty());
    }
    for (platform, document, expected) in [
        ("foreign", b"bad".as_slice(), ErrorKind::Unsupported),
        ("scripted", b"bad".as_slice(), ErrorKind::Config),
    ] {
        assert_eq!(kind(core.validate(platform, config(document))), expected);
        assert_eq!(
            kind(core.describe_media(platform, config(document))),
            expected
        );
        assert_eq!(
            kind(core.export(
                platform,
                config(document),
                ExportRequest::new("raw", &[]),
                &mut MemoryExportSink::new()
            )),
            expected
        );
    }
    assert_eq!(
        kind(core.describe_media("scripted", ConfigDocument::new("unknown", b"bad"))),
        ErrorKind::Unsupported
    );
}

#[test]
fn export_names_are_portable_bounded_and_never_overwrite() {
    for name in ["a", "disk-0.raw", "con", &"a".repeat(64)] {
        assert!(is_export_name(name), "{name:?}");
        let mut sink = MemoryExportSink::new();
        let mut observer = sink.clone();
        let output = sink.create(name).unwrap();
        assert_eq!(kind(observer.create(name)), ErrorKind::Usage);
        output.write(b"abc").unwrap();
        let receipt = output.finish().unwrap();
        assert_eq!(receipt.name, name);
        assert_eq!(receipt.size, 3);
        assert_eq!(sink.outputs()[name], b"abc");
        assert_eq!(kind(sink.create(name)), ErrorKind::Usage);
    }
    for name in [
        "",
        ".",
        "..",
        "a..b",
        "../a",
        "a/b",
        "a\\b",
        "/a",
        "c:a",
        "C:\\a",
        "a_b",
        "A",
        "é",
        "a b",
        "a\0b",
        "-a",
        "a-",
        ".a",
        "a.",
        &"a".repeat(65),
    ] {
        assert!(!is_export_name(name), "{name:?}");
        assert_eq!(kind(MemoryExportSink::new().create(name)), ErrorKind::Usage);
    }
    let mut sink = MemoryExportSink::new();
    let _ = sink.create("partial").unwrap();
    assert_eq!(kind(sink.create("partial")), ErrorKind::Usage);
}

#[test]
fn advertised_formats_are_const_validated_and_unique() {
    const RAW: ExportFormat = ExportFormat::new("raw", "Raw bytes", "raw", ExportScope::Slot);
    const PLATFORM: PlatformDescription =
        PlatformDescription::new("test", "Test").with_export_formats(&[RAW]);
    assert_eq!(PLATFORM.export_formats, &[RAW]);
    assert!(RAW.is_valid());
    for id in ["", "-raw", "Raw", "a.b", &"a".repeat(65)] {
        assert!(std::panic::catch_unwind(|| ExportFormat::new(
            Box::leak(id.to_owned().into_boxed_str()),
            "Raw",
            "raw",
            ExportScope::Slot
        ))
        .is_err());
    }
    for extension in ["", ".raw", "RAW", "a-b", &"a".repeat(17)] {
        assert!(std::panic::catch_unwind(|| ExportFormat::new(
            "raw",
            "Raw",
            Box::leak(extension.to_owned().into_boxed_str()),
            ExportScope::Slot
        ))
        .is_err());
    }
    assert!(
        std::panic::catch_unwind(|| ExportFormat::new("raw", "", "raw", ExportScope::Slot))
            .is_err()
    );
    assert!(std::panic::catch_unwind(
        || PlatformDescription::new("test", "Test").with_export_formats(&[RAW, RAW])
    )
    .is_err());
    let mut invalid = RAW;
    invalid.id = "bad.id";
    assert!(!invalid.is_valid());
    let invalid = Box::leak(Box::new([invalid]));
    assert!(std::panic::catch_unwind(
        || PlatformDescription::new("test", "Test").with_export_formats(invalid)
    )
    .is_err());
}

#[test]
fn scripted_exports_are_deterministic_stream_current_bytes_and_use_sink_names() {
    let core: &dyn Core = &ScriptedCore;
    assert_eq!(core.description().platforms[0].export_formats.len(), 2);
    for format in core.description().platforms[0].export_formats {
        let (first, sink) = export(format.id, b"abc");
        let (second, again) = export(format.id, b"abc");
        assert_eq!(first, second);
        assert_eq!(sink.outputs(), again.outputs());
        assert_eq!(sink.outputs()["slot-0.raw"], b"abc");
        assert_eq!(first.outputs.len(), sink.outputs().len());
        for output in &first.outputs {
            assert_eq!(output.size, sink.outputs()[&output.name].len() as u64);
        }
        if format.scope == ExportScope::Machine {
            assert_eq!(
                sink.outputs()["machine.txt"],
                b"slot=slot-0.raw\nlength=variable\nremovable=true\n"
            );
            assert!(first.notes.is_empty());
        } else {
            assert!(!first.notes.is_empty());
        }
    }
    assert_eq!(
        export("raw", b"changed").1.outputs()["slot-0.raw"],
        b"changed"
    );
}

#[test]
fn sink_hashes_all_bytes_with_known_sha256_vectors() {
    for (bytes, expected) in [
        (
            b"".as_slice(),
            "e3b0c44298fc1c149afbf4c8996fb92427ae41e4649b934ca495991b7852b855",
        ),
        (
            b"abc",
            "ba7816bf8f01cfea414140de5dae2223b00361a396177a9cb410ff61f20015ad",
        ),
        (
            b"abcdbcdecdefdefgefghfghighijhijkijkljklmklmnlmnomnopnopq",
            "248d6a61d20638b8e5c026930c3e6039a33ce45964ff2167f6ecedd419db06c1",
        ),
    ] {
        let report = export("raw", bytes).0;
        let hex: String = report.outputs[0]
            .sha256
            .iter()
            .map(|b| format!("{b:02x}"))
            .collect();
        assert_eq!(hex, expected);
    }
    let report = export("raw", &vec![b'a'; 1_000_000]).0;
    let hex: String = report.outputs[0]
        .sha256
        .iter()
        .map(|b| format!("{b:02x}"))
        .collect();
    assert_eq!(
        hex,
        "cdc76e5c9914fb9281a1c7e284d73e67f1809a48a497200e046d39ccc7112cd0"
    );
}

#[test]
fn export_rejects_bad_requests_before_creating_outputs() {
    let core: &dyn Core = &ScriptedCore;
    let reader = MemoryContent::new(b"abc".to_vec());
    let good = ExportMedium::new(SlotId(0), 3, &reader);
    let unknown = ExportMedium::new(SlotId(99), 3, &reader);
    let mismatch = ExportMedium::new(SlotId(0), 4, &reader);
    for (format, slots, expected) in [
        ("unknown", vec![good], ErrorKind::Usage),
        ("raw", vec![], ErrorKind::Usage),
        ("raw", vec![good, good], ErrorKind::Usage),
        ("raw", vec![unknown], ErrorKind::Usage),
        ("machine", vec![], ErrorKind::Usage),
        ("machine", vec![good, good], ErrorKind::Usage),
        ("machine", vec![unknown], ErrorKind::Usage),
        ("raw", vec![mismatch], ErrorKind::Content),
    ] {
        let mut sink = MemoryExportSink::new();
        assert_eq!(
            kind(core.export(
                "scripted",
                config(b"fixed"),
                ExportRequest::new(format, &slots),
                &mut sink
            )),
            expected
        );
        assert!(sink.outputs().is_empty());
    }
    for size in [2, 4] {
        let slots = [ExportMedium::new(SlotId(0), size, &reader)];
        let mut sink = MemoryExportSink::new();
        assert_eq!(
            kind(core.export(
                "scripted",
                config(b"variable"),
                ExportRequest::new("raw", &slots),
                &mut sink
            )),
            ErrorKind::Content
        );
        assert!(sink.outputs().is_empty());
    }
    let short = MemoryContent::new(vec![0; 2]);
    let slots = [ExportMedium::new(SlotId(0), 2, &short)];
    assert_eq!(
        kind(core.export(
            "scripted",
            config(b"fixed"),
            ExportRequest::new("raw", &slots),
            &mut MemoryExportSink::new()
        )),
        ErrorKind::Content
    );
}

#[test]
fn export_propagates_reader_and_sink_failures_without_a_success_report() {
    let core: &dyn Core = &ScriptedCore;
    for failure in [ErrorKind::Content, ErrorKind::Storage] {
        let reader = MemoryContent::new(b"abc".to_vec());
        reader.fail_next(Error::new(failure, "read failed"));
        let slots = [ExportMedium::new(SlotId(0), 3, &reader)];
        assert_eq!(
            kind(core.export(
                "scripted",
                config(b"fixed"),
                ExportRequest::new("raw", &slots),
                &mut MemoryExportSink::new()
            )),
            failure
        );
    }
    for operation in [
        ExportOperation::Create,
        ExportOperation::Write,
        ExportOperation::Finish,
    ] {
        let mut sink = MemoryExportSink::new();
        sink.fail_next(operation);
        let reader = MemoryContent::new(b"abc".to_vec());
        let slots = [ExportMedium::new(SlotId(0), 3, &reader)];
        assert_eq!(
            kind(core.export(
                "scripted",
                config(b"fixed"),
                ExportRequest::new("machine", &slots),
                &mut sink
            )),
            ErrorKind::Storage
        );
    }
}

#[test]
fn streaming_outputs_append_in_order_and_failed_outputs_cannot_finish() {
    let mut sink = MemoryExportSink::new();
    let mut other = sink.clone();
    let first = sink.create("first.raw").unwrap();
    let second = other.create("second.raw").unwrap();
    first.write(b"a").unwrap();
    second.write(b"abc").unwrap();
    first.write(b"").unwrap();
    first.write(b"bc").unwrap();
    let first = first.finish().unwrap();
    let second = second.finish().unwrap();
    assert_eq!(first.size, second.size);
    assert_eq!(first.sha256, second.sha256);
    assert_eq!(sink.outputs()["first.raw"], b"abc");
    let control = sink.clone();
    let output = sink.create("failed.raw").unwrap();
    output.write(b"prefix").unwrap();
    control.fail_next(ExportOperation::Write);
    assert_eq!(kind(output.write(b"tail")), ErrorKind::Storage);
    assert_eq!(kind(output.write(b"retry")), ErrorKind::Storage);
    assert_eq!(kind(output.finish()), ErrorKind::Storage);
    assert_eq!(sink.outputs()["failed.raw"], b"prefix");
    // Once consumed, the planted fault does not affect another output.
    sink.create("after.raw").unwrap().finish().unwrap();
}

#[test]
fn failed_reads_never_emit_partial_buffers_or_return_partial_success() {
    struct Partial;
    impl ContentReader for Partial {
        fn size(&self) -> u64 {
            5000
        }
        fn read_at(&self, offset: u64, buf: &mut [u8]) -> Result<(), Error> {
            buf.fill(7);
            if offset == 0 {
                Ok(())
            } else {
                Err(Error::new(ErrorKind::Content, "changed during read"))
            }
        }
    }
    let slots = [ExportMedium::new(SlotId(0), 5000, &Partial)];
    let mut sink = MemoryExportSink::new();
    assert_eq!(
        kind(ScriptedCore.export(
            "scripted",
            config(b"variable"),
            ExportRequest::new("raw", &slots),
            &mut sink
        )),
        ErrorKind::Content
    );
    assert_eq!(sink.outputs()["slot-0.raw"], vec![7; 4096]);
    assert_eq!(kind(sink.create("slot-0.raw")), ErrorKind::Usage);
}

#[test]
fn failed_machine_document_leaves_finished_slot_for_frontend_cleanup() {
    struct FailDocument(MemoryExportSink);
    impl ExportSink for FailDocument {
        fn create(&mut self, name: &str) -> Result<&mut (dyn ExportOutput + '_), Error> {
            if name == "machine.txt" {
                self.0.fail_next(ExportOperation::Finish);
            }
            self.0.create(name)
        }
    }
    let reader = MemoryContent::new(b"abc".to_vec());
    let slots = [ExportMedium::new(SlotId(0), 3, &reader)];
    let mut sink = FailDocument(MemoryExportSink::new());
    assert_eq!(
        kind(ScriptedCore.export(
            "scripted",
            config(b"fixed"),
            ExportRequest::new("machine", &slots),
            &mut sink
        )),
        ErrorKind::Storage
    );
    assert_eq!(sink.0.outputs()["slot-0.raw"], b"abc");
    assert_eq!(
        sink.0.outputs()["machine.txt"],
        b"slot=slot-0.raw\nlength=fixed\nremovable=true\n"
    );
    // The front end owns these newly created outputs and discards them on error.
    drop(sink);
}

#[test]
fn existing_sink_outputs_and_live_machines_are_preserved() {
    use std::rc::Rc;
    use unemu_core_api::test_support::MemoryStorage;
    struct NoContent;
    impl ContentSource for NoContent {
        fn open(&self, _: &[u8; 32], _: u64) -> Result<Content, Error> {
            panic!("the scripted config needs no content")
        }
    }
    let core: &dyn Core = &ScriptedCore;
    let mut machine = core
        .create(&CreateRequest::new(
            "scripted",
            config(b"fixed"),
            &NoContent,
            Rc::new(MemoryStorage::new()),
        ))
        .unwrap();
    let descriptions = core.describe_media("scripted", config(b"fixed")).unwrap();
    assert_eq!(descriptions[0].slot, machine.description().slots[0]);
    let reader = MemoryContent::new(b"abc".to_vec());
    let slots = [ExportMedium::new(SlotId(0), 3, &reader)];
    let mut sink = MemoryExportSink::new();
    let output = sink.create("slot-0.raw").unwrap();
    output.write(b"preexisting").unwrap();
    output.finish().unwrap();
    for terminal in [false, true] {
        if terminal {
            machine.finish().unwrap();
        }
        let before = (machine.status(), machine.now(), machine.steps());
        assert_eq!(
            kind(core.export(
                "scripted",
                config(b"fixed"),
                ExportRequest::new("raw", &slots),
                &mut sink
            )),
            ErrorKind::Usage
        );
        assert_eq!(sink.outputs()["slot-0.raw"], b"preexisting");
        core.export(
            "scripted",
            config(b"fixed"),
            ExportRequest::new("machine", &slots),
            &mut MemoryExportSink::new(),
        )
        .unwrap();
        assert_eq!((machine.status(), machine.now(), machine.steps()), before);
    }
    let mut invalid = core.description().platforms[0].export_formats[0];
    invalid.name = "";
    assert!(!invalid.is_valid());
    invalid.name = "Raw";
    invalid.extension = ".raw";
    assert!(!invalid.is_valid());
    assert_eq!(
        MediaDescription::new(descriptions[0].slot.clone())
            .with_size(Some(0))
            .size,
        Some(0)
    );
}

#[test]
fn finished_and_failed_outputs_reject_further_writes_and_finish() {
    for fail_finish in [false, true] {
        let mut sink = MemoryExportSink::new();
        let control = sink.clone();
        let output = sink.create("closed.raw").unwrap();
        output.write(b"abc").unwrap();
        if fail_finish {
            control.fail_next(ExportOperation::Finish);
            assert_eq!(kind(output.finish()), ErrorKind::Storage);
        } else {
            assert_eq!(output.finish().unwrap().size, 3);
        }
        assert_eq!(kind(output.write(b"late")), ErrorKind::Storage);
        assert_eq!(kind(output.finish()), ErrorKind::Storage);
        assert_eq!(sink.outputs()["closed.raw"], b"abc");
    }
}

#[test]
fn sink_close_revokes_unfinished_outputs_and_preserves_bytes() {
    let mut sink = MemoryExportSink::new();
    let mut control = sink.clone();
    let output = sink.create("partial.raw").unwrap();
    output.write(b"prefix").unwrap();
    control.close();
    control.close();
    assert_eq!(kind(output.write(b"late")), ErrorKind::Storage);
    assert_eq!(kind(output.finish()), ErrorKind::Storage);
    assert_eq!(sink.outputs()["partial.raw"], b"prefix");
}

#[test]
fn export_slot_queries_validate_config_and_format_through_dyn_core() {
    let core: &dyn Core = &ScriptedCore;
    for format in core.description().platforms[0].export_formats {
        assert_eq!(
            core.export_slots("scripted", config(b"fixed"), format.id)
                .unwrap(),
            vec![SlotId(0)]
        );
    }
    for (platform, document, format, expected) in [
        ("unknown", config(b"bad"), "unknown", ErrorKind::Unsupported),
        (
            "scripted",
            ConfigDocument::new("unknown", b"bad"),
            "raw",
            ErrorKind::Unsupported,
        ),
        ("scripted", config(b"bad"), "raw", ErrorKind::Config),
        ("scripted", config(b"fixed"), "unknown", ErrorKind::Usage),
    ] {
        assert_eq!(
            kind(core.export_slots(platform, document, format)),
            expected
        );
    }
}

// This offline-only consumer fixture has more configured slots than either
// machine format needs, so a front end cannot infer requests from the listing.
struct SubsetCore;

impl Core for SubsetCore {
    fn description(&self) -> &CoreDescription {
        const DESCRIPTION: CoreDescription =
            CoreDescription::new(ContractVersion::new(0, 6), "subset", "Subset core", "1.0.0")
                .with_platforms(&[PlatformDescription::new("subset", "Subset")
                    .with_export_formats(&[
                        ExportFormat::new("pair", "Pair", "txt", ExportScope::Machine),
                        ExportFormat::new("single", "Single", "txt", ExportScope::Machine),
                        ExportFormat::new("raw", "Raw", "raw", ExportScope::Slot),
                    ])]);
        &DESCRIPTION
    }

    fn create(&self, _: &CreateRequest<'_>) -> Result<Box<dyn Machine>, Error> {
        Err(Error::new(ErrorKind::Unsupported, "offline-only fixture"))
    }

    fn describe_media(
        &self,
        _: &str,
        _: ConfigDocument<'_>,
    ) -> Result<Vec<MediaDescription>, Error> {
        Ok((0..3)
            .map(|id| {
                MediaDescription::new(SlotDescription::new(
                    SlotId(id),
                    SlotKind::Floppy,
                    "Media",
                    true,
                ))
                .with_size(Some(3))
            })
            .collect())
    }

    fn export_slots(
        &self,
        _: &str,
        _: ConfigDocument<'_>,
        format: &str,
    ) -> Result<Vec<SlotId>, Error> {
        match format {
            "pair" => Ok(vec![SlotId(0), SlotId(2)]),
            "single" => Ok(vec![SlotId(1)]),
            "raw" => Ok(vec![SlotId(0), SlotId(1), SlotId(2)]),
            _ => Err(Error::new(ErrorKind::Usage, "unknown format")),
        }
    }

    fn export(
        &self,
        platform: &str,
        config: ConfigDocument<'_>,
        request: ExportRequest<'_>,
        sink: &mut dyn ExportSink,
    ) -> Result<ExportReport, Error> {
        let eligible = self.export_slots(platform, config, request.format)?;
        let mut slots: Vec<_> = request.media.iter().map(|medium| medium.slot).collect();
        slots.sort_by_key(|slot| slot.0);
        let valid = if request.format == "raw" {
            slots.len() == 1 && eligible.contains(&slots[0])
        } else {
            slots == eligible
        };
        if !valid {
            return Err(Error::new(ErrorKind::Usage, "incorrect slot set"));
        }
        for medium in request.media {
            if medium.size != 3 || medium.bytes.size() != 3 {
                return Err(Error::new(ErrorKind::Content, "length mismatch"));
            }
        }
        let mut report = ExportReport::new();
        let mut document = String::new();
        for slot in slots {
            let medium = request
                .media
                .iter()
                .find(|medium| medium.slot == slot)
                .unwrap();
            let name = format!("slot-{}.raw", slot.0);
            let mut bytes = [0; 3];
            medium.bytes.read_at(0, &mut bytes)?;
            let output = sink.create(&name)?;
            output.write(&bytes)?;
            report.outputs.push(output.finish()?);
            document.push_str(&format!("slot={name}\n"));
        }
        if request.format != "raw" {
            let output = sink.create("machine.txt")?;
            output.write(document.as_bytes())?;
            report.outputs.push(output.finish()?);
        }
        Ok(report)
    }
}

#[test]
fn format_slot_subsets_build_requests_and_accept_reordered_media() {
    let core: &dyn Core = &SubsetCore;
    let config = config(b"");
    let descriptions = core.describe_media("subset", config).unwrap();
    assert_eq!(descriptions.len(), 3);
    let readers = [
        MemoryContent::new(b"abc".to_vec()),
        MemoryContent::new(b"def".to_vec()),
        MemoryContent::new(b"ghi".to_vec()),
    ];
    for (format, expected) in [
        ("pair", vec![SlotId(0), SlotId(2)]),
        ("single", vec![SlotId(1)]),
        ("raw", vec![SlotId(0), SlotId(1), SlotId(2)]),
    ] {
        let slots = core.export_slots("subset", config, format).unwrap();
        assert_eq!(slots, expected);
        let media: Vec<_> = slots
            .iter()
            .map(|slot| {
                let description = descriptions
                    .iter()
                    .find(|desc| desc.slot.id == *slot)
                    .unwrap();
                ExportMedium::new(*slot, description.size.unwrap(), &readers[slot.0 as usize])
            })
            .collect();
        if format == "raw" {
            for medium in media {
                let mut sink = MemoryExportSink::new();
                let report = core
                    .export(
                        "subset",
                        config,
                        ExportRequest::new(format, &[medium]),
                        &mut sink,
                    )
                    .unwrap();
                assert_eq!(report.outputs.len(), 1);
            }
            continue;
        }
        let mut sink = MemoryExportSink::new();
        let report = core
            .export(
                "subset",
                config,
                ExportRequest::new(format, &media),
                &mut sink,
            )
            .unwrap();
        let mut expected_names: Vec<_> = slots
            .iter()
            .map(|slot| format!("slot-{}.raw", slot.0))
            .collect();
        let document: String = expected_names
            .iter()
            .map(|name| format!("slot={name}\n"))
            .collect();
        expected_names.push("machine.txt".into());
        assert_eq!(
            report
                .outputs
                .iter()
                .map(|output| output.name.clone())
                .collect::<Vec<_>>(),
            expected_names
        );
        assert_eq!(sink.outputs().len(), slots.len() + 1);
        assert_eq!(sink.outputs()["machine.txt"], document.as_bytes());
        for slot in &slots {
            assert_eq!(
                sink.outputs()[&format!("slot-{}.raw", slot.0)],
                [b"abc", b"def", b"ghi"][slot.0 as usize]
            );
        }
        let mut reversed = media.clone();
        reversed.reverse();
        let mut again = MemoryExportSink::new();
        assert_eq!(
            core.export(
                "subset",
                config,
                ExportRequest::new(format, &reversed),
                &mut again
            )
            .unwrap(),
            report
        );
        assert_eq!(sink.outputs(), again.outputs());
        let all: Vec<_> = (0..3)
            .map(|id| ExportMedium::new(SlotId(id), 3, &readers[id as usize]))
            .collect();
        for invalid in [
            media[..media.len() - 1].to_vec(),
            vec![media[0], media[0]],
            all,
        ] {
            let mut sink = MemoryExportSink::new();
            assert_eq!(
                kind(core.export(
                    "subset",
                    config,
                    ExportRequest::new(format, &invalid),
                    &mut sink
                )),
                ErrorKind::Usage
            );
            assert!(sink.outputs().is_empty());
        }
    }
}
