// SPDX-License-Identifier: MIT OR Apache-2.0
//! In-memory streamed exports and an offline reference core.

use super::ScriptedMachine;
use crate::*;
use std::cell::RefCell;
use std::collections::BTreeMap;
use std::rc::Rc;

/// An export operation that can fail deterministically.
#[non_exhaustive]
#[derive(Clone, Copy, Debug, PartialEq, Eq)]
pub enum ExportOperation {
    Create,
    Write,
    Finish,
}

#[derive(Default)]
struct State {
    outputs: BTreeMap<String, OutputState>,
    fault: Option<ExportOperation>,
}

#[derive(Default)]
struct OutputState {
    bytes: Vec<u8>,
    closed: bool,
}

/// A sink recording current output bytes, including unfinished outputs.
/// A clone shares reservations, bytes and faults. Names remain reserved after
/// dropping an unfinished output, so no export can overwrite partial work.
/// Hashing here is a small dependency-free reference; production sinks can use
/// their own streaming SHA-256 implementation through ExportOutput::finish.
#[derive(Clone, Default)]
pub struct MemoryExportSink {
    state: Rc<RefCell<State>>,
    handles: Vec<Output>,
}

impl MemoryExportSink {
    pub fn new() -> Self {
        Self::default()
    }

    /// Snapshot all created outputs, including partially written ones. A front
    /// end owns cleanup after failure; this sink deliberately retains evidence.
    pub fn outputs(&self) -> BTreeMap<String, Vec<u8>> {
        self.state
            .borrow()
            .outputs
            .iter()
            .map(|(name, output)| (name.clone(), output.bytes.clone()))
            .collect()
    }

    /// Revoke all created outputs, including unfinished ones, before cleanup.
    /// Bytes and reservations remain available for inspection. Clones share
    /// revocation, so a borrowed output cannot bypass closure through a clone.
    pub fn close(&mut self) {
        for output in self.state.borrow_mut().outputs.values_mut() {
            output.closed = true;
        }
    }

    /// Fail the next matching operation with Storage, once.
    pub fn fail_next(&self, operation: ExportOperation) {
        self.state.borrow_mut().fault = Some(operation);
    }
}

fn fault(state: &mut State, operation: ExportOperation) -> Result<(), Error> {
    if state.fault == Some(operation) {
        state.fault = None;
        return Err(Error::new(ErrorKind::Storage, "scripted export failure"));
    }
    Ok(())
}

impl ExportSink for MemoryExportSink {
    fn create(&mut self, name: &str) -> Result<&mut (dyn ExportOutput + '_), Error> {
        let mut state = self.state.borrow_mut();
        if !is_export_name(name) || state.outputs.contains_key(name) {
            return Err(Error::new(
                ErrorKind::Usage,
                "invalid or existing output name",
            ));
        }
        fault(&mut state, ExportOperation::Create)?;
        state
            .outputs
            .insert(name.to_owned(), OutputState::default());
        self.handles.push(Output {
            name: name.to_owned(),
            state: self.state.clone(),
        });
        Ok(self.handles.last_mut().expect("created output"))
    }
}

#[derive(Clone)]
struct Output {
    name: String,
    state: Rc<RefCell<State>>,
}
impl ExportOutput for Output {
    fn write(&mut self, bytes: &[u8]) -> Result<(), Error> {
        let mut state = self.state.borrow_mut();
        if state.outputs[&self.name].closed {
            return Err(Error::new(ErrorKind::Storage, "export output closed"));
        }
        if fault(&mut state, ExportOperation::Write).is_err() {
            state.outputs.get_mut(&self.name).unwrap().closed = true;
            return Err(Error::new(ErrorKind::Storage, "export output failed"));
        }
        state
            .outputs
            .get_mut(&self.name)
            .expect("reserved output")
            .bytes
            .extend_from_slice(bytes);
        Ok(())
    }
    fn finish(&mut self) -> Result<ExportedOutput, Error> {
        let mut state = self.state.borrow_mut();
        let output = state.outputs.get_mut(&self.name).expect("reserved output");
        if output.closed {
            return Err(Error::new(ErrorKind::Storage, "export output closed"));
        }
        output.closed = true;
        fault(&mut state, ExportOperation::Finish)?;
        let bytes = &state.outputs[&self.name].bytes;
        Ok(ExportedOutput::new(
            self.name.clone(),
            bytes.len() as u64,
            sha256(bytes),
        ))
    }
}

/// A stateless reference core for config-only media listing and exports.
/// Platform `scripted`, config type `text/plain`: `fixed` fixes slot 0 at three
/// bytes; `variable` leaves its length unknown. Both create ScriptedMachine's
/// single removable raw-media slot. `raw` exports current bytes; `machine`
/// also writes a document referencing `slot-0.raw`. The machine format preserves
/// the configured removable-slot setting; raw reports its omission as a note.
pub struct ScriptedCore;

