// SPDX-License-Identifier: MIT OR Apache-2.0
//! A baseline core and front end using only the public API. Keep default
//! methods unimplemented so future additions exercise their compatibility.
#![cfg(feature = "test-support")]

use std::io::{Read, Write};
use std::rc::Rc;
use unemu_core_api::test_support::{MemoryContent, MemoryStorage, Operation};
use unemu_core_api::*;

const DIGEST: [u8; 32] = [7; 32];
const DESCRIPTION: CoreDescription = CoreDescription::new(
    ContractVersion::new(0, 5),
    "baseline",
    "Baseline core",
    "1.0.0",
)
.with_platforms(&[PlatformDescription::new("test", "Test").with_config_types(&["text/plain"])]);

struct BaselineCore;

impl Core for BaselineCore {
    fn description(&self) -> &CoreDescription {
        &DESCRIPTION
    }

    fn create(&self, request: &CreateRequest<'_>) -> Result<Box<dyn Machine>, Error> {
        if request.platform != "test" || request.config.media_type != "text/plain" {
            return Err(error(ErrorKind::Unsupported));
        }
        if !request.options.is_empty() {
            return Err(error(ErrorKind::Usage));
        }
        if request.config.bytes != b"program" {
            return Err(error(ErrorKind::Config));
        }
        let content = request.content.open(&DIGEST, 3)?;
        // This deliberately models a core that predates normalization helpers.
        let bytes = match content {
            Content::Bytes(bytes) => bytes,
            Content::Reader(reader) => {
                if reader.size() != 3 {
                    return Err(error(ErrorKind::Content));
                }
                let mut bytes = vec![0; 3];
                reader.read_at(0, &mut bytes)?;
                bytes
            }
            _ => return Err(error(ErrorKind::Unsupported)),
        };
        if bytes.len() != 3 {
            return Err(error(ErrorKind::Content));
        }
        let mut record = request.storage.open("progress")?;
        record.read_at(0, &mut [])?;
        Ok(Box::new(BaselineMachine {
            description: MachineDescription::new(Rate::new(1, 1), "instruction")
                .with_displays(vec![DisplayDescription::new(DisplayId(0), "Display")]),
            status: Status::Ready,
            steps: 0,
            pixel: [0],
            record,
        }))
    }
}

struct BaselineMachine {
    description: MachineDescription,
    status: Status,
    steps: u64,
    pixel: [u32; 1],
    record: Box<dyn Record>,
}

fn error(kind: ErrorKind) -> Error {
    Error::new(kind, "baseline refusal")
}

impl BaselineMachine {
    fn live(&self) -> Result<(), Error> {
        match self.status {
            Status::Failed | Status::Finished => Err(error(ErrorKind::Usage)),
            _ => Ok(()),
        }
    }
    fn optional(&self) -> Result<(), Error> {
        self.live()?;
        Err(error(ErrorKind::Unsupported))
    }
}

