# Platform code

`crates/adeline_core/src/platform.rs` is the OS glue for the engine: process containment, detached and piped spawning, console attachment and owner-only IPC pipes.

- All unsafe code lives in this module. Each `unsafe` block keeps its `SAFETY` comment, which the `undocumented_unsafe_blocks` lint requires.
- Every process the engine starts dies with the engine, even when the engine is killed. Windows puts them in a job object. On Unix each agent leads a process group that is listed in a file, and `stop_children` or the next engine kills those groups.
- The reactor (IOCP or epoll) drives piped readers, so no thread waits on them.
