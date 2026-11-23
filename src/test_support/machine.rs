// SPDX-License-Identifier: MIT OR Apache-2.0
//! A deterministic reference for lifecycle, media and state consumers.

use crate::*;
use std::io::{Read, Write};
use std::rc::Rc;

/// A one-tick, one-retired-instruction machine for consumer conformance tests.
/// It has one display, one removable slot, saved state and both reset kinds.
/// Input ports are keyboard 0, relative pointer 1 (three buttons), absolute
/// pointer 2 (one button) and pad 3 (A and Select).
/// Each executed step changes its single pixel. Scripted stops occur after
/// the next step; planted errors occur before it. No guest hardware is modeled.
pub struct ScriptedMachine {
    description: MachineDescription,
    storage: Rc<dyn Storage>,
    status: Status,
    now: u64,
    steps: u64,
    pending: Vec<InputEvent>,
    held: Vec<InputEvent>,
    messages: Vec<Message>,
    audio: Vec<[i16; 2]>,
    outputs: Outputs,
    pixels: Vec<u32>,
    width: u32,
    height: u32,
    aspect: (u32, u32),
    visible: FrameArea,
    serial: u64,
    picture: bool,
    catalogue: Vec<MediumInfo>,
    kept: Vec<Option<String>>,
    inserted: Option<usize>,
    record: Option<Box<dyn Record>>,
    generation: Vec<u8>,
    next_stop: Option<Stop>,
    run_error: Option<Error>,
    media_error: Option<Error>,
    reset_error: Option<Error>,
    finish_error: Option<Error>,
}

impl ScriptedMachine {
    /// A ready machine with an initial black frame and an empty slot.
    pub fn new(storage: Rc<dyn Storage>) -> Self {
        Self {
            description: MachineDescription::new(Rate::new(8_000, 1), "instruction")
                .with_displays(vec![DisplayDescription::new(DisplayId(0), "Display")])
                .with_audio(Some(AudioDescription::new(Rate::new(8_000, 1))))
                .with_inputs(vec![
                    InputPortDescription::new(PortId(0), InputKind::Keyboard, "Keyboard"),
                    InputPortDescription::new(
                        PortId(1),
                        InputKind::relative_pointer(3),
                        "Relative pointer",
                    ),
                    InputPortDescription::new(
                        PortId(2),
                        InputKind::absolute_pointer(DisplayId(0), 1),
                        "Absolute pointer",
                    ),
                    InputPortDescription::new(
                        PortId(3),
                        InputKind::pad(vec![PadControl::A, PadControl::Select]),
                        "Pad",
                    ),
                ])
                .with_slots(vec![SlotDescription::new(
                    SlotId(0),
                    SlotKind::Floppy,
                    "Media",
                    true,
                )
                .with_formats(vec!["raw".into()])])
                .with_features(
                    Features::NONE
                        .with_resets(&[ResetKind::Soft, ResetKind::Hard])
                        .with_saved_state(true)
                        .with_media_changes(true)
                        .with_media_additions(true)
                        .with_frame_stops(true),
                ),
            storage,
            status: Status::Ready,
            now: 0,
            steps: 0,
            pending: Vec::new(),
            held: Vec::new(),
            messages: Vec::new(),
            audio: Vec::new(),
            outputs: Outputs::NONE,
            pixels: vec![0],
            width: 1,
            height: 1,
            aspect: (1, 1),
            visible: FrameArea::new(0, 0, 1, 1),
            serial: 0,
            picture: true,
            catalogue: Vec::new(),
            kept: Vec::new(),
            inserted: None,
            record: None,
            generation: Vec::new(),
            next_stop: None,
            run_error: None,
            media_error: None,
            reset_error: None,
            finish_error: None,
        }
    }

    /// Restrict advertised features to test optional controls.
    pub fn with_features(mut self, features: Features) -> Self {
        self.description.features = features;
        self
    }

    /// Choose the permitted Usage-before-first-picture behavior.
    pub fn without_initial_picture(mut self) -> Self {
        self.picture = false;
        self
    }

