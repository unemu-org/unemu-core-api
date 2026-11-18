// SPDX-License-Identifier: MIT OR Apache-2.0
//! The core, its description as data, and creating a machine
//! (core-contract.md §3.3, §3.4, §12.5 and §13.2).

use crate::{
    ContentKind, ContentReader, ContractVersion, Error, ErrorKind, Identified, Machine,
    OfferedContent, ResetKind, Storage, Validated,
};
use std::fmt;
use std::rc::Rc;

/// One emulator core: a stateless factory for machines of the platforms it
/// provides.
///
/// A front end holds every compiled-in core for the life of the program and
/// may call it from any thread.
pub trait Core: Send + Sync {
    /// What this core is and what it can do. Constant for the life of the
    /// program.
    fn description(&self) -> &CoreDescription;

    /// Build a machine, stopped at virtual time zero.
    ///
    /// Reads the config document, takes every blob it names from
    /// `request.content`, and keeps what must outlive the run only in
    /// records of `request.storage`. Fails with [`crate::ErrorKind::Config`],
    /// `Content` or `Unsupported`; a failed create leaves nothing running
    /// and nothing half-written that a later create could mistake for a
    /// session.
    fn create(&self, request: &CreateRequest<'_>) -> Result<Box<dyn Machine>, Error>;

    /// Say whether this core runs `content`, and as what.
    ///
    /// `Ok(None)`: the bytes are not content this core opens; the front
    /// end asks the next core. `Ok(Some(..))`: the platform, the kind and
    /// the config of a machine that runs the content with the platform's
    /// defaults. Fails with [`ErrorKind::Unsupported`] when the core
    /// recognises the content and cannot run it, such as a cartridge
    /// board it does not model; the message names what is missing, and
    /// the front end shows it instead of "unknown file". Fails with
    /// `Content` or `Storage` as [`ContentReader::read_at`] did.
    ///
    /// The core reads what it needs through `content.bytes`: a header,
    /// or all of it. It keeps nothing: the call builds no machine and
    /// changes no state. The same bytes give the same answer in every
    /// run of one release of the core.
    ///
    /// The default answers `Ok(None)`: a core that opens no content
    /// recognises nothing.
    fn identify(&self, content: &OfferedContent<'_>) -> Result<Option<Identified>, Error> {
        let _ = content;
        Ok(None)
    }

    /// Check a config document without building a machine, and name the
    /// content it needs.
    ///
    /// Fails with [`ErrorKind::Config`], with diagnostics, as `create`
    /// would for the same document, and with `Unsupported` for a platform
    /// or a config type the core does not read. On success `create` asks
    /// its [`ContentSource`] for exactly the listed content. A front end
    /// verifies that content before it calls `create`; a store-side
    /// validator can be written over this call alone.
    ///
    /// The default answers `Unsupported`.
    fn validate(&self, platform: &str, config: ConfigDocument<'_>) -> Result<Validated, Error> {
        let _ = (platform, config);
        Err(Error::new(
            ErrorKind::Unsupported,
            "this core does not validate configs",
        ))
    }
}

/// A core, as data. A front end lists cores and enables controls from this
/// alone, before any machine exists.
#[non_exhaustive]
#[derive(Clone, Copy, Debug)]
pub struct CoreDescription {
    /// The contract version the core's author wrote the core against: a
    /// literal, not [`crate::CONTRACT`].
    pub implements: ContractVersion,
    /// Stable id of the core, such as `unemupc`.
    pub id: &'static str,
    /// The core's name for people.
    pub name: &'static str,
    /// The core's release, `MAJOR.MINOR.PATCH`.
    pub version: &'static str,
    /// The machine families the core provides.
    pub platforms: &'static [PlatformDescription],
    /// Licence notices of the third-party components compiled into the core,
    /// for the front end's About text.
    pub notices: &'static [Notice],
}

impl CoreDescription {
    /// A core that provides no platform yet and carries no notices.
    pub const fn new(
        implements: ContractVersion,
        id: &'static str,
        name: &'static str,
        version: &'static str,
    ) -> Self {
        Self {
            implements,
            id,
            name,
            version,
            platforms: &[],
            notices: &[],
        }
    }

