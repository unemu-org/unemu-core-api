// SPDX-License-Identifier: AGPL-3.0-or-later
//! Reset and finish (core-contract.md §3.9).
//!
//! Pause has no call: the front end stops calling [`crate::Machine::run`]
//! and re-anchors its pacing when it resumes. A power cycle is a front-end
//! composition (finish, then create) until a core lists [`ResetKind::Hard`].

/// A kind of reset.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ResetKind {
    /// What the machine's reset line or button does: the processor
    /// restarts, memory, devices and media keep their state. Takes no
    /// virtual time.
    Soft,
    /// Power off and on: everything volatile starts fresh; media stay
    /// inserted and non-volatile memory stays.
    Hard,
}

/// What a clean end reports.
#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Finished {
    /// Problems that did not fail the session, such as a retention pass
    /// that could not run.
    pub warnings: Vec<String>,
}

impl Finished {
    /// A clean end without warnings.
    pub fn new() -> Self {
        Self::default()
    }

    pub fn with_warnings(mut self, warnings: Vec<String>) -> Self {
        self.warnings = warnings;
        self
    }
}