impl Machine for BaselineMachine {
    fn description(&self) -> &MachineDescription {
        &self.description
    }
    fn now(&self) -> Ticks {
        Ticks(self.steps)
    }
    fn steps(&self) -> u64 {
        self.steps
    }
    fn status(&self) -> Status {
        self.status
    }
    fn run(&mut self, limit: RunLimit) -> Result<Stop, Error> {
        self.live()?;
        match self.status {
            Status::Halted => return Ok(Stop::Halted),
            Status::Off => return Ok(Stop::PoweredOff),
            _ => {}
        }
        if limit.stop_on_frame {
            return Err(error(ErrorKind::Unsupported));
        }
        if limit.until.is_some_and(|t| t <= self.now()) {
            return Ok(Stop::Deadline);
        }
        if limit.steps.is_some_and(|s| s <= self.steps) {
            return Ok(Stop::Steps);
        }
        // The tiny program retires one instruction, writes a record and halts.
        if let Err(error) = self
            .record
            .append(b"done")
            .and_then(|()| self.record.sync())
        {
            self.status = Status::Failed;
            return Err(error);
        }
        self.steps += 1;
        self.pixel[0] = 1;
        self.status = Status::Halted;
        Ok(Stop::Halted)
    }
    fn frame(&mut self, display: DisplayId) -> Result<Frame<'_>, Error> {
        if display != DisplayId(0) {
            return Err(error(ErrorKind::Usage));
        }
        Ok(Frame::new(1, 1, &self.pixel, (1, 1), self.steps))
    }
    fn drain_audio(&mut self, _: &mut Vec<[i16; 2]>) -> Result<(), Error> {
        Err(error(ErrorKind::Unsupported))
    }
    fn set_outputs(&mut self, _: Outputs) {}
    fn input(&mut self, event: InputEvent) -> Result<(), Error> {
        self.live()?;
        if self.status == Status::Off {
            return Err(error(ErrorKind::Usage));
        }
        match event {
            InputEvent::ReleaseAll => Ok(()),
            InputEvent::Key { .. }
            | InputEvent::PointerMotion { .. }
            | InputEvent::PointerButton { .. }
            | InputEvent::Pad { .. }
            | InputEvent::PointerPosition { .. } => Err(error(ErrorKind::Usage)),
            _ => Err(error(ErrorKind::Unsupported)),
        }
    }
    fn slot_states(&mut self) -> Vec<SlotState> {
        vec![]
    }
    fn add_medium(&mut self, _: SlotId, _: NewMedium<'_>) -> Result<usize, Error> {
        if self.status == Status::Off {
            return Err(error(ErrorKind::Usage));
        }
        self.optional()?;
        unreachable!()
    }
    fn change_medium(&mut self, _: SlotId, _: MediaAction) -> Result<(), Error> {
        if self.status == Status::Off {
            return Err(error(ErrorKind::Usage));
        }
        self.optional()
    }
    fn reset(&mut self, kind: ResetKind) -> Result<(), Error> {
        if self.status == Status::Off && kind != ResetKind::Hard {
            return Err(error(ErrorKind::Usage));
        }
        self.optional()
    }
    fn state_identity(&self) -> Option<String> {
        None
    }
    fn save_state(&mut self, _: &mut dyn Write) -> Result<(), Error> {
        self.optional()
    }
    fn load_state(&mut self, _: &mut dyn Read) -> Result<(), Error> {
        self.optional()
    }
    fn finish(&mut self) -> Result<Finished, Error> {
        self.live()?;
        if let Err(error) = self.record.sync() {
            self.status = Status::Failed;
            return Err(error);
        }
        self.status = Status::Finished;
        Ok(Finished::new())
    }
    fn drain_messages(&mut self, _: &mut Vec<Message>) {}
}

struct Source {
    reader: bool,
    failure: Option<ErrorKind>,
}
impl ContentSource for Source {
    fn open(&self, digest: &[u8; 32], size: u64) -> Result<Content, Error> {
        if digest != &DIGEST || size != 3 {
            return Err(error(ErrorKind::Content));
        }
        if let Some(kind) = self.failure {
            return Err(error(kind));
        }
        if self.reader {
            Ok(Content::Reader(Box::new(MemoryContent::new(
                b"rom".to_vec(),
            ))))
        } else {
            Ok(Content::Bytes(b"rom".to_vec()))
        }
    }
}

fn create(storage: MemoryStorage, source: &Source) -> Result<Box<dyn Machine>, Error> {
    let core: &dyn Core = &BaselineCore;
    core.create(&CreateRequest::new(
        "test",
        ConfigDocument::new("text/plain", b"program"),
        source,
        Rc::new(storage),
    ))
}

fn kind<T>(result: Result<T, Error>) -> ErrorKind {
    match result {
        Ok(_) => panic!("expected failure"),
        Err(error) => error.kind,
    }
}

