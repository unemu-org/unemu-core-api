// SPDX-License-Identifier: MIT OR Apache-2.0
use super::*;
use std::{
    io::{Read, Write},
    path::Path,
};

#[test]
fn a_front_end_drives_only_its_own_minor_during_major_zero() {
    let front = ContractVersion::new(0, 1);
    assert!(front.drives(ContractVersion::new(0, 1)));
    assert!(!front.drives(ContractVersion::new(0, 0)));
    assert!(!front.drives(ContractVersion::new(0, 2)));
    assert!(!front.drives(ContractVersion::new(1, 1)));
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
fn this_source_is_contract_zero_one_and_drives_itself() {
    assert_eq!(CONTRACT, ContractVersion::new(0, 1));
    assert!(CONTRACT.drives(CONTRACT));
    assert_eq!(CONTRACT.to_string(), "0.1");
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
    assert_eq!(
        Rate::new(7, 0),
        Rate {
            numerator: 7,
            denominator: 0
        }
    );
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
/// every deadline: enough to drive it through `dyn Machine`.
struct Counter {
    description: MachineDescription,
    now: u64,
    pixels: Vec<u32>,
    queued: Vec<InputEvent>,
    messages: Vec<Message>,
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
        Status::Ready
    }

    fn run(&mut self, limit: RunLimit) -> Result<Stop, Error> {
        self.queued.clear();
        match (limit.until, limit.steps) {
            (Some(Ticks(until)), _) => {
                self.now = self.now.max(until);
                Ok(Stop::Deadline)
            }
            (None, Some(steps)) => {
                self.now = self.now.max(steps);
                Ok(Stop::Steps)
            }
            (None, None) => Err(Error::new(ErrorKind::Usage, "no limit")),
        }
    }

    fn frame(&mut self, display: DisplayId) -> Result<Frame<'_>, Error> {
        if display != DisplayId(0) {
            return Err(Error::new(ErrorKind::Unsupported, "no such display"));
        }
        self.pixels[0] = self.now as u32;
        Ok(Frame::new(1, 1, &self.pixels, (4, 3), self.now))
    }

    fn drain_audio(&mut self, out: &mut Vec<[i16; 2]>) -> Result<(), Error> {
        out.push([0, 0]);
        Ok(())
    }

    fn set_outputs(&mut self, _outputs: Outputs) {}

    fn input(&mut self, event: InputEvent) {
        self.queued.push(event);
    }

    fn slot_states(&mut self) -> Vec<SlotState> {
        Vec::new()
    }

    fn add_medium(&mut self, _slot: SlotId, _medium: NewMedium<'_>) -> Result<usize, Error> {
        Err(Error::new(ErrorKind::Unsupported, "no slots"))
    }

    fn change_medium(&mut self, _slot: SlotId, _action: MediaAction) -> Result<(), Error> {
        Err(Error::new(ErrorKind::Unsupported, "no slots"))
    }

    fn reset(&mut self, _kind: ResetKind) -> Result<(), Error> {
        Err(Error::new(ErrorKind::Unsupported, "no reset"))
    }

    fn state_identity(&self) -> Option<String> {
        None
    }

    fn save_state(&mut self, _out: &mut dyn Write) -> Result<(), Error> {
        Err(Error::new(ErrorKind::Unsupported, "no saved state"))
    }

    fn load_state(&mut self, _state: &mut dyn Read) -> Result<(), Error> {
        Err(Error::new(ErrorKind::Unsupported, "no saved state"))
    }

    fn finish(&mut self) -> Result<Finished, Error> {
        Ok(Finished::new())
    }

    fn drain_messages(&mut self, out: &mut Vec<Message>) {
        out.append(&mut self.messages);
    }
}

struct CounterCore;

static PLATFORMS: [PlatformDescription; 1] = [PlatformDescription::new("counter", "Counter")
    .with_config_types(&["application/x-counter"])
    .with_features(Features::NONE.with_media_changes(false))];

static DESCRIPTION: CoreDescription =
    CoreDescription::new(ContractVersion::new(0, 1), "counter", "Counter", "0.0.1")
        .with_platforms(&PLATFORMS)
        .with_notices(&[Notice::new("nothing", "no third-party parts")]);

impl Core for CounterCore {
    fn description(&self) -> &CoreDescription {
        &DESCRIPTION
    }

    fn create(&self, request: &CreateRequest<'_>) -> Result<Box<dyn Machine>, Error> {
        if request.config.media_type != "application/x-counter" {
            return Err(Error::new(ErrorKind::Unsupported, "unknown config type"));
        }
        if !request.options.is_empty() {
            return Err(Error::new(ErrorKind::Config, "unknown option"));
        }
        request.content.open(&[0; 32], 0)?;
        Ok(Box::new(Counter {
            description: MachineDescription::new(Rate::new(1, 1), "tick")
                .with_displays(vec![DisplayDescription::new(DisplayId(0), "Screen")])
                .with_audio(Some(AudioDescription::new(Rate::new(48_000, 1))))
                .with_inputs(vec![InputPortDescription::new(
                    PortId(0),
                    InputKind::Keyboard,
                    "Keyboard",
                )]),
            now: 0,
            pixels: vec![0],
            queued: Vec::new(),
            messages: vec![Message::new(MessageLevel::Note, "created")],
        }))
    }
}

struct Empty;

impl ContentSource for Empty {
    fn open(&self, _sha256: &[u8; 32], size: u64) -> Result<Content, Error> {
        Ok(Content::Bytes(vec![0; size as usize]))
    }
}

/// The front end holds cores and machines only as trait objects: these
/// calls compile only while both traits stay object-safe.
#[test]
fn cores_and_machines_are_driven_as_trait_objects() {
    let cores: Vec<&dyn Core> = vec![&CounterCore];
    let core = cores[0];
    assert!(CONTRACT.drives(core.description().implements));
    let platform = &core.description().platforms[0];
    assert_eq!(platform.config_types, ["application/x-counter"]);
    let content: &dyn ContentSource = &Empty;
    let request = CreateRequest::new(
        platform.id,
        ConfigDocument::new("application/x-counter", b""),
        content,
        Path::new("session"),
    );
    let mut machine: Box<dyn Machine> = core.create(&request).unwrap();
    let mut messages = Vec::new();
    machine.drain_messages(&mut messages);
    assert_eq!(messages, [Message::new(MessageLevel::Note, "created")]);
    machine.input(InputEvent::Key {
        port: PortId(0),
        key: Key(0x04),
        state: KeyState::Pressed,
    });
    assert_eq!(machine.run(RunLimit::until(Ticks(5))), Ok(Stop::Deadline));
    assert_eq!(machine.now(), Ticks(5));
    assert_eq!(machine.run(RunLimit::steps(8)), Ok(Stop::Steps));
    let frame = machine.frame(DisplayId(0)).unwrap();
    assert_eq!((frame.width, frame.height, frame.pixels), (1, 1, &[8][..]));
    let mut audio = Vec::new();
    machine.drain_audio(&mut audio).unwrap();
    assert_eq!(audio, [[0, 0]]);
    assert_eq!(
        machine.reset(ResetKind::Soft).unwrap_err().kind,
        ErrorKind::Unsupported
    );
    assert_eq!(machine.finish(), Ok(Finished::new()));
    let refused = core.create(&request.with_options(&[("speed", "fast")]));
    assert_eq!(
        refused.err().map(|error| error.kind),
        Some(ErrorKind::Config)
    );
}
