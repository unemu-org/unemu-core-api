// SPDX-License-Identifier: MIT OR Apache-2.0
//! The contract between front ends and emulator cores; see
//! `backlog/docs/design/core-contract.md` §3, §12.5 and §13.2.
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
//! A core touches nothing of the host. What it reads comes as [`Content`]
//! from a [`ContentSource`], by digest; what must outlive a run goes to
//! [`Record`]s of the [`Storage`] the front end hands to [`Core::create`]. A
//! file a person opens reaches a core through [`Core::identify`], which
//! writes the config [`Core::create`] reads.
//!
//! Rules of the crate (§3.1):
//!
//! - No dependencies, `std` only. Errors implement [`std::error::Error`] by
//!   hand, so nothing here forces a version of another crate on a core.
//! - Object-safe traits without generics: a front end holds `&dyn Core` and
//!   `Box<dyn Machine>`.
//! - Extensible public data structs, enums and struct-like variants are
//!   `#[non_exhaustive]`, with constructors or factories. Opaque structs with
//!   private fields can grow privately. Unit and tuple enum variants have
//!   fixed payloads; growth uses a new variant. The deliberately closed scalar types
//!   are `Ticks(u64)`, `Key(u16)`, `PortId(u8)`, `DisplayId(u8)` and `SlotId(u8)`.
//!   From 1.0 a minor only adds compatible fields, variants and default methods;
//!   before 1.0 every minor may break.
//! - No platform words: no port, interrupt line, scan code, disk geometry,
//!   cartridge mapper or firmware catalogue appears here.
//! - No host clock, no randomness, no threads: time is the core's own, in
//!   virtual [`Ticks`] at an exact [`Rate`] and in steps of the core's own
//!   unit (§3.11).
//! - No presentation policy: pacing, speed, pause, frame rate, window size,
//!   fonts, key bindings and audio devices belong to the front end.
//! - No host: the contract names no file, directory or path; content and
//!   storage are objects the front end hands in (§12.5).
//!
//! The feature `test-support` adds the module `test_support`: a scripted
//! storage and content in memory for the tests of cores and front ends.

#![forbid(unsafe_code)]

mod content;
mod control;
mod core;
mod error;
mod export;
mod input;
mod machine;
mod media;
mod output;
mod storage;
mod version;

#[cfg(any(test, feature = "test-support"))]
pub mod test_support;

pub use crate::content::{ContentKind, ContentRef, Identified, OfferedContent, Validated};
pub use crate::control::{Finished, ResetKind};
pub use crate::core::{
    ConfigDocument, ConfigDocumentBuf, Content, ContentSource, Core, CoreDescription,
    CreateRequest, Features, Notice, OptionDescription, PlatformDescription,
};
pub use crate::error::{Diagnostic, Error, ErrorKind, Message, MessageLevel};
pub use crate::export::{
    is_export_name, ExportFormat, ExportMedium, ExportOutput, ExportReport, ExportRequest,
    ExportScope, ExportSink, ExportedOutput,
};
pub use crate::input::{
    pointer_pixel, InputEvent, InputKind, InputPortDescription, Key, KeyState, PadControl, PortId,
};
pub use crate::machine::{Machine, MachineDescription, Rate, RunLimit, Status, Stop, Ticks};
pub use crate::media::{
    MediaAction, MediaDescription, MediumInfo, NewMedium, SlotDescription, SlotId, SlotKind,
    SlotState, WriteKind, WriteTarget,
};
pub use crate::output::{
    AudioDescription, DisplayDescription, DisplayId, Frame, FrameArea, Outputs,
};
pub use crate::storage::{
    is_kept_record_name, is_record_name, ContentReader, Record, Replacement, Storage,
    KEPT_RECORD_PREFIX,
};

// What `identify` and `validate` answer, and the errors and messages a core
// reports, may be handed to another thread: a front end asks cores from a
// worker. A field that is not `Send` fails the build here.
const _: () = {
    const fn assert_send<T: Send>() {}
    assert_send::<Identified>();
    assert_send::<Validated>();
    assert_send::<MediaDescription>();
    assert_send::<ExportReport>();
    assert_send::<ConfigDocumentBuf>();
    assert_send::<Error>();
    assert_send::<Message>();
};
pub use crate::version::{ContractVersion, CONTRACT};

#[cfg(test)]
mod tests;
