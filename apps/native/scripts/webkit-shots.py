"""WebKitGTK's rendering of the pages `caskade-native --shots` writes (#90):
each `<screen>-<theme>.html` (the same markup, attributes, stylesheets,
fonts and theme assets Blitz rendered) at the same size, saved beside it as
`<screen>-<theme>.webkit.png`. Run it on a private desktop:

    scripts/private-desktop.sh --size 1280x800 -- \
        python3 -I apps/native/scripts/webkit-shots.py <shots folder>
"""

import os
import sys

import gi

gi.require_version("Gtk", "3.0")
gi.require_version("WebKit2", "4.1")
from gi.repository import GLib, Gtk, WebKit2  # noqa: E402

WIDTH, HEIGHT = 1280, 800
# Time for web fonts and images after the load finishes.
SETTLE_MS = 1500


def capture(page, out):
    window = Gtk.OffscreenWindow()
    view = WebKit2.WebView()
    settings = view.get_settings()
    settings.set_allow_file_access_from_file_urls(True)
    settings.set_enable_javascript(False)
    view.set_size_request(WIDTH, HEIGHT)
    window.add(view)
    window.show_all()
    loop = GLib.MainLoop()
    result = {}

    def snapshot():
        def done(view, task):
            try:
                surface = view.get_snapshot_finish(task)
                surface.write_to_png(out)
                result["ok"] = True
            except GLib.Error as error:
                result["error"] = error.message
            loop.quit()

        view.get_snapshot(WebKit2.SnapshotRegion.VISIBLE, WebKit2.SnapshotOptions.NONE, None, done)
        return False

    def changed(view, event):
        if event == WebKit2.LoadEvent.FINISHED:
            GLib.timeout_add(SETTLE_MS, snapshot)

    view.connect("load-changed", changed)
    view.load_uri("file://" + os.path.abspath(page))
    GLib.timeout_add_seconds(60, loop.quit)
    loop.run()
    window.destroy()
    if not result.get("ok"):
        raise RuntimeError(f"{page}: {result.get('error', 'timed out')}")


def main():
    if not os.environ.get("CASKADE_PRIVATE_DESKTOP"):
        sys.exit("webkit-shots.py: run it on a private desktop (scripts/private-desktop.sh)")
    folder = sys.argv[1]
    pages = sorted(name for name in os.listdir(folder) if name.endswith(".html"))
    for name in pages:
        out = os.path.join(folder, name[: -len(".html")] + ".webkit.png")
        capture(os.path.join(folder, name), out)
        print(out, flush=True)


if __name__ == "__main__":
    main()
