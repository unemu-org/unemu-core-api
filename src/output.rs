// SPDX-License-Identifier: MIT OR Apache-2.0
//! Picture and sound out (core-contract.md §3.6 and §13.2).
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
    /// uploaded again. Geometry, aspect and visible area can change with the
    /// same serial; front ends must read them independently on every frame.
    pub serial: u64,
    /// The part of the picture that a display of the machine's time
    /// showed for certain; the rest is overscan, which may hold garbage.
    /// The whole picture when the core has nothing to say, which is what
    /// [`Frame::new`] sets. A front end may show only this part. `aspect`
    /// is always the shape of the whole picture, and the pixels keep
    /// their shape when a front end crops.
    ///
    /// Cropping is the front end's choice, not a setting of the machine:
    /// the core always hands over the whole picture, so a saved state, a
    /// replay and a screenshot do not depend on it.
    pub visible: FrameArea,
}

impl<'a> Frame<'a> {
    /// A picture whose whole area is visible. Width, height and both aspect
    /// components must be positive; pixels must contain exactly width × height
    /// elements. These requirements also apply after public-field mutation.
    ///
    /// # Panics
    /// Panics for invalid dimensions, aspect or pixel count.
    pub fn new(
        width: u32,
        height: u32,
        pixels: &'a [u32],
        aspect: (u32, u32),
        serial: u64,
    ) -> Self {
        assert!(width != 0 && height != 0 && aspect.0 != 0 && aspect.1 != 0);
        assert_eq!(u64::from(width) * u64::from(height), pixels.len() as u64);
        Self {
            width,
            height,
            pixels,
            aspect,
            serial,
            visible: FrameArea {
                x: 0,
                y: 0,
                width,
                height,
            },
        }
    }

    /// Mark the part of the picture a display showed for certain.
    ///
    /// The area is cut to the picture; an area that shares no pixel with
    /// the picture leaves the whole picture visible, as a core with nothing
    /// to say would. The field stays public, so a front end that reads a
    /// frame checks it with [`FrameArea::is_within`] before it crops.
    pub fn with_visible(mut self, visible: FrameArea) -> Self {
        let left = visible.x.min(self.width);
        let top = visible.y.min(self.height);
        let right = visible.x.saturating_add(visible.width).min(self.width);
        let bottom = visible.y.saturating_add(visible.height).min(self.height);
        self.visible = if left < right && top < bottom {
            FrameArea {
                x: left,
                y: top,
                width: right - left,
                height: bottom - top,
            }
        } else {
            FrameArea {
                x: 0,
                y: 0,
                width: self.width,
                height: self.height,
            }
        };
        self
    }
}

/// A rectangle of a picture, in its pixels: `x` columns from the left edge,
/// `y` rows from the top edge. Empty rectangles are representable, but are
/// never valid visible areas: `is_within` rejects them and `with_visible`
/// falls back to the whole picture when the intersection is empty.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub struct FrameArea {
    /// The first column, counted from the left edge.
    pub x: u32,
    /// The first row, counted from the top edge.
    pub y: u32,
    /// The number of columns.
    pub width: u32,
    /// The number of rows.
    pub height: u32,
}

impl FrameArea {
    /// A rectangle, including an empty one; validate before cropping.
    pub const fn new(x: u32, y: u32, width: u32, height: u32) -> Self {
        Self {
            x,
            y,
            width,
            height,
        }
    }

    /// Whether the area lies within a picture of `width` × `height` pixels.
    /// A front end checks a [`Frame::visible`] it did not build before it
    /// crops by it.
    pub const fn is_within(self, width: u32, height: u32) -> bool {
        self.width != 0
            && self.height != 0
            && (self.x as u64) + (self.width as u64) <= width as u64
            && (self.y as u64) + (self.height as u64) <= height as u64
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
    /// Stereo frames per second, exact, from 8 000 to 192 000: the rate
    /// natural to the machine. A core never resamples to suit a host
    /// device; the front end converts to whatever it plays.
    ///
    /// A machine whose sound generator runs faster than 192 000 Hz (a
    /// chip clocked at the processor's rate) filters and divides by a
    /// whole number inside the core, because only the core knows its
    /// signal, and states the divided rate. It is still an exact
    /// fraction: 19 687 500/11 Hz divided by 40 is 984 375/22 Hz.
    ///
    /// A machine with one sound channel puts it in both samples of a
    /// frame.
    ///
    /// Frame `i` is the sound at virtual time `i × clock / rate` ticks from
    /// creation, where `clock` is the machine's clock. At every return of
    /// [`crate::Machine::run`], once [`crate::Machine::drain_audio`] has
    /// been called, the next frame it will hand over has index
    /// `floor(now() × rate / clock)`, within one frame. Frames not drained
    /// when sound is switched off ([`crate::Machine::set_outputs`]), or
    /// before [`crate::Machine::load_state`], are dropped; after
    /// `load_state` the index follows the loaded `now()`.
    pub rate: Rate,
}

impl AudioDescription {
    /// The lowest rate a machine states, in frames per second.
    pub const MIN_RATE: u64 = 8_000;
    /// The highest rate a machine states, in frames per second.
    pub const MAX_RATE: u64 = 192_000;

    pub const fn new(rate: Rate) -> Self {
        Self { rate }
    }

    /// Whether `rate` lies within [`Self::MIN_RATE`] and [`Self::MAX_RATE`],
    /// compared exactly. A rate with a zero denominator names no rate and
    /// is not within them.
    pub const fn rate_in_bounds(&self) -> bool {
        let numerator = self.rate.numerator as u128;
        let denominator = self.rate.denominator as u128;
        denominator != 0
            && numerator >= Self::MIN_RATE as u128 * denominator
            && numerator <= Self::MAX_RATE as u128 * denominator
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
/// Deliberately closed scalar representation.
#[derive(Clone, Copy, Debug, PartialEq, Eq, PartialOrd, Ord, Hash)]
pub struct DisplayId(pub u8);
