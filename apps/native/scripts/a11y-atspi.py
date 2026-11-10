#!/usr/bin/env python3
"""What a Linux screen reader gets from the native UI (#83): opens the
accessibility demo (`examples/accessibility_demo.rs`) on this display, with
an AT-SPI bus and Orca of its own, then reads the app's AT-SPI tree, acts on
it as a screen reader does (focus, activate, step a slider, a live region
update) and prints what Orca said.

Run it only inside the desktop isolation wrapper, never on a real session:

  cargo build -p caskade-native --example accessibility_demo
  env -u DISPLAY -u WAYLAND_DISPLAY -u DBUS_SESSION_BUS_ADDRESS dbus-run-session -- \\
    xvfb-run -a -s '-screen 0 1280x800x24' python3 -I apps/native/scripts/a11y-atspi.py \\
    <target dir>/debug/examples/accessibility_demo [--out DIR]

Needs at-spi2-core, python3-gi with Atspi 2.0, and Orca (for its part).
"""
import argparse
import os
import shutil
import subprocess
import sys
import tempfile
import time

display = os.environ.get("DISPLAY", "")
bus = os.environ.get("DBUS_SESSION_BUS_ADDRESS", "")
if not display or display.split(".")[0] in (":0", ":1") or "/run/user/" in bus or not bus:
    sys.exit(f"refusing to run outside the isolation wrapper (DISPLAY={display!r}, bus={bus!r})")

ap = argparse.ArgumentParser()
ap.add_argument("demo")
ap.add_argument("--out", help="keep Orca's debug log here")
args = ap.parse_args()

# Everything AT-SPI and Orca keep (sockets, settings) goes in a home of its
# own, never the user's runtime dir.
home = tempfile.mkdtemp(prefix="caskade-a11y-")
runtime = os.path.join(home, "run")
os.makedirs(runtime, mode=0o700)
os.environ.update(
    HOME=home,
    XDG_RUNTIME_DIR=runtime,
    XDG_CONFIG_HOME=f"{home}/config",
    XDG_DATA_HOME=f"{home}/data",
    XDG_CACHE_HOME=f"{home}/cache",
    XDG_STATE_HOME=f"{home}/state",
    GTK_A11Y="none",  # Orca's own GTK windows stay off the tree
)
for name in ("AT_SPI_BUS_ADDRESS", "NO_AT_BRIDGE", "WAYLAND_DISPLAY"):
    os.environ.pop(name, None)

# Orca speaks through speech-dispatcher; with no sound server here its
# audio output blocks Orca, so it speaks into libao's null driver.
os.makedirs(f"{home}/config/speech-dispatcher")
with open("/etc/speech-dispatcher/speechd.conf") as system_conf:
    conf = [line for line in system_conf if not line.lstrip().startswith("AudioOutputMethod")]
with open(f"{home}/config/speech-dispatcher/speechd.conf", "w") as fh:
    fh.writelines(conf + ['AudioOutputMethod "libao"\n'])
if os.path.isdir("/etc/speech-dispatcher/modules"):
    os.symlink("/etc/speech-dispatcher/modules", f"{home}/config/speech-dispatcher/modules")
with open(f"{home}/.libao", "w") as fh:
    fh.write("default_driver=null\n")

children = []
app = None
orca = None


def start(cmd, log=None, **kw):
    out = open(log, "w") if log else subprocess.DEVNULL
    proc = subprocess.Popen(cmd, stdout=out, stderr=subprocess.STDOUT, **kw)
    children.append(proc)
    return proc


def gdbus_set(prop, value):
    subprocess.run(
        ["gdbus", "call", "--session", "--dest", "org.a11y.Bus", "--object-path", "/org/a11y/bus",
         "--method", "org.freedesktop.DBus.Properties.Set", "org.a11y.Status", prop, f"<{value}>"],
        check=True, stdout=subprocess.DEVNULL,
    )


ok = True


def check(what, passed, detail=""):
    global ok
    ok &= bool(passed)
    print(f"{'ok  ' if passed else 'FAIL'} {what}" + (f": {detail}" if detail else ""))


