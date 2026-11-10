"""A tray icon that leaves while the watcher still asks for its properties
must not crash the watcher (#122, #88). Starts Caskade (the native app or the
Tauri app) N times against the real xapp-sn-watcher (Cinnamon's) on a private
bus, and quits it through its tray menu's Quit the moment its icon appears,
which is when the watcher's GetAll is in flight. Passes when every start
quit within the time limit, the watcher is still running and it never logged
NoReply (its use-after-free's tell, memory notes of 4 Nov 2026).

Run only on a private desktop: scripts/tray-race/run.sh starts one.

    race.py --app <binary> --home <folder> --log <watcher log> [--runs 20]
"""

import argparse
import os
import subprocess
import sys
import time

import gi

gi.require_version("XApp", "1.0")
from gi.repository import Gio, GLib, XApp  # noqa: E402

WATCHER = "/usr/lib/x86_64-linux-gnu/xapps/xapp-sn-watcher"
SNI = "org.kde.StatusNotifierItem"
MENU = "com.canonical.dbusmenu"


def bus():
    return Gio.bus_get_sync(Gio.BusType.SESSION, None)


def call(conn, name, path, iface, method, args, reply):
    return conn.call_sync(
        name, path, iface, method, args, GLib.VariantType(reply) if reply else None,
        Gio.DBusCallFlags.NONE, 5000, None)


def registered(conn):
    try:
        value = call(conn, "org.kde.StatusNotifierWatcher", "/StatusNotifierWatcher",
                     "org.freedesktop.DBus.Properties", "Get",
                     GLib.Variant("(ss)", ("org.kde.StatusNotifierWatcher",
                                           "RegisteredStatusNotifierItems")), "(v)")
        return list(value.unpack()[0])
    except GLib.Error:
        return []


def quit_item(conn, name, menu_path):
    """The id of the menu item labelled Quit…, from the menu's layout."""
    layout = call(conn, name, menu_path, MENU, "GetLayout",
                  GLib.Variant("(iias)", (0, -1, ["label"])), "(u(ia{sv}av))").unpack()
    stack = [layout[1]]
    while stack:
        item_id, props, children = stack.pop()
        if "Quit" in str(props.get("label", "")):
            return item_id
        stack.extend(children)
    return None


def quit_via_menu(conn, item):
    """Clicks Quit in the menu of `item` ("<bus name>/<path>" or a name)."""
    name, _, path = item.partition("/")
    path = "/" + path if path else "/StatusNotifierItem"
    menu = call(conn, name, path, "org.freedesktop.DBus.Properties", "Get",
                GLib.Variant("(ss)", (SNI, "Menu")), "(v)").unpack()[0]
    target = quit_item(conn, name, menu)
    if target is None:
        raise RuntimeError(f"no Quit in {item}'s menu")
    call(conn, name, menu, MENU, "Event",
         GLib.Variant("(isvu)", (target, "clicked", GLib.Variant("s", ""), 0)), None)


