// Time the same user flow in a Blitsen export (blitsen/test, QuickJS, headless CPU raster)
// and in headless Chromium (Playwright, V8 JIT).
// usage: node flow.mjs blitsen <export> | node flow.mjs chromium <url>
import { createRequire } from "node:module";
import { writeFileSync } from "node:fs";
const require = createRequire(import.meta.url);
const [driver, target] = process.argv.slice(2);
const OUT = "/mnt/Rusty/caskade-blitz-spike/out";
const W = 1024, H = 768;

const ready = {
  shell: () => !!document.querySelector('[data-cs="sidebar"]') && !!document.querySelector('[data-cs="view"] *'),
  library: () => document.querySelectorAll('[data-cs="game-card"]').length > 0,
  settings: () => !!document.querySelector('[data-cs="settings-section"]') || document.body.textContent.includes("couldn't be shown"),
};
const cardCount = () => document.querySelectorAll('[data-cs="game-card"]').length;
const mainScroll = () => Math.max(...[...document.querySelectorAll("*")].map((e) => e.scrollTop || 0));

const results = { driver };
const time = async (label, fn) => {
  const t = performance.now();
  await fn();
  results[label] = Math.round(performance.now() - t);
  console.log(`${label.padEnd(28)} ${results[label]} ms`);
};

if (driver === "blitsen") {
  const { launch } = await import("/mnt/Rusty/caskade-blitz-spike/blitsen-npm/node_modules/blitsen/src/test.mjs");
  let app;
  await time("launch to first content", async () => {
    app = await launch(target, { timeout: 300000, width: W, height: H, artifactsDir: `${OUT}/blitsen-test` });
    await app.waitFor(ready.shell, { timeout: 300000 });
  });
  try {
    await app.screenshot(`${OUT}/flow-blitsen-home.png`);
    await time("open Library (click)", async () => {
      await app.click({ selector: 'a[data-part="nav-item"][href="/library"]' });
      await app.waitFor(ready.library, { timeout: 300000 });
    });
    await time("settle 300 ms after Library", () => app.settle(300));
    await time("open All Games (8,429)", async () => {
      await app.click({ selector: 'a[data-part="sub-item"][href="/library?filter=all"]' });
      await app.waitFor(() => document.querySelectorAll('[data-cs="game-card"]').length > 5, { timeout: 300000 });
    });
    await app.settle(300);
    await app.screenshot(`${OUT}/flow-blitsen-library.png`);
    results.cardsMounted = await app.evaluate(cardCount);
    const steps = [];
    for (let i = 0; i < 10; i++) {
      const t = performance.now();
      await app.wheel({ x: 600, y: 450, deltaY: 600 });
      await app.settle(16);
      steps.push(Math.round(performance.now() - t));
    }
    results.scrollStepsMs = steps;
    results.scrollTopAfter = await app.evaluate(mainScroll);
    results.cardsMountedAfterScroll = await app.evaluate(cardCount);
    console.log("scroll steps", steps, "scrollTop", results.scrollTopAfter, "cards", results.cardsMountedAfterScroll);
    await app.screenshot(`${OUT}/flow-blitsen-scrolled.png`);
    await time("open Settings (click)", async () => {
      await app.click({ selector: 'a[data-part="nav-item"][href="/settings"]' });
      await app.waitFor(ready.settings, { timeout: 300000 });
    });
    await app.settle(300);
    results.settingsError = await app.evaluate(() => document.body.textContent.includes("couldn't be shown"));
    await app.screenshot(`${OUT}/flow-blitsen-settings.png`);
  } finally {
    await app.close();
  }
} else {
  const { chromium } = require("/home/krazy/.paseo/worktrees/3c4l1xfr/white-pony/node_modules/.pnpm/node_modules/playwright-core");
  const browser = await chromium.launch();
  const page = await browser.newPage({ viewport: { width: W, height: H } });
  await time("launch to first content", async () => {
    await page.goto(target);
    await page.waitForFunction(ready.shell, null, { timeout: 300000 });
  });
  await time("open Library (click)", async () => {
    await page.click('a[data-part="nav-item"][href="/library"]');
    await page.waitForFunction(ready.library, null, { timeout: 300000 });
  });
  await page.waitForTimeout(300);
  await time("open All Games (8,429)", async () => {
    await page.click('a[data-part="sub-item"][href="/library?filter=all"]');
    await page.waitForFunction(() => document.querySelectorAll('[data-cs="game-card"]').length > 5, null, { timeout: 300000 });
  });
  await page.waitForTimeout(300);
  results.cardsMounted = await page.evaluate(cardCount);
  const steps = [];
  await page.mouse.move(600, 450);
  for (let i = 0; i < 10; i++) {
    const t = performance.now();
    await page.mouse.wheel(0, 600);
    await page.evaluate(() => new Promise((r) => requestAnimationFrame(() => requestAnimationFrame(r))));
    steps.push(Math.round(performance.now() - t));
  }
  results.scrollStepsMs = steps;
  results.scrollTopAfter = await page.evaluate(mainScroll);
  results.cardsMountedAfterScroll = await page.evaluate(cardCount);
  console.log("scroll steps", steps, "scrollTop", results.scrollTopAfter, "cards", results.cardsMountedAfterScroll);
  await time("open Settings (click)", async () => {
    await page.click('a[data-part="nav-item"][href="/settings"]');
    await page.waitForFunction(ready.settings, null, { timeout: 300000 });
  });
  await browser.close();
}
writeFileSync(`${OUT}/flow-${driver}.json`, JSON.stringify(results, null, 1));
