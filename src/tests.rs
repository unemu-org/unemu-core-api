// SPDX-License-Identifier: MIT OR Apache-2.0
use super::*;
use crate::test_support::{MemoryContent, MemoryStorage, Operation};
use std::io::{Read, Write};
use std::rc::Rc;

#[test]
fn a_front_end_drives_only_its_own_minor_during_major_zero() {
    let front = ContractVersion::new(0, 3);
    assert!(front.drives(ContractVersion::new(0, 3)));
    assert!(!front.drives(ContractVersion::new(0, 2)));
    assert!(!front.drives(ContractVersion::new(0, 4)));
    assert!(!front.drives(ContractVersion::new(1, 3)));
}

#[test]
fn from_major_one_a_front_end_drives_every_minor_of_its_major() {
    let front = ContractVersion::new(1, 3);
    for minor in [0, 1, 3, 4, u16::MAX] {
        assert!(front.drives(ContractVersion::new(1, minor)), "1.{minor}");
    }
    assert!(!front.drives(ContractVersion::new(0, 3)));
    assert!(!front.drives(ContractVersion::new(2, 3)));
}

#[test]
fn this_source_is_contract_zero_three_and_drives_itself() {
    assert_eq!(CONTRACT, ContractVersion::new(0, 3));
    assert!(CONTRACT.drives(CONTRACT));
    assert_eq!(CONTRACT.to_string(), "0.3");
}

#[test]
fn this_source_refuses_a_core_written_for_contract_zero_two() {
    assert!(!CONTRACT.drives(ContractVersion::new(0, 2)));
    assert!(!CONTRACT.drives(ContractVersion::new(0, 1)));
}

#[test]
fn rates_are_kept_in_lowest_terms() {
    assert_eq!(Rate::new(105_000_000, 88), Rate::new(13_125_000, 11));
    assert_eq!(
        Rate::new(105_000_000, 88),
        Rate {
            numerator: 13_125_000,
            denominator: 11
        }
    );
    assert_eq!(
        Rate::new(48_000, 1),
        Rate {
            numerator: 48_000,
            denominator: 1
        }
    );
    assert_eq!(
        Rate::new(0, 5),
        Rate {
            numerator: 0,
            denominator: 1
        }
    );
    assert_eq!(Rate::checked_new(7, 0), None);
}

#[test]
fn errors_read_as_their_message() {
    let error = Error::new(ErrorKind::Storage, "the journal cannot be written")
        .with_diagnostics(vec![Diagnostic::new("drives[0]", "no such file")]);
    assert_eq!(error.to_string(), "the journal cannot be written");
    assert_eq!(error.kind, ErrorKind::Storage);
    let boxed: Box<dyn std::error::Error> = Box::new(error);
    assert_eq!(boxed.to_string(), "the journal cannot be written");
}

#[test]
fn run_limits_build_from_either_bound() {
    let limit = RunLimit::until(Ticks(9)).with_steps(Some(4));
    assert_eq!(limit.until, Some(Ticks(9)));
    assert_eq!(limit.steps, Some(4));
    assert!(!limit.stop_on_frame);
    let limit = RunLimit::steps(3).with_stop_on_frame(true);
    assert_eq!(limit.until, None);
    assert!(limit.stop_on_frame);
}

/// A machine with one display that counts one step per tick and stops at
/// every deadline: enough to drive it through `dyn Machine`. It counts its
/// starts in a battery-backed record, which `finish` writes.
struct Counter {
    description: MachineDescription,
    now: u64,
    status: Status,
    pixels: Vec<u32>,
    queued: Vec<InputEvent>,
    messages: Vec<Message>,
    battery: Box<dyn Record>,
    starts: u64,
    audio_on: bool,
    /// The index of the next sound frame `drain_audio` hands over.
    audio_next: u64,
}

/// The counter's sound: frames per second, at a clock of one tick per second.
const COUNTER_AUDIO_RATE: u64 = 8_000;

/// Frame `i` of the counter's sound, so a test can tell where a drain began.
fn counter_sound(index: u64) -> [i16; 2] {
    [index as i16, index as i16]
}

impl Counter {
    fn live(&self) -> Result<(), Error> {
        if matches!(self.status, Status::Failed | Status::Finished) {
            Err(Error::new(ErrorKind::Usage, "terminal machine"))
        } else {
            Ok(())
        }
    }
    /// The index `floor(now() × rate / clock)` of the frame at `now()`.
    fn audio_index(&self) -> u64 {
        self.now * COUNTER_AUDIO_RATE
    }
}

impl Machine for Counter {
    fn description(&self) -> &MachineDescription {
        &self.description
    }

    fn now(&self) -> Ticks {
        Ticks(self.now)
    }

    fn steps(&self) -> u64 {
        self.now
    }

    fn status(&self) -> Status {
        self.status
    }

    fn run(&mut self, limit: RunLimit) -> Result<Stop, Error> {
        self.live()?;
        if limit.stop_on_frame {
            return Err(Error::new(ErrorKind::Unsupported, "no frame stops"));
        }
        self.queued.clear();
        let stop = match (limit.until, limit.steps) {
            (Some(Ticks(until)), steps) if steps.is_none_or(|steps| until <= steps) => {
                self.now = self.now.max(until);
                Ok(Stop::Deadline)
            }
            (_, Some(steps)) => {
                self.now = self.now.max(steps);
                Ok(Stop::Steps)
            }
            _ => {
                // With no guest event or limit, this arithmetic model reaches exhaustion.
                self.now = u64::MAX;
                self.status = Status::Failed;
                return Err(Error::new(ErrorKind::Guest, "virtual counter exhausted"));
            }
        };
        if !self.audio_on {
            // Nobody takes the sound: its frames are dropped, not owed.
            self.audio_next = self.audio_index();
        }
        stop
    }

    fn frame(&mut self, display: DisplayId) -> Result<Frame<'_>, Error> {
        if display != DisplayId(0) {
            return Err(Error::new(ErrorKind::Unsupported, "no such display"));
        }
        self.pixels[1] = self.now as u32;
        // The left column is overscan.
        Ok(
            Frame::new(2, 1, &self.pixels, (8, 3), self.now).with_visible(FrameArea {
                x: 1,
                y: 0,
                width: 1,
                height: 1,
            }),
        )
    }

    fn drain_audio(&mut self, out: &mut Vec<[i16; 2]>) -> Result<(), Error> {
        if self.audio_on {
            let end = self.audio_index();
            out.extend((self.audio_next..end).map(counter_sound));
            self.audio_next = end;
        }
        Ok(())
    }

    fn set_outputs(&mut self, outputs: Outputs) {
        if self.live().is_err() {
            return;
        }
        if outputs.audio != self.audio_on {
            // Frames not drained when sound is switched off are dropped, and
            // sound switched on starts at `now()`.
            self.audio_on = outputs.audio;
            self.audio_next = self.audio_index();
        }
    }

    fn input(&mut self, event: InputEvent) -> Result<(), Error> {
        self.live()?;
        self.queued.push(event);
        Ok(())
    }

    fn slot_states(&mut self) -> Vec<SlotState> {
        Vec::new()
    }

    fn add_medium(&mut self, _slot: SlotId, _medium: NewMedium<'_>) -> Result<usize, Error> {
        self.live()?;
        Err(Error::new(ErrorKind::Unsupported, "no slots"))
    }

    fn change_medium(&mut self, _slot: SlotId, _action: MediaAction) -> Result<(), Error> {
        self.live()?;
        Err(Error::new(ErrorKind::Unsupported, "no slots"))
    }

    fn reset(&mut self, _kind: ResetKind) -> Result<(), Error> {
        self.live()?;
        Err(Error::new(ErrorKind::Unsupported, "no reset"))
    }

    fn state_identity(&self) -> Option<String> {
        None
    }

    fn save_state(&mut self, _out: &mut dyn Write) -> Result<(), Error> {
        self.live()?;
        Err(Error::new(ErrorKind::Unsupported, "no saved state"))
    }

    fn load_state(&mut self, _state: &mut dyn Read) -> Result<(), Error> {
        self.live()?;
        Err(Error::new(ErrorKind::Unsupported, "no saved state"))
    }

    fn finish(&mut self) -> Result<Finished, Error> {
        self.live()?;
        let result = (|| {
            let mut replacement = self.battery.replace()?;
            replacement.append(&self.starts.to_le_bytes())?;
            replacement.commit()?;
            Ok(Finished::new())
        })();
        self.status = if result.is_ok() {
            Status::Finished
        } else {
            Status::Failed
        };
        result
    }

    fn drain_messages(&mut self, out: &mut Vec<Message>) {
        out.append(&mut self.messages);
    }
}

const COUNTER_CONFIG: &str = "application/x-counter";
const TAPE_MAGIC: &[u8; 4] = b"CNTR";

/// A core whose content is a tape: `CNTR`, a version byte (1 is the one it
/// models), then the tape's title. Its config is the tape's digest and size.
struct CounterCore;

static TAPE_KINDS: [ContentKind; 1] = [ContentKind::new("tape", "Counter tapes", &["ctr"])];

static PLATFORMS: [PlatformDescription; 1] = [PlatformDescription::new("counter", "Counter")
    .with_config_types(&[COUNTER_CONFIG])
    .with_features(Features::NONE.with_media_changes(false))
    .with_content(&TAPE_KINDS)];

static DESCRIPTION: CoreDescription =
    CoreDescription::new(ContractVersion::new(0, 3), "counter", "Counter", "0.0.2")
        .with_platforms(&PLATFORMS)
        .with_notices(&[Notice::new("nothing", "no third-party parts")]);

