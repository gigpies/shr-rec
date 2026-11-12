# Current use and setup

[Project home](../README.md) · [Engineering](ENGINEERING.md)

**Standalone take playback is available. Standalone recording is not wired yet.**
Open native REC raw takes or SHR format-1 take folders directly; no song file,
SHR process or GigPies session is needed. Opening a take never changes its files.

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
Keys `1`–`8` activate the eight visible controls in reading order. All playback
operations are reachable by touch. Standalone MIDI/controller input is pending.

## Keep settings separate

Default private data location is `$XDG_DATA_HOME/rec`, or `~/.local/share/rec`.
Use `--data-root /your/private/folder` or `REC_DATA_ROOT` to change it.

```sh
./target/release/shr-rec --data-root ./user --init-config
```

This creates 18 disarmed, unassigned inputs without replacing an existing
`config.json`. Input arming lasts for this run; Record, capture meters and source
discovery remain unfinished. Playback SAVE writes a separate `playback.json`,
remembering the last saved take's routes/mix. Opening/browsing alone writes no
settings, creates no data root and does not open an audio device.

## Recording through GigPies

The unchanged raw library writes aligned mono PCM24 stems and a take manifest
when fed by its host. GigPies integration does not enable the standalone Record
button. [GigPies recording scope](https://github.com/gigpies/gigpies/blob/main/docs/README.md#recording).
A damaged/interrupted take is not complete. [Recording and recovery reference](RAW_RECORDER.md).

[Playback formats and engineering limits](PLAYBACK.md) · [Detailed configuration and launcher setup](reference/LOCAL_SETUP.md)
