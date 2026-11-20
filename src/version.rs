// SPDX-License-Identifier: MIT OR Apache-2.0
//! The contract's version and the rule a front end checks a core by
//! (core-contract.md §3.2).

/// A version of the contract between front ends and cores.
///
/// `major` changes when a core or a front end has to be edited to keep
/// working. `minor` changes when something is added: a method with a default
/// body, a field, an enum variant. While `major` is 0 every minor may break.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct ContractVersion {
    pub major: u16,
    pub minor: u16,
}

/// The contract this source defines.
///
/// A core names the version it was written against with a literal in
/// [`crate::CoreDescription::implements`], never with this constant: the
/// constant follows whichever copy of the crate Cargo resolved.
pub const CONTRACT: ContractVersion = ContractVersion { major: 0, minor: 3 };

impl ContractVersion {
    /// A version from its two numbers.
    pub const fn new(major: u16, minor: u16) -> Self {
        Self { major, minor }
    }

    /// Whether a front end written for `self` may drive a core that
    /// implements `core`: the same major, and during major 0 the same minor.
    /// Within one major, a core that implements an older minor is driven
    /// with the newer additions treated as absent.
    pub const fn drives(self, core: ContractVersion) -> bool {
        self.major == core.major && (self.major != 0 || self.minor == core.minor)
    }
}

impl std::fmt::Display for ContractVersion {
    fn fmt(&self, formatter: &mut std::fmt::Formatter<'_>) -> std::fmt::Result {
        write!(formatter, "{}.{}", self.major, self.minor)
    }
}