fn tape_config(sha256: [u8; 32], size: u64) -> ConfigDocumentBuf {
    let mut bytes = sha256.to_vec();
    bytes.extend_from_slice(&size.to_le_bytes());
    ConfigDocumentBuf::new(COUNTER_CONFIG, bytes)
}

impl Core for CounterCore {
    fn description(&self) -> &CoreDescription {
        &DESCRIPTION
    }

    fn create(&self, request: &CreateRequest<'_>) -> Result<Box<dyn Machine>, Error> {
        let validated = self.validate(request.platform, request.config)?;
        if !request.options.is_empty() {
            return Err(Error::new(ErrorKind::Config, "unknown option"));
        }
        let tape = &validated.content[0];
        request.content.open(&tape.sha256, tape.size)?;
        let mut battery = request.storage.open("counter.battery")?;
        let mut starts = [0; 8];
        if battery.size()? == 8 {
            battery.read_at(0, &mut starts)?;
        }
        Ok(Box::new(Counter {
            description: MachineDescription::new(Rate::new(1, 1), "tick")
                .with_displays(vec![DisplayDescription::new(DisplayId(0), "Screen")])
                .with_audio(Some(AudioDescription::new(Rate::new(
                    COUNTER_AUDIO_RATE,
                    1,
                ))))
                .with_inputs(vec![
                    InputPortDescription::new(PortId(0), InputKind::Keyboard, "Keyboard"),
                    InputPortDescription::new(
                        PortId(1),
                        InputKind::AbsolutePointer {
                            display: DisplayId(0),
                            buttons: 1,
                        },
                        "Light pen",
                    ),
                ]),
            now: 0,
            status: Status::Ready,
            pixels: vec![0, 0],
            queued: Vec::new(),
            messages: vec![Message::new(MessageLevel::Note, "created")],
            battery,
            starts: u64::from_le_bytes(starts) + 1,
            audio_on: true,
            audio_next: 0,
        }))
    }

    fn identify(&self, content: &OfferedContent<'_>) -> Result<Option<Identified>, Error> {
        if content.size < 5 {
            return Ok(None);
        }
        let mut header = [0; 5];
        content.bytes.read_at(0, &mut header)?;
        if &header[..4] != TAPE_MAGIC {
            return Ok(None);
        }
        if header[4] != 1 {
            return Err(Error::new(
                ErrorKind::Unsupported,
                format!("counter tapes of version {} are not modelled", header[4]),
            ));
        }
        let mut title = vec![0; (content.size - 5) as usize];
        content.bytes.read_at(5, &mut title)?;
        Ok(Some(
            Identified::new("counter", "tape", tape_config(content.sha256, content.size))
                .with_title(String::from_utf8(title).ok()),
        ))
    }

    fn validate(&self, platform: &str, config: ConfigDocument<'_>) -> Result<Validated, Error> {
        if platform != "counter" || config.media_type != COUNTER_CONFIG {
            return Err(Error::new(ErrorKind::Unsupported, "unknown config type"));
        }
        let Ok(bytes) = <[u8; 40]>::try_from(config.bytes) else {
            return Err(Error::new(ErrorKind::Config, "the config names no tape")
                .with_diagnostics(vec![Diagnostic::new("tape", "40 bytes expected")]));
        };
        let mut sha256 = [0; 32];
        sha256.copy_from_slice(&bytes[..32]);
        let size = u64::from_le_bytes(bytes[32..].try_into().unwrap());
        Ok(Validated::new(vec![ContentRef::new(sha256, size, "tape")]))
    }
}

/// A content source that holds one tape, under the digest a front end
/// computed for it.
struct Tapes {
    sha256: [u8; 32],
    bytes: Vec<u8>,
}

impl ContentSource for Tapes {
    fn open(&self, sha256: &[u8; 32], size: u64) -> Result<Content, Error> {
        if *sha256 != self.sha256 || size != self.bytes.len() as u64 {
            return Err(Error::new(ErrorKind::Content, "no such tape"));
        }
        Ok(Content::Reader(Box::new(MemoryContent::new(
            self.bytes.clone(),
        ))))
    }
}

/// The front end holds cores and machines only as trait objects: these
/// calls compile only while both traits stay object-safe. A file is opened
/// in two calls (`identify` writes the config, `create` reads it), and what
/// one machine keeps in its storage the next one reads.
#[test]
fn cores_and_machines_are_driven_as_trait_objects() {
    let cores: Vec<&dyn Core> = vec![&CounterCore];
    let core = cores[0];
    assert!(CONTRACT.drives(core.description().implements));
    let platform = &core.description().platforms[0];
    assert_eq!(platform.config_types, [COUNTER_CONFIG]);
    assert_eq!(platform.content[0].extensions, ["ctr"]);

    let tape = b"CNTR\x01Hello".to_vec();
    let sha256 = [7; 32];
    let reader = MemoryContent::new(tape.clone());
    let offered = OfferedContent::new("hello.ctr", sha256, tape.len() as u64, &reader);
    let identified = core.identify(&offered).unwrap().unwrap();
    assert_eq!((identified.platform, identified.kind), ("counter", "tape"));
    assert_eq!(identified.title.as_deref(), Some("Hello"));
    assert!(identified.notes.is_empty());
    let config = identified.config.as_document();
    let validated = core.validate(identified.platform, config).unwrap();
    assert_eq!(
        validated.content,
        [ContentRef::new(sha256, tape.len() as u64, "tape")]
    );

    let content: &dyn ContentSource = &Tapes {
        sha256,
        bytes: tape,
    };
    let storage = MemoryStorage::new();
    let request = CreateRequest::new(platform.id, config, content, Rc::new(storage.clone()));
    let mut machine: Box<dyn Machine> = core.create(&request).unwrap();
    assert!(storage.is_open("counter.battery"));
    let mut messages = Vec::new();
    machine.drain_messages(&mut messages);
    assert_eq!(messages, [Message::new(MessageLevel::Note, "created")]);
    machine
        .input(InputEvent::Key {
            port: PortId(0),
            key: Key(0x04),
            state: KeyState::Pressed,
        })
        .unwrap();
    machine
        .input(InputEvent::PointerPosition {
            port: PortId(1),
            position: Some((0x8000, 0)),
        })
        .unwrap();
    assert_eq!(machine.run(RunLimit::until(Ticks(5))), Ok(Stop::Deadline));
    assert_eq!(machine.now(), Ticks(5));
    assert_eq!(machine.run(RunLimit::steps(8)), Ok(Stop::Steps));
    let frame = machine.frame(DisplayId(0)).unwrap();
    assert_eq!(
        (frame.width, frame.height, frame.pixels),
        (2, 1, &[0, 8][..])
    );
    assert_eq!(
        frame.visible,
        FrameArea {
            x: 1,
            y: 0,
            width: 1,
            height: 1
        }
    );
    let mut audio = Vec::new();
    machine.drain_audio(&mut audio).unwrap();
    assert_eq!(audio.len() as u64, 8 * COUNTER_AUDIO_RATE);
    assert_eq!(audio[0], counter_sound(0));
    assert_eq!(
        machine.reset(ResetKind::Soft).unwrap_err().kind,
        ErrorKind::Unsupported
    );
    assert_eq!(machine.finish(), Ok(Finished::new()));
    drop(machine);
    assert_eq!(
        storage.durable("counter.battery"),
        Some(1u64.to_le_bytes().to_vec())
    );

    // The next machine reads what the first one kept.
    let mut machine = core.create(&request).unwrap();
    machine.finish().unwrap();
    drop(machine);
    assert_eq!(
        storage.durable("counter.battery"),
        Some(2u64.to_le_bytes().to_vec())
    );

    let refused = core.create(&request.with_options(&[("speed", "fast")]));
    assert_eq!(
        refused.err().map(|error| error.kind),
        Some(ErrorKind::Config)
    );
}

#[test]
fn identify_answers_none_for_foreign_bytes_and_unsupported_for_a_version_it_does_not_model() {
    let foreign = MemoryContent::new(b"MZ\x90\x00\x03".to_vec());
    let offered = OfferedContent::new("tool.exe", [1; 32], 5, &foreign);
    assert_eq!(CounterCore.identify(&offered), Ok(None));
    let newer = MemoryContent::new(b"CNTR\x02".to_vec());
    let offered = OfferedContent::new("new.ctr", [2; 32], 5, &newer);
    let error = CounterCore.identify(&offered).unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unsupported);
    assert_eq!(error.message, "counter tapes of version 2 are not modelled");
}

#[test]
fn validate_refuses_a_broken_config_with_diagnostics() {
    let error = CounterCore
        .validate("counter", ConfigDocument::new(COUNTER_CONFIG, b"short"))
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Config);
    assert_eq!(
        error.diagnostics,
        [Diagnostic::new("tape", "40 bytes expected")]
    );
    let error = CounterCore
        .validate("other", ConfigDocument::new(COUNTER_CONFIG, &[0; 40]))
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unsupported);
}

/// A core that implements only what contract 0.1 asked of it.
struct BareCore;

static BARE: CoreDescription =
    CoreDescription::new(ContractVersion::new(0, 3), "bare", "Bare", "0.0.1");

impl Core for BareCore {
    fn description(&self) -> &CoreDescription {
        &BARE
    }

    fn create(&self, _request: &CreateRequest<'_>) -> Result<Box<dyn Machine>, Error> {
        Err(Error::new(ErrorKind::Unsupported, "no platform"))
    }
}

