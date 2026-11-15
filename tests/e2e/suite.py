"""End-to-end tests for the Captionist settings window.

Runs inside the private headless session (session.sh / in.sh). Drives the
window the way a user or screen reader would (AT-SPI actions and the
keyboard), then checks the widget, GSettings and the daemon, and measures how
long each step takes and whether the window stays responsive.

Usage: tests/e2e/in.sh python3 tests/e2e/suite.py [t_general t_models ...]
Screenshots and results.json go to $E2E_ROOT/shots.
"""
import json
import os
import subprocess
import sys
import threading
import time
import traceback

sys.path.insert(0, os.path.dirname(os.path.abspath(__file__)))
from a11y import Atspi, actions, checked, do, find, focused, text, value  # noqa: E402
from desk import Desk, pump  # noqa: E402
from gi.repository import Gio, GLib  # noqa: E402

SCHEMA = "io.github.m_umair_khan_dev.Captionist"
DAEMON = "io.github.m_umair_khan_dev.Captionist.Daemon"
DPATH = "/io/github/m_umair_khan_dev/Captionist/Daemon"
DIFACE = "io.github.m_umair_khan_dev.Captionist.Daemon1"
SHOTS = os.path.join(os.environ.get("E2E_ROOT", os.path.expanduser("~/.cache/captionist-e2e")), "shots")
os.makedirs(SHOTS, exist_ok=True)
STALL_MS = 250  # a UI round trip slower than this counts as the window freezing

RESULTS = []


def log(*a):
    print(*a, flush=True)


def check(name, ok, detail="", ms=None):
    RESULTS.append({"name": name, "ok": bool(ok), "detail": str(detail), "ms": ms})
    t = f" [{ms:.0f} ms]" if ms is not None else ""
    log(f"{'PASS' if ok else 'FAIL'}  {name}{t}  {detail}")
    return ok


# ---------------------------------------------------------------- settings
def gs(key):
    return subprocess.run(["gsettings", "get", SCHEMA, key], capture_output=True, text=True).stdout.strip()


def gs_set(key, val):
    subprocess.run(["gsettings", "set", SCHEMA, key, val], check=True)


def gs_reset(key):
    subprocess.run(["gsettings", "reset", SCHEMA, key], check=True)


def wait(pred, timeout=5.0, step=0.01):
    t0 = time.monotonic()
    while time.monotonic() - t0 < timeout:
        try:
            if pred():
                return (time.monotonic() - t0) * 1000
        except Exception:
            pass
        pump(step)
    return None


# ---------------------------------------------------------------- daemon
class Daemon:
    def __init__(self):
        self.bus = Gio.bus_get_sync(Gio.BusType.SESSION)
        self.signals = []
        self.bus.signal_subscribe(DAEMON, DIFACE, None, DPATH, None, Gio.DBusSignalFlags.NONE, self._on)

    def _on(self, _c, _s, _p, _i, name, params):
        self.signals.append((time.monotonic(), name, params.unpack()))

    def call(self, method, args=None, rtype=None, timeout=30000):
        r = self.bus.call_sync(DAEMON, DPATH, DIFACE, method, args,
                               GLib.VariantType(rtype) if rtype else None,
                               Gio.DBusCallFlags.NONE, timeout, None)
        return r.unpack() if r else None

    def status(self):
        return json.loads(self.call("Status", None, "(s)")[0])

    def models(self, sort="recommended"):
        return json.loads(self.call("ListModels", GLib.Variant("(s)", (sort,)), "(s)")[0])

    def mark(self):
        return len(self.signals)

    def since(self, mark, name=None):
        return [s for s in self.signals[mark:] if name is None or s[1] == name]


