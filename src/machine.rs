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
///
/// Call/status matrix (every cell gives the result and following status).
/// `same` means unchanged status; `inspect` means the documented observation
/// or drain, with no guest execution. Apply the matrix's status rejections
/// first (including Off), then return `Unsupported` for an absent capability,
/// then check arguments (`Usage` if invalid). These rejections preserve status.
/// Halted and Off runs return their stop before capability or argument checks.
///
/// | Method | Ready | Halted | Off | Failed | Finished |
/// | --- | --- | --- | --- | --- | --- |
/// | `description` | description; same | description; same | description; same | description; same | description; same |
/// | `now` | ticks; same | ticks; same | ticks; same | ticks; same | ticks; same |
/// | `steps` | total; same | total; same | total; same | total; same | total; same |
/// | `status` | Ready | Halted | Off | Failed | Finished |
/// | `frame` | inspect; same | inspect; same | inspect; same | inspect; same | inspect; same |
/// | `drain_audio` | inspect; same | inspect; same | inspect; same | inspect; same | inspect; same |
/// | `set_outputs` | set; same | set; same | set; same | ignored; Failed | ignored; Finished |
/// | `input` | queue; same | queue; same | Usage; Off | Usage; Failed | Usage; Finished |
/// | `slot_states` | inspect; same | inspect; same | inspect; same | inspect; same | inspect; same |
/// | `state_identity` | token/None; same | token/None; same | token/None; same | token/None; same | token/None; same |
/// | `drain_messages` | drain; same | drain; same | drain; same | drain; same | drain; same |
/// | `run` | stop; see below | Halted stop; Halted | PoweredOff stop; Off | Usage; Failed | Usage; Finished |
/// | `add_medium` | index; same | index; same | Usage; Off | Usage; Failed | Usage; Finished |
/// | `change_medium` | success; same | success; same | Usage; Off | Usage; Failed | Usage; Finished |
/// | `reset` | success; Ready | success; Ready | Hard: success; Ready; Soft: Usage; Off | Usage; Failed | Usage; Finished |
/// | `save_state` | success; same | success; same | success; same | Usage; Failed | Usage; Finished |
/// | `load_state` | success; restored status | success; restored status | success; restored status | Usage; Failed | Usage; Finished |
/// | `finish` | report; Finished | report; Finished | report; Finished | Usage; Failed | Usage; Finished |
///
/// In Ready, a run with no deadline, step total or frame stop is allowed:
/// it runs until an event (halt, power-off or error). A requested frame stop
/// also ends a run. Front ends enforce their own budgets. Unsupported frame
/// stops are rejected before consuming input. A valid run applies queued input
/// in order at the current time, even if a limit was already reached; it then
/// returns without executing a step for that limit.
/// Halted and Off runs return their stop immediately, without consuming input
/// or validating limits. A run does not wake a halted machine: reset does.
///
/// At a boundary with simultaneous reasons, report an execution error first,
/// then power-off, halt, requested frame, deadline, steps (in that order).
/// An event not requested as a stop does not end the run. Deadline, Steps and
/// Frame leave Ready; Halted leaves Halted; PoweredOff leaves Off. Execution
/// errors follow `run`'s failure rules below. Before a tick or step counter
/// would overflow, fail with `Guest` and leave Failed; never wrap or silently
/// saturate. Already-reached limits take precedence over attempting more work.
///
/// Both reset kinds preserve `now()` and the absolute step total and take no
/// virtual time. Successful reset clears queued input and held controls,
/// preserving absolute pointer positions; media stay inserted. A failed reset
/// keeps the queue and counters. Unsupported reset kinds leave status unchanged.
/// Reset failures otherwise follow `reset`'s documented rules.
///
/// Media additions and selections complete immediately, including required
/// content/record opens. The call doing the open reports its error; it must
/// not defer that error to `run`. Usage, Unsupported, Content and ordinary
/// Storage errors preserve the previous catalogue, insertion and status.
/// StorageUncertain leaves Failed (without promising rollback of storage).
/// A successful media call does not wake Halted. Selection requires
/// `media_changes`; extension requires `media_additions`; neither implies the
/// other. Slot removability and `accepts_changes` still constrain selection.
/// A non-removable slot cannot be extended. Compatible catalogue aliases
/// with the same base identity, format and kept-record name reuse the held
/// record when selected; they do not reopen an exclusively held record.
///
/// A failed finish leaves Failed; a second finish after success returns Usage.
/// Failed sessions cannot be marked complete: drop/recreate is the contract's
/// recovery path. Read-only inspection and drains remain available in both
/// terminal statuses. `set_outputs` has no error return and is ignored there.
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
    /// Queued input takes effect first, at `now()`. Media changes have
    /// already taken effect when their calls returned. A deadline is met at
    /// the core's next boundary, so
    /// `now()` may pass `limit.until` by less than one step. A core that
    /// turns the deadline into a budget of steps may also answer
    /// [`Stop::Deadline`] with `now()` short of `limit.until` by less than
    /// one step (time that passed while the guest waited rounds down); the
    /// front end runs again while it is short. Otherwise a run never returns
    /// before a limit without an event. A guest-requested reset is not an
    /// event: the core performs it inside the run and reports it as a
    /// [`Message`]. `limit.stop_on_frame` on a machine whose features do
    /// not list [`crate::Features::frame_stops`] fails with
    /// [`crate::ErrorKind::Unsupported`].
    ///
    /// A run fails, leaving the status [`Status::Failed`], with:
    ///
    /// - [`crate::ErrorKind::Guest`] when the emulated machine cannot
    ///   continue;
    /// - [`crate::ErrorKind::Storage`] when a host read or write of the
    ///   machine's storage failed during execution;
    /// - [`crate::ErrorKind::StorageUncertain`] when a host write failed
    ///   with an unknown outcome;
    /// - [`crate::ErrorKind::Content`] when a [`crate::ContentReader`]
    ///   reported that its bytes are no longer the verified ones.
    ///
    /// A `Storage` or `StorageUncertain` failure while the guest halts
    /// (making its writes durable) may leave the status [`Status::Halted`]
    /// instead, so the session can still be finished. Storage follows
    /// [`crate::Record`]: a destructive cut can discard synchronized bytes;
    /// a failed append may leave a partial tail. Replacement preserves old
    /// or new contents whole. The front end retries by dropping the machine
    /// and creating it again; reopening allows writes without rolling back
    /// to the last successful sync. The core recovers its own record format.
    fn run(&mut self, limit: RunLimit) -> Result<Stop, Error>;

    /// The picture of a display at the last completed boundary.
    ///
    /// A machine that draws its picture line by line returns the last
    /// picture it finished, never one it is still drawing, so a front end
    /// that runs by deadline shows whole frames without
    /// [`RunLimit::stop_on_frame`]. After an error the last picture stays
    /// readable. Before the first completed picture a core may return
    /// `Usage` or a blank frame of its initial geometry. Front ends must
    /// handle both. The borrow ends before the next call on the machine.
    fn frame(&mut self, display: DisplayId) -> Result<Frame<'_>, Error>;

    /// Move the sound rendered up to `now()` since the last call to the end
    /// of `out`, at `description().audio` rate.
    ///
    /// The sound keeps its place in virtual time, where `rate` is the audio
    /// rate and `clock` is `description().clock`. Frame `i` is the sound at virtual time `i × clock / rate` ticks
    /// from creation. At every return of [`Machine::run`], once `drain_audio`
    /// has been called, the next frame it will hand over has index
    /// `floor(now() × rate / clock)`, within one frame. Frames not drained
    /// when sound is switched off ([`Machine::set_outputs`]), or before
    /// [`Machine::load_state`], are dropped; after `load_state` the index
    /// follows the loaded `now()`. A front
    /// end may rely on this to keep sound and picture together.
    fn drain_audio(&mut self, out: &mut Vec<[i16; 2]>) -> Result<(), Error>;

    /// Say which outputs the front end consumes now. A core need not render
    /// or keep history for an output nobody takes.
    fn set_outputs(&mut self, outputs: Outputs);

    /// Queue one input event. It reaches the guest at the start of the next
    /// `run`, in order, at `now()`. The core owns everything after that:
    /// encoding, repeat, device back-pressure, coalescing and overflow.
    /// Returns `Usage` in Off, Failed or Finished, without queuing the event.
    fn input(&mut self, event: InputEvent) -> Result<(), Error>;

    /// Every slot's state now. Reading it takes the activity seen since the
    /// last call, for drive lights.
    fn slot_states(&mut self) -> Vec<SlotState>;

    /// Add a medium to a slot's catalogue and return its index. It is not
    /// inserted by this call. Requires `Features::media_additions`.
    /// Catalogue extension is immediate; any required open fails this call.
    /// Fails with [`crate::ErrorKind::Usage`] when
    /// the medium's [`crate::WriteTarget::Kept`] names a record outside the
    /// rules of [`crate::is_kept_record_name`].
    fn add_medium(&mut self, slot: SlotId, medium: NewMedium<'_>) -> Result<usize, Error>;

    /// Change what a slot holds immediately, before returning success.
    /// Requires `Features::media_changes`. Required content/record opens
    /// complete here; their failure is returned here, never on a later run.
    fn change_medium(&mut self, slot: SlotId, action: MediaAction) -> Result<(), Error>;

    /// Reset the machine. Fails with `Unsupported` for a kind the
    /// description does not list, and with `Usage` when the kind is not
    /// possible in the current status. A `Storage` or `Guest` failure of the
    /// reset itself leaves the status as it was (a halted machine stays
    /// `Halted`, so its session can still be finished). A
    /// `StorageUncertain` failure leaves the status [`Status::Failed`], or
    /// `Halted` for a machine that was halted, as in [`Machine::run`].
    fn reset(&mut self, kind: ResetKind) -> Result<(), Error>;

    /// A token naming everything a saved state depends on: the core's
    /// release, the machine's config and the identity of its media. A state
    /// loads only into a machine with an equal token. `None` when the
    /// machine cannot save state. The token is opaque, with no text framing
    /// restrictions. It covers core release, effective config, catalogue and
    /// immutable base identities (digest and size). Mutable overlays and
    /// battery memory must either be captured by the state or have their
    /// generation included in the token, so incompatible external contents
    /// cannot silently be loaded. Tokens may change after media changes.
    fn state_identity(&self) -> Option<String>;

    /// Write the whole machine state at the current boundary, including time,
    /// step total, Ready/Halted/Off status, queued input and held controls.
    /// Saving does not change guest state or make external records durable.
    fn save_state(&mut self, out: &mut dyn Write) -> Result<(), Error>;

    /// Replace the machine state with a compatible saved one. Success restores
    /// time, steps, status, queued input and held controls; old pending audio
    /// is dropped and its next index follows restored time. Output preferences
    /// stay with the caller. Loading never rewinds or commits external records.
    /// A state that needs incompatible external data is rejected with `State`.
    /// On any failure the observable machine state, queues and external records
    /// are unchanged; consuming bytes from `state` is allowed. Implementations
    /// must validate/stage before applying rather than partially mutate.
    fn load_state(&mut self, state: &mut dyn Read) -> Result<(), Error>;

    /// End the session cleanly: make everything written durable and close
    /// it as complete and enter [`Status::Finished`]. In Failed or Finished
    /// it returns `Usage` without work; a second successful finish is forbidden.
    /// A failed `finish` leaves the status [`Status::Failed`]. Storage follows
    /// [`crate::Record`], including destructive cuts and partial tails;
    /// replacement preserves old or new contents whole. Dropping a machine
    /// without `finish` performs no durability barrier and marks nothing
    /// complete. An ordinary recreate sees current storage, without rollback;
    /// an actual power failure may lose unsynchronized changes.
    fn finish(&mut self) -> Result<Finished, Error>;

    /// Move the core's messages since the last call to the end of `out`. A
    /// core never writes to the process's standard streams.
    fn drain_messages(&mut self, out: &mut Vec<Message>);
}