#[test]
fn identify_and_validate_answer_nothing_and_unsupported_by_default() {
    let core: &dyn Core = &BareCore;
    let reader = MemoryContent::new(b"CNTR\x01".to_vec());
    let offered = OfferedContent::new("hello.ctr", [0; 32], 5, &reader);
    assert_eq!(core.identify(&offered), Ok(None));
    let error = core
        .validate("counter", ConfigDocument::new(COUNTER_CONFIG, &[0; 40]))
        .unwrap_err();
    assert_eq!(error.kind, ErrorKind::Unsupported);
}

#[test]
fn a_platform_opens_no_content_unless_it_says_so() {
    let platform = PlatformDescription::new("pc", "PC");
    assert!(platform.content.is_empty());
    assert_eq!(PLATFORMS[0].content, TAPE_KINDS);
}

#[test]
fn content_kinds_take_lower_case_ids_and_extensions_without_the_dot() {
    let kind = ContentKind::new("ines", "NES cartridge images", &["nes", "unf"]);
    assert_eq!(kind.id, "ines");
    assert_eq!(kind.extensions, ["nes", "unf"]);
    let kind = ContentKind::new("prg-2", "Program files", &[]);
    assert!(kind.extensions.is_empty());
}

#[test]
#[should_panic(expected = "a content kind id")]
fn a_content_kind_id_with_capitals_is_refused() {
    let _ = ContentKind::new("iNES", "NES cartridge images", &["nes"]);
}

#[test]
#[should_panic(expected = "a content kind id")]
fn an_empty_content_kind_id_is_refused() {
    let _ = ContentKind::new("", "NES cartridge images", &["nes"]);
}

#[test]
#[should_panic(expected = "a content kind id")]
fn a_content_kind_id_starting_with_a_dash_is_refused() {
    let _ = ContentKind::new("-ines", "NES cartridge images", &["nes"]);
}

#[test]
#[should_panic(expected = "an extension")]
fn an_extension_with_the_dot_is_refused() {
    let _ = ContentKind::new("ines", "NES cartridge images", &[".nes"]);
}

#[test]
#[should_panic(expected = "an extension")]
fn an_extension_in_capitals_is_refused() {
    let _ = ContentKind::new("ines", "NES cartridge images", &["NES"]);
}

#[test]
#[should_panic(expected = "an extension")]
fn an_empty_extension_is_refused() {
    let _ = ContentKind::new("ines", "NES cartridge images", &[""]);
}

#[test]
#[should_panic(expected = "a content kind has a name")]
fn a_content_kind_without_a_name_is_refused() {
    let _ = ContentKind::new("ines", "", &["nes"]);
}

#[test]
fn a_new_frame_shows_the_whole_picture() {
    let pixels = [0; 6];
    let frame = Frame::new(3, 2, &pixels, (4, 3), 1);
    assert_eq!(
        frame.visible,
        FrameArea {
            x: 0,
            y: 0,
            width: 3,
            height: 2
        }
    );
}

#[test]
fn the_visible_area_always_lies_within_the_picture() {
    let pixels = [0; 256 * 240];
    let frame = Frame::new(256, 240, &pixels, (128, 105), 1);
    let inside = FrameArea {
        x: 8,
        y: 8,
        width: 240,
        height: 224,
    };
    assert_eq!(frame.with_visible(inside).visible, inside);
    // Reaching past the right and bottom edges: cut to the picture.
    let over = FrameArea {
        x: 8,
        y: 8,
        width: 300,
        height: u32::MAX,
    };
    assert_eq!(
        frame.with_visible(over).visible,
        FrameArea {
            x: 8,
            y: 8,
            width: 248,
            height: 232
        }
    );
    // Sharing no pixel with the picture, or empty: the whole picture.
    let whole = FrameArea {
        x: 0,
        y: 0,
        width: 256,
        height: 240,
    };
    for outside in [
        FrameArea {
            x: 256,
            y: 0,
            width: 10,
            height: 10,
        },
        FrameArea {
            x: u32::MAX,
            y: u32::MAX,
            width: u32::MAX,
            height: u32::MAX,
        },
        FrameArea {
            x: 10,
            y: 10,
            width: 0,
            height: 10,
        },
    ] {
        assert_eq!(frame.with_visible(outside).visible, whole, "{outside:?}");
    }
}

#[test]
fn a_front_end_checks_an_area_against_the_picture() {
    let area = FrameArea {
        x: 8,
        y: 0,
        width: 248,
        height: 240,
    };
    assert!(area.is_within(256, 240));
    assert!(!area.is_within(255, 240));
    assert!(!area.is_within(256, 239));
    let far = FrameArea {
        x: u32::MAX,
        y: 0,
        width: 1,
        height: 0,
    };
    assert!(!far.is_within(u32::MAX, 0));
}

#[test]
fn a_pointer_position_maps_to_the_first_and_the_last_pixel_at_any_size() {
    for (width, height) in [(256, 240), (320, 200), (640, 480)] {
        assert_eq!(pointer_pixel((0, 0), width, height), (0, 0));
        assert_eq!(
            pointer_pixel((u16::MAX, u16::MAX), width, height),
            (width - 1, height - 1)
        );
        assert_eq!(
            pointer_pixel((0x8000, 0x8000), width, height),
            (width / 2, height / 2)
        );
    }
}

#[test]
fn a_light_gun_points_at_one_display() {
    let port = InputPortDescription::new(
        PortId(1),
        InputKind::AbsolutePointer {
            display: DisplayId(0),
            buttons: 1,
        },
        "Light gun",
    );
    assert_eq!(
        port.kind,
        InputKind::AbsolutePointer {
            display: DisplayId(0),
            buttons: 1
        }
    );
    let away = InputEvent::PointerPosition {
        port: PortId(1),
        position: None,
    };
    assert_ne!(
        away,
        InputEvent::PointerPosition {
            port: PortId(1),
            position: Some((0, 0))
        }
    );
}

#[test]
fn sound_rates_lie_from_8_000_to_192_000_frames_per_second() {
    let within = |numerator, denominator| {
        AudioDescription::new(Rate::new(numerator, denominator)).rate_in_bounds()
    };
    assert!(within(8_000, 1));
    assert!(within(48_000, 1));
    assert!(within(192_000, 1));
    // A chip clocked at the processor's rate, divided by 40 inside the core.
    assert!(within(984_375, 22));
    assert!(!within(19_687_500, 11));
    assert!(!within(7_999, 1));
    assert!(!within(192_001, 1));
    assert!(!within(15_999, 2));
    assert!(Rate::checked_new(48_000, 0).is_none());
    assert!(!within(u64::MAX, 1));
    assert!(within(u64::MAX / 2, u64::MAX / 96_000));
}

#[test]
fn record_names_are_short_lower_case_words() {
    for name in [
        "a",
        "0",
        "journal.0",
        "drive-c.journal",
        "nvram",
        &"x".repeat(64),
    ] {
        assert!(is_record_name(name), "{name}");
    }
    for name in [
        "",
        ".hidden",
        "-dash",
        "trailing.",
        "trailing-",
        "Upper",
        "nvRAM",
        "a/b",
        "a\\b",
        "a b",
        "ä",
        &"x".repeat(65),
    ] {
        assert!(!is_record_name(name), "{name}");
    }
}

#[test]
fn kept_record_names_start_with_the_front_ends_prefix() {
    assert_eq!(KEPT_RECORD_PREFIX, "kept-");
    for name in ["kept-a", "kept-drive-a.journal", "kept-0"] {
        assert!(is_kept_record_name(name), "{name}");
    }
    for name in [
        "kept-",
        "kept",
        "drive-a.journal",
        "kept-A",
        "kept-a.",
        "akept-a",
        &format!("kept-{}", "x".repeat(60)),
    ] {
        assert!(!is_kept_record_name(name), "{name}");
    }
}

#[test]
fn content_kinds_check_themselves() {
    assert!(TAPE_KINDS[0].is_valid());
    let mut kind = ContentKind::new("ines", "NES cartridge images", &["nes"]);
    assert!(kind.is_valid());
    kind.extensions = &["NES"];
    assert!(!kind.is_valid());
    kind.extensions = &["nes"];
    kind.id = "iNES";
    assert!(!kind.is_valid());
    kind.id = "ines";
    kind.name = "";
    assert!(!kind.is_valid());
}

#[test]
fn content_is_bytes_or_a_reader() {
    let content = Content::Reader(Box::new(MemoryContent::new(vec![1, 2, 3])));
    assert_eq!(format!("{content:?}"), "Reader { size: 3 }");
    assert_eq!(format!("{:?}", Content::Bytes(vec![1])), "Bytes([1])");
    let Content::Reader(reader) = content else {
        unreachable!()
    };
    let mut buf = [0; 2];
    reader.read_at(1, &mut buf).unwrap();
    assert_eq!(buf, [2, 3]);
    assert_eq!(
        reader.read_at(2, &mut buf).unwrap_err().kind,
        ErrorKind::Usage
    );
    assert_eq!(
        reader.read_at(u64::MAX, &mut buf).unwrap_err().kind,
        ErrorKind::Usage
    );
}

#[test]
fn a_kept_medium_names_a_record() {
    let target = WriteTarget::Kept {
        record: "kept-drive-a.journal",
    };
    assert_eq!(
        target,
        WriteTarget::Kept {
            record: "kept-drive-a.journal"
        }
    );
    let medium = NewMedium::new(
        "disk.img",
        Content::Bytes(vec![0; 512]),
        [0; 32],
        512,
        "floppy-160k",
        target,
    );
    assert_eq!(medium.writes, target);
}