try:
    start(["/usr/libexec/at-spi-bus-launcher", "--launch-immediately"])
    for _ in range(50):
        if subprocess.run(["gdbus", "call", "--session", "--dest", "org.a11y.Bus", "--object-path",
                           "/org/a11y/bus", "--method", "org.a11y.Bus.GetAddress"],
                          capture_output=True).returncode == 0:
            break
        time.sleep(0.1)
    gdbus_set("IsEnabled", "true")
    gdbus_set("ScreenReaderEnabled", "true")

    import gi

    gi.require_version("Atspi", "2.0")
    from gi.repository import Atspi, GLib

    Atspi.init()
    orca_log = os.path.join(home, "orca.log")
    if shutil.which("orca"):
        orca = start(["orca", "--replace", "-u", f"{home}/orca", "--debug-file", orca_log])
        time.sleep(3)
    demo = start([args.demo], log=os.path.join(home, "demo.log"), env={**os.environ, "RUST_LOG": os.environ.get("RUST_LOG", "warn")})

    def pump(seconds):
        end = time.monotonic() + seconds
        context = GLib.MainContext.default()
        while time.monotonic() < end:
            while context.pending():
                context.iteration(False)
            time.sleep(0.02)

    def find_app():
        desktop = Atspi.get_desktop(0)
        for i in range(desktop.get_child_count()):
            app = desktop.get_child_at_index(i)
            if app is not None and app.get_process_id() == demo.pid:
                return app
        return None

    app = None
    for _ in range(600):
        pump(0.1)
        app = find_app()
        if app is not None and app.get_child_count() > 0:
            break
    check("the demo is on the AT-SPI bus", app is not None)
    if app is None:
        desktop = Atspi.get_desktop(0)
        for i in range(desktop.get_child_count()):
            other = desktop.get_child_at_index(i)
            print("  on the bus:", other.get_name(), other.get_process_id())
        print("demo running:", demo.poll() is None)
        print(open(os.path.join(home, "demo.log"), errors="replace").read()[-3000:])
        raise SystemExit(1)
    # No window manager here: give the demo's window the input focus (on
    # this private display), as a desktop does, or AccessKit reports no
    # focus and Orca ignores the window.
    windows = subprocess.run(["xdotool", "search", "--pid", str(demo.pid)],
                             capture_output=True, text=True).stdout.split()
    for window in windows:
        subprocess.run(["xdotool", "windowfocus", "--sync", window],
                       stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, timeout=10)
    pump(1)

    def walk(node, depth=0, out=None):
        out = [] if out is None else out
        out.append((depth, node))
        for i in range(node.get_child_count()):
            child = node.get_child_at_index(i)
            if child is not None:
                walk(child, depth + 1, out)
        return out

    def states(node):
        return {s.value_nick for s in node.get_state_set().get_states()}

    def describe(node):
        parts = [node.get_role_name(), repr(node.get_name() or "")]
        value = node.get_value_iface()
        if value is not None:
            parts.append(f"value={value.get_current_value():g}")
        interesting = states(node) & {"focusable", "focused", "checked", "pressed", "expanded",
                                       "collapsed", "selected", "modal", "required", "invalid-entry",
                                       "busy", "disabled", "expandable", "has-popup"}
        if interesting:
            parts.append(",".join(sorted(interesting)))
        action = node.get_action_iface()
        if action is not None and action.get_n_actions():
            parts.append("[" + " ".join(action.get_name(i) for i in range(action.get_n_actions())) + "]")
        return " ".join(parts)

    def by_name(role, name):
        for _, node in walk(app):
            if node.get_role_name() == role and (node.get_name() or "") == name:
                return node
        return None

    tree = walk(app)
    print("\nAT-SPI tree:")
    for depth, node in tree:
        if node.get_role_name() in ("text", "static") and not node.get_name():
            continue
        print("  " * depth + describe(node))
    print()

    expected = [
        ("toggle button", "Favourite"), ("push button", "Sort by name"), ("check box", "Show hidden games"),
        ("slider", "Volume"), ("entry", "Search"), ("page tab", "Overview"), ("dialog", "Uninstall Hollow Knight?"),
        ("link", "All games"), ("progress bar", "Celeste"),
    ]
    for role, name in expected:
        check(f"{role} {name!r}", by_name(role, name) is not None)
    # A live region reads its text; ARIA doesn't name it from its content.
    alert = next((n for _, n in walk(app) if n.get_role_name() == "notification"), None)
    check("the alert's text", alert is not None and any(
        (child.get_name() or "") == "Couldn't reach RomM"
        for child in (alert.get_child_at_index(i) for i in range(alert.get_child_count()))))
    check("the window is named", app.get_child_at_index(0).get_name() == "Caskade accessibility demo",
          repr(app.get_child_at_index(0).get_name()))
    check("the checkbox is checked", "checked" in states(by_name("check box", "Show hidden games")))
    # accesskit_atspi_common maps no AT-SPI state for `invalid` (an AccessKit
    # gap, docs/spikes/blitz-4.md), so only `required` is checked.
    check("the search field is required", "required" in states(by_name("entry", "Search")))
    check("the dialog is modal", "modal" in states(by_name("dialog", "Uninstall Hollow Knight?")))
    slider = by_name("slider", "Volume")
    check("the slider's value", slider.get_value_iface().get_current_value() == 40)
    bounds = by_name("toggle button", "Favourite").get_component_iface().get_extents(Atspi.CoordType.WINDOW)
    check("the button has bounds", bounds.width > 0 and bounds.height > 0,
          f"{bounds.x},{bounds.y} {bounds.width}x{bounds.height}")

    def act(node, name):
        action = node.get_action_iface()
        for i in range(action.get_n_actions()):
            if action.get_name(i) == name:
                action.do_action(i)
                pump(0.5)
                return True
        return False

    favourite = by_name("toggle button", "Favourite")
    favourite.get_component_iface().grab_focus()
    pump(0.5)
    check("grab_focus focuses the button", "focused" in states(favourite))
    check("click activates the button", act(favourite, "click") and "pressed" in states(favourite),
          describe(favourite))
    # AT-SPI sets a value rather than stepping: Blitz steps towards it.
    slider.get_value_iface().set_current_value(100)
    pump(0.5)
    check("setting the slider's value steps it", slider.get_value_iface().get_current_value() == 50)
    uninstall = by_name("push button", "Uninstall")
    for _ in range(3):
        act(uninstall, "click")
    pump(1.5)
    check("the status says the install finished",
          any((n.get_name() or "") == "Celeste is installed" or "Celeste is installed" in (n.get_name() or "")
              for _, n in walk(app)))

    pump(5)  # Orca's speech for the last events
finally:
    # Orca first, while the demo still answers it: it writes the rest of its
    # debug log as it shuts down.
    for proc in [orca] + [p for p in reversed(children) if p is not orca]:
        if proc is None:
            continue
        proc.terminate()
        try:
            proc.wait(20)
        except subprocess.TimeoutExpired:
            print(f"killing {proc.args[0]}: it didn't stop")
            proc.kill()
            proc.wait()
    # Orca's debug log is complete once it has exited.
    orca_log = os.path.join(home, "orca.log")
    if os.path.exists(orca_log) and app is not None:
        spoken = [line.split("SPEECH OUTPUT:", 1)[1].strip()
                  for line in open(orca_log, errors="replace") if "SPEECH OUTPUT:" in line]
        print("\nOrca said:")
        for line in spoken:
            print("  " + line.split(" {")[0])
        said = " ".join(spoken)
        check("Orca announced the focused button", "Favourite" in said)
        check("Orca announced the live region", "Celeste is installed" in said)
        if args.out:
            os.makedirs(args.out, exist_ok=True)
            shutil.copy(orca_log, os.path.join(args.out, "orca.log"))
    shutil.rmtree(home, ignore_errors=True)

sys.exit(0 if ok else 1)