/// What one machine has.
#[non_exhaustive]
#[derive(Clone, Debug)]
pub struct MachineDescription {
    /// The positive rate of [`Ticks`], exact (`Rate::is_clock`).
    pub clock: Rate,
    /// A word for one step, for messages: `instruction`, `cycle`.
    pub step_unit: &'static str,
    /// Display ids are unique within this list.
    pub displays: Vec<DisplayDescription>,
    pub audio: Option<AudioDescription>,
    /// The input ports, with unique ids, constant for the machine's life.
    /// Which device sits
    /// in a controller port (a pad, a light gun, nothing) is part of the
    /// machine's config, and these ports show the result. The ports of
    /// kind [`crate::InputKind::Pad`] are the players, first to last, in
    /// ascending [`crate::PortId`].
    pub inputs: Vec<InputPortDescription>,
    /// Slot ids are unique within this list.
    pub slots: Vec<SlotDescription>,
    /// What this machine can do, within its platform's `Features`.
    pub features: Features,
}

impl MachineDescription {
    /// A machine with no display, sound, input port or slot, and no feature.
    ///
    /// # Panics
    /// Panics when `clock` is not positive.
    pub fn new(clock: Rate, step_unit: &'static str) -> Self {
        assert!(clock.is_clock(), "a machine clock must be positive");
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

    /// Set displays; panics if ids repeat. Public-field mutation must also preserve uniqueness.
    pub fn with_displays(mut self, displays: Vec<DisplayDescription>) -> Self {
        assert!(
            displays
                .iter()
                .enumerate()
                .all(|(i, item)| displays[..i].iter().all(|previous| previous.id != item.id)),
            "duplicate displays id"
        );
        self.displays = displays;
        self
    }

    pub fn with_audio(mut self, audio: Option<AudioDescription>) -> Self {
        self.audio = audio;
        self
    }

    /// Set inputs; panics if ids repeat. Public-field mutation must also preserve uniqueness.
    pub fn with_inputs(mut self, inputs: Vec<InputPortDescription>) -> Self {
        assert!(
            inputs
                .iter()
                .enumerate()
                .all(|(i, item)| inputs[..i].iter().all(|previous| previous.id != item.id)),
            "duplicate inputs id"
        );
        self.inputs = inputs;
        self
    }

    /// Set slots; panics if ids repeat. Public-field mutation must also preserve uniqueness.
    pub fn with_slots(mut self, slots: Vec<SlotDescription>) -> Self {
        assert!(
            slots
                .iter()
                .enumerate()
                .all(|(i, item)| slots[..i].iter().all(|previous| previous.id != item.id)),
            "duplicate slots id"
        );
        self.slots = slots;
        self
    }

    pub fn with_features(mut self, features: Features) -> Self {
        self.features = features;
        self
    }
}

/// A point in virtual time. Deliberately closed scalar representation.
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct Ticks(pub u64);

/// An exact rate per second, as a fraction in lowest terms.
///
/// A zero denominator has always been invalid. Zero numerator is valid for
/// a general rate, but has always been invalid for a machine clock. Public
/// fields permit inspection and mutation; callers must preserve these rules.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq, Hash)]
pub struct Rate {
    pub numerator: u64,
    pub denominator: u64,
}

impl Rate {
    /// Validate and reduce a rate, returning `None` for a zero denominator.
    pub const fn checked_new(numerator: u64, denominator: u64) -> Option<Self> {
        if denominator == 0 {
            None
        } else {
            Some(Self::new(numerator, denominator))
        }
    }