// The storage in memory, through the contract's traits only.

fn memory() -> (MemoryStorage, Rc<dyn Storage>) {
    let storage = MemoryStorage::new();
    let shared: Rc<dyn Storage> = Rc::new(storage.clone());
    (storage, shared)
}

fn read_all(record: &mut dyn Record) -> Vec<u8> {
    let mut bytes = vec![0; record.size().unwrap() as usize];
    record.read_at(0, &mut bytes).unwrap();
    bytes
}

#[test]
fn a_storage_refuses_names_outside_the_rules() {
    let (_, storage) = memory();
    for name in ["", "Upper", "a/b", ".x"] {
        assert_eq!(
            storage.open(name).err().map(|error| error.kind),
            Some(ErrorKind::Usage),
            "{name}"
        );
    }
}

#[test]
fn a_record_is_held_by_one_object_at_a_time() {
    let (inside, storage) = memory();
    let record = storage.open("nvram").unwrap();
    assert!(inside.is_open("nvram"));
    assert_eq!(
        storage.open("nvram").err().map(|error| error.kind),
        Some(ErrorKind::Usage)
    );
    drop(record);
    assert!(!inside.is_open("nvram"));
    assert!(storage.open("nvram").is_ok());
}

#[test]
fn a_new_record_opens_empty_and_grows_by_appends() {
    let (inside, storage) = memory();
    let mut record = storage.open("journal").unwrap();
    assert_eq!(record.size(), Ok(0));
    record.append(b"head").unwrap();
    record.append(b"er").unwrap();
    assert_eq!(read_all(&mut *record), b"header");
    let mut two = [0; 2];
    record.read_at(4, &mut two).unwrap();
    assert_eq!(&two, b"er");
    assert_eq!(
        record.read_at(5, &mut two).unwrap_err().kind,
        ErrorKind::Usage
    );
    assert_eq!(inside.names(), ["journal"]);
}

#[test]
fn only_synchronised_bytes_survive_a_power_cut() {
    let (inside, storage) = memory();
    let mut record = storage.open("journal").unwrap();
    record.append(b"frame 1;").unwrap();
    record.sync().unwrap();
    record.append(b"frame 2;").unwrap();
    assert_eq!(inside.contents("journal").unwrap(), b"frame 1;frame 2;");
    assert_eq!(inside.durable("journal").unwrap(), b"frame 1;");
    drop(record);
    inside.power_cut();
    let mut record = storage.open("journal").unwrap();
    assert_eq!(read_all(&mut *record), b"frame 1;");
}

#[test]
fn a_cut_shortens_a_record_and_never_lengthens_it() {
    let (inside, storage) = memory();
    inside.insert("journal", b"good tail");
    let mut record = storage.open("journal").unwrap();
    record.truncate(4).unwrap();
    assert_eq!(read_all(&mut *record), b"good");
    assert_eq!(inside.durable("journal").unwrap(), b"good tail");
    record.sync().unwrap();
    assert_eq!(inside.durable("journal").unwrap(), b"good");
    assert_eq!(record.truncate(5).unwrap_err().kind, ErrorKind::Usage);
    record.truncate(4).unwrap();
}

#[test]
fn a_committed_replacement_takes_the_place_of_the_old_contents() {
    let (inside, storage) = memory();
    inside.insert("battery", b"old");
    let mut record = storage.open("battery").unwrap();
    let mut replacement = record.replace().unwrap();
    // The old bytes stay readable and appendable meanwhile.
    record.append(b"+").unwrap();
    assert_eq!(read_all(&mut *record), b"old+");
    assert_eq!(
        record.replace().err().map(|error| error.kind),
        Some(ErrorKind::Usage)
    );
    replacement.append(b"new").unwrap();
    replacement.append(b"er").unwrap();
    assert_eq!(replacement.size(), Ok(5));
    let mut check = [0; 5];
    replacement.read_at(0, &mut check).unwrap();
    assert_eq!(&check, b"newer");
    assert_eq!(
        replacement.read_at(1, &mut check).unwrap_err().kind,
        ErrorKind::Usage
    );
    replacement.commit().unwrap();
    assert_eq!(read_all(&mut *record), b"newer");
    assert_eq!(inside.durable("battery").unwrap(), b"newer");
    record.append(b"!").unwrap();
    assert_eq!(read_all(&mut *record), b"newer!");
    // The next replacement may start.
    assert!(record.replace().is_ok());
}

#[test]
fn a_dropped_replacement_is_discarded() {
    let (inside, storage) = memory();
    inside.insert("battery", b"old");
    let mut record = storage.open("battery").unwrap();
    let mut replacement = record.replace().unwrap();
    replacement.append(b"new").unwrap();
    drop(replacement);
    assert_eq!(read_all(&mut *record), b"old");
    assert!(record.replace().is_ok());
}

#[test]
fn a_replacement_whose_record_was_dropped_does_not_commit() {
    let (inside, storage) = memory();
    inside.insert("battery", b"old");
    let mut record = storage.open("battery").unwrap();
    let mut replacement = record.replace().unwrap();
    replacement.append(b"new").unwrap();
    drop(record);
    assert_eq!(replacement.commit().unwrap_err().kind, ErrorKind::Usage);
    assert_eq!(inside.durable("battery").unwrap(), b"old");
    assert_eq!(inside.contents("battery").unwrap(), b"old");
}

#[test]
fn a_replacement_of_a_released_record_does_not_commit() {
    let (inside, storage) = memory();
    let mut record = storage.open("battery").unwrap();
    let mut replacement = record.replace().unwrap();
    replacement.append(b"new").unwrap();
    drop(record);
    let reopened = storage.open("battery").unwrap();
    assert_eq!(replacement.commit().unwrap_err().kind, ErrorKind::Usage);
    drop(reopened);
    assert_eq!(inside.durable("battery").unwrap(), b"");
}

#[test]
fn a_commit_that_failed_before_anything_was_put_in_place_leaves_the_record() {
    let (inside, storage) = memory();
    inside.insert("battery", b"old");
    let mut record = storage.open("battery").unwrap();
    inside.fail_next(
        "battery",
        Operation::Commit,
        Error::new(ErrorKind::Storage, "disk full"),
    );
    let mut replacement = record.replace().unwrap();
    replacement.append(b"new").unwrap();
    assert_eq!(replacement.commit().unwrap_err().kind, ErrorKind::Storage);
    assert_eq!(read_all(&mut *record), b"old");
    assert_eq!(inside.durable("battery").unwrap(), b"old");
    // The record is still writable, and the fault fired once.
    let mut replacement = record.replace().unwrap();
    replacement.append(b"new").unwrap();
    replacement.commit().unwrap();
    assert_eq!(inside.durable("battery").unwrap(), b"new");
}

#[test]
fn an_uncertain_commit_needs_the_record_opened_again() {
    for takes_effect in [false, true] {
        let (inside, storage) = memory();
        inside.insert("battery", b"old");
        let mut record = storage.open("battery").unwrap();
        inside.uncertain_commit("battery", takes_effect);
        let mut replacement = record.replace().unwrap();
        replacement.append(b"new").unwrap();
        assert_eq!(
            replacement.commit().unwrap_err().kind,
            ErrorKind::StorageUncertain
        );
        // Reads still work; writes wait for a new opening.
        let now = read_all(&mut *record);
        assert_eq!(
            now,
            if takes_effect {
                &b"new"[..]
            } else {
                &b"old"[..]
            }
        );
        for refused in [
            record.append(b"x"),
            record.truncate(0),
            record.sync(),
            record.replace().map(drop),
        ] {
            assert_eq!(refused.unwrap_err().kind, ErrorKind::Usage);
        }
        drop(record);
        let mut record = storage.open("battery").unwrap();
        assert_eq!(read_all(&mut *record), now);
        record.append(b"!").unwrap();
    }
}

#[test]
fn planted_faults_fire_once_without_effect() {
    let (inside, storage) = memory();
    inside.fail_next(
        "journal",
        Operation::Open,
        Error::new(ErrorKind::Storage, "no medium"),
    );
    assert_eq!(
        storage.open("journal").err().map(|error| error.kind),
        Some(ErrorKind::Storage)
    );
    let mut record = storage.open("journal").unwrap();
    for operation in [
        Operation::Append,
        Operation::Truncate,
        Operation::Read,
        Operation::Replace,
        Operation::ReplacementAppend,
    ] {
        inside.fail_next(
            "journal",
            operation,
            Error::new(ErrorKind::Storage, "write error"),
        );
    }
    assert_eq!(record.append(b"x").unwrap_err().kind, ErrorKind::Storage);
    assert_eq!(record.size(), Ok(0));
    record.append(b"xy").unwrap();
    assert_eq!(record.truncate(1).unwrap_err().kind, ErrorKind::Storage);
    assert_eq!(record.size(), Ok(2));
    let mut one = [0; 1];
    assert_eq!(
        record.read_at(0, &mut one).unwrap_err().kind,
        ErrorKind::Storage
    );
    assert_eq!(
        record.replace().err().map(|error| error.kind),
        Some(ErrorKind::Storage)
    );
    record.sync().unwrap();
    record.truncate(1).unwrap();
    record.read_at(0, &mut one).unwrap();
    assert_eq!(&one, b"x");
    let mut replacement = record.replace().unwrap();
    assert_eq!(
        replacement.append(b"new").unwrap_err().kind,
        ErrorKind::Storage
    );
    assert_eq!(replacement.size(), Ok(0));
    replacement.append(b"new").unwrap();
    replacement.commit().unwrap();
    assert_eq!(inside.durable("journal").unwrap(), b"new");
}