def main():
    parser = argparse.ArgumentParser()
    parser.add_argument("--app", required=True)
    parser.add_argument("--home", required=True)
    parser.add_argument("--log", required=True)
    parser.add_argument("--runs", type=int, default=20)
    parser.add_argument("--limit", type=float, default=1.0,
                        help="seconds from start the quit should come by; it comes the moment "
                             "the icon appears, so a later icon is reported, not failed")
    parser.add_argument("--kill", action="store_true",
                        help="SIGKILL the app (its PID) the moment its icon appears instead of "
                             "quitting: a crash mustn't take the watcher down either")
    args = parser.parse_args()
    if not os.environ.get("CASKADE_PRIVATE_DESKTOP"):
        sys.exit("race.py: run it through scripts/tray-race/run.sh (a private desktop)")

    # An applet's monitor first: the watcher serves icons to monitors, and
    # with one it proxies every icon (GetAll).
    monitor = XApp.StatusIconMonitor()
    seen = []
    monitor.connect("icon-added", lambda _m, icon: seen.append(icon.props.name))
    context = GLib.MainContext.default()
    log = open(args.log, "w")
    watcher = subprocess.Popen([WATCHER], stdout=log, stderr=subprocess.STDOUT,
                               env={**os.environ, "G_MESSAGES_DEBUG": "all",
                                    "XDG_CURRENT_DESKTOP": "X-Cinnamon"})
    conn = bus()
    # The watcher answers once it owns its name.
    for _ in range(200):
        while context.pending():
            context.iteration(False)
        if conn.call_sync("org.freedesktop.DBus", "/org/freedesktop/DBus", "org.freedesktop.DBus",
                          "NameHasOwner", GLib.Variant("(s)", ("org.kde.StatusNotifierWatcher",)),
                          GLib.VariantType("(b)"), Gio.DBusCallFlags.NONE, 1000, None).unpack()[0]:
            break
        time.sleep(0.05)
    else:
        sys.exit("race.py: the watcher never took its name")

    env = {**os.environ, "HOME": args.home, "XDG_DATA_HOME": f"{args.home}/data",
           "XDG_CONFIG_HOME": f"{args.home}/config", "XDG_CACHE_HOME": f"{args.home}/cache",
           "XDG_STATE_HOME": f"{args.home}/state", "RUST_LOG": "info"}
    results = []
    for run in range(args.runs):
        before = set(registered(conn))
        started = time.monotonic()
        out = open(os.path.join(os.path.dirname(args.log), f"app-{run}.log"), "w")
        app = subprocess.Popen([args.app], stdout=out, stderr=subprocess.STDOUT, env=env,
                               stdin=subprocess.DEVNULL)
        item = None
        while time.monotonic() - started < 15 and app.poll() is None:
            while context.pending():
                context.iteration(False)
            new = [entry for entry in registered(conn) if entry not in before]
            if new:
                item = new[0]
                break
            time.sleep(0.005)
        if item is None:
            app.kill()
            app.wait()
            results.append({"run": run, "error": "no icon"})
            continue
        asked = time.monotonic() - started
        if args.kill:
            app.kill()
            app.wait()
            for _ in range(40):
                while context.pending():
                    context.iteration(False)
                time.sleep(0.025)
            results.append({"run": run, "icon_s": round(asked, 3), "killed": True})
            print(f"run {run}: icon after {asked:.3f} s, killed", flush=True)
            continue
        try:
            quit_via_menu(conn, item)
        except Exception as error:  # noqa: BLE001 - reported per run
            app.kill()
            app.wait()
            results.append({"run": run, "error": f"quit: {error}"})
            continue
        try:
            code = app.wait(timeout=15)
        except subprocess.TimeoutExpired:
            app.kill()
            code = app.wait()
            results.append({"run": run, "error": "didn't quit"})
            continue
        ended = time.monotonic() - started
        for _ in range(40):  # the watcher's last word on this icon
            while context.pending():
                context.iteration(False)
            time.sleep(0.025)
        results.append({"run": run, "icon_s": round(asked, 3), "exit_s": round(ended, 3),
                        "code": code})
        print(f"run {run}: icon after {asked:.3f} s, quit asked then, exited after {ended:.3f} s"
              f" ({code})", flush=True)

    alive = watcher.poll() is None
    watcher.terminate()
    watcher.wait()
    log.close()
    text = open(args.log).read()
    no_reply = text.count("NoReply")
    errors = [r for r in results if "error" in r]
    slow = [r for r in results if "icon_s" in r and r["icon_s"] > args.limit]
    print(f"runs {len(results)}, errors {len(errors)}, quit asked later than {args.limit} s:"
          f" {len(slow)}, watcher alive {alive}, NoReply {no_reply}, icons seen {len(seen)}")
    for r in errors:
        print(f"  run {r['run']}: {r['error']}")
    if args.kill:
        # A killed app can't answer anything: the watcher must survive it.
        sys.exit(0 if alive and not errors else 1)
    sys.exit(0 if alive and no_reply == 0 and not errors else 1)


if __name__ == "__main__":
    main()