    pub const fn with_platforms(mut self, platforms: &'static [PlatformDescription]) -> Self {
        self.platforms = platforms;
        self
    }

    pub const fn with_notices(mut self, notices: &'static [Notice]) -> Self {
        self.notices = notices;
        self
    }
}

/// One machine family of a core.
#[non_exhaustive]
#[derive(Clone, Copy, Debug)]
pub struct PlatformDescription {
    /// The platform id artifacts name, in its current spelling. Retired
    /// spellings are the front end's concern; the core sees the current one.
    pub id: &'static str,
    /// The platform's name for people.
    pub name: &'static str,
    /// Media types of the config documents [`Core::create`] reads. Empty
    /// while the core cannot create machines of this platform itself, so no
    /// front end offers it.
    pub config_types: &'static [&'static str],
    /// The most a machine of this platform can do; one machine may do less
    /// and says so in its [`crate::MachineDescription`].
    pub features: Features,
    /// Run-only options [`Core::create`] accepts, outside machine identity.
    pub options: &'static [OptionDescription],
    /// The kinds of content a person can open to start a machine of this
    /// platform. Empty for a platform whose machines start only from a
    /// config, such as a computer that is assembled from parts.
    ///
    /// A platform that opens content lists at least one config type,
    /// because [`Core::identify`] answers in one.
    pub content: &'static [ContentKind],
}

impl PlatformDescription {
    /// A platform that reads no config type, can do nothing beyond running,
    /// takes no options and opens no content.
    pub const fn new(id: &'static str, name: &'static str) -> Self {
        Self {
            id,
            name,
            config_types: &[],
            features: Features::NONE,
            options: &[],
            content: &[],
        }
    }

    pub const fn with_config_types(mut self, config_types: &'static [&'static str]) -> Self {
        self.config_types = config_types;
        self
    }

    pub const fn with_features(mut self, features: Features) -> Self {
        self.features = features;
        self
    }

    pub const fn with_options(mut self, options: &'static [OptionDescription]) -> Self {
        self.options = options;
        self
    }

    /// The kinds of content a person can open to start a machine.
    pub const fn with_content(mut self, content: &'static [ContentKind]) -> Self {
        self.content = content;
        self
    }
}

/// What machines can do. Every field has a "no" value, so a newer field is
/// "no" for an older core.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Features {
    /// Kinds of reset the machine accepts.
    pub resets: &'static [ResetKind],
    /// Whether state can be saved and loaded.
    pub saved_state: bool,
    /// Whether removable media can change while the machine runs.
    pub media_changes: bool,
    /// Whether a run can stop when a display finishes a frame
    /// ([`crate::RunLimit::stop_on_frame`]).
    pub frame_stops: bool,
}

impl Features {
    /// Nothing beyond running.
    pub const NONE: Self = Self {
        resets: &[],
        saved_state: false,
        media_changes: false,
        frame_stops: false,
    };

    pub const fn with_resets(mut self, resets: &'static [ResetKind]) -> Self {
        self.resets = resets;
        self
    }

    pub const fn with_saved_state(mut self, saved_state: bool) -> Self {
        self.saved_state = saved_state;
        self
    }

    pub const fn with_media_changes(mut self, media_changes: bool) -> Self {
        self.media_changes = media_changes;
        self
    }

    pub const fn with_frame_stops(mut self, frame_stops: bool) -> Self {
        self.frame_stops = frame_stops;
        self
    }
}

impl Default for Features {
    fn default() -> Self {
        Self::NONE
    }
}

/// A third-party component inside a core and its licence notice.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Notice {
    pub component: &'static str,
    pub text: &'static str,
}

impl Notice {
    pub const fn new(component: &'static str, text: &'static str) -> Self {
        Self { component, text }
    }
}