const DESCRIPTION: CoreDescription = CoreDescription::new(
    ContractVersion::new(0, 6),
    "scripted",
    "Scripted core",
    "1.0.0",
)
.with_platforms(&[PlatformDescription::new("scripted", "Scripted")
    .with_config_types(&["text/plain"])
    .with_features(
        Features::NONE
            .with_resets(&[ResetKind::Soft, ResetKind::Hard])
            .with_saved_state(true)
            .with_media_changes(true)
            .with_media_additions(true)
            .with_frame_stops(true),
    )
    .with_export_formats(&[
        ExportFormat::new("raw", "Raw bytes", "raw", ExportScope::Slot),
        ExportFormat::new("machine", "Scripted machine", "txt", ExportScope::Machine),
    ])]);

impl Core for ScriptedCore {
    fn description(&self) -> &CoreDescription {
        &DESCRIPTION
    }

    fn validate(&self, platform: &str, config: ConfigDocument<'_>) -> Result<Validated, Error> {
        if platform != "scripted" || config.media_type != "text/plain" {
            return Err(Error::new(
                ErrorKind::Unsupported,
                "unknown platform or config type",
            ));
        }
        if config.bytes != b"fixed" && config.bytes != b"variable" {
            return Err(Error::new(ErrorKind::Config, "expected fixed or variable"));
        }
        Ok(Validated::new(Vec::new()))
    }

    fn create(&self, request: &CreateRequest<'_>) -> Result<Box<dyn Machine>, Error> {
        self.validate(request.platform, request.config)?;
        if !request.options.is_empty() {
            return Err(Error::new(ErrorKind::Usage, "unknown run option"));
        }
        Ok(Box::new(ScriptedMachine::new(request.storage.clone())))
    }

    fn describe_media(
        &self,
        platform: &str,
        config: ConfigDocument<'_>,
    ) -> Result<Vec<MediaDescription>, Error> {
        self.validate(platform, config)?;
        let size = if config.bytes == b"fixed" {
            Some(3)
        } else {
            None
        };
        Ok(vec![MediaDescription::new(
            SlotDescription::new(SlotId(0), SlotKind::Floppy, "Media", true)
                .with_formats(vec!["raw".into()]),
        )
        .with_detail("Scripted raw medium")
        .with_size(size)])
    }

    fn export_slots(
        &self,
        platform: &str,
        config: ConfigDocument<'_>,
        format_id: &str,
    ) -> Result<Vec<SlotId>, Error> {
        self.validate(platform, config)?;
        if !DESCRIPTION.platforms[0]
            .export_formats
            .iter()
            .any(|format| format.id == format_id)
        {
            return Err(Error::new(ErrorKind::Usage, "unknown export format"));
        }
        Ok(vec![SlotId(0)])
    }

    fn export(
        &self,
        platform: &str,
        config: ConfigDocument<'_>,
        request: ExportRequest<'_>,
        sink: &mut dyn ExportSink,
    ) -> Result<ExportReport, Error> {
        let slots = self.export_slots(platform, config, request.format)?;
        let descriptions = self.describe_media(platform, config)?;
        let format = DESCRIPTION.platforms[0]
            .export_formats
            .iter()
            .find(|format| format.id == request.format)
            .ok_or_else(|| Error::new(ErrorKind::Usage, "unknown export format"))?;
        // Both reference formats export exactly the one configured slot.
        if request.media.len() != 1 || request.media[0].slot != slots[0] {
            return Err(Error::new(ErrorKind::Usage, "expected exactly slot 0"));
        }
        let medium = request.media[0];
        if medium.size != medium.bytes.size()
            || descriptions[0].size.is_some_and(|size| size != medium.size)
        {
            return Err(Error::new(
                ErrorKind::Content,
                "export medium length mismatch",
            ));
        }
        let output = sink.create("slot-0.raw")?;
        let mut buffer = [0; 4096];
        let mut offset = 0;
        while offset < medium.size {
            let count = (medium.size - offset).min(buffer.len() as u64) as usize;
            medium
                .bytes
                .read_at(offset, &mut buffer[..count])
                .map_err(|error| {
                    if error.kind == ErrorKind::Usage {
                        Error::new(
                            ErrorKind::Content,
                            "export reader rejected an in-range read",
                        )
                    } else {
                        error
                    }
                })?;
            output.write(&buffer[..count])?;
            offset += count as u64;
        }
        let mut report = ExportReport::new().with_outputs(vec![output.finish()?]);
        if format.scope == ExportScope::Machine {
            let document = sink.create("machine.txt")?;
            let document_bytes = if config.bytes == b"fixed" {
                b"slot=slot-0.raw\nlength=fixed\nremovable=true\n".as_slice()
            } else {
                b"slot=slot-0.raw\nlength=variable\nremovable=true\n".as_slice()
            };
            document.write(document_bytes)?;
            report.outputs.push(document.finish()?);
        } else {
            report.notes.push(
                "Raw bytes cannot express the medium-length policy or removable-slot setting."
                    .into(),
            );
        }
        Ok(report)
    }
}