    /// Script a guest halt, power-off or frame completion after one step.
    pub fn stop_next(&mut self, stop: Stop) {
        self.next_stop = Some(stop);
    }
    /// Plant a run error before execution.
    pub fn fail_run(&mut self, error: Error) {
        self.run_error = Some(error);
    }
    /// Plant a media error before the next media operation takes effect.
    pub fn fail_media(&mut self, error: Error) {
        self.media_error = Some(error);
    }
    /// Plant a reset error before the reset takes effect.
    pub fn fail_reset(&mut self, error: Error) {
        self.reset_error = Some(error);
    }
    /// Plant a finalization error.
    pub fn fail_finish(&mut self, error: Error) {
        self.finish_error = Some(error);
    }
    /// Set counters for exhaustion and absolute-budget tests.
    pub fn set_counters(&mut self, now: Ticks, steps: u64) {
        self.now = now.0;
        self.steps = steps;
    }
    /// Change metadata independently of the pixel serial.
    pub fn set_frame_metadata(&mut self, aspect: (u32, u32), visible: FrameArea) {
        assert!(aspect.0 != 0 && aspect.1 != 0 && visible.is_within(self.width, self.height));
        self.aspect = aspect;
        self.visible = visible;
    }
    /// Input awaiting a run boundary.
    pub fn queued_input(&self) -> &[InputEvent] {
        &self.pending
    }
    /// Applied controls, recorded in order; ReleaseAll clears this list.
    pub fn applied_input(&self) -> &[InputEvent] {
        &self.held
    }

    fn usage() -> Error {
        Error::new(ErrorKind::Usage, "call is invalid in this status")
    }
    fn live(&self) -> Result<(), Error> {
        if matches!(self.status, Status::Failed | Status::Finished) {
            Err(Self::usage())
        } else {
            Ok(())
        }
    }
    fn interactive(&self) -> Result<(), Error> {
        self.live()?;
        if self.status == Status::Off {
            Err(Self::usage())
        } else {
            Ok(())
        }
    }
    fn media_check(&mut self, slot: SlotId, adding: bool) -> Result<(), Error> {
        self.interactive()?;
        let supported = if adding {
            self.description.features.media_additions
        } else {
            self.description.features.media_changes
        };
        if !supported {
            return Err(Error::new(
                ErrorKind::Unsupported,
                "media operation unavailable",
            ));
        }
        if slot != SlotId(0) {
            return Err(Self::usage());
        }
        if let Some(error) = self.media_error.take() {
            if error.kind == ErrorKind::StorageUncertain {
                self.status = Status::Failed;
            }
            return Err(error);
        }
        Ok(())
    }
}

