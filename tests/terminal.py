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
                while b"implemented" not in output:
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
