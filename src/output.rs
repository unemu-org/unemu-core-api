// SPDX-License-Identifier: AGPL-3.0-or-later
//! Picture and sound out (core-contract.md §3.6).
//!
//! The picture is pixels; a text mode, its cells and its fonts are the
//! core's business, and the core rasterises. Output is pulled: the front end
//! asks for the picture and drains the sound, so nothing of the front end is
//! shared with the machine.

use crate::Rate;

/// One picture.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct Frame<'a> {
    pub width: u32,
    pub height: u32,
    /// `width * height` pixels, rows top to bottom, each `0x00RRGGBB`.
    pub pixels: &'a [u32],
    /// The shape of the whole picture on the emulated display, width to
    /// height, such as 4:3. The front end scales to it.
    pub aspect: (u32, u32),
    /// Changes whenever the pixels changed, so an unchanged picture is not
    /// uploaded again.
    pub serial: u64,
}

impl<'a> Frame<'a> {
    pub fn new(
        width: u32,
        height: u32,
        pixels: &'a [u32],
        aspect: (u32, u32),
        serial: u64,
    ) -> Self {
        Self {
            width,
            height,
            pixels,
            aspect,
            serial,
        }
    }
}

/// One display of a machine.
#[non_exhaustive]
#[derive(Clone, Debug, PartialEq, Eq)]
pub struct DisplayDescription {
    pub id: DisplayId,
    /// The display's name for people, such as `VGA`.
    pub label: String,
}

impl DisplayDescription {
    pub fn new(id: DisplayId, label: impl Into<String>) -> Self {
        Self {
            id,
            label: label.into(),
        }
    }
}

/// The sound of a machine.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct AudioDescription {
    /// Stereo frames per second, exact.
    pub rate: Rate,
}

impl AudioDescription {
    pub const fn new(rate: Rate) -> Self {
        Self { rate }
    }
}

/// The outputs the front end consumes now.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, Default, PartialEq, Eq)]
pub struct Outputs {
    pub audio: bool,
}

impl Outputs {
    /// No output is consumed.
    pub const NONE: Self = Self { audio: false };

    pub const fn with_audio(mut self, audio: bool) -> Self {
        self.audio = audio;
        self
    }
}

/// A display of a machine, by the number its description gives it.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DisplayId(pub u8);