# ---------------------------------------------------------------- UI
class Ui:
    def __init__(self, desk):
        self.desk = desk
        self.app = None
        self.refind()

    def refind(self, timeout=15):
        end = time.monotonic() + timeout
        while time.monotonic() < end:
            d = Atspi.get_desktop(0)
            for i in range(d.get_child_count()):
                a = d.get_child_at_index(i)
                if a is not None and a.get_name() == "captionist":
                    self.app = a
                    return a
            pump(0.2)
        raise RuntimeError("settings window not found")

    @property
    def frame(self):
        for i in range(self.app.get_child_count()):
            f = self.app.get_child_at_index(i)
            if f.get_role_name() == "frame":
                return f

    def one(self, role, name, timeout=5.0):
        found = []
        wait(lambda: found.extend(find(self.app, role, name)) or found, timeout)
        if not found:
            raise LookupError(f"{role} {name!r} not found")
        return found[0]

    def exists(self, role, name):
        return bool(find(self.app, role, name))

    def tab(self, name):
        t = self.one("page tab", name)
        t0 = time.monotonic()
        do(t)
        ms = wait(lambda: t.get_state_set().contains(Atspi.StateType.SELECTED), 3)
        pump(0.3)
        return ms if ms is not None else (time.monotonic() - t0) * 1000

    def focus(self, role, name, max_tabs=80):
        for back in (False, True):
            for _ in range(max_tabs):
                if any(n.get_role_name() == role and n.get_name() == name for n in focused(self.app)):
                    return True
                self.desk.key("Shift_L", "Tab") if back else self.desk.key("Tab")
                pump(0.06)
        return False

    def choose(self, combo, index):
        """Open a dropdown with the keyboard and pick the option at index.
        (The popup's items have no accessible names, so pick by position.)"""
        if not self.focus("combo box", combo):
            raise LookupError(f"could not focus dropdown {combo!r}")
        self.desk.key("space")
        pump(0.4)
        self.desk.key("Home")
        for _ in range(index):
            self.desk.key("Down")
        self.desk.key("Return")
        pump(0.2)

    def combo_label(self, combo):
        """The option a dropdown currently shows (inside its value list,
        not the row's title or subtitle)."""
        c = self.one("combo box", combo)
        for lst in find_all(c, "list"):
            for lab in find_all(lst, "label"):
                return text(lab)
        return None

    def ping(self):
        """Round trip to the app's main thread (AT-SPI is answered there)."""
        t0 = time.monotonic()
        Atspi.Component.get_extents(self.frame, Atspi.CoordType.WINDOW)
        return (time.monotonic() - t0) * 1000

    def dialog_open(self, anywhere=False):
        """Dialogs in the app; with anywhere=True also those of other
        processes (file choosers come from the desktop portal)."""
        out = []

        def w(n, depth=0):
            try:
                if n.get_role_name() in ("dialog", "alert", "file chooser", "color chooser"):
                    out.append((n.get_role_name(), n.get_name()))
                if depth < 12:
                    for i in range(n.get_child_count()):
                        w(n.get_child_at_index(i), depth + 1)
            except Exception:
                pass
        if anywhere:
            d = Atspi.get_desktop(0)
            for i in range(d.get_child_count()):
                a = d.get_child_at_index(i)
                if a is not None and a.get_name() != "gnome-shell":
                    for j in range(a.get_child_count()):
                        f = a.get_child_at_index(j)
                        if f.get_role_name() in ("dialog", "file chooser") or (
                                a.get_name() != "captionist" and f.get_role_name() == "frame"):
                            out.append((a.get_name(), f.get_role_name(), f.get_name()))
                        w(f)
        else:
            w(self.app)
        return out

    def shot(self, name):
        try:
            return self.desk.screenshot(os.path.join(SHOTS, f"{name}.png"))
        except Exception as e:
            log(f"   (screenshot {name} failed: {e})")


class Responsiveness:
    """Pings the UI from a thread while something slow runs; reports the worst stall."""

    def __init__(self, ui):
        self.ui, self.samples, self.stop = ui, [], False

    def __enter__(self):
        def run():
            while not self.stop:
                try:
                    self.samples.append(self.ui.ping())
                except Exception:
                    pass
                time.sleep(0.05)
        self.t = threading.Thread(target=run, daemon=True)
        self.t.start()
        return self

    def __exit__(self, *a):
        self.stop = True
        self.t.join(2)

    @property
    def worst(self):
        return max(self.samples) if self.samples else None

    def check(self, what):
        w = self.worst
        check(f"UI responsive during {what}", (w or 0) < STALL_MS,
              f"worst UI stall {w:.0f} ms over {len(self.samples)} samples" if w else "no samples")


# ---------------------------------------------------------------- tree helpers
def find_all(root, role):
    out = []

    def w(n):
        try:
            if n.get_role_name() == role:
                out.append(n)
            for i in range(n.get_child_count()):
                w(n.get_child_at_index(i))
        except Exception:
            pass
    w(root)
    return out


def row_has(ui, row_name, role, name):
    return any(find(r, role, name) for r in find(ui.app, "list item", row_name))


def find_btn(ui, row_name, button):
    return row_has(ui, row_name, "button", button)