#[test]
fn default_identify_rejects_mismatched_receipt_sizes_through_core_object() {
    let core: &dyn Core = &BaselineCore;
    let content = MemoryContent::new(b"rom".to_vec());
    for size in [0, 2, 4, u64::MAX] {
        assert_eq!(
            kind(core.identify(&OfferedContent::new("image", DIGEST, size, &content))),
            ErrorKind::Content
        );
    }
    assert!(core
        .identify(&OfferedContent::new("image", DIGEST, 3, &content))
        .unwrap()
        .is_none());
}

#[test]
fn newer_frontend_drives_baseline_core_with_defaults_and_both_content_forms() {
    let core: &dyn Core = &BaselineCore;
    assert!(CONTRACT.drives(core.description().implements));
    let content = MemoryContent::new(vec![]);
    assert!(core
        .identify(&OfferedContent::new("image", [0; 32], 0, &content))
        .unwrap()
        .is_none());
    assert_eq!(
        kind(core.validate("test", ConfigDocument::new("text/plain", b"program"))),
        ErrorKind::Unsupported
    );
    for reader in [false, true] {
        let storage = MemoryStorage::new();
        let mut machine = create(
            storage.clone(),
            &Source {
                reader,
                failure: None,
            },
        )
        .unwrap();
        assert_eq!(machine.description().features, Features::NONE);
        assert_eq!(machine.now(), Ticks(0));
        assert_eq!(machine.run(RunLimit::at_steps(1)).unwrap(), Stop::Halted);
        assert_eq!(machine.frame(DisplayId(0)).unwrap().pixels, &[1]);
        assert_eq!(machine.steps(), 1);
        assert_eq!(kind(machine.frame(DisplayId(99))), ErrorKind::Usage);
        assert_eq!(
            kind(machine.drain_audio(&mut vec![])),
            ErrorKind::Unsupported
        );
        assert_eq!(kind(machine.reset(ResetKind::Hard)), ErrorKind::Unsupported);
        assert_eq!(
            kind(machine.change_medium(SlotId(99), MediaAction::Insert(99))),
            ErrorKind::Unsupported
        );
        assert_eq!(
            kind(machine.add_medium(
                SlotId(99),
                NewMedium::new(
                    "Image",
                    Content::Bytes(vec![]),
                    [0; 32],
                    0,
                    "raw",
                    WriteTarget::ReadOnly
                )
            )),
            ErrorKind::Unsupported
        );
        assert!(machine.state_identity().is_none());
        assert_eq!(
            kind(machine.save_state(&mut vec![])),
            ErrorKind::Unsupported
        );
        assert_eq!(
            kind(machine.load_state(&mut &b""[..])),
            ErrorKind::Unsupported
        );
        machine.finish().unwrap();
        assert_eq!(machine.status(), Status::Finished);
        assert_eq!(kind(machine.run(RunLimit::at_steps(2))), ErrorKind::Usage);
        assert_eq!(kind(machine.finish()), ErrorKind::Usage);
        assert_eq!(kind(machine.save_state(&mut vec![])), ErrorKind::Usage);
        assert_eq!(
            kind(machine.change_medium(SlotId(99), MediaAction::Insert(99))),
            ErrorKind::Usage
        );
        assert_eq!(machine.frame(DisplayId(0)).unwrap().pixels, &[1]);
        assert!(machine.slot_states().is_empty());
        machine.drain_messages(&mut vec![]);
        drop(machine);
        assert_eq!(storage.open("progress").unwrap().size().unwrap(), 4);
    }
}