#[test]
fn a_failed_sync_is_uncertain_and_keeps_what_was_durable() {
    let (inside, storage) = memory();
    let mut record = storage.open("journal").unwrap();
    record.append(b"one;").unwrap();
    record.sync().unwrap();
    record.append(b"two;").unwrap();
    inside.fail_next(
        "journal",
        Operation::Sync,
        Error::new(ErrorKind::StorageUncertain, "flush failed"),
    );
    assert_eq!(record.sync().unwrap_err().kind, ErrorKind::StorageUncertain);
    // Reads still work; writes wait for a new opening.
    assert_eq!(read_all(&mut *record), b"one;two;");
    assert_eq!(record.sync().unwrap_err().kind, ErrorKind::Usage);
    drop(record);
    inside.power_cut();
    let mut record = storage.open("journal").unwrap();
    assert_eq!(read_all(&mut *record), b"one;");
    record.append(b"two;").unwrap();
    record.sync().unwrap();
}

#[test]
#[should_panic(expected = "a failed sync is StorageUncertain")]
fn a_sync_cannot_be_planted_to_fail_with_a_plain_storage_error() {
    let (inside, _) = memory();
    inside.fail_next(
        "journal",
        Operation::Sync,
        Error::new(ErrorKind::Storage, "write error"),
    );
}

#[test]
fn an_uncertain_write_needs_the_record_opened_again() {
    for operation in [
        Operation::Append,
        Operation::Truncate,
        Operation::ReplacementAppend,
    ] {
        let (inside, storage) = memory();
        inside.insert("journal", b"base");
        let mut record = storage.open("journal").unwrap();
        let mut replacement = record.replace().unwrap();
        inside.fail_next(
            "journal",
            operation,
            Error::new(ErrorKind::StorageUncertain, "torn write"),
        );
        let failed = match operation {
            Operation::Append => record.append(b"x"),
            Operation::Truncate => record.truncate(1),
            _ => replacement.append(b"x"),
        };
        assert_eq!(
            failed.unwrap_err().kind,
            ErrorKind::StorageUncertain,
            "{operation:?}"
        );
        assert_eq!(read_all(&mut *record), b"base", "{operation:?}");
        assert_eq!(
            record.append(b"y").unwrap_err().kind,
            ErrorKind::Usage,
            "{operation:?}"
        );
        assert_eq!(
            replacement.commit().unwrap_err().kind,
            ErrorKind::Usage,
            "{operation:?}"
        );
        drop(record);
        let mut record = storage.open("journal").unwrap();
        record.append(b"!").unwrap();
        assert_eq!(read_all(&mut *record), b"base!");
    }
}

#[test]
fn content_in_memory_fails_on_request_and_when_it_changed() {
    let content = MemoryContent::new(b"tape".to_vec());
    let mut buf = [0; 2];
    content.fail_next(Error::new(ErrorKind::Storage, "read error"));
    assert_eq!(
        content.read_at(0, &mut buf).unwrap_err().kind,
        ErrorKind::Storage
    );
    content.read_at(2, &mut buf).unwrap();
    assert_eq!(&buf, b"pe");
    content.change();
    for _ in 0..2 {
        assert_eq!(
            content.read_at(0, &mut buf).unwrap_err().kind,
            ErrorKind::Content
        );
    }
}

#[test]
fn sound_keeps_its_place_in_virtual_time() {
    let content = Tapes {
        sha256: [7; 32],
        bytes: b"CNTR\x01".to_vec(),
    };
    let config = tape_config([7; 32], 5);
    let request = CreateRequest::new(
        "counter",
        config.as_document(),
        &content,
        Rc::new(MemoryStorage::new()),
    );
    let mut machine = CounterCore.create(&request).unwrap();
    let rate = COUNTER_AUDIO_RATE;
    let mut audio = Vec::new();
    machine.drain_audio(&mut audio).unwrap();
    assert!(audio.is_empty());
    // The next frame handed over has index floor(now() × rate / clock).
    machine.run(RunLimit::until(Ticks(2))).unwrap();
    machine.drain_audio(&mut audio).unwrap();
    assert_eq!(audio.len() as u64, 2 * rate);
    assert_eq!(audio[0], counter_sound(0));
    // Sound switched off: what was not drained is dropped, nothing is owed.
    machine.run(RunLimit::until(Ticks(3))).unwrap();
    machine.set_outputs(Outputs::NONE);
    machine.run(RunLimit::until(Ticks(5))).unwrap();
    machine.set_outputs(Outputs::NONE.with_audio(true));
    machine.run(RunLimit::until(Ticks(6))).unwrap();
    audio.clear();
    machine.drain_audio(&mut audio).unwrap();
    assert_eq!(audio.len() as u64, rate);
    assert_eq!(audio[0], counter_sound(5 * rate));
    // Switched off and on again between two runs: the frames not drained
    // before the switch are dropped all the same.
    machine.run(RunLimit::until(Ticks(7))).unwrap();
    machine.set_outputs(Outputs::NONE);
    machine.set_outputs(Outputs::NONE.with_audio(true));
    machine.run(RunLimit::until(Ticks(8))).unwrap();
    audio.clear();
    machine.drain_audio(&mut audio).unwrap();
    assert_eq!(audio.len() as u64, rate);
    assert_eq!(audio[0], counter_sound(7 * rate));
}

#[test]
#[should_panic(expected = "is open")]
fn a_power_cut_comes_after_the_machine_is_gone() {
    let (inside, storage) = memory();
    let _record = storage.open("journal").unwrap();
    inside.power_cut();
}

fn scripted() -> test_support::ScriptedMachine {
    test_support::ScriptedMachine::new(Rc::new(test_support::MemoryStorage::new()))
}

fn scripted_status(status: Status) -> test_support::ScriptedMachine {
    let mut m = scripted();
    match status {
        Status::Ready => {}
        Status::Halted | Status::Off => {
            m.stop_next(if status == Status::Halted {
                Stop::Halted
            } else {
                Stop::PoweredOff
            });
            m.run(RunLimit::at_steps(1)).unwrap();
        }
        Status::Failed => {
            m.fail_run(Error::new(ErrorKind::Guest, "scripted failure"));
            m.run(RunLimit::at_steps(1)).unwrap_err();
        }
        Status::Finished => {
            m.finish().unwrap();
        }
    }
    m
}