def row_button(ui, row_name, button, timeout=5.0):
    found = []

    def look():
        found.clear()
        for row in find(ui.app, "list item", row_name):
            found.extend(find(row, "button", button))
        return found
    wait(look, timeout)
    if not found:
        raise LookupError(f"button {button!r} in row {row_name!r} not found")
    return found[0]


def labels_in_row(ui, row_name):
    out = []
    for r in find(ui.app, "list item", row_name):
        for c in find_all(r, "label"):
            out.append(text(c) or "")
    return out


def answer_dialog(ui, label):
    for b in find(ui.app, "button", label):
        do(b)
        return True
    return False


# ---------------------------------------------------------------- generic widget tests
def switch_test(ui, name, key):
    sw = [s for s in find(ui.app, "switch", name) if "toggle" in actions(s)][0]
    before = gs(key)
    do(sw, "toggle")
    ms = wait(lambda: gs(key) != before, 3)
    after = gs(key)
    check(f"switch '{name}' writes {key}", ms is not None and (after == "true") == checked(sw),
          f"{before} -> {after}, widget checked={checked(sw)}", ms)
    do(sw, "toggle")
    ms = wait(lambda: gs(key) == before, 3)
    check(f"switch '{name}' toggles back", ms is not None, f"-> {gs(key)}", ms)
    gs_set(key, "false" if before == "true" else "true")
    ms = wait(lambda: checked(sw) == (before != "true"), 3)
    check(f"switch '{name}' follows external change", ms is not None, "", ms)
    gs_set(key, before)
    wait(lambda: checked(sw) == (before == "true"), 3)


def spin_test(ui, name, key, lo, hi, probe):
    sp = ui.one("spin button", name)
    before = gs(key)
    Atspi.Value.set_current_value(sp, float(probe))
    ms = wait(lambda: gs(key) == str(probe), 3)
    check(f"number '{name}' writes {key}", ms is not None, f"{before} -> {gs(key)}", ms)
    Atspi.Value.set_current_value(sp, float(hi + 1000))
    wait(lambda: gs(key) == str(hi), 2)
    check(f"number '{name}' clamps at max {hi}", gs(key) == str(hi), f"got {gs(key)}")
    Atspi.Value.set_current_value(sp, float(lo - 1000))
    wait(lambda: gs(key) == str(lo), 2)
    check(f"number '{name}' clamps at min {lo}", gs(key) == str(lo), f"got {gs(key)}")
    gs_set(key, before)
    ms = wait(lambda: value(sp) == float(before), 3)
    check(f"number '{name}' follows external change", ms is not None, f"widget={value(sp)}", ms)


def combo_test(ui, combo, key, choices, restore=True):
    before = gs(key)
    for i, (val, label) in enumerate(choices):
        t0 = time.monotonic()
        ui.choose(combo, i)
        ok = wait(lambda: gs(key) == f"'{val}'", 3) is not None
        shown = ui.combo_label(combo)
        check(f"dropdown '{combo}' -> {label}", ok and shown == label,
              f"{key}={gs(key)}, shows {shown!r}", (time.monotonic() - t0) * 1000)
    if restore:
        gs_set(key, before)
        want = dict(choices).get(before.strip("'"))
        ms = wait(lambda: ui.combo_label(combo) == want, 3)
        check(f"dropdown '{combo}' follows external change", ms is not None,
              f"shows {ui.combo_label(combo)!r}", ms)


# ================================================================= tests
def t_general(ui, dm):
    check("tab General opens", True, "", ui.tab("General"))
    m = dm.mark()
    sw = [s for s in find(ui.app, "switch", "Live captions") if "toggle" in actions(s)][0]
    with Responsiveness(ui) as r:
        do(sw, "toggle")
        ms = wait(lambda: gs("enabled") == "true", 3)
        check("Live captions on writes enabled=true", ms is not None, "", ms)
        ms = wait(lambda: dm.since(m, "StatusChanged"), 30)
        st = dm.status()
        check("daemon reacts to Live captions on", ms is not None, f"status={st.get('state')} {st.get('message', '')[:60]}", ms)
        ms = wait(lambda: dm.status().get("state") == "listening", 90)
        st = dm.status()
        check("captions start listening", ms is not None,
              f"state={st.get('state')} live={st.get('live')} refine={st.get('refine')}", ms)
    r.check("caption start")
    ms = wait(lambda: any("istening" in l for l in labels_in_row(ui, "Status")), 5)
    check("Status row shows listening", ms is not None, f"{labels_in_row(ui, 'Status')}", ms)
    ui.shot("general-captions-on")
    do(sw, "toggle")
    ms = wait(lambda: dm.status().get("state") == "off", 15)
    check("Live captions off stops the daemon", ms is not None, f"state={dm.status().get('state')}", ms)
    ms = wait(lambda: any(l == "Off" for l in labels_in_row(ui, "Status")), 5)
    check("Status row shows Off", ms is not None, f"{labels_in_row(ui, 'Status')}", ms)

    switch_test(ui, "Top bar icon", "show-indicator")
    switch_test(ui, "Quick Settings toggle", "show-quick-settings")

    for row, what in (("Report a problem", "Report a problem"), ("About Captionist", "About")):
        if ui.focus("list item", row):
            ui.desk.key("Return")
            ms = wait(lambda: ui.dialog_open(), 5)
            ui.shot(f"general-{what.lower().replace(' ', '-')}")
            check(f"'{row}' opens a dialog", ms is not None, f"{ui.dialog_open()}", ms)
            ui.desk.key("Escape")
            ms = wait(lambda: not ui.dialog_open(), 3)
            check(f"'{row}' dialog closes with Escape", ms is not None, "", ms)
        else:
            check(f"'{row}' reachable by keyboard", False, "could not focus row")


