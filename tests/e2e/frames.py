"""Measure when the screen actually changes after an action.

The screen cast only produces a frame when something on screen changes, so
the frames that arrive after an action show when it became visible and when
the screen settled.
"""
import subprocess
import threading
import time


class FrameWatch:
    def __init__(self, desk):
        self.node = desk.node
        self.times = []
        self.proc = subprocess.Popen(
            ["gst-launch-1.0", "-v", "pipewiresrc", f"path={self.node}", "always-copy=true", "!",
             "videoconvert", "!", "identity", "silent=false", "!", "fakesink"],
            stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True, bufsize=1)
        threading.Thread(target=self._read, daemon=True).start()
        time.sleep(1.0)  # let the stream start and its first frame pass

    def _read(self):
        for line in self.proc.stdout:
            if "chain" in line:
                self.times.append(time.monotonic())

    def measure(self, action, settle=1.5):
        """Run action(); return (ms to first changed frame, ms to last frame)."""
        start = len(self.times)
        t0 = time.monotonic()
        action()
        time.sleep(settle)
        new = [t for t in self.times[start:] if t >= t0]
        if not new:
            return None, None
        return (new[0] - t0) * 1000, (new[-1] - t0) * 1000

    def close(self):
        self.proc.terminate()
