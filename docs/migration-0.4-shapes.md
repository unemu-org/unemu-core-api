# Migrating the 0.4 API shapes

This is the first part of the unreleased breaking 0.4 minor. Package versions,
CONTRACT and release pins still name 0.3 until release preparation. The table
lists source and behavior changes required by this part. Existing valid rates,
scalar IDs, ownership bounds, content representations and storage interfaces
continue to work.

| Type or operation | Old pattern | New pattern |
| --- | --- | --- |
| ContractVersion | `ContractVersion { major, minor }` | `ContractVersion::new(major, minor)`; field patterns include `..`. Keep declaring the implemented version explicitly. |
| Rate | `Rate { numerator, denominator }` | `Rate::new(numerator, denominator)` for known values, `Rate::checked_new` for fallible validation; field patterns include `..`. `new` panics for denominator zero. |
| MachineDescription clock | Zero or undefined clock | Positive numerator and denominator; `Rate::is_clock` checks it. `MachineDescription::new` panics for an invalid clock. |
| MachineDescription ID lists | Duplicate IDs in displays, inputs or slots | Unique IDs within each list, including after public-field mutation. The corresponding builders reject duplicates. |
| FrameArea | `FrameArea { x, y, width, height }` | Const `FrameArea::new(x, y, width, height)`; field patterns include `..`. Empty rectangles are representable but fail `is_within`. |
| Frame | Zero dimensions/aspect or mismatched pixels | Positive width, height and aspect components; exactly width × height pixels. `Frame::new` rejects invalid values; preserve validity after field mutation. |
| Frame serial | Skip all frame processing when serial matches | Serial tracks pixels. Refresh geometry, aspect and visible area every frame. Reconfigure a texture when its geometry changes even if serial matches. |
| Initial frame | Assume a picture exists before execution | Accept either Usage until the first completed picture or an initial blank frame. Preserve the last completed frame through Failed and Finished. |
| InputEvent::Key | `Key { port, key, state }` | `InputEvent::key(port, key, state)`; match `Key { port, key, state, .. }`. |
| InputEvent::PointerMotion | `PointerMotion { port, dx, dy }` | `InputEvent::pointer_motion(port, dx, dy)`; add `..` to patterns. |
| InputEvent::PointerButton | `PointerButton { port, button, pressed }` | `InputEvent::pointer_button(port, button, pressed)`; add `..` to patterns. |
| InputEvent::Pad | `Pad { port, control, pressed }` | `InputEvent::pad(port, control, pressed)`; add `..` to patterns. |
| InputEvent::PointerPosition | `PointerPosition { port, position }` | `InputEvent::pointer_position(port, position)`; add `..` to patterns. |
| InputKind::RelativePointer | `RelativePointer { buttons }` | `InputKind::relative_pointer(buttons)`; match with `..`. |
| InputKind::Pad | `Pad { controls }` | Const `InputKind::pad(controls)`, including `Vec::new()` in constants; match with `..`. All replacement variant factories preserve const construction. |
| InputKind::AbsolutePointer | `AbsolutePointer { display, buttons }` | `InputKind::absolute_pointer(display, buttons)`; match with `..`. |
| WriteTarget::Kept | `Kept { record }` | `WriteTarget::kept(record)`; match `Kept { record, .. }`. Kept-record naming rules remain unchanged. |
| Machine::input | Implement or call a method returning `()` | Implement `Result<(), Error>` and handle the result at every call site. Return Usage without queuing in Off, Failed and Finished. |
| Machine::finish / Status | Successful finish leaves a runnable status | Set Finished only after successful finalization. Reject a second finish and every finish after Failed with Usage. Failed finalization sets Failed. Handle Finished in status-dependent controls and exhaustive local status translations. |
| Terminal method guards | Mutations after a clean finish or failure | Apply matrix status rejections (including Off) first, absent capabilities as Unsupported second, and invalid arguments as Usage last. Reject run, reset, input, media and state I/O in terminal statuses with Usage. Continue description/time/status/frame/slot/identity inspection and message/audio drains. Ignore set_outputs in terminal states. |
| Halted/Off run | Reject every status other than Ready | Return Halted/PoweredOff immediately without executing, consuming input or checking capabilities/arguments, including unsupported frame stops and unbounded requests. Reset is the way to wake Halted. |
| RunLimit | Interpret `steps(n)` as n more steps | It is an absolute total; the clearer alias is `at_steps(n)`. Compute a relative budget with checked addition to `machine.steps()`. |
| Ready run validation | Reject a request without limits or consume input before capability rejection | Allow no deadline, step total or frame stop: run until halt, power-off or error; front ends enforce their own budgets. A requested frame stop also ends a run. Reject unavailable frame stops with Unsupported before consuming input. |
| Already-reached limits | Execute one more instruction or retain queued input | Apply queued input in order, then return the reached limit without executing. |
| Simultaneous stops | Arbitrary priority | Error, power-off, halt, requested frame, deadline, steps. Do not stop for an unrequested frame. |
| Counter exhaustion | Wrap or saturate virtual counters | Fail with Guest and set Failed before the next step would overflow either counter. Already-reached limits still return normally. |
| Reset | Reset counters or leave queued controls for the next boot | Preserve ticks and absolute steps for both reset kinds. Clear queued input and held controls; preserve absolute pointer positions and inserted media. Success enters Ready. Soft in Off returns Usage; advertised Hard can restart Off. Preserve queue/counters on failure; apply the documented failure status rules. |
| Features::media_changes | Promise selection and catalogue extension together | Advertise selection/ejection only. Selection-only cores may keep add_medium Unsupported. |
| Features::media_additions | No separate capability | Advertise with `with_media_additions(true)` only when add_medium works; front ends gate their add control separately. It defaults to false. Non-removable slots cannot be extended. |
| Media boundaries | Queue insertion or opening failure for run | Complete catalogue addition/selection and required opens in the media call. Return opening errors there. Preserve catalogue/insertion/status on ordinary errors; StorageUncertain sets Failed. Do not wake Halted. Reuse the held record when selecting compatible aliases with the same base identity, format and kept-record name. Execution failures surface from run and normally set Failed, with the documented halt-time durability exception; other failures follow method-specific rules. |
| Saved-state identity | Release/config/media token without mutable-data scope | Include effective configuration, catalogue and immutable media identities. Capture mutable overlays/battery data or include their generation in the opaque token. Do not impose text framing restrictions. |
| Save/load | Partially mutate on failed load or flush/rewind records implicitly | Save preserves guest state. Load validates/stages before applying; any failure preserves observable state, queues and records. Success restores time, steps, Ready/Halted/Off status and input; drops old audio and follows restored audio position. Keep caller output preferences. Never rewind or commit external records. Reject incompatible external data with State. |
| HostFilesystem | `HostFilesystem` unit value | `HostFilesystem::new()` or `HostFilesystem::default()`. |
| FileError::stage | Compare to strings such as `"resolve path"` | Match FileStage::{ResolvePath, Lock, Open, Metadata, VerifyPath, Cleanup, DirectorySync} with a wildcard for future stages. `Display` preserves the old words; `source` preserves the host error. Use `FileError::new` to construct it. |

