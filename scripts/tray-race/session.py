"""A stand-in desktop session's tray, for checks that suites leave a
desktop alone (#122): Cinnamon's real StatusNotifierWatcher and an applet's
monitor on this process's (private) bus, until killed.

    python3 -I scripts/tray-race/session.py   (inside scripts/private-desktop.sh)
"""

import os
import signal
import subprocess
import sys

import gi

gi.require_version("XApp", "1.0")
from gi.repository import GLib, XApp  # noqa: E402

if not os.environ.get("CASKADE_PRIVATE_DESKTOP"):
    sys.exit("session.py: run it inside scripts/private-desktop.sh")
monitor = XApp.StatusIconMonitor()
watcher = subprocess.Popen(["/usr/lib/x86_64-linux-gnu/xapps/xapp-sn-watcher"],
                           env={**os.environ, "XDG_CURRENT_DESKTOP": "X-Cinnamon"},
                           stdout=subprocess.DEVNULL, stderr=subprocess.DEVNULL)
loop = GLib.MainLoop()
GLib.unix_signal_add(GLib.PRIORITY_DEFAULT, signal.SIGTERM, loop.quit)
loop.run()
watcher.terminate()
watcher.wait()