def t_race(ui, dm):
    """Turning captions off, or restarting them, while models are still loading."""
    gs_set("enabled", "false")
    wait(lambda: dm.status().get("state") == "off", 10)
    gs_set("enabled", "true")
    wait(lambda: dm.status().get("state") == "loading", 5)
    gs_set("enabled", "false")
    pump(12)
    st = dm.status()
    check("off while loading stays off", st.get("state") == "off", f"state 12 s later: {st.get('state')}")
    gs_set("enabled", "true")
    wait(lambda: dm.status().get("state") == "loading", 5)
    gs_set("source", "'mic'")
    ms = wait(lambda: dm.status().get("state") == "listening", 90)
    check("restart while loading ends up listening", ms is not None, f"state={dm.status().get('state')}", ms)
    gs_set("enabled", "false")
    gs_reset("source")
    wait(lambda: dm.status().get("state") == "off", 10)


def t_audio(ui, dm):
    check("tab Audio opens", True, "", ui.tab("Audio"))
    choices = [("speakers", "Speakers / headphones"), ("mic", "Microphone"),
               ("both", "Speakers + microphone"), ("app", "One app only")]
    combo_test(ui, "Source", "source", choices, restore=False)
    ms = wait(lambda: ui.exists("combo box", "App"), 3)
    check("'One app only' shows the App picker", ms is not None, "", ms)
    ui.shot("audio-app-picker")
    ui.choose("Source", 0)
    ms = wait(lambda: not ui.exists("combo box", "App"), 3)
    check("App picker hides for other sources", ms is not None, "", ms)
    spin_test(ui, "Hide captions after silence", "hide-after", 0, 120, 15)


def rep_title(rep):
    rec = rep["recommendation"]
    return " + ".join(c["model_name"] for c in (rec.get("live"), rec.get("refine")) if c)


def accel_title(o):
    return {"intel-gpu": "Intel graphics (OpenVINO)", "intel-npu": "Intel NPU (OpenVINO)"}.get(o.get("kind"), o.get("device"))


def installed(dm, mid):
    return any(x["installed"] for x in dm.models()["rows"] if x["id"] == mid)