impl Machine for ScriptedMachine {
    fn description(&self) -> &MachineDescription {
        &self.description
    }
    fn now(&self) -> Ticks {
        Ticks(self.now)
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
        if limit.stop_on_frame && !self.description.features.frame_stops {
            return Err(Error::new(
                ErrorKind::Unsupported,
                "frame stops unavailable",
            ));
        }
        for event in self.pending.drain(..) {
            if event == InputEvent::ReleaseAll {
                self.held
                    .retain(|e| matches!(e, InputEvent::PointerPosition { .. }));
            } else {
                self.held.push(event);
            }
        }
        loop {
            if limit.until.is_some_and(|t| self.now >= t.0) {
                return Ok(Stop::Deadline);
            }
            if limit.steps.is_some_and(|s| self.steps >= s) {
                return Ok(Stop::Steps);
            }
            if let Some(error) = self.run_error.take() {
                self.status =
                    if matches!(error.kind, ErrorKind::Storage | ErrorKind::StorageUncertain)
                        && self.next_stop == Some(Stop::Halted)
                    {
                        Status::Halted
                    } else {
                        Status::Failed
                    };
                return Err(error);
            }
            let (Some(now), Some(steps)) = (self.now.checked_add(1), self.steps.checked_add(1))
            else {
                self.status = Status::Failed;
                return Err(Error::new(ErrorKind::Guest, "virtual counter exhausted"));
            };
            self.now = now;
            self.steps = steps;
            self.pixels[0] ^= 1;
            self.serial = self.serial.wrapping_add(1);
            self.picture = true;
            if self.outputs.audio {
                self.audio.push([self.pixels[0] as i16; 2]);
            }
            match self.next_stop.take() {
                Some(Stop::PoweredOff) => {
                    self.status = Status::Off;
                    return Ok(Stop::PoweredOff);
                }
                Some(Stop::Halted) => {
                    self.status = Status::Halted;
                    return Ok(Stop::Halted);
                }
                Some(Stop::Frame(id)) if limit.stop_on_frame => return Ok(Stop::Frame(id)),
                _ if limit.stop_on_frame => return Ok(Stop::Frame(DisplayId(0))),
                _ => {}
            }
        }
    }
    fn frame(&mut self, display: DisplayId) -> Result<Frame<'_>, Error> {
        if display != DisplayId(0) || !self.picture {
            return Err(Self::usage());
        }
        Ok(Frame::new(
            self.width,
            self.height,
            &self.pixels,
            self.aspect,
            self.serial,
        )
        .with_visible(self.visible))
    }
    fn drain_audio(&mut self, out: &mut Vec<[i16; 2]>) -> Result<(), Error> {
        out.append(&mut self.audio);
        Ok(())
    }
    fn set_outputs(&mut self, outputs: Outputs) {
        if self.live().is_err() {
            return;
        }
        if !outputs.audio {
            self.audio.clear();
        }
        self.outputs = outputs;
    }
    fn input(&mut self, event: InputEvent) -> Result<(), Error> {
        self.interactive()?;
        let valid = match &event {
            InputEvent::ReleaseAll => true,
            InputEvent::Key { port, .. }
            | InputEvent::PointerMotion { port, .. }
            | InputEvent::PointerButton { port, .. }
            | InputEvent::Pad { port, .. }
            | InputEvent::PointerPosition { port, .. } => {
                self.description.inputs.iter().any(|input| {
                    input.id == *port
                        && match (&event, &input.kind) {
                            (InputEvent::Key { .. }, InputKind::Keyboard)
                            | (
                                InputEvent::PointerMotion { .. },
                                InputKind::RelativePointer { .. },
                            )
                            | (
                                InputEvent::PointerPosition { .. },
                                InputKind::AbsolutePointer { .. },
                            ) => true,
                            (
                                InputEvent::PointerButton { button, .. },
                                InputKind::RelativePointer { buttons }
                                | InputKind::AbsolutePointer { buttons, .. },
                            ) => button < buttons,
                            (InputEvent::Pad { control, .. }, InputKind::Pad { controls }) => {
                                controls.contains(control)
                            }
                            _ => false,
                        }
                })
            }
        };
        if !valid {
            return Err(Error::new(
                ErrorKind::Usage,
                "invalid input port or control",
            ));
        }
        self.pending.push(event);
        Ok(())
    }
    fn slot_states(&mut self) -> Vec<SlotState> {
        vec![SlotState::new(SlotId(0))
            .with_catalogue(self.catalogue.clone())
            .with_inserted(self.inserted)
            .with_accepts_changes(
                self.interactive().is_ok() && self.description.features.media_changes,
            )]
    }
    fn add_medium(&mut self, slot: SlotId, medium: NewMedium<'_>) -> Result<usize, Error> {
        self.media_check(slot, true)?;
        let (kind, kept) = match medium.writes {
            WriteTarget::ReadOnly => (WriteKind::ReadOnly, None),
            WriteTarget::Discarded => (WriteKind::Discarded, None),
            WriteTarget::Kept { record } => {
                if !is_kept_record_name(record) {
                    return Err(Self::usage());
                }
                (WriteKind::Kept, Some(record.to_owned()))
            }
        };
        let index = self.catalogue.len();
        self.catalogue.push(MediumInfo::new(
            medium.name,
            medium.sha256,
            medium.size,
            medium.format,
            kind,
        ));
        self.kept.push(kept);
        Ok(index)
    }
    fn change_medium(&mut self, slot: SlotId, action: MediaAction) -> Result<(), Error> {
        self.media_check(slot, false)?;
        let index = match action {
            MediaAction::Eject => None,
            MediaAction::Insert(i) if i < self.catalogue.len() => Some(i),
            MediaAction::Next if !self.catalogue.is_empty() => {
                Some(self.inserted.map_or(0, |i| (i + 1) % self.catalogue.len()))
            }
            _ => return Err(Self::usage()),
        };
        if index == self.inserted {
            return Ok(());
        }
        if let (Some(old), Some(new)) = (self.inserted, index) {
            let previous = &self.catalogue[old];
            let selected = &self.catalogue[new];
            if self.record.is_some()
                && self.kept[old] == self.kept[new]
                && previous.sha256 == selected.sha256
                && previous.size == selected.size
                && previous.format == selected.format
            {
                // Aliases share one overlay, whose exclusive handle is already held.
                self.inserted = index;
                return Ok(());
            }
        }
        // Acquire the new record before releasing the old insertion so ordinary
        // opening failures leave the previous medium usable.
        let mut record = match index.and_then(|i| self.kept[i].as_deref()) {
            Some(name) => match self.storage.open(name) {
                Ok(record) => Some(record),
                Err(error) => {
                    if error.kind == ErrorKind::StorageUncertain {
                        self.status = Status::Failed;
                    }
                    return Err(error);
                }
            },
            None => None,
        };
        let generation = (|| {
            let mut generation = Vec::new();
            if let Some(record) = &mut record {
                let size = usize::try_from(record.size()?).map_err(|_| Self::usage())?;
                generation.resize(size, 0);
                record.read_at(0, &mut generation)?;
            }
            Ok::<_, Error>(generation)
        })()
        .inspect_err(|error| {
            if error.kind == ErrorKind::StorageUncertain {
                self.status = Status::Failed;
            }
        })?;
        self.generation = generation;
        self.record = record;
        self.inserted = index;
        Ok(())
    }
    fn reset(&mut self, kind: ResetKind) -> Result<(), Error> {
        self.live()?;
        if self.status == Status::Off && kind != ResetKind::Hard {
            return Err(Self::usage());
        }
        if !self.description.features.resets.contains(&kind) {
            return Err(Error::new(ErrorKind::Unsupported, "reset unavailable"));
        }
        if let Some(error) = self.reset_error.take() {
            if error.kind == ErrorKind::StorageUncertain && self.status != Status::Halted {
                self.status = Status::Failed;
            }
            return Err(error);
        }
        self.pending.clear();
        self.held
            .retain(|e| matches!(e, InputEvent::PointerPosition { .. }));
        self.status = Status::Ready;
        Ok(())
    }
    fn state_identity(&self) -> Option<String> {
        self.description.features.saved_state.then(|| {
            format!(
                "scripted-v1:{:?}:{:?}:{:?}:{:?}",
                self.catalogue, self.kept, self.inserted, self.generation
            )
        })
    }
    fn save_state(&mut self, out: &mut dyn Write) -> Result<(), Error> {
        self.live()?;
        let identity = self
            .state_identity()
            .ok_or_else(|| Error::new(ErrorKind::Unsupported, "state unavailable"))?;
        // The model never writes kept records; their exact opened bytes are
        // part of the token, so an external generation change rejects a load.
        let mut bytes = Vec::new();
        put(&mut bytes, identity.len() as u64);
        bytes.extend(identity.as_bytes());
        for n in [
            self.now,
            self.steps,
            self.status as u64,
            self.pixels[0] as u64,
            self.serial,
            self.picture as u64,
            self.inserted.map_or(u64::MAX, |i| i as u64),
            self.aspect.0 as u64,
            self.aspect.1 as u64,
            self.visible.x as u64,
            self.visible.y as u64,
            self.visible.width as u64,
            self.visible.height as u64,
        ] {
            put(&mut bytes, n);
        }
        events(&mut bytes, &self.pending);
        events(&mut bytes, &self.held);
        out.write_all(&bytes)
            .map_err(|e| Error::new(ErrorKind::Storage, e.to_string()))
    }
    fn load_state(&mut self, state: &mut dyn Read) -> Result<(), Error> {
        self.live()?;
        let identity = self
            .state_identity()
            .ok_or_else(|| Error::new(ErrorKind::Unsupported, "state unavailable"))?;
        let invalid = || Error::new(ErrorKind::State, "incompatible or malformed state");
        let mut bytes = Vec::new();
        state
            .read_to_end(&mut bytes)
            .map_err(|e| Error::new(ErrorKind::Storage, e.to_string()))?;
        let mut data = bytes.as_slice();
        let len = take(&mut data).ok_or_else(invalid)?;
        if len != identity.len() as u64 || !data.starts_with(identity.as_bytes()) {
            return Err(invalid());
        }
        data = &data[identity.len()..];
        let now = take(&mut data).ok_or_else(invalid)?;
        let steps = take(&mut data).ok_or_else(invalid)?;
        let status = match take(&mut data) {
            Some(0) => Status::Ready,
            Some(1) => Status::Halted,
            Some(2) => Status::Off,
            _ => return Err(invalid()),
        };
        let pixel = u32::try_from(take(&mut data).ok_or_else(invalid)?).map_err(|_| invalid())?;
        let serial = take(&mut data).ok_or_else(invalid)?;
        let picture = match take(&mut data) {
            Some(0) => false,
            Some(1) => true,
            _ => return Err(invalid()),
        };
        let inserted = match take(&mut data).ok_or_else(invalid)? {
            u64::MAX => None,
            i if i < self.catalogue.len() as u64 => Some(i as usize),
            _ => return Err(invalid()),
        };
        let aspect = (
            u32::try_from(take(&mut data).ok_or_else(invalid)?).map_err(|_| invalid())?,
            u32::try_from(take(&mut data).ok_or_else(invalid)?).map_err(|_| invalid())?,
        );
        let mut area = [0u32; 4];
        for n in &mut area {
            *n = u32::try_from(take(&mut data).ok_or_else(invalid)?).map_err(|_| invalid())?;
        }
        let visible = FrameArea::new(area[0], area[1], area[2], area[3]);
        if aspect.0 == 0 || aspect.1 == 0 || !visible.is_within(self.width, self.height) {
            return Err(invalid());
        }
        let pending = read_events(&mut data).ok_or_else(invalid)?;
        let held = read_events(&mut data).ok_or_else(invalid)?;
        if !data.is_empty() || inserted != self.inserted {
            return Err(invalid());
        }
        self.now = now;
        self.steps = steps;
        self.status = status;
        self.pixels[0] = pixel;
        self.serial = serial;
        self.picture = picture;
        self.inserted = inserted;
        self.aspect = aspect;
        self.visible = visible;
        self.pending = pending;
        self.held = held;
        self.audio.clear();
        Ok(())
    }
    fn finish(&mut self) -> Result<Finished, Error> {
        self.live()?;
        if let Some(error) = self.finish_error.take() {
            self.status = Status::Failed;
            return Err(error);
        }
        if let Some(record) = &mut self.record {
            if let Err(error) = record.sync() {
                self.status = Status::Failed;
                return Err(error);
            }
        }
        self.status = Status::Finished;
        Ok(Finished::new())
    }
    fn drain_messages(&mut self, out: &mut Vec<Message>) {
        out.append(&mut self.messages);
    }
}

