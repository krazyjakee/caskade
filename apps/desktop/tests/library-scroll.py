#!/usr/bin/env python3
"""Measure the real browser DOM/rAF; requires an isolated CDP browser and dev fixtures.

Start Vite with VITE_CASKADE_FIXTURES=1 VITE_CASKADE_FIXTURE_COUNT=1203.
Start Chromium with a disposable profile and --remote-debugging-port=9223.
python3 apps/desktop/tests/library-scroll.py --url http://localhost:1421
Requires Python websockets; never attaches to a user's normal browser profile.
"""
import argparse
import asyncio
import json
import urllib.request
from pathlib import Path

import websockets


async def run(args):
    with urllib.request.urlopen(f"http://127.0.0.1:{args.port}/json/version", timeout=5) as response:
        version = json.load(response)
    sequence = 0

    async def request(socket, method, params=None):
        nonlocal sequence
        sequence += 1
        current = sequence
        await socket.send(json.dumps({"id": current, "method": method, "params": params or {}}))
        # Unrelated CDP events must not reset the request deadline indefinitely.
        async with asyncio.timeout(45):
            while True:
                reply = json.loads(await socket.recv())
                if reply.get("id") == current:
                    if "error" in reply:
                        raise RuntimeError(reply["error"])
                    return reply["result"]

    results = []
    async with websockets.connect(version["webSocketDebuggerUrl"]) as browser:
        for layout in ["grid", "list"]:
            target = await request(browser, "Target.createTarget", {"url": "about:blank"})
            target_id = target["targetId"]
            try:
                with urllib.request.urlopen(f"http://127.0.0.1:{args.port}/json", timeout=5) as response:
                    targets = json.load(response)
                endpoint = next(t["webSocketDebuggerUrl"] for t in targets if t["id"] == target_id)
                async with websockets.connect(endpoint, max_size=8 * 1024 * 1024) as page:
                    await request(page, "Page.enable")
                    await request(page, "Emulation.setDeviceMetricsOverride", {"width": 1440, "height": 1000, "deviceScaleFactor": 1, "mobile": False})
                    if args.ipc_fixtures:
                        await request(page, "Page.addScriptToEvaluateOnNewDocument", {"source": args.ipc_fixtures.read_text()})
                    await request(page, "Page.navigate", {"url": args.url + f"/library?filter=all&layout={layout}"})
                    await request(page, "Page.bringToFront")
                    for attempt in range(100):
                        ready = await request(page, "Runtime.evaluate", {"returnByValue": True, "expression": "document.readyState === 'complete' && location.pathname === '/library'"})
                        if ready.get("result", {}).get("value") is True:
                            break
                        await asyncio.sleep(0.1)
                    if args.build == "release":
                        development = await request(page, "Runtime.evaluate", {"returnByValue": True, "expression": "[...document.scripts].some(s => s.src.includes('/@vite/client'))"})
                        if development.get("result", {}).get("value") is not False:
                            raise RuntimeError("Release measurement requires a production frontend build")
                    measured = await request(page, "Runtime.evaluate", {"awaitPromise": True, "returnByValue": True, "expression": """
                    (async () => {
                      const sleep = ms => new Promise(resolve => setTimeout(resolve, ms));
                      const deadline = performance.now() + 20000;
                      let viewport;
                      while (!(viewport = document.querySelector('[data-cs="virtual-library"]')) || !viewport.querySelector('[data-cs="game-card"]')) {
                        if (performance.now() > deadline) throw Error('Library did not render');
                        await sleep(20);
                      }
                      await document.fonts.ready;
                      viewport.scrollTop = viewport.scrollHeight;
                      await sleep(1500); // load remaining bounded pages before timed traversal
                      viewport.scrollTop = 0;
                      await sleep(300);
                      const frame = () => new Promise(requestAnimationFrame);
                      const idle = [];
                      let previous = await frame();
                      for (let i = 0; i < 30; i++) { const now = await frame(); idle.push(now - previous); previous = now; }
                      const baseline = [...idle].sort((a,b)=>a-b)[15];
                      const intervals = [];
                      let mountedMax = 0;
                      const distance = viewport.scrollHeight - viewport.clientHeight;
                      previous = await frame();
                      for (let i = 0; i < 240; i++) {
                        viewport.scrollTop = distance * i / 239;
                        const now = await frame(); intervals.push(now - previous); previous = now;
                        mountedMax = Math.max(mountedMax, viewport.querySelectorAll('[data-cs="game-card"]').length);
                      }
                      return {layout:viewport.dataset.layout, games:document.querySelector('[role="status"]')?.textContent,
                        baselineMs:baseline, samples:intervals.length, maxIntervalMs:Math.max(...intervals),
                        droppedFrames:intervals.reduce((n,d)=>n+Math.max(0,Math.round(d/baseline)-1),0), mountedMax,
                        lastGame:[...viewport.querySelectorAll('[data-cs="game-card"] [data-part="title"]')].at(-1)?.textContent};
                    })()
                    """})
                    if measured.get("exceptionDetails"):
                        raise RuntimeError(measured["exceptionDetails"])
                    result = measured["result"]["value"]
                    results.append(result)
                    print(json.dumps(result), flush=True)
            finally:
                await request(browser, "Target.closeTarget", {"targetId": target_id})
    if args.output:
        args.output.parent.mkdir(parents=True, exist_ok=True)
        args.output.write_text(json.dumps({"browser": version["Browser"], "build": args.build, "ipcFixtures": str(args.ipc_fixtures) if args.ipc_fixtures else None, "results": results}, indent=2) + "\n")
    if any(r["droppedFrames"] or r["mountedMax"] >= 100 or r["mountedMax"] == 0 for r in results):
        raise SystemExit(1)


if __name__ == "__main__":
    parser = argparse.ArgumentParser(description=__doc__)
    parser.add_argument("--url", default="http://localhost:1421")
    parser.add_argument("--port", type=int, default=9223)
    parser.add_argument("--output", type=Path)
    parser.add_argument("--build", choices=["development", "release"], default="development")
    parser.add_argument("--ipc-fixtures", type=Path, help="Test-only native IPC preload, including for fixture-free release builds")
    asyncio.run(run(parser.parse_args()))