def t_models(ui, dm):
    check("tab Models opens", True, "", ui.tab("Models"))
    rep = dm.models()
    ui.shot("models-initial")

    # Mode: Automatic <-> Manual
    combo_test(ui, "Mode", "model-mode", [("auto", "Automatic (recommended)"), ("manual", "Manual")], restore=False)
    missing = [r for r in rep["rows"] if r["recommended"] and not r["installed"]]
    if missing:
        ms = wait(lambda: any(b.get_name().startswith("Download (") for r in find(ui.app, "list item", rep_title(rep))
                              for b in find_all(r, "button")), 5)
        check("Manual mode, recommended models missing: offers 'Download (…)'", ms is not None,
              f"missing: {[r['name'] for r in missing]}", ms)
        gs_set("model-mode", "'auto'")
    else:
        ms = wait(lambda: find_btn(ui, rep_title(rep), "Use recommended"), 5)
        check("Manual mode offers 'Use recommended'", ms is not None, "", ms)
        do(row_button(ui, rep_title(rep), "Use recommended"))
        ms = wait(lambda: gs("model-mode") == "'auto'", 3)
        check("'Use recommended' switches back to Automatic", ms is not None, gs("model-mode"), ms)
    ms = wait(lambda: ui.combo_label("Mode") == "Automatic (recommended)", 3)
    check("Mode dropdown follows the switch back to Automatic", ms is not None, ui.combo_label("Mode"), ms)

    # Sort
    sorts = [("recommended", "Recommended"), ("accuracy", "Most accurate"), ("speed", "Fastest"),
             ("download", "Smallest download"), ("memory", "Least memory")]
    names = {r["name"] for r in rep["rows"]}
    for i, (val, label) in enumerate(sorts):
        with Responsiveness(ui) as r:
            ui.choose("Sort list by", i)
            ms = wait(lambda: gs("model-sort") == f"'{val}'", 3)
            pump(0.6)
        shown = [n for n in names if ui.exists("list item", n)]
        check(f"sort '{label}' applies and keeps all {len(names)} models",
              ms is not None and len(shown) == len(names), f"{len(shown)} shown", ms)
    gs_set("model-sort", "'recommended'")
    pump(0.6)

    # Use another installed model -> manual + daemon switches -> back to Automatic
    gs_set("enabled", "true")
    wait(lambda: dm.status().get("state") == "listening", 90)
    rep = dm.models()
    active = (rep.get("active_live"), rep.get("active_refine"))
    cand = [r for r in rep["rows"] if r["installed"] and r["id"] not in active]
    if cand:
        m = sorted(cand, key=lambda r: r["role"] != "live")[0]
        key, field = ("live-model", "live") if m["role"] == "live" else ("refine-model", "refine")
        do(row_button(ui, m["name"], "Use"))
        pump(0.5)
        dlg = ui.dialog_open()
        if dlg:
            ui.shot("models-use-confirm")
            answer_dialog(ui, "Use anyway")
        check(f"'Use {m['name']}' asks first only when it may lag (fit={m['fit']})",
              bool(dlg) == (m["fit"] not in ("Smooth", "Unknown")), f"dialog={dlg}")
        ms = wait(lambda: gs(key) == f"'{m['id']}'" and gs("model-mode") == "'manual'", 5)
        check(f"'Use' {m['name']} sets Manual + {key}", ms is not None, f"mode={gs('model-mode')} {key}={gs(key)}", ms)
        with Responsiveness(ui) as r:
            ms = wait(lambda: dm.status().get(field) == m["name"] and dm.status().get("state") == "listening", 90)
        check(f"daemon switches to {m['name']}", ms is not None, f"status={dm.status()}", ms)
        r.check("model switch")
        ms = wait(lambda: "In use" in labels_in_row(ui, m["name"]), 10)
        check(f"{m['name']} row shows 'In use'", ms is not None, f"{labels_in_row(ui, m['name'])}", ms)
        ui.shot("models-manual")
        ui.choose("Mode", 0)
        ms = wait(lambda: gs("model-mode") == "'auto'" and dm.status().get(field) != m["name"]
                  and dm.status().get("state") == "listening", 90)
        check("back to Automatic restores the recommended models", ms is not None, f"status={dm.status()}", ms)
    else:
        check("another installed model to switch to", False, "none installed")
    gs_set("enabled", "false")
    wait(lambda: dm.status().get("state") == "off", 15)

    # Re-check
    with Responsiveness(ui) as r:
        do(ui.one("button", "Re-check"))
        ms = wait(lambda: dm.models().get("busy"), 10)
        check("Re-check starts a hardware/speed check", ms is not None, f"busy={dm.models().get('busy')}", ms)
        ui.shot("models-recheck")
        ms = wait(lambda: not dm.models().get("busy"), 300, step=0.2)
    check("Re-check finishes", ms is not None, "", ms)
    r.check("Re-check")

    # Delete a model (the private hard-linked copy), then download it again
    rep = dm.models()
    active = (rep.get("active_live"), rep.get("active_refine"))
    deletable = [r for r in rep["rows"] if r["installed"] and r["id"] not in active
                 and find_btn(ui, r["name"], "Delete this model")]
    if deletable:
        m = sorted(deletable, key=lambda r: r["download_mb"])[0]
        # Keyboard focus survives a rebuild of the rows (a sort change rebuilds them)
        if ui.focus("button", "Delete this model"):
            gs_set("model-sort", "'accuracy'")  # not via the dropdown, which would take focus
            pump(1.5)
            f = [n.get_name() for n in focused(ui.app)]
            check("keyboard focus stays on the same button when the list rebuilds",
                  "Delete this model" in f, f"focused={f}")
            gs_set("model-sort", "'recommended'")
            pump(1.0)
        do(row_button(ui, m["name"], "Delete this model"))
        ms = wait(lambda: ui.dialog_open(), 3)
        ui.shot("models-delete-confirm")
        check(f"deleting {m['name']} asks first", ms is not None, f"{ui.dialog_open()}", ms)
        ui.desk.key("Escape")
        wait(lambda: not ui.dialog_open(), 3)
        pump(0.5)
        check("cancelling keeps the model", installed(dm, m["id"]), "")
        do(row_button(ui, m["name"], "Delete this model"))
        wait(lambda: ui.dialog_open(), 3)
        answer_dialog(ui, "Delete")
        ms = wait(lambda: find_btn(ui, m["name"], "Download"), 10)
        check(f"delete {m['name']} -> row offers Download", ms is not None, "", ms)
        check(f"{m['name']} removed on disk", not installed(dm, m["id"]))
        progress = set()
        with Responsiveness(ui) as r:
            do(row_button(ui, m["name"], "Download"))
            ms = wait(lambda: progress.update(l for l in labels_in_row(ui, m["name"]) if l.startswith("Downloading"))
                      or installed(dm, m["id"]), 900, step=0.1)
        check(f"download {m['name']} shows progress in its row", bool(progress), f"{sorted(progress)[:5]}")
        check(f"download {m['name']} completes", ms is not None, "", ms)
        r.check("download")
        ms = wait(lambda: find_btn(ui, m["name"], "Use"), 10)
        check(f"{m['name']} row offers Use after download", ms is not None, "", ms)
    else:
        check("a deletable model exists", False, "none")

    # Accelerator on/off (the service restarts itself to load the runtime)
    accel = [o for o in rep.get("accel", []) if o.get("pack_installed") and o.get("driver_ok")]
    if accel and find_btn(ui, accel_title(accel[0]), "Turn on"):
        title = accel_title(accel[0])
        with Responsiveness(ui) as r:
            do(row_button(ui, title, "Turn on"))
            ms = wait(lambda: gs("accelerator") != "'cpu'", 3)
            check(f"'Turn on' {title} sets accelerator", ms is not None, gs("accelerator"), ms)
            ms = wait(lambda: find_btn(ui, title, "Turn off"), 120, step=0.2)
        ui.shot("models-accel-on")
        check(f"{title} shows 'In use' after the service restarts", ms is not None,
              f"labels={labels_in_row(ui, title)}", ms)
        r.check("accelerator switch")
        if ms is not None:
            do(row_button(ui, title, "Turn off"))
        else:
            gs_set("accelerator", "'cpu'")
        ms = wait(lambda: gs("accelerator") == "'cpu'" and find_btn(ui, title, "Turn on"), 120, step=0.2)
        check(f"'Turn off' {title} returns to the processor", ms is not None, "", ms)
    else:
        log("SKIP  accelerator on/off: no installed accelerator runtime")


