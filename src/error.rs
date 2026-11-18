// SPDX-License-Identifier: AGPL-3.0-or-later
//! The one error type of the contract, and the messages a core reports
//! (core-contract.md §3.10).
//!
//! A core keeps its own typed errors inside and maps them at its adapter;
//! the message is the typed error's text. Host failures stay out of the
//! guest: they surface as [`ErrorKind::Storage`] at a run boundary.

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
    /// A host write or read below the session directory failed. The guest
    /// never sees it.
    Storage,
    /// A saved state does not fit this machine.
    State,
    /// The call is not valid in the machine's current status.
    Usage,
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
