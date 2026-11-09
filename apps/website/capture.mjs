// Manual asset capture, not a test: downloads public store artwork, then
// renders the real app with an isolated sample library. Never opens a user's
// native profile or changes desktop source files. Normal site builds are offline.
import { mkdir, writeFile } from "node:fs/promises";
import { createRequire } from "node:module";
import { fileURLToPath } from "node:url";

const require = createRequire(new URL("../desktop/package.json", import.meta.url));
const { chromium } = require("playwright-core");
const origin = new URL(process.argv[2] ?? "http://127.0.0.1:1430");
if (!["127.0.0.1", "localhost"].includes(origin.hostname)) {
  throw new Error("Capture requires a local Vite fixture server");
}
const output = new URL("assets/", import.meta.url);
await mkdir(output, { recursive: true });
const games = [
  { title: "Hades", steam: 1145360, genre: "Action" },
  { title: "Hollow Knight", steam: 367520, genre: "Adventure" },
  { title: "Stardew Valley", steam: 413150, genre: "Simulation" },
  { title: "Celeste", steam: 504230, genre: "Platformer" },
  { title: "Portal 2", steam: 620, genre: "Puzzle" },
  { title: "Terraria", steam: 105600, genre: "Adventure" },
  { title: "Dead Cells", steam: 588650, genre: "Action" },
  { title: "No Man’s Sky", steam: 275850, genre: "Exploration" },
];
const artwork = new Map(
  await Promise.all(
    games.map(async (game) => {
      const url = `https://shared.fastly.steamstatic.com/store_item_assets/steam/apps/${game.steam}/library_600x900.jpg`;
      const response = await fetch(url, { signal: AbortSignal.timeout(30_000) });
      if (!response.ok) throw new Error(`Artwork for ${game.title}: HTTP ${response.status}`);
      return [String(game.steam), Buffer.from(await response.arrayBuffer())];
    }),
  ),
);

const browser = await chromium.launch({
  headless: true,
  executablePath: process.env.CHROME_PATH,
});
try {
  for (const language of ["en", "sv"]) {
    for (const mode of ["desktop", "console"]) {
      const context = await browser.newContext({
        viewport: { width: 1440, height: 900 },
        locale: language === "sv" ? "sv-SE" : "en-GB",
        reducedMotion: "reduce",
      });
      // Only the manual artwork download above reaches the internet. The app
      // itself sees a local fixture API and local in-memory image responses.
      await context.route("**/*", async (route) => {
        const url = new URL(route.request().url());
        if (url.origin !== origin.origin) return route.abort();
        if (url.pathname.startsWith("/__website-art/")) {
          const image = artwork.get(url.pathname.split("/").at(-1));
          if (!image) throw new Error(`Unknown artwork: ${url.pathname}`);
          return route.fulfill({ contentType: "image/jpeg", body: image });
        }
        if (url.pathname !== "/src/api/bootstrap.ts") return route.continue();
        return route.fulfill({
          contentType: "application/javascript",
          body: `
            import { createFixtureApi, createFixtureGames } from '/src/api/fixtures.ts';
            import { settingsFixtureApi } from '/src/api/settingsFixtures.ts';
            export async function connectLibraryApi() {
              const settings = await settingsFixtureApi.get('core.onboarding');
              await settingsFixtureApi.set('core.onboarding', { ...settings.value, completed: true }, settings.revision);
              const samples = ${JSON.stringify(games)};
              const records = createFixtureGames(samples.length).map((game, index) => ({
                ...game, title: samples[index].title, genre: samples[index].genre,
                status: 'installed', favorite: index < 3,
                coverGrid: '/__website-art/' + samples[index].steam,
                coverList: '/__website-art/' + samples[index].steam,
                description: 'Sample library for Caskade screenshots.',
                playtimeSeconds: (8 - index) * 3600,
                lastPlayedAt: new Date(Date.UTC(2026, 10, 8 - index, 18)).toISOString(),
                entries: [{ ...game.entries[0], source: 'steam', state: 'installed', canLaunch: true }],
              }));
              return createFixtureApi(records);
            }
          `,
        });
      });
      const page = await context.newPage();
      page.setDefaultTimeout(30_000);
      console.log(`Rendering ${mode} in ${language}`);
      const errors = [];
      page.on("pageerror", (error) => errors.push(error.message));
      page.on("console", (message) => {
        if (message.type() === "error") console.error(message.text());
      });
      const query = new URLSearchParams({ lang: language });
      if (mode === "console") query.set("console", "");
      await page.goto(`${origin.origin}/?${query}`, { waitUntil: "networkidle" });
      console.log(`Loaded ${await page.title()}`);
      await page.locator('[data-view="home"] [data-cs="hero"]').waitFor();
      console.log("Home view ready");
      await page.evaluate(() => document.fonts.ready);
      // Off-screen shelves deliberately lazy-load their covers. Wait only for
      // images in the capture, rather than trying to decode hidden shelves.
      await page.waitForFunction(() =>
        [...document.images].every((image) => {
          const box = image.getBoundingClientRect();
          return (
            !box.width ||
            !box.height ||
            box.top >= innerHeight ||
            box.bottom <= 0 ||
            box.left >= innerWidth ||
            box.right <= 0 ||
            (image.complete && image.naturalWidth)
          );
        }),
      );
      if (await page.locator('[data-cs="view-error"]').count()) {
        throw new Error("The app displayed an error instead of the screenshot view");
      }
      if (errors.length) throw new Error(errors.join("\n"));
      const destination = new URL(`${mode}-${language}.jpg`, output);
      await writeFile(
        destination,
        await page.screenshot({
          type: "jpeg",
          quality: 88,
          // Keep the desktop hero and first shelf, excluding the fixture job
          // status and the cut-off next shelf. Console keeps its button hints.
          clip: mode === "desktop" ? { x: 0, y: 0, width: 1440, height: 780 } : undefined,
        }),
      );
      console.log(`Captured ${fileURLToPath(destination)}`);
      await context.close();
    }
  }
} finally {
  await browser.close();
}