#[test]
fn creation_errors_release_records_and_preserve_classification() {
    let core: &dyn Core = &BaselineCore;
    let source = Source {
        reader: false,
        failure: None,
    };
    let storage = MemoryStorage::new();
    let request = CreateRequest::new(
        "test",
        ConfigDocument::new("text/plain", b"program"),
        &source,
        Rc::new(storage.clone()),
    );
    assert_eq!(
        kind(core.create(&request.with_options(&[("unknown", "true")]))),
        ErrorKind::Usage
    );
    let request = CreateRequest::new(
        "foreign",
        ConfigDocument::new("text/plain", b"bad"),
        &source,
        Rc::new(storage.clone()),
    )
    .with_options(&[("unknown", "true")]);
    assert_eq!(kind(core.create(&request)), ErrorKind::Unsupported);
    let request = CreateRequest::new(
        "test",
        ConfigDocument::new("text/plain", b"bad"),
        &source,
        Rc::new(storage.clone()),
    );
    assert_eq!(kind(core.create(&request)), ErrorKind::Config);
    for failure in [ErrorKind::Content, ErrorKind::Storage] {
        assert_eq!(
            kind(create(
                storage.clone(),
                &Source {
                    reader: true,
                    failure: Some(failure)
                }
            )),
            failure
        );
    }
    storage.fail_next("progress", Operation::Open, error(ErrorKind::Storage));
    assert_eq!(kind(create(storage.clone(), &source)), ErrorKind::Storage);
    let held = storage.open("progress").unwrap();
    assert_eq!(kind(create(storage.clone(), &source)), ErrorKind::Usage);
    drop(held);
    {
        let mut record = storage.open("progress").unwrap();
        record.append(b"saved").unwrap();
        record.sync().unwrap();
    }
    storage.fail_next("progress", Operation::Read, error(ErrorKind::Storage));
    assert_eq!(
        kind(core.create(&CreateRequest::new(
            "test",
            ConfigDocument::new("text/plain", b"program"),
            &source,
            Rc::new(storage.clone()),
        ))),
        ErrorKind::Storage
    );
    let mut reopened = storage.open("progress").unwrap();
    let mut bytes = [0; 5];
    reopened.read_at(0, &mut bytes).unwrap();
    assert_eq!(&bytes, b"saved");
    assert_eq!(reopened.size().unwrap(), 5);
    drop(reopened);
    assert!(create(storage, &source).is_ok());
}

#[test]
fn failed_storage_is_distinct_and_execution_failure_remains_inspectable() {
    for failure in [ErrorKind::Storage, ErrorKind::StorageUncertain] {
        let storage = MemoryStorage::new();
        let mut machine = create(
            storage.clone(),
            &Source {
                reader: false,
                failure: None,
            },
        )
        .unwrap();
        let operation = if failure == ErrorKind::Storage {
            Operation::Append
        } else {
            Operation::Sync
        };
        storage.fail_next("progress", operation, error(failure));
        assert_eq!(kind(machine.run(RunLimit::at_steps(1))), failure);
        assert_eq!(machine.status(), Status::Failed);
        assert_eq!(machine.now(), Ticks(0));
        assert_eq!(kind(machine.finish()), ErrorKind::Usage);
        assert_eq!(machine.frame(DisplayId(0)).unwrap().pixels, &[0]);
    }
}

// An older front end knows only its original cases. Later cases reach the
// wildcard even though this build necessarily knows the variant's spelling.
fn old_stop(stop: Stop) -> Option<bool> {
    match stop {
        Stop::Steps => Some(false),
        Stop::Halted => Some(true),
        _ => None,
    }
}
fn old_error(kind: ErrorKind) -> &'static str {
    match kind {
        ErrorKind::Storage => "retry",
        ErrorKind::Guest => "guest",
        _ => "stop and report",
    }
}
fn old_status(status: Status) -> bool {
    matches!(status, Status::Ready)
}

