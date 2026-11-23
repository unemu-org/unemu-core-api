// SPDX-License-Identifier: MIT OR Apache-2.0
//! Downstream construction and matching: this module has no crate-private access.
use unemu_core_api::*;

const VERSION: ContractVersion = ContractVersion::new(0, 5);
const RATE: Rate = Rate::new(8_000, 1);
const AREA: FrameArea = FrameArea::new(0, 0, 1, 1);
const KIND: ContentKind = ContentKind::new("raw", "Image", &["img"]);
const OPTIONS: &[OptionDescription] = &[OptionDescription::new("mode", "Mode")];
const NOTICES: &[Notice] = &[Notice::new("Component", "Notice")];
const PLATFORM: PlatformDescription = PlatformDescription::new("test", "Test");

#[test]
fn extensible_variant_factories_preserve_const_construction() {
    const PAD: InputKind = InputKind::pad(Vec::new());
    const RELATIVE: InputKind = InputKind::relative_pointer(3);
    const ABSOLUTE: InputKind = InputKind::absolute_pointer(DisplayId(0), 2);
    const KEY: InputEvent = InputEvent::key(PortId(0), Key::A, KeyState::Pressed);
    const MOTION: InputEvent = InputEvent::pointer_motion(PortId(0), 1, -1);
    const BUTTON: InputEvent = InputEvent::pointer_button(PortId(0), 0, true);
    const CONTROL: InputEvent = InputEvent::pad(PortId(0), PadControl::A, true);
    const POSITION: InputEvent = InputEvent::pointer_position(PortId(0), Some((1, 2)));
    const KEPT: WriteTarget<'static> = WriteTarget::kept("kept-const");
    assert!(matches!(PAD, InputKind::Pad { controls, .. } if controls.is_empty()));
    assert!(matches!(
        RELATIVE,
        InputKind::RelativePointer { buttons: 3, .. }
    ));
    assert!(matches!(
        ABSOLUTE,
        InputKind::AbsolutePointer { buttons: 2, .. }
    ));
    assert_eq!(KEY, InputEvent::key(PortId(0), Key::A, KeyState::Pressed));
    assert_eq!(MOTION, InputEvent::pointer_motion(PortId(0), 1, -1));
    assert_eq!(BUTTON, InputEvent::pointer_button(PortId(0), 0, true));
    assert_eq!(CONTROL, InputEvent::pad(PortId(0), PadControl::A, true));
    assert_eq!(
        POSITION,
        InputEvent::pointer_position(PortId(0), Some((1, 2)))
    );
    assert!(matches!(
        KEPT,
        WriteTarget::Kept {
            record: "kept-const",
            ..
        }
    ));
}

