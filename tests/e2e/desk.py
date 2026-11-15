"""Keyboard, pointer and screenshots for the private headless GNOME Shell,
via Mutter's RemoteDesktop + ScreenCast D-Bus APIs (what gnome-remote-desktop uses)."""
import subprocess
import time

import gi

gi.require_version("Gio", "2.0")
from gi.repository import Gio, GLib  # noqa: E402

RD = "org.gnome.Mutter.RemoteDesktop"
SC = "org.gnome.Mutter.ScreenCast"

KEYSYMS = {
    "Tab": 0xFF09, "Return": 0xFF0D, "Escape": 0xFF1B, "space": 0x20, "Up": 0xFF52,
    "Down": 0xFF54, "Left": 0xFF51, "Right": 0xFF53, "Home": 0xFF50, "End": 0xFF57,
    "BackSpace": 0xFF08, "Shift_L": 0xFFE1, "Control_L": 0xFFE3, "Alt_L": 0xFFE9,
    "Super_L": 0xFFEB, "Page_Up": 0xFF55, "Page_Down": 0xFF56, "Delete": 0xFFFF,
}


def pump(seconds=0.0):
    ctx = GLib.MainContext.default()
    end = time.monotonic() + seconds
    while True:
        while ctx.pending():
            ctx.iteration(False)
        if time.monotonic() >= end:
            return
        time.sleep(0.005)


class Desk:
    def __init__(self, connector="Meta-0"):
        self.bus = Gio.bus_get_sync(Gio.BusType.SESSION)
        self.rd = self._call(RD, "/org/gnome/Mutter/RemoteDesktop", RD, "CreateSession", None, "(o)")[0]
        sid = self._call(RD, self.rd, "org.freedesktop.DBus.Properties", "Get",
                         GLib.Variant("(ss)", (RD + ".Session", "SessionId")), "(v)")[0]
        sc = self._call(SC, "/org/gnome/Mutter/ScreenCast", SC, "CreateSession",
                        GLib.Variant("(a{sv})", ({"remote-desktop-session-id": GLib.Variant("s", sid)},)), "(o)")[0]
        self.stream = self._call(SC, sc, SC + ".Session", "RecordMonitor",
                                 GLib.Variant("(sa{sv})", (connector, {"cursor-mode": GLib.Variant("u", 1)})),
                                 "(o)")[0]
        self.node = None

        def on_added(_c, _s, _p, _i, _n, params):
            self.node = params[0]

        self.bus.signal_subscribe(SC, SC + ".Stream", "PipeWireStreamAdded", self.stream, None,
                                  Gio.DBusSignalFlags.NONE, on_added)
        self._call(RD, self.rd, RD + ".Session", "Start", None, None)
        end = time.monotonic() + 10
        while self.node is None and time.monotonic() < end:
            pump(0.05)
        if self.node is None:
            raise RuntimeError("screen cast stream never started")

    def _call(self, dest, path, iface, method, args, rtype):
        r = self.bus.call_sync(dest, path, iface, method, args,
                               GLib.VariantType(rtype) if rtype else None,
                               Gio.DBusCallFlags.NONE, 10000, None)
        return r.unpack() if r else None

    def _rd(self, method, args):
        self._call(RD, self.rd, RD + ".Session", method, args, None)

    def key(self, *names, hold=0.02):
        """Press a chord, e.g. key("Control_L", "a") or key("Tab")."""
        syms = [KEYSYMS.get(n, ord(n) if len(n) == 1 else None) for n in names]
        if None in syms:
            raise ValueError(f"unknown key in {names}")
        for s in syms:
            self._rd("NotifyKeyboardKeysym", GLib.Variant("(ub)", (s, True)))
        time.sleep(hold)
        for s in reversed(syms):
            self._rd("NotifyKeyboardKeysym", GLib.Variant("(ub)", (s, False)))
        time.sleep(0.03)

    def type(self, text):
        for ch in text:
            self.key(ch)

    def click(self, x, y, button=0x110):
        self._rd("NotifyPointerMotionAbsolute", GLib.Variant("(sdd)", (self.stream, float(x), float(y))))
        time.sleep(0.05)
        self._rd("NotifyPointerButton", GLib.Variant("(ib)", (button, True)))
        time.sleep(0.03)
        self._rd("NotifyPointerButton", GLib.Variant("(ib)", (button, False)))
        time.sleep(0.05)

    def screenshot(self, path):
        # One frame: an unchanged screen sends no more, so asking for several hangs.
        subprocess.run(
            ["gst-launch-1.0", "-q", "pipewiresrc", f"path={self.node}", "num-buffers=1",
             "always-copy=true", "!", "videoconvert", "!", "pngenc", "!", "filesink", f"location={path}"],
            check=True, timeout=20, capture_output=True)
        return path

    def close(self):
        try:
            self._rd("Stop", None)
        except GLib.Error:
            pass