def t_appearance(ui, dm):
    check("tab Appearance opens", True, "", ui.tab("Appearance"))
    combo_test(ui, "Start position", "position", [("bottom", "Bottom of screen"), ("top", "Top of screen")])
    spin_test(ui, "Window width", "width-percent", 25, 100, 70)
    spin_test(ui, "Font size", "font-size", 12, 72, 30)
    # The caption window's own text-size slider must not pull it back into its range
    for size in (60, 12):
        Atspi.Value.set_current_value(ui.one("spin button", "Font size"), float(size))
        wait(lambda: gs("font-size") == str(size), 2)
        pump(1.0)
        check(f"font size {size} sticks (caption window doesn't override it)", gs("font-size") == str(size),
              f"font-size={gs('font-size')}")
    gs_reset("font-size")
    spin_test(ui, "Lines", "lines", 1, 6, 3)
    spin_test(ui, "Background opacity", "bg-opacity", 0, 100, 40)
    gs_set("floating-x", "300")
    gs_set("floating-y", "400")
    if ui.focus("list item", "Reset position"):
        ui.desk.key("Return")
        ms = wait(lambda: gs("floating-x") == "-1" and gs("floating-y") == "-1", 3)
        check("Reset position clears the dragged position", ms is not None,
              f"x={gs('floating-x')} y={gs('floating-y')}", ms)
    else:
        check("Reset position reachable by keyboard", False, "")
    switch_test(ui, "Dim text until it's corrected", "dim-drafts")

    presets = [("White on black", "'#ffffff'", "'#000000'", "75"), ("Yellow on black", "'#ffd60a'", "'#000000'", "85"),
               ("High contrast", "'#ffffff'", "'#000000'", "100"), ("Light", "'#1c1c1c'", "'#f6f5f4'", "90")]
    for i, (label, fg, bg, op) in enumerate(presets, start=1):
        ui.choose("Style preset", i)
        ms = wait(lambda: (gs("text-color"), gs("bg-color"), gs("bg-opacity")) == (fg, bg, op), 3)
        check(f"style preset '{label}' applies colours", ms is not None,
              f"{gs('text-color')} on {gs('bg-color')} @ {gs('bg-opacity')}%", ms)
    # 'Light' is selected; tweak a colour by hand, then choose 'Light' again
    gs_set("bg-opacity", "30")
    pump(0.3)
    shown = ui.combo_label("Style preset")
    check("preset dropdown stops naming a preset after a manual change", shown != "Light",
          f"shows {shown!r} with opacity 30")
    ui.choose("Style preset", 4)
    ms = wait(lambda: gs("bg-opacity") == "90", 2)
    check("choosing the same preset again re-applies it", ms is not None, f"opacity {gs('bg-opacity')}", ms)
    ui.shot("appearance")
    for k in ("text-color", "bg-color", "bg-opacity"):
        gs_reset(k)

    before = gs("text-color")
    do(ui.one("button", "Text colour"))
    ms = wait(lambda: ui.dialog_open(), 5)
    ui.shot("appearance-colour-dialog")
    check("Text colour opens a colour chooser", ms is not None, f"{ui.dialog_open()}", ms)
    ui.desk.key("Escape")
    wait(lambda: not ui.dialog_open(), 3)
    check("cancelling the colour chooser keeps the colour", gs("text-color") == before, gs("text-color"))