fn put(out: &mut Vec<u8>, n: u64) {
    out.extend(n.to_le_bytes());
}
fn take(data: &mut &[u8]) -> Option<u64> {
    let bytes = data.get(..8)?;
    let n = u64::from_le_bytes(bytes.try_into().ok()?);
    *data = &data[8..];
    Some(n)
}
fn events(out: &mut Vec<u8>, events: &[InputEvent]) {
    put(out, events.len() as u64);
    for event in events {
        let fields = match *event {
            InputEvent::ReleaseAll => [0, 0, 0, 0],
            InputEvent::Key { port, key, state } => [1, port.0 as u64, key.0 as u64, state as u64],
            InputEvent::PointerMotion { port, dx, dy } => {
                [2, port.0 as u64, dx as u32 as u64, dy as u32 as u64]
            }
            InputEvent::PointerButton {
                port,
                button,
                pressed,
            } => [3, port.0 as u64, button as u64, pressed as u64],
            InputEvent::Pad {
                port,
                control,
                pressed,
            } => [4, port.0 as u64, control as u64, pressed as u64],
            InputEvent::PointerPosition { port, position } => [
                5,
                port.0 as u64,
                position.map_or(u64::MAX, |(x, y)| (x as u64) << 16 | y as u64),
                0,
            ],
        };
        for n in fields {
            put(out, n);
        }
    }
}
fn read_events(data: &mut &[u8]) -> Option<Vec<InputEvent>> {
    let count = take(data)?;
    if count > (data.len() / 32) as u64 {
        return None;
    }
    let mut result = Vec::new();
    for _ in 0..count {
        let tag = take(data)?;
        let port = PortId(u8::try_from(take(data)?).ok()?);
        let a = take(data)?;
        let b = take(data)?;
        let event = match tag {
            0 if a == 0 && b == 0 => InputEvent::ReleaseAll,
            1 => InputEvent::key(
                port,
                Key(u16::try_from(a).ok()?),
                match b {
                    0 => KeyState::Pressed,
                    1 => KeyState::Repeated,
                    2 => KeyState::Released,
                    _ => return None,
                },
            ),
            2 => InputEvent::pointer_motion(
                port,
                u32::try_from(a).ok()? as i32,
                u32::try_from(b).ok()? as i32,
            ),
            3 if b <= 1 => InputEvent::pointer_button(port, u8::try_from(a).ok()?, b == 1),
            4 if b <= 1 => InputEvent::pad(
                port,
                *[
                    PadControl::Up,
                    PadControl::Down,
                    PadControl::Left,
                    PadControl::Right,
                    PadControl::A,
                    PadControl::B,
                    PadControl::X,
                    PadControl::Y,
                    PadControl::L,
                    PadControl::R,
                    PadControl::Start,
                    PadControl::Select,
                ]
                .get(usize::try_from(a).ok()?)?,
                b == 1,
            ),
            5 if b == 0 => InputEvent::pointer_position(
                port,
                if a == u64::MAX {
                    None
                } else if a <= u32::MAX as u64 {
                    Some(((a >> 16) as u16, a as u16))
                } else {
                    return None;
                },
            ),
            _ => return None,
        };
        result.push(event);
    }
    Some(result)
}
