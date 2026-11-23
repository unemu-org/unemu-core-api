// SPDX-License-Identifier: MIT OR Apache-2.0
//! Content a person opens directly, and what a config needs
//! (core-contract.md §13.2).
//!
//! A stored image names its platform, and the front end knows which core
//! runs it. A file on disk names nothing. A core says which kinds of file it
//! opens, as data ([`ContentKind`]), and decides about one file by reading it
//! ([`crate::Core::identify`]). Opening a file is two calls: `identify`
//! writes a config, [`crate::Core::create`] reads it. So a machine started
//! from a file and one started later from a stored image with the same
//! config are the same machine.

use crate::{ConfigDocumentBuf, ContentReader, Message};

/// A kind of content a person opens directly to start a machine: a
/// cartridge image, a tape image, a program file.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ContentKind {
    /// Stable id of the kind within its platform, such as `ines`: 1 to 64
    /// bytes of `a-z`, `0-9` and `-`, the first a letter or a digit.
    pub id: &'static str,
    /// The kind's name for people, in the plural, as the filter of a file
    /// dialog says it: `NES cartridge images`.
    pub name: &'static str,
    /// File name extensions of the kind, in lower case and without the
    /// dot: each 1 to 16 bytes of `a-z` and `0-9`. A hint: a front end uses
    /// them to filter a file dialog and to decide which core to ask first.
    /// They prove nothing; [`crate::Core::identify`] decides. A front end
    /// compares a file's extension with these after lowering its ASCII
    /// letters, so `GAME.NES` matches `nes`.
    pub extensions: &'static [&'static str],
}

impl ContentKind {
    /// A kind of content.
    ///
    /// # Panics
    ///
    /// When `id`, `name` or an extension breaks the rules of its field. In a
    /// `static` or `const` description that is a compile error.
    pub const fn new(
        id: &'static str,
        name: &'static str,
        extensions: &'static [&'static str],
    ) -> Self {
        assert!(
            is_kind_id(id),
            "a content kind id is 1 to 64 bytes of a-z, 0-9 and -, the first a letter or a digit"
        );
        assert!(!name.is_empty(), "a content kind has a name");
        let mut index = 0;
        while index < extensions.len() {
            assert!(
                is_extension(extensions[index]),
                "an extension is 1 to 16 bytes of a-z and 0-9, without the dot"
            );
            index += 1;
        }
        Self {
            id,
            name,
            extensions,
        }
    }

    /// Whether every field follows its rules, as [`ContentKind::new`]
    /// checks them. The fields are public, so a front end that reads a
    /// core's description checks a kind with this before it trusts it.
    pub const fn is_valid(&self) -> bool {
        if !is_kind_id(self.id) || self.name.is_empty() {
            return false;
        }
        let mut index = 0;
        while index < self.extensions.len() {
            if !is_extension(self.extensions[index]) {
                return false;
            }
            index += 1;
        }
        true
    }
}

const fn is_kind_id(id: &str) -> bool {
    let bytes = id.as_bytes();
    if bytes.is_empty() || bytes.len() > 64 || bytes[0] == b'-' {
        return false;
    }
    let mut index = 0;
    while index < bytes.len() {
        let byte = bytes[index];
        if !(byte.is_ascii_lowercase() || byte.is_ascii_digit() || byte == b'-') {
            return false;
        }
        index += 1;
    }
    true
}

const fn is_extension(extension: &str) -> bool {
    let bytes = extension.as_bytes();
    if bytes.is_empty() || bytes.len() > 16 {
        return false;
    }
    let mut index = 0;
    while index < bytes.len() {
        if !(bytes[index].is_ascii_lowercase() || bytes[index].is_ascii_digit()) {
            return false;
        }
        index += 1;
    }
    true
}

/// A file a person asked to open, as the front end hands it to a core.
#[non_exhaustive]
pub struct OfferedContent<'a> {
    /// The file's name as the person sees it, without any directory. A
    /// hint for the kind and for a title; never a path, and nothing the
    /// core can open.
    pub name: &'a str,
    /// SHA-256 of the whole content, computed by the front end over all
    /// of its bytes.
    pub sha256: [u8; 32],
    /// The length of the whole content in bytes, as `bytes.size()` answers.
    pub size: u64,
    /// The bytes with that digest and size.
    pub bytes: &'a dyn ContentReader,
}

impl<'a> OfferedContent<'a> {
    /// Offered content from its parts, which the front end has checked
    /// against each other.
    pub fn new(name: &'a str, sha256: [u8; 32], size: u64, bytes: &'a dyn ContentReader) -> Self {
        Self {
            name,
            sha256,
            size,
            bytes,
        }
    }
}

/// What a core makes of offered content.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct Identified {
    /// The platform that runs it: an id of this core's description.
    pub platform: &'static str,
    /// The kind it is: the id of one of that platform's content kinds.
    pub kind: &'static str,
    /// The config of a machine that runs the content with the platform's
    /// defaults, in one of the platform's `config_types`. It names the
    /// content by digest and size, never by name. [`crate::Core::validate`]
    /// accepts it and lists the content; [`crate::Core::create`] reads it.
    pub config: ConfigDocumentBuf,
    /// The content's own title, when it carries one. A front end shows
    /// the file's name otherwise.
    pub title: Option<String>,
    /// What a person should know before the machine starts, such as a
    /// header that contradicts the content.
    pub notes: Vec<Message>,
}

impl Identified {
    /// Content without a title of its own and without notes.
    pub fn new(platform: &'static str, kind: &'static str, config: ConfigDocumentBuf) -> Self {
        Self {
            platform,
            kind,
            config,
            title: None,
            notes: Vec::new(),
        }
    }

    /// The content's own title.
    pub fn with_title(mut self, title: Option<String>) -> Self {
        self.title = title;
        self
    }

    /// What a person should know before the machine starts.
    pub fn with_notes(mut self, notes: Vec<Message>) -> Self {
        self.notes = notes;
        self
    }
}

/// What a valid config needs.
#[non_exhaustive]
#[derive(Clone, Debug, Default, PartialEq, Eq)]
pub struct Validated {
    /// Content identities needed by successful [`crate::Core::create`] with
    /// equal platform/config and baseline options: the set of (SHA-256, length)
    /// pairs, not an access trace. Order and duplicates have no meaning;
    /// labels are descriptive. Creation may cache or repeat opens and may
    /// fail before requesting the whole set.
    pub content: Vec<ContentRef>,
}

impl Validated {
    /// A valid config that needs this content.
    pub fn new(content: Vec<ContentRef>) -> Self {
        Self { content }
    }
}

/// One piece of content a config names.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ContentRef {
    /// SHA-256 of the whole content.
    pub sha256: [u8; 32],
    /// The length of the content in bytes.
    pub size: u64,
    /// What it is, for messages: `cartridge`, `firmware`.
    pub label: String,
}

impl ContentRef {
    /// One piece of content, by digest and size, with a label for messages.
    pub fn new(sha256: [u8; 32], size: u64, label: impl Into<String>) -> Self {
        Self {
            sha256,
            size,
            label: label.into(),
        }
    }
}
