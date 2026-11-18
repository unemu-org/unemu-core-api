// SPDX-License-Identifier: MIT OR Apache-2.0
//! The contract between front ends and emulator cores; see
//! `backlog/docs/design/core-contract.md` §3.
//!
//! A front end holds every compiled-in [`Core`] and asks it for machines. A
//! machine is driven by five kinds of call on [`Machine`]: run to a limit,
//! read the picture and the sound, send input, change media, and control
//! (reset, save and load state, finish). Pause is the absence of a call.
//!
//! What a core can do is described as data, at three levels each narrower
//! than the last: the core ([`CoreDescription`], which platforms), the
//! platform ([`PlatformDescription`] and its [`Features`], what any of its
//! machines may do) and the machine ([`MachineDescription`], what this one
//! has). A front end enables a control from a description and never by trying
//! a call; a call the description promised that answers
//! [`ErrorKind::Unsupported`] is a core bug.
//!
//! Rules of the crate (§3.1):
//!
//! - No dependencies, `std` only. Errors implement [`std::error::Error`] by
//!   hand, so nothing here forces a version of another crate on a core.
//! - Object-safe traits without generics: a front end holds `&dyn Core` and
//!   `Box<dyn Machine>`.
//! - Additive: every struct and enum is `#[non_exhaustive]`; a struct a core
//!   builds has a `new` with its required fields and `with_…` setters. A minor
//!   version only adds fields, variants and trait methods with a default body.
//! - No platform words: no port, interrupt line, scan code, disk geometry,
//!   cartridge mapper or firmware catalogue appears here.
//! - No host clock, no randomness, no threads: time is the core's own, in
//!   virtual [`Ticks`] at an exact [`Rate`] and in steps of the core's own
//!   unit (§3.11).
//! - No presentation policy: pacing, speed, pause, frame rate, window size,
//!   fonts, key bindings and audio devices belong to the front end.

#![forbid(unsafe_code)]

mod control;
mod core;
mod error;
mod input;
mod machine;
mod media;
mod output;
mod version;

pub use crate::control::{Finished, ResetKind};
pub use crate::core::{
    ConfigDocument, Content, ContentSource, Core, CoreDescription, CreateRequest, Features, Notice,
    OptionDescription, PlatformDescription,
};
pub use crate::error::{Diagnostic, Error, ErrorKind, Message, MessageLevel};
pub use crate::input::{
    InputEvent, InputKind, InputPortDescription, Key, KeyState, PadControl, PortId,
};
pub use crate::machine::{Machine, MachineDescription, Rate, RunLimit, Status, Stop, Ticks};
pub use crate::media::{
    MediaAction, MediumInfo, NewMedium, SlotDescription, SlotId, SlotKind, SlotState, WriteKind,
    WriteTarget,
};
pub use crate::output::{AudioDescription, DisplayDescription, DisplayId, Frame, Outputs};
pub use crate::version::{ContractVersion, CONTRACT};

#[cfg(test)]
mod tests;