def t_accuracy(ui, dm):
    check("tab Accuracy opens", True, "", ui.tab("Accuracy"))
    switch_test(ui, "Correct each sentence", "refine-enabled")
    switch_test(ui, "Mask swear words", "profanity-filter")

    gs_reset("vocabulary")
    if ui.focus("text", "Add a word or name"):
        ui.desk.type("Kubernetes")
        ui.desk.key("Return")
        ms = wait(lambda: "Kubernetes" in gs("vocabulary"), 3)
        check("vocabulary: type a word + Enter adds it", ms is not None, gs("vocabulary"), ms)
        ui.desk.type("kubernetes")
        ui.desk.key("Return")
        pump(0.3)
        check("vocabulary: same word in other case is ignored", gs("vocabulary") == "['Kubernetes']", gs("vocabulary"))
        check("vocabulary: entry clears after adding", (text(ui.one("text", "Add a word or name")) or "") == "", "")
        ms = wait(lambda: find_btn(ui, "Kubernetes", "Remove “Kubernetes”"), 3)
        check("vocabulary: row appears with a named Remove button", ms is not None, "", ms)
        if ms is not None:
            do(row_button(ui, "Kubernetes", "Remove “Kubernetes”"))
            ms = wait(lambda: gs("vocabulary") in ("@as []", "[]"), 3)
            check("vocabulary: Remove deletes it", ms is not None, gs("vocabulary"), ms)
    else:
        check("vocabulary entry reachable by keyboard", False, "")

    gs_reset("replacements")
    named = ui.exists("text", "Heard") and ui.exists("text", "Write instead")
    check("replacement fields have accessible names", named, "")
    if named and ui.focus("text", "Heard"):
        ui.desk.type("pipe wire")
        ui.focus("text", "Write instead")
        ui.desk.type("PipeWire")
        ui.desk.key("Return")
        ms = wait(lambda: "PipeWire" in gs("replacements"), 3)
        check("replacement: Heard + Write instead + Enter adds it", ms is not None, gs("replacements"), ms)
        check("replacement: focus returns to 'Heard'", any(n.get_name() == "Heard" for n in focused(ui.app)), "")
        row, b = "pipe wire  →  PipeWire", "Remove “pipe wire” → “PipeWire”"
        ms = wait(lambda: find_btn(ui, row, b), 3)
        check("replacement row appears with a named Remove button", ms is not None, "", ms)
        if ms is not None:
            do(row_button(ui, row, b))
            ms = wait(lambda: gs("replacements") in ("@a{ss} {}", "{}"), 3)
            check("replacement: Remove deletes it", ms is not None, gs("replacements"), ms)
    ui.shot("accuracy")


