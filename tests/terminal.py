"""Short production CLI regression, invoked by cargo test; no hardware access."""
import fcntl
import os
import pty
import select
import signal
import struct
import subprocess
import sys
import tempfile
import termios
import time

for action in (b"q", b"\x1b[<0;36;12M", signal.SIGTERM, signal.SIGINT, signal.SIGHUP):
    with tempfile.TemporaryDirectory(prefix="rec-terminal-") as root:
        master, slave = pty.openpty()
        try:
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 13, 40, 0, 0))
            before = termios.tcgetattr(slave)
            proc = subprocess.Popen([sys.argv[1], "--data-root", root], stdin=slave, stdout=slave, stderr=slave)
            try:
                output = b""
                deadline = time.monotonic() + 5
                while b"SETUP" not in output:
                    assert time.monotonic() < deadline, repr(output)
                    if select.select([master], [], [], 0.1)[0]:
                        output += os.read(master, 65536)
                if isinstance(action, bytes):
                    os.write(master, action)
                else:
                    proc.send_signal(action)
                assert proc.wait(timeout=5) == 0
                while select.select([master], [], [], 0.05)[0]:
                    output += os.read(master, 65536)
                assert termios.tcgetattr(slave) == before, "terminal flags were not restored"
                for command in (b"\x1b[?1049l", b"\x1b[?25h", b"\x1b[?1000l", b"\x1b[?1006l"):
                    assert command in output, (command, output)
                assert not os.listdir(root), "ordinary launch wrote private state"
            finally:
                if proc.poll() is None:
                    proc.kill()
                    proc.wait()
        finally:
            os.close(master)
            os.close(slave)

# The real CLI's direct take-open path must remain read-only and device-free.
# Only the null-device Rust tests exercise PLAY; this PTY never requests JACK.
import json
import pathlib
import wave

for action in (b"q", signal.SIGTERM):
    with tempfile.TemporaryDirectory(prefix="rec-player-terminal-") as root:
        source = pathlib.Path(root) / "take"
        source.mkdir()
        tracks = []
        for index in range(2):
            name = f"stem-{index}.wav"
            with wave.open(str(source / name), "wb") as wav:
                wav.setnchannels(1)
                wav.setsampwidth(3)
                wav.setframerate(48000)
                wav.writeframes(b"\x01\x00\x00" * 10)
            tracks.append(dict(id=f"stem-{index}", label=f"Stem {index}",
                               role="mono", group=None, preferred_source=None,
                               actual_jack_port=f"synthetic:in-{index}",
                               wav_file=name, frames=10, finalized=True))
        manifest = dict(format_version=1, session_name="PTY synthetic take",
                        sample_rate=48000, total_frames=10,
                        duration_seconds=10 / 48000, completeness="complete",
                        finalization="finalized", dropped_frames=0,
                        overflow_events=0, callback_violations=0, xruns=0,
                        writer_high_water_frames=0, recovery=[], tracks=tracks)
        (source / "session.json").write_text(json.dumps(manifest))
        before_files = {p.name: p.read_bytes() for p in source.iterdir()}
        private = pathlib.Path(root) / "private"
        master, slave = pty.openpty()
        try:
            fcntl.ioctl(slave, termios.TIOCSWINSZ, struct.pack("HHHH", 13, 40, 0, 0))
            before = termios.tcgetattr(slave)
            proc = subprocess.Popen([sys.argv[1], "--data-root", str(private),
                                     "--takes-root", root, "--open", str(source)],
                                    stdin=slave, stdout=slave, stderr=slave)
            try:
                output = b""
                deadline = time.monotonic() + 5
                # Ratatui may position the cursor between words instead of writing spaces.
                while b"Assign" not in output:
                    assert time.monotonic() < deadline, repr(output)
                    if select.select([master], [], [], 0.1)[0]:
                        output += os.read(master, 65536)
                if isinstance(action, bytes):
                    os.write(master, action)
                else:
                    proc.send_signal(action)
                assert proc.wait(timeout=5) == 0
                assert termios.tcgetattr(slave) == before
                assert not private.exists(), "direct opening created private state"
                assert before_files == {p.name: p.read_bytes() for p in source.iterdir()}
            finally:
                if proc.poll() is None:
                    proc.kill()
                    proc.wait()
        finally:
            os.close(master)
            os.close(slave)