/// A run-only option: a permission or a diagnostic switch that never enters
/// a machine's identity, its config or a stored artifact.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct OptionDescription {
    pub key: &'static str,
    pub summary: &'static str,
}

impl OptionDescription {
    pub const fn new(key: &'static str, summary: &'static str) -> Self {
        Self { key, summary }
    }
}

/// Everything a core needs to build one machine.
#[non_exhaustive]
pub struct CreateRequest<'a> {
    /// The platform id, in its current spelling.
    pub platform: &'a str,
    /// The resolved machine configuration: the document a stored image keeps
    /// as its config, after any local settings were applied to it.
    pub config: ConfigDocument<'a>,
    /// Verified content by digest: firmware, disk bases, media.
    pub content: &'a dyn ContentSource,
    /// Where the machine keeps what must outlive the run: written sectors,
    /// non-volatile memory, battery-backed memory. The machine holds it for
    /// its whole life and opens a record when it needs one, such as when a
    /// medium is inserted. The core never writes content it took from
    /// `content`.
    pub storage: Rc<dyn Storage>,
    /// Run-only options, by the keys the platform describes. An unknown key
    /// is refused.
    pub options: &'a [(&'a str, &'a str)],
}

impl<'a> CreateRequest<'a> {
    /// A request without run-only options.
    pub fn new(
        platform: &'a str,
        config: ConfigDocument<'a>,
        content: &'a dyn ContentSource,
        storage: Rc<dyn Storage>,
    ) -> Self {
        Self {
            platform,
            config,
            content,
            storage,
            options: &[],
        }
    }

    pub fn with_options(mut self, options: &'a [(&'a str, &'a str)]) -> Self {
        self.options = options;
        self
    }
}

/// A config document with its media type.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct ConfigDocument<'a> {
    pub media_type: &'a str,
    pub bytes: &'a [u8],
}

impl<'a> ConfigDocument<'a> {
    pub fn new(media_type: &'a str, bytes: &'a [u8]) -> Self {
        Self { media_type, bytes }
    }
}

/// A config document a core wrote, such as the answer of
/// [`Core::identify`].
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct ConfigDocumentBuf {
    /// One of the platform's `config_types`.
    pub media_type: &'static str,
    pub bytes: Vec<u8>,
}

impl ConfigDocumentBuf {
    /// A document of `media_type` holding `bytes`.
    pub fn new(media_type: &'static str, bytes: Vec<u8>) -> Self {
        Self { media_type, bytes }
    }

    /// The document, borrowed, as [`Core::validate`] and [`Core::create`]
    /// take it.
    pub fn as_document(&self) -> ConfigDocument<'_> {
        ConfigDocument::new(self.media_type, &self.bytes)
    }
}

/// Content addressed by SHA-256.
///
/// The source has hashed the whole content, never a prefix and never a
/// sample, before it hands it over, however large it is. The core trusts
/// the receipt and does not hash again. This keeps layered disk images
/// possible: a base is read-only content named by its digest, and a chain
/// of hash-linked layers can later stand behind the same answer.
pub trait ContentSource {
    /// The exact bytes with this digest and length.
    fn open(&self, sha256: &[u8; 32], size: u64) -> Result<Content, Error>;
}

/// Verified read-only content.
#[non_exhaustive]
pub enum Content {
    /// Small content, in memory.
    Bytes(Vec<u8>),
    /// Large content, read at any offset. The core reads it and never
    /// writes it. Replaces `File` of contract 0.1.
    Reader(Box<dyn ContentReader>),
}

impl fmt::Debug for Content {
    fn fmt(&self, formatter: &mut fmt::Formatter<'_>) -> fmt::Result {
        match self {
            Self::Bytes(bytes) => formatter.debug_tuple("Bytes").field(bytes).finish(),
            // A reader is not `Debug`; its size is what identifies it here.
            Self::Reader(reader) => formatter
                .debug_struct("Reader")
                .field("size", &reader.size())
                .finish(),
        }
    }
}
