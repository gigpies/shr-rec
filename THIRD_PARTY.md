# Reference provenance

The initial shell, configuration and transport code are newly written here.
No source code, private configuration or assets were copied from SHR-DAW.

Design references live in `../shr-daw` and were inspected read-only at revision
`0e9ed0fc3873a22d6253b5b7cc4885daeff0cfae`:

- `docs/MULTITRACK_RECORDING.md`, `MULTITRACK_PRESENTATION.md`,
  `MR18_TEST_PLAN.md`, `CONTROLLER_INTERFACE.md`;
- `docs/CODE_REVIEW_2026-09-05.md`, including its implementation record;
- `src/audio_recorder.rs`, `recording.rs`, `loop_player.rs`, `jack.rs`,
  `controller_profile.rs`, `controller_learn.rs`.

SHR-DAW is MIT licensed, copyright (c) 2026 SHR-DAW contributors. Any subsequent
copied/adapted source must retain its copyright and license notice here or in a
source-specific notice. Reference hardware results do not establish shr-rec results.

Ratatui 0.20 and Crossterm 0.26 follow the inspected project's dependency baseline;
this is a deliberate reuse of known APIs, not a claim that they are newest.
Cargo.lock pins the complete Rust dependency graph. Dependencies retain their
own package licenses: anyhow (MIT OR Apache-2.0), crossterm/ratatui (MIT),
serde/serde_json/libc (MIT OR Apache-2.0), signal-hook (Apache-2.0 OR MIT).
