// SPDX-License-Identifier: MIT OR Apache-2.0
//! The one error type of the contract, and the messages a core reports
//! (core-contract.md §3.10).
//!
//! A core keeps its own typed errors inside and maps them at its adapter;
//! the message is the typed error's text. Host failures stay out of the
//! guest: they surface as [`ErrorKind::Storage`] or
//! [`ErrorKind::StorageUncertain`] at a run boundary.

use std::fmt;

/// The one error type of the contract.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Error {
    pub kind: ErrorKind,
    /// One sentence a person can read, without a host path the user did
    /// not supply.
    pub message: String,
    /// For `Config`: where in the document, when the core knows.
    pub diagnostics: Vec<Diagnostic>,
}

impl Error {
    pub fn new(kind: ErrorKind, message: impl Into<String>) -> Self {
        Self {
            kind,
            message: message.into(),
            diagnostics: Vec::new(),
        }
    }

    pub fn with_diagnostics(mut self, diagnostics: Vec<Diagnostic>) -> Self {
        self.diagnostics = diagnostics;
        self
    }
}

impl fmt::Display for Error {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        formatter.write_str(&self.message)
    }
}

impl std::error::Error for Error {}

/// What kind of failure an [`Error`] is.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ErrorKind {
    /// The config document is malformed or names something this core does
    /// not have.
    Config,
    /// Content is missing or is not what its digest promised.
    Content,
    /// The core, the platform or this machine cannot do what was asked.
    Unsupported,
    /// The emulated machine cannot continue.
    Guest,
    /// A host read or write of the machine's storage or of its content
    /// failed. The guest never sees it.
    Storage,
    /// A saved-state document is malformed or does not fit this machine.
    /// Host I/O while reading it is Storage instead.
    State,
    /// The call is not valid in the machine's current status, or not valid
    /// for the object it was made on, including invalid handles, arguments
    /// and run-only options. A rejected config document is Config instead.
    Usage,
    /// A host write failed in a way that leaves it unknown whether it took
    /// effect. The object it was made on must be opened again before it is
    /// written ([`crate::Record`], [`crate::Replacement::commit`]). At a run
    /// boundary it fails the machine as `Storage` does
    /// ([`crate::Machine::run`]).
    StorageUncertain,
}

/// Where in a config document a problem is.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Diagnostic {
    /// A field path in the config document.
    pub path: String,
    pub message: String,
}

impl Diagnostic {
    pub fn new(path: impl Into<String>, message: impl Into<String>) -> Self {
        Self {
            path: path.into(),
            message: message.into(),
        }
    }
}

/// A line a core reports instead of writing to the standard streams.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Message {
    pub level: MessageLevel,
    pub text: String,
}

impl Message {
    pub fn new(level: MessageLevel, text: impl Into<String>) -> Self {
        Self {
            level,
            text: text.into(),
        }
    }
}

/// How much a [`Message`] matters.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum MessageLevel {
    Note,
    Warning,
}
