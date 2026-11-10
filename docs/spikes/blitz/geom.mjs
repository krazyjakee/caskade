import { createRequire } from "node:module";
const require = createRequire(import.meta.url);
const [driver, target] = process.argv.slice(2);
const probe = `(() => { let el = document.querySelector('[data-cs="virtual-library"]'); const out = []; for (let i = 0; el && i < 5; i++, el = el.parentElement) { const r = el.getBoundingClientRect(); const cs = getComputedStyle(el); out.push({ tag: el.tagName, cs: el.getAttribute('data-cs'), clientH: el.clientHeight, scrollH: el.scrollHeight, rectH: Math.round(r.height), overflowY: cs.overflowY, cards: i ? undefined : document.querySelectorAll('[data-cs="game-card"]').length }); } return out; })()`;
const goAll = 'a[data-part="sub-item"][href="/library?filter=all"]';
if (driver === "blitsen") {
  const { launch } = await import("/mnt/Rusty/caskade-blitz-spike/blitsen-npm/node_modules/blitsen/src/test.mjs");
  const app = await launch(target, { timeout: 120000, width: 1024, height: 768 });
  try {
    await app.waitFor(() => !!document.querySelector('[data-cs="sidebar"]'));
    await app.click({ selector: 'a[data-part="nav-item"][href="/library"]' });
    await app.settle(300);
    await app.click({ selector: goAll });
    await app.waitFor(() => document.querySelectorAll('[data-cs="game-card"]').length > 5);
    await app.settle(300);
    console.log(JSON.stringify(await app.evaluate(probe), null, 0));
  } finally { await app.close(); }
} else {
  const { chromium } = require("/home/krazy/.paseo/worktrees/3c4l1xfr/white-pony/node_modules/.pnpm/node_modules/playwright-core");
  const b = await chromium.launch(); const p = await b.newPage({ viewport: { width: 1024, height: 768 } });
  await p.goto(target); await p.waitForSelector('[data-cs="sidebar"]');
  await p.click('a[data-part="nav-item"][href="/library"]'); await p.waitForTimeout(300);
  await p.click(goAll); await p.waitForFunction(() => document.querySelectorAll('[data-cs="game-card"]').length > 5); await p.waitForTimeout(300);
  console.log(JSON.stringify(await p.evaluate(probe), null, 0)); await b.close();
}