READ — concrete migration sites in the local consumer source snapshots:

- PC `crates/pc-core/src/machine.rs:214,739` constructs RelativePointer and
  implements input; input field matches starting at 741 need `..`.
  Its reset at 861 currently accepts only Halted: Ready must also accept an
  advertised reset under the new matrix. Its finish at 908 must enter Finished.
  Its immediate media selection already matches the chosen boundary; advertise
  catalogue extension as absent while add_medium remains Unsupported.
- NES `crates/nes-core/src/machine.rs:31,87,137` needs the FrameArea and pad
  factories and extensible field patterns. Its input at 281 needs a result;
  run at 197 must distinguish Halted/Off from terminal rejection. Reset at 312
  must clear queued input/held controls and set Ready after success, preserving
  its existing counter carryover. Finish at 354 must enter Finished.
- Front end `crates/player-ui/src/lib.rs:864,885` needs extensible input-kind
  patterns. `crates/window/src/shell_model.rs:472` needs FrameArea::new.
  The shared front-end session in `crates/machine-view/src/session.rs:365`
  currently refreshes geometry only when serial changes: refresh metadata
  independently. Its input calls at 568, 615, 677, 717, 728, 731 and 850 need
  factory migration and result handling. Texture upload decisions in
  `crates/player-ui/src/screen.rs:74` must also account for geometry changes.

These sites were read, not edited or built. Audit consumer tests and other
adapters for the same patterns. Neither existing core advertises saved state;
keep that feature absent until implemented. No dependency revision, package
version or implements value should be updated until the coordinated release.
