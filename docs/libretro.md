# Libretro extension plan

The accepted implementation scope is **after 1.0, NES first**. This release
makes no API change for libretro and implements no wrapper. The wrapper is a
separate crate; its implementation is not a prerequisite for contract 1.0.
These are adapter designs to validate when the wrapper is built, rather than
claims that either current core implements optional facilities.

| Adapter concern | Existing seam / missing information | Compatible way forward |
| --- | --- | --- |
| One frame per call | NES advertises frame stops; PC does not | Poll input, run until a frame stop, read frame and drain audio. Bound error/no-progress cases. Keep PC out of the first wrapper. |
| Refresh rate, region and maximum geometry | Audio rate and current Frame exist; display refresh rate, region and maximum dimensions are absent | A NES-specific wrapper can know its fixed profile. A generic wrapper can later use optional non-exhaustive display/timing metadata. Never infer refresh from pixel serial changes. |
| Video/audio callbacks | Borrowed RGB pixels and stereo i16 already exist | Wrapper owns callback lifetimes, format conversion, audio batching and presentation choices. |
| Save-state size | Streaming save/load and identity exist; no stable maximum serialized size | Initially report unsupported for current NES. Later add an optional bound/query with Unsupported default, or a proven NES-specific bound. One sample serialization does not prove a future maximum. |
| Battery saves | Storage checkpoints exist; live SRAM is not exposed | Wrapper-provided Storage can persist through host/VFS facilities. A ten-second checkpoint is not a live SRAM view. Native SRAM exchange needs an explicit optional synchronization/view design, or stays unavailable. |
| Controller selection and options | Ports are config-fixed; options are creation-only strings | Fixed NES pads work. Runtime replug/options need separate optional capability methods or controlled recreation. Changing a flag does not implicitly change a running machine. |
| Cheats, memory maps and rewind fast paths | Absent; save/load can use in-memory buffers | Leave unsupported initially; add optional methods only for an actual need. |
| C boundary and lifecycle | Safe Rust objects and explicit errors | Separate wrapper owns FFI, panic/error translation, thread confinement and teardown. It must not impose global state, unsafe code or a C ABI on the contract/core library. |

Later extensions must preserve existing behavior, object safety, ownership and
thread bounds. Optional methods need safe defaults (Unsupported for a fallible
capability), and metadata needs backward-compatible absence/default meaning.
Reinterpreting storage records as live SRAM or making new timing/state queries
mandatory would require a breaking contract change.