def t_history(ui, dm):
    check("tab History opens", True, "", ui.tab("History"))
    combo_test(ui, "History", "history-mode",
               [("off", "Off"), ("session", "This session only"), ("keep", "Keep on this computer")], restore=False)
    keep = ui.one("list item", "Keep for")
    check("'Keep for' enabled when history is kept", keep.get_state_set().contains(Atspi.StateType.SENSITIVE), "")
    spin_test(ui, "Keep for", "history-days", 1, 365, 7)
    ui.choose("History", 1)
    wait(lambda: gs("history-mode") == "'session'", 3)
    ms = wait(lambda: not keep.get_state_set().contains(Atspi.StateType.SENSITIVE), 3)
    check("'Keep for' greys out when history isn't kept", ms is not None, "", ms)
    sp = ui.one("spin button", "Keep for")
    check("greyed-out 'Keep for' number also reports disabled",
          not sp.get_state_set().contains(Atspi.StateType.SENSITIVE), "")
    before = gs("history-days")
    try:
        Atspi.Value.set_current_value(sp, 99.0)
    except Exception:
        pass
    pump(0.3)
    check("greyed-out 'Keep for' can't be changed", gs("history-days") == before, f"{before} -> {gs('history-days')}")
    gs_set("history-days", before)

    do(ui.one("button", "Export…"))
    ms = wait(lambda: ui.dialog_open(anywhere=True), 8)
    ui.shot("history-export")
    check("Export… opens a file chooser", ms is not None, f"{ui.dialog_open(anywhere=True)}", ms)
    ui.desk.key("Escape")
    ms = wait(lambda: not ui.dialog_open(anywhere=True), 5)
    check("file chooser closes with Escape", ms is not None, "", ms)

    m = dm.mark()
    do(ui.one("button", "Clear history"))
    pump(0.5)
    if ui.dialog_open():
        ui.shot("history-clear-confirm")
        check("Clear history asks for confirmation", True, f"{ui.dialog_open()}")
        ui.desk.key("Escape")
        pump(0.3)
    else:
        ms = wait(lambda: dm.since(m, "HistoryChanged"), 3)
        check("Clear history reaches the daemon", ms is not None, "no confirmation dialog", ms)
    gs_reset("history-mode")


def t_tabs(ui, dm):
    """Time from clicking a tab until the screen changes (measured on the
    screen cast, not via AT-SPI, whose state updates lag behind)."""
    from frames import FrameWatch
    fw = FrameWatch(ui.desk)
    first, settled = [], []
    for _ in range(2):
        for t in ["General", "Audio", "Models", "Appearance", "Accuracy", "History"]:
            tab = ui.one("page tab", t)
            f, s = fw.measure(lambda: do(tab))
            first.append(f if f is not None else 9999)
            settled.append(s if s is not None else 9999)
    fw.close()
    first.sort()
    settled.sort()
    med = first[len(first) // 2]
    check("tab switch shows the new page", med < 100,
          f"median {med:.0f} ms to first frame, {settled[len(settled)//2]:.0f} ms until the transition settles")


TESTS = [t_general, t_race, t_audio, t_models, t_appearance, t_accuracy, t_history, t_tabs]


def main():
    only = sys.argv[1:]
    desk = Desk()
    ui = Ui(desk)
    dm = Daemon()
    for b in find(ui.app, "button", "Don't send"):  # first-run consent dialog
        do(b)
        pump(0.5)
    # Known starting point: captions off, processor only (the service restarts
    # itself when the accelerator changes).
    gs_set("enabled", "false")
    if gs("accelerator") != "'cpu'":
        gs_set("accelerator", "'cpu'")
        wait(lambda: not any(o.get("active") for o in dm.models().get("accel", [])), 60, step=0.5)
    for t in TESTS:
        if only and t.__name__ not in only:
            continue
        log(f"\n== {t.__name__}")
        try:
            t(ui, dm)
        except Exception as e:
            check(f"{t.__name__} crashed", False, f"{e!r}\n{traceback.format_exc()}")
            ui.shot(f"crash-{t.__name__}")
            for _ in range(3):
                desk.key("Escape")
    desk.close()
    fails = [r for r in RESULTS if not r["ok"]]
    log(f"\n{len(RESULTS) - len(fails)} passed, {len(fails)} failed")
    with open(os.path.join(SHOTS, "results.json"), "w") as f:
        json.dump(RESULTS, f, indent=1)
    sys.exit(1 if fails else 0)


if __name__ == "__main__":
    main()
