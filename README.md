![SHR Rec](docs/assets/banner.svg)

# SHR Rec

**Aligned multitrack recording for GigPies.** The recording library writes a
separate 24-bit WAV for each channel, with take information and recovery support.

[**Current use and setup**](docs/LOCAL_SETUP.md) · [**GigPies guide**](https://github.com/gigpies/gigpies/blob/main/docs/README.md) · [Engineering](docs/ENGINEERING.md)

## What works

GigPies uses the raw recording library. The standalone 40×13 application is
still a shell: input selection, session-only arming and configuration work,
but **Record, Play, meters and MIDI are not connected**. Desk also has no
recording controls yet. Do not use the standalone shell to capture a take.

## Try the shell

```sh
cargo build --locked --release
./target/release/shr-rec
```

Touch PREV/NEXT to select, ARM to toggle and EXIT to leave.
Keyboard: arrows, Space, `q` or Escape. No audio device opens.

[Setup and configuration](docs/LOCAL_SETUP.md) · [Development status](docs/STATUS.md) · [MIT license](LICENSE)