// FIPS 180-4, sections 5 and 6.2: full SHA-256 for test-support receipts.
// Keeping this local avoids imposing a hashing dependency on consumers.
fn sha256(bytes: &[u8]) -> [u8; 32] {
    const K: [u32; 64] = [
        0x428a2f98, 0x71374491, 0xb5c0fbcf, 0xe9b5dba5, 0x3956c25b, 0x59f111f1, 0x923f82a4,
        0xab1c5ed5, 0xd807aa98, 0x12835b01, 0x243185be, 0x550c7dc3, 0x72be5d74, 0x80deb1fe,
        0x9bdc06a7, 0xc19bf174, 0xe49b69c1, 0xefbe4786, 0x0fc19dc6, 0x240ca1cc, 0x2de92c6f,
        0x4a7484aa, 0x5cb0a9dc, 0x76f988da, 0x983e5152, 0xa831c66d, 0xb00327c8, 0xbf597fc7,
        0xc6e00bf3, 0xd5a79147, 0x06ca6351, 0x14292967, 0x27b70a85, 0x2e1b2138, 0x4d2c6dfc,
        0x53380d13, 0x650a7354, 0x766a0abb, 0x81c2c92e, 0x92722c85, 0xa2bfe8a1, 0xa81a664b,
        0xc24b8b70, 0xc76c51a3, 0xd192e819, 0xd6990624, 0xf40e3585, 0x106aa070, 0x19a4c116,
        0x1e376c08, 0x2748774c, 0x34b0bcb5, 0x391c0cb3, 0x4ed8aa4a, 0x5b9cca4f, 0x682e6ff3,
        0x748f82ee, 0x78a5636f, 0x84c87814, 0x8cc70208, 0x90befffa, 0xa4506ceb, 0xbef9a3f7,
        0xc67178f2,
    ];
    let mut state: [u32; 8] = [
        0x6a09e667, 0xbb67ae85, 0x3c6ef372, 0xa54ff53a, 0x510e527f, 0x9b05688c, 0x1f83d9ab,
        0x5be0cd19,
    ];
    let blocks = (bytes.len() + 9).div_ceil(64);
    let length = (bytes.len() as u64).wrapping_mul(8).to_be_bytes();
    for block in 0..blocks {
        let mut chunk = [0; 64];
        for (index, byte) in chunk.iter_mut().enumerate() {
            let offset = block * 64 + index;
            *byte = if offset < bytes.len() {
                bytes[offset]
            } else if offset == bytes.len() {
                0x80
            } else if offset >= blocks * 64 - 8 {
                length[offset - (blocks * 64 - 8)]
            } else {
                0
            };
        }
        let mut w = [0u32; 64];
        for (i, word) in chunk.as_chunks::<4>().0.iter().enumerate() {
            w[i] = u32::from_be_bytes(*word);
        }
        for i in 16..64 {
            let s0 = w[i - 15].rotate_right(7) ^ w[i - 15].rotate_right(18) ^ (w[i - 15] >> 3);
            let s1 = w[i - 2].rotate_right(17) ^ w[i - 2].rotate_right(19) ^ (w[i - 2] >> 10);
            w[i] = w[i - 16]
                .wrapping_add(s0)
                .wrapping_add(w[i - 7])
                .wrapping_add(s1);
        }
        let [mut a, mut b, mut c, mut d, mut e, mut f, mut g, mut h] = state;
        for i in 0..64 {
            let s1 = e.rotate_right(6) ^ e.rotate_right(11) ^ e.rotate_right(25);
            let choose = (e & f) ^ (!e & g);
            let t1 = h
                .wrapping_add(s1)
                .wrapping_add(choose)
                .wrapping_add(K[i])
                .wrapping_add(w[i]);
            let s0 = a.rotate_right(2) ^ a.rotate_right(13) ^ a.rotate_right(22);
            let majority = (a & b) ^ (a & c) ^ (b & c);
            let t2 = s0.wrapping_add(majority);
            h = g;
            g = f;
            f = e;
            e = d.wrapping_add(t1);
            d = c;
            c = b;
            b = a;
            a = t1.wrapping_add(t2);
        }
        for (item, value) in state.iter_mut().zip([a, b, c, d, e, f, g, h]) {
            *item = item.wrapping_add(value);
        }
    }
    let mut digest = [0; 32];
    for (word, out) in state.iter().zip(digest.as_chunks_mut::<4>().0.iter_mut()) {
        out.copy_from_slice(&word.to_be_bytes());
    }
    digest
}