#[test]
fn public_data_is_constructible_and_extensible_patterns_are_usable() {
    let _ = ContractVersion::new(VERSION.major, VERSION.minor);
    assert_eq!(Rate::checked_new(8_000, 1), Some(RATE));
    let features = Features::NONE
        .with_resets(&[ResetKind::Soft, ResetKind::Hard])
        .with_saved_state(true)
        .with_media_changes(true)
        .with_media_additions(true)
        .with_frame_stops(true);
    let _ = Features::default();
    let _ = PLATFORM
        .with_config_types(&["text/plain"])
        .with_features(features)
        .with_options(OPTIONS)
        .with_content(&[KIND]);
    let _ = CoreDescription::new(VERSION, "test", "Test", "1")
        .with_platforms(&[PLATFORM])
        .with_notices(NOTICES);
    let _ = MachineDescription::new(RATE, "instruction")
        .with_displays(vec![DisplayDescription::new(DisplayId(0), "Display")])
        .with_audio(Some(AudioDescription::new(RATE)))
        .with_inputs(vec![InputPortDescription::new(
            PortId(0),
            InputKind::Keyboard,
            "Keyboard",
        )])
        .with_slots(vec![SlotDescription::new(
            SlotId(0),
            SlotKind::Floppy,
            "Slot",
            true,
        )
        .with_formats(vec!["raw".into()])])
        .with_features(features);
    let _ = Ticks(0);
    let _ = Key(0);
    let _ = PortId(0);
    let _ = DisplayId(0);
    let _ = SlotId(0);
    let _ = Outputs::NONE.with_audio(true);
    let _ = Outputs::default();
    let _ = Frame::new(1, 1, &[0], (1, 1), 0).with_visible(AREA);
    let _ = RunLimit::until(Ticks(1))
        .with_steps(Some(1))
        .with_stop_on_frame(true);
    let _ = RunLimit::steps(1).with_until(None);
    let _ = RunLimit::at_steps(1);
    for event in [
        InputEvent::key(PortId(0), Key::A, KeyState::Pressed),
        InputEvent::pointer_motion(PortId(0), 1, -1),
        InputEvent::pointer_button(PortId(0), 0, true),
        InputEvent::pad(PortId(0), PadControl::A, true),
        InputEvent::pointer_position(PortId(0), None),
        InputEvent::ReleaseAll,
    ] {
        match event {
            InputEvent::Key {
                port, key, state, ..
            } => {
                let _ = (port, key, state);
            }
            InputEvent::PointerMotion { port, dx, dy, .. } => {
                let _ = (port, dx, dy);
            }
            InputEvent::PointerButton {
                port,
                button,
                pressed,
                ..
            } => {
                let _ = (port, button, pressed);
            }
            InputEvent::Pad {
                port,
                control,
                pressed,
                ..
            } => {
                let _ = (port, control, pressed);
            }
            InputEvent::PointerPosition { port, position, .. } => {
                let _ = (port, position);
            }
            _ => {}
        }
    }
    for kind in [
        InputKind::relative_pointer(3),
        InputKind::pad(vec![PadControl::A]),
        InputKind::absolute_pointer(DisplayId(0), 1),
        InputKind::Keyboard,
    ] {
        match kind {
            InputKind::RelativePointer { buttons, .. } => {
                let _ = buttons;
            }
            InputKind::Pad { controls, .. } => {
                let _ = controls;
            }
            InputKind::AbsolutePointer {
                display, buttons, ..
            } => {
                let _ = (display, buttons);
            }
            _ => {}
        }
    }
    let target = WriteTarget::kept("kept-image");
    if let WriteTarget::Kept { record, .. } = target {
        assert_eq!(record, "kept-image");
    }
    let _ = NewMedium::new("Image", Content::Bytes(vec![0]), [0; 32], 1, "raw", target);
    let _ = SlotState::new(SlotId(0))
        .with_catalogue(vec![MediumInfo::new(
            "Image",
            [0; 32],
            1,
            "raw",
            WriteKind::Kept,
        )])
        .with_inserted(Some(0))
        .with_active(true)
        .with_accepts_changes(true);
    let _ = Error::new(ErrorKind::Usage, "Invalid")
        .with_diagnostics(vec![Diagnostic::new("item", "Invalid")]);
    let _ = Message::new(MessageLevel::Note, "Created");
    let _ = Finished::new().with_warnings(vec!["Warning".into()]);
    let _ = Finished::default();
    let config = ConfigDocumentBuf::new("text/plain", vec![]);
    let _ = Identified::new("test", "raw", config.clone())
        .with_title(Some("Title".into()))
        .with_notes(vec![]);
    let _ = Validated::new(vec![ContentRef::new([0; 32], 1, "Image")]);
    let _ = ConfigDocument::new("text/plain", b"");
    let _ = config.as_document();
    // Unit and tuple variants have deliberately fixed payloads; new variants
    // still require a wildcard when inspecting an extensible enum.
    let _ = (
        Status::Ready,
        Status::Halted,
        Status::Off,
        Status::Failed,
        Status::Finished,
    );
    let _ = (
        Stop::Deadline,
        Stop::Steps,
        Stop::Frame(DisplayId(0)),
        Stop::Halted,
        Stop::PoweredOff,
    );
    let _ = (
        MediaAction::Eject,
        MediaAction::Insert(0),
        MediaAction::Next,
    );
    let _ = (
        WriteTarget::ReadOnly,
        WriteTarget::Discarded,
        WriteKind::ReadOnly,
        WriteKind::Discarded,
    );
    let _ = (
        SlotKind::FixedDisk,
        SlotKind::Optical,
        SlotKind::Cartridge,
        SlotKind::Tape,
    );
    let _ = (KeyState::Repeated, KeyState::Released);
}

#[cfg(feature = "test-support")]
#[test]
fn public_requests_and_opaque_test_helpers_are_constructible() {
    use std::rc::Rc;
    use unemu_core_api::test_support::*;
    struct Source;
    impl ContentSource for Source {
        fn open(&self, _: &[u8; 32], _: u64) -> Result<Content, Error> {
            Ok(Content::Reader(Box::new(MemoryContent::new(vec![0]))))
        }
    }
    let storage = MemoryStorage::new();
    let _ = MemoryStorage::default();
    let reader = MemoryContent::new(vec![0]);
    let _ = OfferedContent::new("Image", [0; 32], 1, &reader);
    let _ = CreateRequest::new(
        "test",
        ConfigDocument::new("text/plain", b""),
        &Source,
        Rc::new(storage.clone()),
    )
    .with_options(&[]);
    let _ = ScriptedMachine::new(Rc::new(storage)).without_initial_picture();
    let _ = Operation::Open;
}
