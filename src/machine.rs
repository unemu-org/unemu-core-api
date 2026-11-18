// SPDX-License-Identifier: MIT OR Apache-2.0
//! The running machine: its description, its time and the one trait a front
//! end drives (core-contract.md §3.5 to §3.10).

use crate::{
    AudioDescription, DisplayDescription, DisplayId, Error, Features, Finished, Frame, InputEvent,
    InputPortDescription, MediaAction, Message, NewMedium, Outputs, ResetKind, SlotDescription,
    SlotId, SlotState,
};
use std::io::{Read, Write};

/// One running machine.
///
/// A machine lives on the thread that created it; it need not be `Send`.
/// It advances only inside [`Machine::run`]. Between calls nothing happens:
/// pause is the front end not calling `run`.
///
/// Determinism (§3.11): no call takes or returns host time. The same
/// creation followed by the same sequence of calls with the same arguments
/// gives the same guest and the same pictures, sound, messages and stops, so
/// a front end that records its calls with [`Machine::now`] and
/// [`Machine::steps`] at each boundary can replay a session. Two different
/// ways of slicing a span into runs need not give the same guest; a front
/// end that needs slice independence drives by steps. Sound and picture
/// never pace execution, and host failures never reach the guest.
pub trait Machine {
    /// What this machine has. Constant for the machine's life.
    fn description(&self) -> &MachineDescription;

    /// Virtual time now, in ticks at `description().clock`.
    fn now(&self) -> Ticks;

    /// Work done so far, in the core's own step unit
    /// (`description().step_unit`), for budgets that must not depend on the
    /// clock rate.
    fn steps(&self) -> u64;

    /// Whether the machine can run.
    fn status(&self) -> Status;

    /// Advance until a limit or an event, whichever comes first.
    ///
    /// Input and media changes made since the last return take effect
    /// first, at `now()`. A deadline is met at the core's next boundary, so
    /// `now()` may pass `limit.until` by less than one step. A core that
    /// turns the deadline into a budget of steps may also answer
    /// [`Stop::Deadline`] with `now()` short of `limit.until` by less than
    /// one step (time that passed while the guest waited rounds down); the
    /// front end runs again while it is short. Otherwise a run never returns
    /// before a limit without an event. A guest-requested reset is not an
    /// event: the core performs it inside the run and reports it as a
    /// [`Message`]. Fails with [`crate::ErrorKind::Guest`] when the emulated
    /// machine cannot continue and with [`crate::ErrorKind::Storage`] when a
    /// host write failed; either leaves the status [`Status::Failed`],
    /// except that a `Storage` failure while the guest halts (making its
    /// writes durable) may leave the status [`Status::Halted`], so the
    /// session can still be finished. `limit.stop_on_frame` on a machine
    /// whose features do not list [`crate::Features::frame_stops`] fails
    /// with [`crate::ErrorKind::Unsupported`].
    fn run(&mut self, limit: RunLimit) -> Result<Stop, Error>;

    /// The picture of a display at the last completed boundary.
    ///
    /// After an error the last picture stays readable. The borrow ends
    /// before the next call on the machine.
    fn frame(&mut self, display: DisplayId) -> Result<Frame<'_>, Error>;

    /// Move the sound rendered up to `now()` since the last call to the end
    /// of `out`, at `description().audio` rate.
    fn drain_audio(&mut self, out: &mut Vec<[i16; 2]>) -> Result<(), Error>;

    /// Say which outputs the front end consumes now. A core need not render
    /// or keep history for an output nobody takes.
    fn set_outputs(&mut self, outputs: Outputs);

    /// Queue one input event. It reaches the guest at the start of the next
    /// `run`, in order, at `now()`. The core owns everything after that:
    /// encoding, repeat, device back-pressure, coalescing and overflow.
    fn input(&mut self, event: InputEvent);

    /// Every slot's state now. Reading it takes the activity seen since the
    /// last call, for drive lights.
    fn slot_states(&mut self) -> Vec<SlotState>;

