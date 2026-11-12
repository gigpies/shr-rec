# Current use and setup

[Project home](../README.md) · [Engineering](ENGINEERING.md)

**Standalone capture and synchronized take playback are available for software review.**
Open native REC raw takes or SHR format-1 take folders directly; no song file,
SHR process or GigPies session is needed. Opening a take never changes its files.

## Record a take

Use a 40×13 terminal and an existing JACK server. REC never starts or changes
that server. No audio connection is made until an explicit RECORD or PLAY.

1. Touch SETUP (or the input header), then REFRESH. Inspect the JACK rate and
   available inputs. PREV/NEXT selects a track; ASSIGN opens exact source names.
   PREV/NEXT chooses a source, USE applies, UNSET clears and CANCEL preserves
   the old assignment. Missing/replaced sources never fall back to other ports.
2. DEST selects a private capture destination. OPEN enters a child folder, UP
   returns to its parent, USE DIR selects the current folder, CANCEL keeps the
   previous destination. The default is DATA_ROOT/takes; `--takes-root` overrides
   saved destination/library preferences for this run. SAVE remembers sources,
   destination and arm states privately. BACK returns to inputs.
3. Select each required track and touch ARM. Every armed track must show READY.
   Inspect the displayed RIFF time ceiling and payload rate. RECORD prepares a
   new generated take, waits for writer readiness, then attaches every exact
   source. Capture begins only after successful attachment. There is no hidden
   input monitoring or capture output. Use external monitoring.
4. The shared status shows preparation/recording/finalization/faults. Progress
   shows seconds, accepted frames (A), written frames (W), drops and overflows.
   During capture, row 8 shows 18 circular input LEDs in configuration order;
   PREV/NEXT follows banks for larger configurations. No second input owner is opened.
   Written frames are not proof of durable completion. Tap the status row (or
   `e`) for full paths/messages; PREV/NEXT scroll, and its title closes it.
5. STOP quiesces capture, then drains/finalizes on a worker. Wait for Complete;
   neither a Stop request nor the last written-frame count proves success.
   TAKES discovers the destination; OPEN validates the take, then choose OUTPUTS
   explicitly before PLAY. Recording input numbers never imply playback outputs.

EXIT while preparing/recording offers STOP AND FINALIZE / CANCEL. Cancel keeps
recording. An incomplete/error result remains visible before another deliberate
EXIT. Signals also stop and finalize; the retained path/result is printed after
terminal restoration. A forced crash leaves evidence for recovery. Do not remove
partial stems, timeline, failure reports or a stale settings temporary blindly.

For an interrupted native take: TAKES → select its folder → RECOVER → COPY.
This explicitly validates every stem and makes a new recovered-incomplete take
in the selected destination. Originals remain unchanged, including partial
publication evidence. Wait for completion, return to TAKES, select the new copy,
OPEN and deliberately ACCEPT its warning before assigning outputs. Failed
recovery retains source evidence; ambiguous/malformed/symlink inputs refuse.
Recovery never turns damaged data into a normal complete take.

Capture meters while stopped, stereo capture grouping and MIDI/controller input
remain pending. Sustained native 18-channel capture/playback, physical routing,
listening and storage load need a separate on-device acceptance session.
[Capture lifecycle, limits, recovery and offline tests](CAPTURE.md).

## Open a take

Build with this checkout's pinned toolchain, then use a **40×13** terminal:

```sh
CARGO_INCREMENTAL=0 CARGO_BUILD_JOBS=1 cargo build --locked --release
./target/release/shr-rec --takes-root /your/take-library
# Or open one take directory directly:
./target/release/shr-rec --open /your/take-library/rehearsal.take
```

In the input screen touch TAKES. PREV/NEXT or a take's title selects it; OPEN
validates every stem. REFRESH keeps selection by path. The default library is
`DATA_ROOT/takes`. `--takes-root` scans direct child folders only. An invalid take
shows an error. Valid incomplete/recovered takes require ACCEPT and keep their
warning while playing; damaged partial files need recovery first.

## Choose outputs, then play

1. Touch OUTPUTS. An existing JACK server must already be running; REC never
   starts, restarts or reconfigures one. Missing audio leaves the terminal usable.
2. Touch MODE for exact stems or stereo audition. PREV/NEXT selects a stem or
   stereo side; ASSIGN cycles the fully named destination ports. Every exact
   stem needs its own destination. Stereo needs distinct left/right destinations.
3. For stereo, MIX provides each stem's LEVEL −/+, PAN and MUTE. The displayed
   mix reserve is 6 dB plus 1/N for the total stem count. Master playback volume
   stays 100%; set listening loudness on the physical amplifier.
4. BACK returns from MIX. APPLY uses the output draft; CANCEL restores the prior
   assignment. SAVE also remembers that take's exact routes/mix privately.
5. PLAY/PAUSE controls the common timeline. −5s/+5s seeks every stem together;
   STOP closes playback and resets to zero. At the end, PLAY restarts. An underrun,
   read error, disconnected route, xrun, changed rate or lost server stops all stems
   and shows the fault. STOP resets; PLAY deliberately retries after correction.

Take and JACK rates must match exactly. There is no implicit resampling or
missing-channel substitution. Recorded input numbers never choose output ports.
Output/mix editing requires STOP. Tap the status row (or press `e`) to read the
full message; tap its expanded title to close it. EXIT stops playback and returns
to the caller. Ctrl-C, SIGINT, SIGTERM and SIGHUP use the same cleanup path.

Keyboard equivalents: arrows select; Enter opens/confirms a take; `p` or Space
plays/pauses; `s` stops; left/right seek; `q` exits; Escape goes back/cancels.
Keys `1`–`8` activate the eight visible controls in reading order. On inputs,
`i` opens setup, `r` records, Space/Enter toggles arm, and `e` opens status. All playback
operations are reachable by touch. Standalone MIDI/controller input is pending.

## Keep settings separate

Default private data location is `$XDG_DATA_HOME/rec`, or `~/.local/share/rec`.
Use `--data-root /your/private/folder` or `REC_DATA_ROOT` to change it.

```sh
./target/release/shr-rec --data-root ./user --init-config
```

This creates 18 disarmed, unassigned inputs without replacing an existing
`config.json`. Input changes last for this run unless SETUP → SAVE writes private `capture.json`.
Playback SAVE writes a separate `playback.json`,
remembering the last saved take's routes/mix. Opening/browsing alone writes no
settings, creates no data root and does not open an audio device.

## Recording through GigPies

The unchanged raw library writes aligned mono PCM24 stems and a take manifest
when fed by its host. The standalone JACK adapter is separate from GigPies host capture; both use
the same raw writer without changing its ABI. [GigPies recording scope](https://github.com/gigpies/gigpies/blob/main/docs/README.md#recording).
A damaged/interrupted take is not complete. [Recording and recovery reference](RAW_RECORDER.md).

[Playback formats and engineering limits](PLAYBACK.md) · [Detailed configuration and launcher setup](reference/LOCAL_SETUP.md)