fn test_medium(writes: WriteTarget<'_>) -> NewMedium<'_> {
    NewMedium::new("Image", Content::Bytes(vec![0]), [1; 32], 1, "raw", writes)
}

#[test]
fn every_status_allows_inspection_and_drains_without_execution() {
    for status in [
        Status::Ready,
        Status::Halted,
        Status::Off,
        Status::Failed,
        Status::Finished,
    ] {
        let mut m = scripted_status(status);
        let before = (m.now(), m.steps());
        let machine: &mut dyn Machine = &mut m;
        assert!(machine.description().clock.is_clock());
        assert_eq!(machine.status(), status);
        assert!(machine.frame(DisplayId(0)).is_ok());
        assert_eq!(
            machine.frame(DisplayId(1)).unwrap_err().kind,
            ErrorKind::Usage
        );
        machine.drain_audio(&mut Vec::new()).unwrap();
        machine.slot_states();
        assert!(machine.state_identity().is_some());
        machine.drain_messages(&mut Vec::new());
        machine.set_outputs(Outputs::NONE.with_audio(true));
        assert_eq!(
            (machine.now(), machine.steps(), machine.status()),
            (before.0, before.1, status)
        );
    }
}

#[test]
fn terminal_calls_return_usage_before_capability_or_argument_checks() {
    for status in [Status::Failed, Status::Finished] {
        let mut m = scripted_status(status).with_features(Features::NONE);
        let before = (m.now(), m.steps());
        assert_eq!(
            m.run(RunLimit::at_steps(0)).unwrap_err().kind,
            ErrorKind::Usage
        );
        assert_eq!(
            m.input(InputEvent::ReleaseAll).unwrap_err().kind,
            ErrorKind::Usage
        );
        assert_eq!(
            m.add_medium(SlotId(99), test_medium(WriteTarget::ReadOnly))
                .unwrap_err()
                .kind,
            ErrorKind::Usage
        );
        assert_eq!(
            m.change_medium(SlotId(99), MediaAction::Insert(99))
                .unwrap_err()
                .kind,
            ErrorKind::Usage
        );
        assert_eq!(m.reset(ResetKind::Hard).unwrap_err().kind, ErrorKind::Usage);
        assert_eq!(
            m.save_state(&mut Vec::new()).unwrap_err().kind,
            ErrorKind::Usage
        );
        assert_eq!(
            m.load_state(&mut &b"bad"[..]).unwrap_err().kind,
            ErrorKind::Usage
        );
        assert_eq!(m.finish().unwrap_err().kind, ErrorKind::Usage);
        assert_eq!(
            (m.now(), m.steps(), m.status()),
            (before.0, before.1, status)
        );
        assert!(m.queued_input().is_empty());
    }
}

#[test]
fn live_status_calls_follow_the_matrix() {
    for status in [Status::Ready, Status::Halted, Status::Off] {
        let mut m = scripted_status(status);
        let before = (m.now(), m.steps());
        let input = m.input(InputEvent::ReleaseAll);
        let add = m.add_medium(SlotId(0), test_medium(WriteTarget::ReadOnly));
        let change = m.change_medium(SlotId(0), MediaAction::Insert(0));
        if status == Status::Off {
            for error in [input.unwrap_err(), add.unwrap_err(), change.unwrap_err()] {
                assert_eq!(error.kind, ErrorKind::Usage);
            }
        } else {
            input.unwrap();
            add.unwrap();
            change.unwrap();
        }
        let mut state = Vec::new();
        m.save_state(&mut state).unwrap();
        m.load_state(&mut state.as_slice()).unwrap();
        assert_eq!(m.status(), status);
        assert_eq!((m.now(), m.steps()), before);
        assert_eq!(m.finish(), Ok(Finished::new()));
        assert_eq!(m.status(), Status::Finished);
        assert_eq!(m.finish().unwrap_err().kind, ErrorKind::Usage);
    }
}

#[test]
fn unsupported_frame_runs_do_not_consume_input() {
    let mut m = scripted().with_features(Features::NONE);
    m.input(InputEvent::ReleaseAll).unwrap();
    assert_eq!(
        m.run(RunLimit::at_steps(0).with_stop_on_frame(true))
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
    assert_eq!(m.queued_input(), &[InputEvent::ReleaseAll]);
    assert_eq!(
        (m.now(), m.steps(), m.status()),
        (Ticks(0), 0, Status::Ready)
    );
}

#[test]
fn absolute_limits_apply_input_even_when_already_reached() {
    let mut m = scripted();
    m.run(RunLimit::steps(3)).unwrap();
    m.input(InputEvent::key(PortId(0), Key::A, KeyState::Pressed))
        .unwrap();
    assert_eq!(m.run(RunLimit::at_steps(2)), Ok(Stop::Steps));
    assert_eq!((m.now(), m.steps()), (Ticks(3), 3));
    assert!(m.queued_input().is_empty());
    assert_eq!(m.applied_input().len(), 1);
    assert_eq!(m.run(RunLimit::until(Ticks(2))), Ok(Stop::Deadline));
    assert_eq!(RunLimit::steps(5), RunLimit::at_steps(5));
    m.run(RunLimit::at_steps(5)).unwrap();
    assert_eq!(m.steps(), 5);
    let mut m = scripted();
    assert_eq!(
        m.run(RunLimit::at_steps(2).with_until(Some(Ticks(5)))),
        Ok(Stop::Steps)
    );
}

#[test]
fn simultaneous_stops_have_a_deterministic_priority() {
    for stop in [
        Stop::PoweredOff,
        Stop::Halted,
        Stop::Frame(DisplayId(0)),
        Stop::Deadline,
    ] {
        let mut m = scripted();
        m.stop_next(stop);
        assert_eq!(
            m.run(
                RunLimit::at_steps(1)
                    .with_until(Some(Ticks(1)))
                    .with_stop_on_frame(stop != Stop::Deadline)
            ),
            Ok(stop)
        );
    }
    let mut m = scripted();
    m.stop_next(Stop::PoweredOff);
    m.fail_run(Error::new(ErrorKind::Guest, "failed before power-off"));
    assert_eq!(
        m.run(RunLimit::at_steps(1)).unwrap_err().kind,
        ErrorKind::Guest
    );
    assert_eq!(m.status(), Status::Failed);
    let mut m = scripted();
    m.stop_next(Stop::Frame(DisplayId(0)));
    assert_eq!(m.run(RunLimit::at_steps(2)), Ok(Stop::Steps));
}

#[test]
fn unbounded_counter_runs_until_exhaustion() {
    let content = Tapes {
        sha256: [7; 32],
        bytes: b"CNTR\x01".to_vec(),
    };
    let config = tape_config([7; 32], 5);
    let request = CreateRequest::new(
        "counter",
        config.as_document(),
        &content,
        Rc::new(MemoryStorage::new()),
    );
    let mut m = CounterCore.create(&request).unwrap();
    assert_eq!(
        m.run(RunLimit::at_steps(0).with_steps(None))
            .unwrap_err()
            .kind,
        ErrorKind::Guest
    );
    assert_eq!(
        (m.now(), m.steps(), m.status()),
        (Ticks(u64::MAX), u64::MAX, Status::Failed)
    );
}

#[test]
fn unbounded_runs_apply_input_and_continue_until_an_event() {
    for stop in [Stop::Halted, Stop::PoweredOff] {
        let mut m = scripted().with_features(Features::NONE);
        let event = InputEvent::key(PortId(0), Key::A, KeyState::Pressed);
        m.input(event.clone()).unwrap();
        m.stop_next(stop);
        assert_eq!(m.run(RunLimit::at_steps(0).with_steps(None)), Ok(stop));
        assert_eq!((m.now(), m.steps()), (Ticks(1), 1));
        assert!(m.queued_input().is_empty());
        assert_eq!(m.applied_input(), &[event]);
    }
    let mut m = scripted();
    m.fail_run(Error::new(ErrorKind::Content, "content changed"));
    assert_eq!(
        m.run(RunLimit::at_steps(0).with_steps(None))
            .unwrap_err()
            .kind,
        ErrorKind::Content
    );
    assert_eq!(m.status(), Status::Failed);
}

#[test]
fn stopped_unbounded_runs_return_without_executing_or_consuming_input() {
    for status in [Status::Halted, Status::Off] {
        let mut m = scripted_status(status);
        if status == Status::Halted {
            m.input(InputEvent::ReleaseAll).unwrap();
        }
        let before = (m.now(), m.steps(), m.queued_input().to_vec());
        assert_eq!(
            m.run(RunLimit::at_steps(0).with_steps(None)),
            Ok(if status == Status::Halted {
                Stop::Halted
            } else {
                Stop::PoweredOff
            })
        );
        assert_eq!((m.now(), m.steps(), m.queued_input().to_vec()), before);
        assert_eq!(m.status(), status);
    }
}

#[test]
fn stopped_runs_skip_unsupported_frame_checks() {
    for status in [Status::Halted, Status::Off] {
        let mut m = scripted_status(status).with_features(Features::NONE);
        if status == Status::Halted {
            m.input(InputEvent::ReleaseAll).unwrap();
        }
        let before = (m.now(), m.steps(), m.queued_input().to_vec());
        assert_eq!(
            m.run(
                RunLimit::at_steps(0)
                    .with_steps(None)
                    .with_stop_on_frame(true)
            ),
            Ok(if status == Status::Halted {
                Stop::Halted
            } else {
                Stop::PoweredOff
            })
        );
        assert_eq!((m.now(), m.steps(), m.queued_input().to_vec()), before);
        assert_eq!(m.status(), status);
    }
}

#[test]
fn counter_exhaustion_fails_without_wrapping_and_reached_limits_still_work() {
    for (ticks, steps) in [(u64::MAX, 0), (0, u64::MAX)] {
        let mut m = scripted();
        m.set_counters(Ticks(ticks), steps);
        assert_eq!(m.run(RunLimit::at_steps(steps)), Ok(Stop::Steps));
        assert_eq!(
            m.run(
                RunLimit::at_steps(0)
                    .with_steps(None)
                    .with_stop_on_frame(true)
            )
            .unwrap_err()
            .kind,
            ErrorKind::Guest
        );
        assert_eq!(
            (m.now(), m.steps(), m.status()),
            (Ticks(ticks), steps, Status::Failed)
        );
    }
}

#[test]
fn resets_preserve_counters_and_media_and_clear_controls_and_pending_input() {
    for status in [Status::Ready, Status::Halted, Status::Off] {
        for kind in [ResetKind::Soft, ResetKind::Hard] {
            let mut m = scripted();
            m.add_medium(SlotId(0), test_medium(WriteTarget::ReadOnly))
                .unwrap();
            m.change_medium(SlotId(0), MediaAction::Insert(0)).unwrap();
            let position = InputEvent::pointer_position(PortId(0), Some((10, 20)));
            m.input(position.clone()).unwrap();
            m.input(InputEvent::key(PortId(0), Key::A, KeyState::Pressed))
                .unwrap();
            if status != Status::Ready {
                m.stop_next(if status == Status::Off {
                    Stop::PoweredOff
                } else {
                    Stop::Halted
                });
            }
            m.run(RunLimit::at_steps(1)).unwrap();
            if status != Status::Off {
                m.input(InputEvent::ReleaseAll).unwrap();
            }
            let before = (m.now(), m.steps(), m.queued_input().to_vec());
            if status == Status::Off && kind == ResetKind::Soft {
                assert_eq!(m.reset(kind).unwrap_err().kind, ErrorKind::Usage);
                assert_eq!(m.status(), status);
                assert_eq!(m.queued_input(), before.2);
            } else {
                m.reset(kind).unwrap();
                assert_eq!(m.status(), Status::Ready);
                assert!(m.queued_input().is_empty());
                assert_eq!(m.applied_input(), &[position]);
            }
            assert_eq!((m.now(), m.steps()), (before.0, before.1));
            assert_eq!(m.slot_states()[0].inserted, Some(0));
        }
    }
}

#[test]
fn reset_run_and_finish_failures_follow_status_rules() {
    for status in [Status::Ready, Status::Halted, Status::Off] {
        for kind in [
            ErrorKind::Storage,
            ErrorKind::Guest,
            ErrorKind::StorageUncertain,
        ] {
            let mut m = scripted_status(status);
            let before = (m.now(), m.steps());
            if status != Status::Off {
                m.input(InputEvent::ReleaseAll).unwrap();
            }
            let pending = m.queued_input().to_vec();
            m.fail_reset(Error::new(kind, "reset failed"));
            assert_eq!(m.reset(ResetKind::Hard).unwrap_err().kind, kind);
            let expected = if kind == ErrorKind::StorageUncertain && status != Status::Halted {
                Status::Failed
            } else {
                status
            };
            assert_eq!(
                (m.now(), m.steps(), m.status()),
                (before.0, before.1, expected)
            );
            assert_eq!(m.queued_input(), pending);
        }
        let mut m = scripted_status(status);
        m.fail_finish(Error::new(ErrorKind::Storage, "finish failed"));
        assert_eq!(m.finish().unwrap_err().kind, ErrorKind::Storage);
        assert_eq!(m.status(), Status::Failed);
    }
    for kind in [
        ErrorKind::Guest,
        ErrorKind::Storage,
        ErrorKind::StorageUncertain,
        ErrorKind::Content,
    ] {
        let mut m = scripted();
        m.fail_run(Error::new(kind, "run failed"));
        assert_eq!(m.run(RunLimit::at_steps(1)).unwrap_err().kind, kind);
        assert_eq!(m.status(), Status::Failed);
    }
    for kind in [ErrorKind::Storage, ErrorKind::StorageUncertain] {
        let mut m = scripted();
        m.stop_next(Stop::Halted);
        m.fail_run(Error::new(kind, "halt flush failed"));
        m.run(RunLimit::at_steps(1)).unwrap_err();
        assert_eq!(m.status(), Status::Halted);
        m.finish().unwrap();
    }
}

#[test]
fn media_selection_and_extension_are_independent_and_immediate() {
    let mut m = scripted().with_features(Features::NONE.with_media_additions(true));
    assert_eq!(
        m.add_medium(SlotId(0), test_medium(WriteTarget::ReadOnly)),
        Ok(0)
    );
    assert_eq!(m.slot_states()[0].catalogue.len(), 1);
    assert_eq!(m.slot_states()[0].inserted, None);
    assert_eq!(
        m.change_medium(SlotId(0), MediaAction::Next)
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
    let mut m = m.with_features(Features::NONE.with_media_changes(true));
    assert_eq!(
        m.add_medium(SlotId(0), test_medium(WriteTarget::ReadOnly))
            .unwrap_err()
            .kind,
        ErrorKind::Unsupported
    );
    m.change_medium(SlotId(0), MediaAction::Next).unwrap();
    assert_eq!(m.slot_states()[0].inserted, Some(0));
    m.change_medium(SlotId(0), MediaAction::Eject).unwrap();
    assert_eq!(m.slot_states()[0].inserted, None);
    assert_eq!(m.now(), Ticks(0));
    m.change_medium(SlotId(0), MediaAction::Insert(99))
        .unwrap_err();
    assert_eq!(m.slot_states()[0].inserted, None);
}

#[test]
fn compatible_kept_aliases_reuse_the_held_record() {
    let storage = MemoryStorage::new();
    let mut m = test_support::ScriptedMachine::new(Rc::new(storage.clone()));
    for name in ["First label", "Second label"] {
        let mut medium = test_medium(WriteTarget::kept("kept-alias"));
        medium.name = name;
        m.add_medium(SlotId(0), medium).unwrap();
    }
    m.change_medium(SlotId(0), MediaAction::Insert(0)).unwrap();
    for action in [MediaAction::Insert(1), MediaAction::Next, MediaAction::Next] {
        m.change_medium(SlotId(0), action).unwrap();
        assert!(storage.is_open("kept-alias"));
    }
    assert_eq!(m.slot_states()[0].inserted, Some(1));
    assert_eq!(
        (m.now(), m.steps(), m.status()),
        (Ticks(0), 0, Status::Ready)
    );
    m.change_medium(SlotId(0), MediaAction::Eject).unwrap();
    assert!(!storage.is_open("kept-alias"));
    m.change_medium(SlotId(0), MediaAction::Insert(0)).unwrap();
    m.finish().unwrap();
    drop(m);
    assert!(!storage.is_open("kept-alias"));
}

#[test]
fn media_open_errors_are_reported_by_the_call_and_preserve_the_previous_insertion() {
    let storage = MemoryStorage::new();
    let mut m = test_support::ScriptedMachine::new(Rc::new(storage.clone()));
    m.add_medium(SlotId(0), test_medium(WriteTarget::ReadOnly))
        .unwrap();
    m.add_medium(SlotId(0), test_medium(WriteTarget::kept("kept-image")))
        .unwrap();
    m.change_medium(SlotId(0), MediaAction::Insert(0)).unwrap();
    assert_eq!(
        m.add_medium(SlotId(0), test_medium(WriteTarget::kept("invalid")))
            .unwrap_err()
            .kind,
        ErrorKind::Usage
    );
    storage.fail_next(
        "kept-image",
        test_support::Operation::Open,
        Error::new(ErrorKind::Storage, "open failed"),
    );
    assert_eq!(
        m.change_medium(SlotId(0), MediaAction::Insert(1))
            .unwrap_err()
            .kind,
        ErrorKind::Storage
    );
    assert_eq!(m.slot_states()[0].inserted, Some(0));
    assert_eq!(m.status(), Status::Ready);
    assert_eq!(m.run(RunLimit::at_steps(1)), Ok(Stop::Steps));
    m.change_medium(SlotId(0), MediaAction::Next).unwrap();
    assert_eq!(m.slot_states()[0].inserted, Some(1));
    assert!(storage.is_open("kept-image"));
    m.change_medium(SlotId(0), MediaAction::Next).unwrap();
    assert_eq!(m.slot_states()[0].inserted, Some(0));
    assert!(!storage.is_open("kept-image"));
    for kind in [
        ErrorKind::Usage,
        ErrorKind::Unsupported,
        ErrorKind::Content,
        ErrorKind::Storage,
    ] {
        let before = m.slot_states();
        m.fail_media(Error::new(kind, "media failed"));
        assert_eq!(
            m.add_medium(SlotId(0), test_medium(WriteTarget::ReadOnly))
                .unwrap_err()
                .kind,
            kind
        );
        assert_eq!(m.slot_states(), before);
        m.fail_media(Error::new(kind, "selection failed"));
        m.change_medium(SlotId(0), MediaAction::Next).unwrap_err();
        assert_eq!(m.slot_states(), before);
    }
    m.fail_media(Error::new(ErrorKind::StorageUncertain, "uncertain media"));
    m.change_medium(SlotId(0), MediaAction::Next).unwrap_err();
    assert_eq!(m.status(), Status::Failed);
}

#[test]
fn halted_media_changes_do_not_wake_and_absent_features_are_honest() {
    let mut m = scripted_status(Status::Halted);
    m.add_medium(SlotId(0), test_medium(WriteTarget::ReadOnly))
        .unwrap();
    m.change_medium(SlotId(0), MediaAction::Next).unwrap();
    assert_eq!(m.status(), Status::Halted);
    let mut m = scripted().with_features(Features::NONE);
    assert_eq!(
        m.reset(ResetKind::Soft).unwrap_err().kind,
        ErrorKind::Unsupported
    );
    assert_eq!(m.state_identity(), None);
    assert_eq!(
        m.save_state(&mut Vec::new()).unwrap_err().kind,
        ErrorKind::Unsupported
    );
    assert_eq!(
        m.load_state(&mut &b""[..]).unwrap_err().kind,
        ErrorKind::Unsupported
    );
    assert_eq!(m.status(), Status::Ready);
}

#[test]
fn initial_picture_and_metadata_are_independent_of_pixel_serial() {
    let mut m = scripted();
    assert_eq!(m.frame(DisplayId(0)).unwrap().pixels, &[0]);
    let mut unavailable = scripted().without_initial_picture();
    assert_eq!(
        unavailable.frame(DisplayId(0)).unwrap_err().kind,
        ErrorKind::Usage
    );
    unavailable.run(RunLimit::at_steps(1)).unwrap();
    assert!(unavailable.frame(DisplayId(0)).is_ok());
    let serial = m.frame(DisplayId(0)).unwrap().serial;
    m.set_frame_metadata((4, 3), FrameArea::new(0, 0, 1, 1));
    let frame = m.frame(DisplayId(0)).unwrap();
    assert_eq!(frame.serial, serial);
    assert_eq!(frame.aspect, (4, 3));
    m.run(RunLimit::at_steps(1)).unwrap();
    assert_ne!(m.frame(DisplayId(0)).unwrap().serial, serial);
    m.finish().unwrap();
    assert!(m.frame(DisplayId(0)).is_ok());
}

#[test]
fn empty_visible_areas_are_invalid_and_fall_back_to_whole_picture() {
    const EMPTY: FrameArea = FrameArea::new(0, 0, 0, 1);
    assert!(!EMPTY.is_within(1, 1));
    assert!(!FrameArea::new(0, 0, 1, 0).is_within(1, 1));
    assert_eq!(
        Frame::new(1, 1, &[0], (1, 1), 0)
            .with_visible(EMPTY)
            .visible,
        FrameArea::new(0, 0, 1, 1)
    );
}

#[test]
fn invalid_rates_clocks_frames_and_duplicate_ids_are_rejected() {
    assert!(std::panic::catch_unwind(|| Rate::new(1, 0)).is_err());
    assert_eq!(Rate::checked_new(1, 0), None);
    assert!(!Rate::new(0, 1).is_clock());
    assert!(
        std::panic::catch_unwind(|| MachineDescription::new(Rate::new(0, 1), "instruction"))
            .is_err()
    );
    for (width, height, aspect, pixels) in [
        (0, 1, (1, 1), &[][..]),
        (1, 0, (1, 1), &[][..]),
        (1, 1, (0, 1), &[0][..]),
        (1, 1, (1, 0), &[0][..]),
        (1, 1, (1, 1), &[][..]),
    ] {
        assert!(std::panic::catch_unwind(|| Frame::new(width, height, pixels, aspect, 0)).is_err());
    }
    let description = || MachineDescription::new(Rate::new(1, 1), "instruction");
    assert!(
        std::panic::catch_unwind(|| description().with_displays(vec![
            DisplayDescription::new(DisplayId(0), "First"),
            DisplayDescription::new(DisplayId(0), "Second")
        ]))
        .is_err()
    );
    assert!(std::panic::catch_unwind(|| description().with_inputs(vec![
        InputPortDescription::new(PortId(0), InputKind::Keyboard, "First"),
        InputPortDescription::new(PortId(0), InputKind::Keyboard, "Second")
    ]))
    .is_err());
    assert!(std::panic::catch_unwind(|| description().with_slots(vec![
        SlotDescription::new(SlotId(0), SlotKind::Floppy, "First", true),
        SlotDescription::new(SlotId(0), SlotKind::Floppy, "Second", true)
    ]))
    .is_err());
}

#[test]
fn release_all_preserves_absolute_pointer_position_and_input_order() {
    let mut m = scripted();
    let position = InputEvent::pointer_position(PortId(0), Some((3, 4)));
    let key = InputEvent::key(PortId(0), Key::A, KeyState::Pressed);
    for event in [position.clone(), key, InputEvent::ReleaseAll] {
        m.input(event).unwrap();
    }
    assert!(m.applied_input().is_empty());
    m.run(RunLimit::at_steps(0)).unwrap();
    assert_eq!(m.applied_input(), &[position]);
}

#[test]
fn saved_states_restore_counters_status_and_all_input_variants_and_drop_audio() {
    let events = vec![
        InputEvent::key(PortId(0), Key::A, KeyState::Pressed),
        InputEvent::key(PortId(0), Key::B, KeyState::Repeated),
        InputEvent::key(PortId(0), Key::C, KeyState::Released),
        InputEvent::pointer_motion(PortId(0), -7, 9),
        InputEvent::pointer_button(PortId(0), 1, true),
        InputEvent::pad(PortId(0), PadControl::Select, true),
        InputEvent::pointer_position(PortId(0), Some((u16::MAX, 42))),
        InputEvent::pointer_position(PortId(0), None),
    ];
    for status in [Status::Ready, Status::Halted, Status::Off] {
        let mut source = scripted();
        source.set_outputs(Outputs::NONE.with_audio(true));
        for event in &events {
            source.input(event.clone()).unwrap();
        }
        if status != Status::Ready {
            source.stop_next(if status == Status::Halted {
                Stop::Halted
            } else {
                Stop::PoweredOff
            });
        }
        source.run(RunLimit::at_steps(1)).unwrap();
        if status != Status::Off {
            for event in &events {
                source.input(event.clone()).unwrap();
            }
            source.input(InputEvent::ReleaseAll).unwrap();
        }
        let pending = source.queued_input().to_vec();
        let mut bytes = Vec::new();
        source.save_state(&mut bytes).unwrap();
        assert_eq!(source.queued_input(), pending);
        let mut target = scripted();
        target.set_outputs(Outputs::NONE.with_audio(true));
        target.run(RunLimit::at_steps(3)).unwrap();
        target.load_state(&mut bytes.as_slice()).unwrap();
        assert_eq!(
            (target.now(), target.steps(), target.status()),
            (Ticks(1), 1, status)
        );
        assert_eq!(target.queued_input(), pending);
        assert_eq!(target.applied_input(), events);
        let mut audio = Vec::new();
        target.drain_audio(&mut audio).unwrap();
        assert!(audio.is_empty());
        target.reset(ResetKind::Hard).unwrap();
        target.run(RunLimit::at_steps(2)).unwrap();
        target.drain_audio(&mut audio).unwrap();
        assert_eq!(audio.len(), 1);
    }
}

#[test]
fn failed_loads_are_atomic_including_reader_errors_and_external_generations() {
    struct BrokenReader;
    impl Read for BrokenReader {
        fn read(&mut self, _: &mut [u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("read failed"))
        }
    }
    let storage = MemoryStorage::new();
    let mut m = test_support::ScriptedMachine::new(Rc::new(storage.clone()));
    m.add_medium(SlotId(0), test_medium(WriteTarget::kept("kept-state")))
        .unwrap();
    m.change_medium(SlotId(0), MediaAction::Insert(0)).unwrap();
    m.input(InputEvent::ReleaseAll).unwrap();
    m.set_outputs(Outputs::NONE.with_audio(true));
    m.run(RunLimit::at_steps(1)).unwrap();
    m.input(InputEvent::key(PortId(0), Key::A, KeyState::Pressed))
        .unwrap();
    let mut original = Vec::new();
    m.save_state(&mut original).unwrap();
    let identity = m.state_identity().unwrap();
    for length in 0..original.len() {
        assert_eq!(
            m.load_state(&mut &original[..length]).unwrap_err().kind,
            ErrorKind::State
        );
        let mut after = Vec::new();
        m.save_state(&mut after).unwrap();
        assert_eq!(after, original);
    }
    assert_eq!(
        m.load_state(&mut BrokenReader).unwrap_err().kind,
        ErrorKind::Storage
    );
    let mut after = Vec::new();
    m.save_state(&mut after).unwrap();
    assert_eq!(after, original);
    let mut audio = Vec::new();
    m.drain_audio(&mut audio).unwrap();
    assert_eq!(audio.len(), 1);
    assert_eq!(storage.durable("kept-state"), Some(vec![]));
    m.load_state(&mut original.as_slice()).unwrap();
    assert_eq!(storage.durable("kept-state"), Some(vec![]));
    m.change_medium(SlotId(0), MediaAction::Eject).unwrap();
    let mut record = storage.open("kept-state").unwrap();
    record.append(b"new generation").unwrap();
    record.sync().unwrap();
    drop(record);
    m.change_medium(SlotId(0), MediaAction::Insert(0)).unwrap();
    assert_ne!(m.state_identity().unwrap(), identity);
    assert_eq!(
        m.load_state(&mut original.as_slice()).unwrap_err().kind,
        ErrorKind::State
    );
    assert_eq!(
        storage.durable("kept-state"),
        Some(b"new generation".to_vec())
    );
    let mut other = scripted();
    assert_eq!(
        other.load_state(&mut original.as_slice()).unwrap_err().kind,
        ErrorKind::State
    );
}

#[test]
fn terminal_output_changes_are_ignored_without_losing_pending_audio() {
    for status in [Status::Failed, Status::Finished] {
        let mut m = scripted();
        m.set_outputs(Outputs::NONE.with_audio(true));
        m.run(RunLimit::at_steps(1)).unwrap();
        if status == Status::Failed {
            m.fail_run(Error::new(ErrorKind::Guest, "run failed"));
            m.run(RunLimit::at_steps(2)).unwrap_err();
        } else {
            m.finish().unwrap();
        }
        m.set_outputs(Outputs::NONE);
        let mut audio = Vec::new();
        m.drain_audio(&mut audio).unwrap();
        assert_eq!(audio.len(), 1);
        assert_eq!(m.status(), status);
    }
}

#[test]
fn save_failure_preserves_guest_state_and_finish_sync_failure_is_terminal() {
    struct BrokenWriter;
    impl Write for BrokenWriter {
        fn write(&mut self, _: &[u8]) -> std::io::Result<usize> {
            Err(std::io::Error::other("write failed"))
        }
        fn flush(&mut self) -> std::io::Result<()> {
            Ok(())
        }
    }
    let storage = MemoryStorage::new();
    let mut m = test_support::ScriptedMachine::new(Rc::new(storage.clone()));
    m.add_medium(SlotId(0), test_medium(WriteTarget::kept("kept-sync")))
        .unwrap();
    m.change_medium(SlotId(0), MediaAction::Next).unwrap();
    m.input(InputEvent::ReleaseAll).unwrap();
    let mut before = Vec::new();
    m.save_state(&mut before).unwrap();
    assert_eq!(
        m.save_state(&mut BrokenWriter).unwrap_err().kind,
        ErrorKind::Storage
    );
    let mut after = Vec::new();
    m.save_state(&mut after).unwrap();
    assert_eq!(before, after);
    storage.fail_next(
        "kept-sync",
        test_support::Operation::Sync,
        Error::new(ErrorKind::StorageUncertain, "sync failed"),
    );
    assert_eq!(m.finish().unwrap_err().kind, ErrorKind::StorageUncertain);
    assert_eq!(m.status(), Status::Failed);
    assert_eq!(m.finish().unwrap_err().kind, ErrorKind::Usage);
}

#[test]
fn geometry_and_visibility_can_change_without_a_pixel_serial_change() {
    let pixels = [0; 2];
    let first = Frame::new(2, 1, &pixels, (2, 1), 5);
    let second = Frame::new(1, 2, &pixels, (1, 2), 5).with_visible(FrameArea::new(0, 0, 1, 1));
    assert_eq!(first.serial, second.serial);
    assert_eq!(first.pixels, second.pixels);
    assert_ne!((first.width, first.height), (second.width, second.height));
    assert_ne!(first.visible, second.visible);
    let mut invalid_rate = Rate::new(1, 1);
    invalid_rate.denominator = 0;
    assert!(!invalid_rate.is_clock());
    assert!(!AudioDescription::new(invalid_rate).rate_in_bounds());
}
