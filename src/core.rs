// SPDX-License-Identifier: MIT OR Apache-2.0
//! The core, its description as data, and creating a machine
//! (core-contract.md §3.3 and §3.4).

use crate::{ContractVersion, Error, Machine, ResetKind};
use std::path::Path;

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
    /// `request.content`, and writes only below `request.session`. Fails
    /// with [`crate::ErrorKind::Config`], `Content` or `Unsupported`; a
    /// failed create leaves nothing running and nothing half-written that a
    /// later create could mistake for a session.
    fn create(&self, request: &CreateRequest<'_>) -> Result<Box<dyn Machine>, Error>;
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
}

impl PlatformDescription {
    /// A platform that reads no config type, can do nothing beyond running,
    /// and takes no options.
    pub const fn new(id: &'static str, name: &'static str) -> Self {
        Self {
            id,
            name,
            config_types: &[],
            features: Features::NONE,
            options: &[],
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
    /// A directory the front end owns for this machine. The core keeps
    /// there what must outlive the run: written sectors, non-volatile
    /// memory, saved battery memory. The core never writes outside it and
    /// never changes a file it took from `content`.
    pub session: &'a Path,
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
        session: &'a Path,
    ) -> Self {
        Self {
            platform,
            config,
            content,
            session,
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
#[derive(Debug)]
pub enum Content {
    /// Small content, in memory.
    Bytes(Vec<u8>),
    /// Large content, opened for random access. The core reads it and never
    /// writes it.
    File(std::fs::File),
}