#[test]
fn older_frontend_handles_newer_enum_cases_with_wildcards() {
    assert_eq!(old_stop(Stop::Frame(DisplayId(0))), None);
    assert_eq!(old_error(ErrorKind::StorageUncertain), "stop and report");
    assert!(!old_status(Status::Finished));
    let core: &dyn Core = &BaselineCore;
    let mut machine = create(
        MemoryStorage::new(),
        &Source {
            reader: true,
            failure: None,
        },
    )
    .unwrap();
    assert!(old_status(machine.status()));
    assert_eq!(
        old_stop(machine.run(RunLimit::at_steps(1)).unwrap()),
        Some(true)
    );
    assert_eq!(core.description().id, "baseline");
    machine.finish().unwrap();
    assert!(!old_status(machine.status()));
}

struct PlanningCore;
impl Core for PlanningCore {
    fn description(&self) -> &CoreDescription {
        &DESCRIPTION
    }
    fn validate(&self, platform: &str, config: ConfigDocument<'_>) -> Result<Validated, Error> {
        if platform != "test" || config.media_type != "text/plain" {
            return Err(error(ErrorKind::Unsupported));
        }
        if config.bytes != b"program" {
            return Err(error(ErrorKind::Config));
        }
        Ok(Validated::new(vec![
            ContentRef::new(DIGEST, 3, "program"),
            ContentRef::new([8; 32], 3, "firmware"),
            ContentRef::new(DIGEST, 3, "same program in another slot"),
        ]))
    }
    fn create(&self, request: &CreateRequest<'_>) -> Result<Box<dyn Machine>, Error> {
        self.validate(request.platform, request.config)?;
        // The core may read firmware first and re-open a cached program.
        request.content.open(&[8; 32], 3)?;
        request.content.open(&DIGEST, 3)?;
        BaselineCore.create(request)
    }
}

#[test]
fn validation_content_is_an_identity_set_instead_of_an_open_trace() {
    use std::cell::RefCell;
    use std::collections::BTreeSet;
    struct Traced(RefCell<Vec<([u8; 32], u64)>>);
    impl ContentSource for Traced {
        fn open(&self, digest: &[u8; 32], size: u64) -> Result<Content, Error> {
            self.0.borrow_mut().push((*digest, size));
            if ![DIGEST, [8; 32]].contains(digest) || size != 3 {
                return Err(error(ErrorKind::Content));
            }
            Ok(Content::Bytes(b"rom".to_vec()))
        }
    }
    let source = Traced(RefCell::new(vec![]));
    let config = ConfigDocument::new("text/plain", b"program");
    let core: &dyn Core = &PlanningCore;
    let validated = core.validate("test", config).unwrap();
    assert!(source.0.borrow().is_empty());
    let mut machine = core
        .create(&CreateRequest::new(
            "test",
            config,
            &source,
            Rc::new(MemoryStorage::new()),
        ))
        .unwrap();
    let listed: Vec<_> = validated
        .content
        .iter()
        .map(|c| (c.sha256, c.size))
        .collect();
    let opened = source.0.borrow();
    assert_ne!(listed, *opened);
    assert_eq!(
        listed.into_iter().collect::<BTreeSet<_>>(),
        opened.iter().copied().collect()
    );
    machine.finish().unwrap();
}

#[test]
fn baseline_core_defaults_offline_operations_to_unsupported_through_dyn_core() {
    let core: &dyn Core = &BaselineCore;
    assert!(core.description().platforms[0].export_formats.is_empty());
    let config = ConfigDocument::new("unknown", b"bad");
    let mut sink = unemu_core_api::test_support::MemoryExportSink::new();
    sink.fail_next(unemu_core_api::test_support::ExportOperation::Create);
    assert_eq!(
        kind(core.describe_media("unknown", config)),
        ErrorKind::Unsupported
    );
    assert_eq!(
        kind(core.export_slots("unknown", config, "unknown")),
        ErrorKind::Unsupported
    );
    assert_eq!(
        kind(core.export(
            "unknown",
            config,
            ExportRequest::new("unknown", &[]),
            &mut sink
        )),
        ErrorKind::Unsupported
    );
    assert!(sink.outputs().is_empty());
    assert_eq!(kind(sink.create("probe")), ErrorKind::Storage);
}