    /// Whether this rate can be a machine clock.
    pub const fn is_clock(self) -> bool {
        self.numerator != 0 && self.denominator != 0
    }

    /// `numerator / denominator` per second, reduced to lowest terms.
    ///
    /// # Panics
    /// Panics if `denominator` is zero. Use [`Self::checked_new`] for untrusted values.
    pub const fn new(numerator: u64, denominator: u64) -> Self {
        assert!(denominator != 0, "a rate needs a nonzero denominator");
        let mut a = numerator;
        let mut b = denominator;
        while b != 0 {
            let r = a % b;
            a = b;
            b = r;
        }
        if a <= 1 {
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

    /// Stop at an absolute total of retired steps, not `steps` more.
    /// Equivalent to [`Self::at_steps`]. For ten more steps, use
    /// `RunLimit::at_steps(machine.steps().checked_add(10).unwrap())`.
    pub const fn steps(steps: u64) -> Self {
        Self {
            until: None,
            steps: Some(steps),
            stop_on_frame: false,
        }
    }

    /// Stop when [`Machine::steps`] reaches this absolute total.
    pub const fn at_steps(steps: u64) -> Self {
        Self::steps(steps)
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
    /// Execution or finalization failed. Inspection remains available;
    /// execution and mutation are forbidden. Drop and recreate to retry.
    Failed,
    /// A successful [`Machine::finish`] ended the session. Inspection
    /// remains available; execution, mutation and another finish are forbidden.
    Finished,
}
