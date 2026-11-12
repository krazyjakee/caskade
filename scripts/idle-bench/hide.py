#!/usr/bin/env python3
"""Closes the app's main window as its title bar's close does, so the app
hides to the tray (scripts/idle-bench/README.md, --hide). Waits for a
top-level window named like Caskade on $DISPLAY (the benchmark's own Xvfb),
then sends it WM_DELETE_WINDOW. Exits 1 if no such window shows up in time.
"""
import sys, time
from Xlib import X, display, protocol

def windows(d):
    out = []
    def walk(w):
        try:
            name = w.get_wm_name() or ""
            if isinstance(name, bytes):
                name = name.decode("utf-8", "replace")
            protocols = w.get_wm_protocols() or []
            attrs = w.get_attributes()
        except Exception:
            return
        if "Caskade" in name and attrs.map_state == X.IsViewable:
            out.append((w, protocols))
        for child in w.query_tree().children:
            walk(child)
    walk(d.screen().root)
    return out

def main():
    timeout = float(sys.argv[1]) if len(sys.argv) > 1 else 120
    d = display.Display()
    delete = d.intern_atom("WM_DELETE_WINDOW")
    wm_protocols = d.intern_atom("WM_PROTOCOLS")
    deadline = time.time() + timeout
    while time.time() < deadline:
        found = [(w, p) for w, p in windows(d) if delete in p]
        if found:
            for w, _ in found:
                event = protocol.event.ClientMessage(
                    window=w, client_type=wm_protocols, data=(32, [delete, X.CurrentTime, 0, 0, 0]))
                w.send_event(event)
            d.flush()
            print(f"closed {len(found)} window(s)")
            return 0
        time.sleep(1)
    print("no Caskade window to close", file=sys.stderr)
    return 1

sys.exit(main())