    /// Add a medium to a slot's catalogue and return its index. It is not
    /// inserted by this call.
    fn add_medium(&mut self, slot: SlotId, medium: NewMedium<'_>) -> Result<usize, Error>;

    /// Change what a slot holds, at the next run boundary.
    fn change_medium(&mut self, slot: SlotId, action: MediaAction) -> Result<(), Error>;

    /// Reset the machine. Fails with `Unsupported` for a kind the
    /// description does not list, and with `Usage` when the kind is not
    /// possible in the current status. A `Storage` or `Guest` failure of the
    /// reset itself leaves the status as it was (a halted machine stays
    /// `Halted`, so its session can still be finished).
    fn reset(&mut self, kind: ResetKind) -> Result<(), Error>;

    /// A token naming everything a saved state depends on: the core's
    /// release, the machine's config and the identity of its media. A state
    /// loads only into a machine with an equal token. `None` when the
    /// machine cannot save state.
    fn state_identity(&self) -> Option<String>;

    /// Write the whole machine state at the current boundary.
    fn save_state(&mut self, out: &mut dyn Write) -> Result<(), Error>;

    /// Replace the machine state with a saved one. On failure the machine
    /// is unchanged.
    fn load_state(&mut self, state: &mut dyn Read) -> Result<(), Error>;

    /// End the session cleanly: make everything written durable and close
    /// it as complete. Not valid once the status is `Failed`. Dropping a
    /// machine without `finish` is the equivalent of a power cut: what was
    /// already durable stays, and nothing is marked complete.
    fn finish(&mut self) -> Result<Finished, Error>;

    /// Move the core's messages since the last call to the end of `out`. A
    /// core never writes to the process's standard streams.
    fn drain_messages(&mut self, out: &mut Vec<Message>);
}

/// What one machine has.
#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct MachineDescription {
    /// The rate of [`Ticks`], exact.
    pub clock: Rate,
    /// A word for one step, for messages: `instruction`, `cycle`.
    pub step_unit: &'static str,
    pub displays: Vec<DisplayDescription>,
    pub audio: Option<AudioDescription>,
    pub inputs: Vec<InputPortDescription>,
    pub slots: Vec<SlotDescription>,
    /// What this machine can do, within its platform's `Features`.
    pub features: Features,
}

impl MachineDescription {
    /// A machine with no display, sound, input port or slot, and no feature.
    pub fn new(clock: Rate, step_unit: &'static str) -> Self {
        Self {
            clock,
            step_unit,
            displays: Vec::new(),
            audio: None,
            inputs: Vec::new(),
            slots: Vec::new(),
            features: Features::NONE,
        }
    }

    pub fn with_displays(mut self, displays: Vec<DisplayDescription>) -> Self {
        self.displays = displays;
        self
    }

    pub fn with_audio(mut self, audio: Option<AudioDescription>) -> Self {
        self.audio = audio;
        self
    }

    pub fn with_inputs(mut self, inputs: Vec<InputPortDescription>) -> Self {
        self.inputs = inputs;
        self
    }

    pub fn with_slots(mut self, slots: Vec<SlotDescription>) -> Self {
        self.slots = slots;
        self
    }

    pub fn with_features(mut self, features: Features) -> Self {
        self.features = features;
        self
    }
}

/// A point in virtual time.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ticks(pub u64);

/// An exact rate per second, as a fraction in lowest terms.
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rate {
    pub numerator: u64,
    pub denominator: u64,
}

impl Rate {
    /// `numerator / denominator` per second, reduced to lowest terms. A zero
    /// denominator is kept as given and names no rate.
    pub const fn new(numerator: u64, denominator: u64) -> Self {
        let mut a = numerator;
        let mut b = denominator;
        while b != 0 {
            let r = a % b;
            a = b;
            b = r;
        }
        if a <= 1 || denominator == 0 {
            Self {
                numerator,
                denominator,
            }
        } else {
            Self {
                numerator: numerator / a,
                denominator: denominator / a,
            }
        }
    }
}

/// Where a run stops if nothing else stops it first. Built with
/// [`RunLimit::until`], [`RunLimit::steps`] and `with_…` setters.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct RunLimit {
    /// Stop at this virtual time.
    pub until: Option<Ticks>,
    /// Stop when [`Machine::steps`] reaches this value.
    pub steps: Option<u64>,
    /// Stop when a display finishes a frame; only for a machine whose
    /// features list [`crate::Features::frame_stops`].
    pub stop_on_frame: bool,
}

impl RunLimit {
    /// Stop at virtual time `until`.
    pub const fn until(until: Ticks) -> Self {
        Self {
            until: Some(until),
            steps: None,
            stop_on_frame: false,
        }
    }

    /// Stop when [`Machine::steps`] reaches `steps`.
    pub const fn steps(steps: u64) -> Self {
        Self {
            until: None,
            steps: Some(steps),
            stop_on_frame: false,
        }
    }

    pub const fn with_until(mut self, until: Option<Ticks>) -> Self {
        self.until = until;
        self
    }

    pub const fn with_steps(mut self, steps: Option<u64>) -> Self {
        self.steps = steps;
        self
    }

    pub const fn with_stop_on_frame(mut self, stop_on_frame: bool) -> Self {
        self.stop_on_frame = stop_on_frame;
        self
    }
}

/// Why a run returned.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Stop {
    /// `limit.until` was reached.
    Deadline,
    /// `limit.steps` was reached.
    Steps,
    /// A display finished a frame.
    Frame(DisplayId),
    /// The guest stopped executing and nothing scheduled can wake it; the
    /// status is [`Status::Halted`]. A reset or a media change may still
    /// follow.
    Halted,
    /// The guest switched the machine off.
    PoweredOff,
}

/// Whether a machine can run.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum Status {
    /// [`Machine::run`] advances the machine.
    Ready,
    /// The guest waits for a reset; `run` returns [`Stop::Halted`] at once.
    Halted,
    /// The guest switched the machine off.
    Off,
    /// A `Guest` or `Storage` error ended the run; only messages, the last
    /// frame and dropping the machine remain.
    Failed,
}
