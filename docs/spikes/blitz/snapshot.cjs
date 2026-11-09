// Snapshot Caskade's rendered fixture UI into a self-contained, script-free HTML file
// so WebKitGTK and Blitz can be compared on identical DOM + CSS with no JS on either side.
// usage: node snapshot.cjs <url> <out-prefix> [width] [height]
const { chromium } = require(
  "/home/krazy/.paseo/worktrees/3c4l1xfr/white-pony/node_modules/.pnpm/node_modules/playwright-core",
);

(async () => {
  const [url, prefix, w = "1400", h = "900"] = process.argv.slice(2);
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: +w, height: +h } });
  const t0 = Date.now();
  await page.goto(url, { waitUntil: "networkidle" });
  const skip = page.getByRole("button", { name: /skip setup/i });
  await skip.first().waitFor({ timeout: 15000 });
  await skip.first().click();
  await page.waitForTimeout(1500);
  // Client-side navigation keeps the in-memory fixture state.
  const path = new URL(url).pathname;
  await page.evaluate((p) => { history.pushState({}, "", p); dispatchEvent(new PopStateEvent("popstate")); }, path);
  await page.waitForTimeout(2500);
  const loadMs = Date.now() - t0;
  await page.screenshot({ path: `${prefix}-chromium.png` });

  const result = await page.evaluate(() => {
    const css = [];
    for (const sheet of document.styleSheets) {
      try {
        css.push([...sheet.cssRules].map((r) => r.cssText).join("\n"));
      } catch (e) {
        css.push(`/* unreadable sheet ${sheet.href} */`);
      }
    }
    const clone = document.documentElement.cloneNode(true);
    for (const el of clone.querySelectorAll("script, link[rel=stylesheet], link[rel=modulepreload], style"))
      el.remove();
    // Absolutise URLs so file:// renders resolve against the server.
    for (const el of clone.querySelectorAll("[src]")) el.setAttribute("src", new URL(el.getAttribute("src"), location.href).href);
    const style = document.createElement("style");
    style.textContent = css.join("\n").replace(/url\((['"]?)(\/[^)'"]+)\1\)/g, (_m, q, p) => `url(${q}${location.origin}${p}${q})`);
    clone.querySelector("head").appendChild(style);
    return {
      html: "<!doctype html>\n" + clone.outerHTML,
      nodes: document.getElementsByTagName("*").length,
      cssBytes: style.textContent.length,
      jsHeap: performance.memory ? performance.memory.usedJSHeapSize : null,
    };
  });
  require("fs").writeFileSync(`${prefix}.html`, result.html);
  console.log(JSON.stringify({ url, loadMs, nodes: result.nodes, cssBytes: result.cssBytes, htmlBytes: result.html.length, jsHeapBytes: result.jsHeap }));
  await browser.close();
})();
