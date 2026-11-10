#!/usr/bin/env python3
"""Types into the native UI's text field through a real input method (#84):
IBus with its XIM bridge, on this display and session bus only, then copies
and pastes through this display's clipboard. Opens `examples/text_field_demo.rs`,
which prints what its field hears.

The composition is IBus's own: the simple engine's Unicode entry
(Ctrl+Shift+U, hex digits, Space) shows a preedit and commits a character,
over XIM as an IME for Chinese or Japanese does.

Run it only inside the desktop isolation wrapper, never on a real session:

  cargo build -p caskade-native --example text_field_demo
  env -u DISPLAY -u WAYLAND_DISPLAY -u DBUS_SESSION_BUS_ADDRESS dbus-run-session -- \\
    xvfb-run -a -s '-screen 0 1280x800x24' python3 -I apps/native/scripts/ime-ibus.py \\
    <target dir>/debug/examples/text_field_demo

Needs ibus, xdotool and xclip.
"""
import os
import shutil
import subprocess
import sys
import tempfile
import threading
import time

display = os.environ.get("DISPLAY", "")
bus = os.environ.get("DBUS_SESSION_BUS_ADDRESS", "")
if not display or display.split(".")[0] in (":0", ":1") or "/run/user/" in bus or not bus:
    sys.exit(f"refusing to run outside the isolation wrapper (DISPLAY={display!r}, bus={bus!r})")
if len(sys.argv) != 2:
    sys.exit(__doc__)
demo_binary = sys.argv[1]

# IBus keeps its socket and config under the home: a private one.
home = tempfile.mkdtemp(prefix="caskade-ime-")
runtime = os.path.join(home, "run")
os.makedirs(runtime, mode=0o700)
os.environ.update(
    HOME=home,
    XDG_RUNTIME_DIR=runtime,
    XDG_CONFIG_HOME=f"{home}/config",
    XDG_DATA_HOME=f"{home}/data",
    XDG_CACHE_HOME=f"{home}/cache",
    XDG_STATE_HOME=f"{home}/state",
    XMODIFIERS="@im=ibus",
    GTK_IM_MODULE="ibus",
    NO_AT_BRIDGE="1",
)
for name in ("IBUS_ADDRESS", "WAYLAND_DISPLAY"):
    os.environ.pop(name, None)

def private_bus(home):
    """A session bus of this run's own, started after the environment above:
    the services it activates (a portal, a keyring, gvfs) get this run's
    HOME and runtime dir. dbus-run-session's bus hands them the wrapper's,
    the user's real ones."""
    sock_dir = os.path.join(home, "bus")
    os.makedirs(sock_dir, mode=0o700)
    config = os.path.join(home, "bus.conf")
    with open(config, "w") as fh:
        fh.write(f"""<!DOCTYPE busconfig PUBLIC "-//freedesktop//DTD D-Bus Bus Configuration 1.0//EN"
 "http://www.freedesktop.org/standards/dbus/1.0/busconfig.dtd">
<busconfig>
  <type>session</type>
  <listen>unix:dir={sock_dir}</listen>
  <auth>EXTERNAL</auth>
  <standard_session_servicedirs/>
  <policy context="default">
    <allow send_destination="*" eavesdrop="true"/>
    <allow eavesdrop="true"/>
    <allow own="*"/>
  </policy>
</busconfig>
""")
    proc = subprocess.Popen(["dbus-daemon", f"--config-file={config}", "--nofork", "--print-address=1"],
                            stdout=subprocess.PIPE, text=True)
    address = proc.stdout.readline().strip()
    os.environ["DBUS_SESSION_BUS_ADDRESS"] = address
    return proc


children = []
heard = []
ok = True


def check(what, passed, detail=""):
    global ok
    ok &= bool(passed)
    print(f"{'ok  ' if passed else 'FAIL'} {what}" + (f": {detail}" if detail else ""))


def start(cmd, **kw):
    proc = subprocess.Popen(cmd, **kw)
    children.append(proc)
    return proc


def xdo(*args):
    subprocess.run(["xdotool", *args], check=True, stdout=subprocess.DEVNULL, timeout=30)


def wait_for(what, predicate, seconds=20):
    end = time.monotonic() + seconds
    while time.monotonic() < end:
        if predicate():
            return True
        time.sleep(0.1)
    return False


def lines_since(mark):
    return heard[mark:]


try:
    children.append(private_bus(home))
    start(["ibus-daemon", "--xim", "--panel=disable", "--config=/usr/libexec/ibus-memconf",
           "--replace"], stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
    check("IBus starts on the private bus", wait_for("ibus", lambda: subprocess.run(
        ["ibus", "engine", "xkb:us::eng"], capture_output=True).returncode == 0))
    time.sleep(1)  # ibus-x11 registers its XIM server

    demo = start([demo_binary], stdout=subprocess.PIPE, stderr=subprocess.DEVNULL, text=True)

    def read():
        for line in demo.stdout:
            if line.startswith("field: "):
                heard.append(line[len("field: "):].rstrip("\n"))

    threading.Thread(target=read, daemon=True).start()

    window = None

    def find_window():
        global window
        found = subprocess.run(["xdotool", "search", "--onlyvisible", "--pid", str(demo.pid)],
                               capture_output=True, text=True).stdout.split()
        window = found[-1] if found else None
        return window is not None

    check("the demo opens", wait_for("window", find_window))
    if window is None:
        raise SystemExit(1)
    time.sleep(2)
    xdo("windowfocus", "--sync", window)
    xdo("mousemove", "--window", window, "100", "36", "click", "1")
    time.sleep(0.5)

    # IBus's Unicode entry: U+4F60 你.
    mark = len(heard)
    xdo("key", "--delay", "120", "ctrl+shift+u", "4", "f", "6", "0", "space")
    wait_for("commit", lambda: any(line.startswith("compositionend") for line in lines_since(mark)), 5)
    composition = lines_since(mark)
    check("IBus's preedit reaches the field as a composition",
          composition[:1] == ["compositionstart"]
          and any(line.startswith("compositionupdate") for line in composition), composition)
    check("the commit is the field's input", "input 你" in composition and "compositionend 你" in composition,
          composition)

    mark = len(heard)
    xdo("type", "--delay", "80", "abc")
    wait_for("typing", lambda: "input 你abc" in lines_since(mark), 5)
    check("plain typing still works", "input 你abc" in lines_since(mark), lines_since(mark))

    # Copy and paste through this display's clipboard.
    xdo("key", "ctrl+a", "ctrl+c")
    time.sleep(0.5)
    copied = subprocess.run(["xclip", "-o", "-selection", "clipboard"], capture_output=True,
                            text=True, timeout=10).stdout
    check("Ctrl+C copies the field's text", copied == "你abc", repr(copied))
    owner = start(["xclip", "-i", "-selection", "clipboard", "-loops", "1"], stdin=subprocess.PIPE,
                  stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL, text=True)
    owner.stdin.write("pasted\nline")
    owner.stdin.close()
    time.sleep(0.5)
    mark = len(heard)
    xdo("key", "ctrl+a", "ctrl+v")
    wait_for("paste", lambda: any(line.startswith("input pasted") for line in lines_since(mark)), 5)
    check("Ctrl+V pastes, without the line break", "input pastedline" in lines_since(mark),
          lines_since(mark))
finally:
    for proc in reversed(children):
        proc.terminate()
        try:
            proc.wait(10)
        except subprocess.TimeoutExpired:
            proc.kill()
            proc.wait()
    shutil.rmtree(home, ignore_errors=True)

print("\nThe field heard:")
for line in heard:
    print("  " + line)
sys.exit(0 if ok else 1)
