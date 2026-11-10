// M3 exit test (#145): the second theme restyles every core view.
//
// Starts the fixture dev server (VITE_CASKADE_FIXTURES=1) on a free port,
// compiles both themes with the real compiler (`caskade-theme build`), and
// visits every core view in Chromium under each theme. A theme is applied
// the way the app applies one: its compiled stylesheet in the single
// `[data-cs-theme]` element, served from `caskade-theme.localhost` (the
// scheme's Windows form), plus `<html data-color-scheme>`. The app gets no
// theme-specific hook.
//
// For each view it records the computed styles of every anatomy root and its
// parts, checks that the second theme changes colours, typography and radii,
// that its declared tokens are in effect and that elements built on its
// radius and font tokens follow them, and that every root is restyled or
// allowlisted below. Screenshots and a coverage report go to target/theming/.
//
//   pnpm --filter @caskade/desktop test:themes [options]
//
// Options: --base <dir> (themes/default), --theme <dir> (themes/paper),
// --css <dir>=<file> (a precompiled stylesheet; skips cargo), --url <origin>
// (an already running fixture dev server), --chrome <path> (or CHROME_PATH),
// --out <dir> (target/theming). Cargo runs under the heavy lock
// (CASKADE_HEAVY_LOCK, default /tmp/caskade-heavy.lock; empty to skip it), so
// don't wrap this script in flock.
import { spawnSync } from "node:child_process";
import { existsSync, mkdirSync, readFileSync, writeFileSync } from "node:fs";
import { createServer as createNetServer } from "node:net";
import { homedir, tmpdir } from "node:os";
import { basename, delimiter, dirname, extname, join, normalize, resolve, sep } from "node:path";
import { fileURLToPath } from "node:url";
import { parseArgs } from "node:util";
import { chromium } from "playwright-core";

const appDir = resolve(dirname(fileURLToPath(import.meta.url)), "..");
const repo = resolve(appDir, "../..");
const THEME_ORIGIN = "http://caskade-theme.localhost";

/** Roots the second theme may leave exactly as the default draws them. */
const UNCHANGED_ALLOWLIST = {
  // "root-name": "why the theme leaves it alone",
};

/** Token findings that are intended, keyed `<root>><target>` (`hero>part=title`). */
const TOKEN_ALLOWLIST = {
  // "game-art>root": "why it doesn't follow the token",
};

const COLOUR = [
  "color",
  "background-color",
  "background-image",
  "border-top-color",
  "border-right-color",
  "border-bottom-color",
  "border-left-color",
  "outline-color",
];
const TYPOGRAPHY = [
  "font-family",
  "font-size",
  "font-weight",
  "font-style",
  "letter-spacing",
  "line-height",
  "text-transform",
];
const RADIUS = [
  "border-top-left-radius",
  "border-top-right-radius",
  "border-bottom-right-radius",
  "border-bottom-left-radius",
];
const BORDER = ["top", "right", "bottom", "left"].flatMap((side) => [
  `border-${side}-width`,
  `border-${side}-style`,
]);
const EFFECT = [
  "box-shadow",
  "filter",
  "backdrop-filter",
  "opacity",
  "clip-path",
  "mix-blend-mode",
];
const PROPS = [...COLOUR, ...TYPOGRAPHY, ...RADIUS, ...BORDER, ...EFFECT];
const PSEUDO_PROPS = [
  "content",
  "background-color",
  "background-image",
  "border-top-color",
  "box-shadow",
  "opacity",
];
/** Domain state that tells instances of a root apart. */
const KEY_ATTRS = [
  "data-variant",
  "data-status",
  "data-tone",
  "data-kind",
  "data-layout",
  "data-size",
  "data-source",
  "data-store",
  "data-storage-status",
];
const MAX_KEYS_PER_ROOT = 6;

function category(prop) {
  if (prop === "font faces" || TYPOGRAPHY.includes(prop)) return "typography";
  if (COLOUR.includes(prop)) return "colour";
  if (RADIUS.includes(prop)) return "radius";
  if (BORDER.includes(prop)) return "border";
  return "effect";
}

// `pnpm run` may forward a leading `--`.
const argv = process.argv.slice(2);
const { values: args } = parseArgs({
  args: argv[0] === "--" ? argv.slice(1) : argv,
  options: {
    base: { type: "string", default: "themes/default" },
    theme: { type: "string", default: "themes/paper" },
    css: { type: "string", multiple: true, default: [] },
    url: { type: "string" },
    chrome: { type: "string" },
    out: { type: "string", default: "target/theming" },
  },
});

// ── Themes ──

function loadTheme(dir) {
  const folder = resolve(repo, dir);
  const manifest = JSON.parse(readFileSync(join(folder, "theme.json"), "utf8"));
  return {
    dir: folder,
    slug: basename(folder),
    id: manifest.id,
    name: manifest.name,
    modes: manifest.modes ?? ["dark"],
    previews: manifest.previews ?? [],
  };
}

function hasFlock() {
  return process.platform !== "win32" && spawnSync("flock", ["--version"]).status === 0;
}

/** The compiled stylesheet: a precompiled file from --css, or the CLI's output. */
function compile(theme, others) {
  for (const entry of args.css) {
    const [dir, file] = entry.split("=");
    if (!dir || !file || resolve(repo, dir) !== theme.dir) continue;
    return readFileSync(resolve(file), "utf8");
  }
  const cargo = [
    "cargo",
    "run",
    "--quiet",
    "-p",
    "caskade-theme",
    "--features",
    "cli",
    "--",
    "build",
    theme.dir,
    ...others.map((other) => other.dir),
  ];
  const lock = process.env.CASKADE_HEAVY_LOCK ?? "/tmp/caskade-heavy.lock";
  const command = lock && hasFlock() ? ["flock", lock, ...cargo] : cargo;
  const run = spawnSync(command[0], command.slice(1), {
    cwd: repo,
    encoding: "utf8",
    maxBuffer: 16 << 20,
    stdio: ["ignore", "pipe", "inherit"],
  });
  if (run.status !== 0) throw new Error(`caskade-theme build ${theme.dir} failed`);
  return run.stdout;
}

/** `--cs-*` declarations in the compiled `:root` block. */
function declaredTokens(css) {
  const block = /:root\s*\{([^}]*)\}/.exec(css)?.[1] ?? "";
  const pairs = [...block.matchAll(/(--cs-[\w-]+)\s*:\s*([^;]+);/g)];
  return Object.fromEntries(pairs.map(([, name, value]) => [name, value.trim()]));
}

/** Families declared by `@font-face` rules. */
function fontFaces(css) {
  const rules = [...css.matchAll(/@font-face\s*\{[^}]*?font-family:\s*["']?([^;"']+)["']?\s*;/g)];
  return [...new Set(rules.map(([, family]) => family.trim()))];
}

const MIME = {
  ".css": "text/css",
  ".svg": "image/svg+xml",
  ".png": "image/png",
  ".jpg": "image/jpeg",
  ".jpeg": "image/jpeg",
  ".webp": "image/webp",
  ".gif": "image/gif",
  ".avif": "image/avif",
  ".woff2": "font/woff2",
  ".woff": "font/woff",
  ".ttf": "font/ttf",
  ".otf": "font/otf",
};

/** Serves `/<id>/theme.css` and `/<id>/assets/…` like the core's theme scheme. */
function serveThemes(themes) {
  return async (route) => {
    const url = new URL(route.request().url());
    const [id, ...rest] = url.pathname.split("/").filter(Boolean).map(decodeURIComponent);
    const theme = themes.find((candidate) => candidate.id === id);
    const path = rest.join("/");
    const headers = { "access-control-allow-origin": "*", "cache-control": "no-cache" };
    if (theme && path === "theme.css") {
      return route.fulfill({ status: 200, contentType: "text/css", headers, body: theme.css });
    }
    const file = theme && normalize(join(theme.dir, path));
    const allowed =
      file?.startsWith(`${theme.dir}${sep}`) &&
      (path.startsWith("assets/") || theme.previews.includes(path)) &&
      MIME[extname(file).toLowerCase()] &&
      existsSync(file);
    if (!allowed) return route.fulfill({ status: 404, headers, body: "" });
    return route.fulfill({
      status: 200,
      contentType: MIME[extname(file).toLowerCase()],
      headers,
      body: readFileSync(file),
    });
  };
}

// ── Views ──

/** Settings tabs, read from SettingsView so a new tab is covered. */
function settingsTabs() {
  const source = readFileSync(join(appDir, "src/views/SettingsView.tsx"), "utf8");
  const block = /settingsTabs\s*=\s*\[([\s\S]*?)\]\s*as const/.exec(source)?.[1] ?? "";
  const ids = [...block.matchAll(/id:\s*"([^"]+)"/g)].map(([, id]) => id);
  if (ids.length === 0) throw new Error("no settings tabs found in SettingsView.tsx");
  return ids;
}

const PLUGIN_NOTE =
  "The fixture build has no plugins (plugin UI halves come from the core over caskade-plugin://), so this route shows the shell and the view's 'isn't available' status, not a plugin's own UI.";

/** Settings tabs whose fixtures show error states on purpose, and why. */
const EXPECTED_ALERTS = {
  stores:
    "the fixtures' store accounts include one that can't be reached and one whose sign-in expired",
};

const views = [
  { name: "home", path: "/", ready: '[data-view="home"]' },
  {
    name: "library-grid",
    path: "/library?layout=grid",
    ready: '[data-view="library"] [data-cs="game-card"]',
  },
  {
    name: "library-list",
    path: "/library?layout=list",
    ready: '[data-view="library"] [data-cs="game-row"]',
  },
  { name: "game", path: "/games/fixture-1", ready: '[data-view="game"]' },
  { name: "store", path: "/store", ready: '[data-view="store"]' },
  { name: "collections", path: "/collections", ready: '[data-view="collections"]' },
  { name: "collection", path: "/collections/favourites", ready: '[data-view="collection"]' },
  { name: "activity", path: "/activity", ready: '[data-view="activity"]' },
  {
    name: "plugin",
    path: "/plugins/caskade.example/view",
    ready: '[data-cs="view"] [role="status"]',
    note: PLUGIN_NOTE,
  },
  ...settingsTabs().map((tab) => ({
    name: `settings-${tab}`,
    path: `/settings?tab=${tab}`,
    ready: '[data-view="settings"] [role="tabpanel"]',
    settings: true,
    expectedAlerts: EXPECTED_ALERTS[tab],
  })),
];

/** A Settings tab's panel has rendered its content: anatomy roots and no
 * "Loading …" placeholder left. */
function panelLoadedInPage() {
  const panel = document.querySelector('[data-view="settings"] [role="tabpanel"]');
  if (!panel?.querySelector("[data-cs]")) return false;
  return !/Loading [^…]*…/.test(panel.textContent ?? "");
}

/** What keeps a Settings panel from showing its real content. Alerts are
 * one, unless the tab's fixtures show them on purpose (`expectedAlerts`). */
function panelProblemsInPage(alertsExpected) {
  const panel = document.querySelector('[data-view="settings"] [role="tabpanel"]');
  if (!panel) return ["no tab panel"];
  const problems = [];
  const loading = /Loading [^…]*…/.exec(panel.textContent ?? "");
  if (loading) problems.push(`still showing "${loading[0]}"`);
  if (!panel.querySelector("[data-cs]")) problems.push("no anatomy roots in the panel");
  if (/settings go here\./.test(panel.textContent ?? "")) problems.push("placeholder panel");
  for (const alert of alertsExpected ? [] : panel.querySelectorAll('[role="alert"]')) {
    problems.push(`error state: "${(alert.textContent ?? "").trim().slice(0, 160)}"`);
  }
  return problems;
}

/** Waits for a view; Settings tabs also wait for their panel's content. */
async function waitForView(page, view, timeout) {
  await page.waitForSelector(view.ready, { timeout });
  if (!view.settings) return [];
  await page.waitForFunction(panelLoadedInPage, undefined, { timeout: 30_000 }).catch(() => {});
  return page.evaluate(panelProblemsInPage, Boolean(view.expectedAlerts));
}

// ── In-page helpers (serialized into the page; self-contained) ──

async function applyInPage({ id, href, scheme }) {
  const before = document.head.querySelectorAll("[data-cs-theme]").length;
  const link = document.createElement("link");
  link.rel = "stylesheet";
  link.href = href;
  link.dataset.csTheme = id;
  const loaded = new Promise((done, fail) => {
    link.addEventListener("load", () => done(), { once: true });
    link.addEventListener("error", () => fail(new Error(`${href} didn't load`)), { once: true });
  });
  document.head.append(link);
  await loaded;
  for (const old of document.head.querySelectorAll("[data-cs-theme]")) {
    if (old !== link) old.remove();
  }
  document.documentElement.dataset.colorScheme = scheme;
  document.body.getBoundingClientRect();
  return before;
}

async function settleInPage({ quietMs, maxMs }) {
  const until = performance.now() + maxMs;
  let last = performance.now();
  const observer = new MutationObserver(() => {
    last = performance.now();
  });
  observer.observe(document.body, {
    subtree: true,
    childList: true,
    attributes: true,
    characterData: true,
  });
  while (performance.now() < until && performance.now() - last < quietMs) {
    await new Promise((done) => setTimeout(done, 50));
  }
  observer.disconnect();
  const frame = () => new Promise((done) => requestAnimationFrame(() => done()));
  await frame();
  await frame();
  await document.fonts.ready;
  const finite = document
    .getAnimations()
    .filter(
      (animation) =>
        animation.playState === "running" &&
        animation.effect?.getComputedTiming().endTime !== Number.POSITIVE_INFINITY,
    );
  await Promise.race([
    Promise.all(finite.map((animation) => animation.finished.catch(() => {}))),
    new Promise((done) => setTimeout(done, maxMs)),
  ]);
  await frame();
}

function recordInPage({ props, pseudoProps, keyAttrs, maxKeys, tokens, faces }) {
  const visible = (el) => {
    const rect = el.getBoundingClientRect();
    return rect.width > 0 && rect.height > 0;
  };
  const owner = (el) =>
    (el.hasAttribute("data-cs") ? el.parentElement : el)?.closest("[data-cs]") ?? null;
  const styleOf = (el, list, pseudo) => {
    const style = getComputedStyle(el, pseudo);
    return Object.fromEntries(list.map((prop) => [prop, style.getPropertyValue(prop).trim()]));
  };
  const pseudo = (el, which) => {
    const style = styleOf(el, pseudoProps, which);
    return style.content && style.content !== "none" && style.content !== "normal" ? style : null;
  };
  const textFaces = (root) => {
    const found = new Set();
    const walker = document.createTreeWalker(root, NodeFilter.SHOW_TEXT);
    let budget = 4000;
    for (let node = walker.nextNode(); node && budget > 0; node = walker.nextNode()) {
      budget -= 1;
      const el = node.parentElement;
      if (!node.textContent?.trim() || !el || el.closest("[data-cs]") !== root) continue;
      found.add(getComputedStyle(el).fontFamily);
    }
    return [...found].sort();
  };

  // The elements to record: the first (preferably visible) instance per root
  // and state, kept across the theme switch so both themes see the same ones.
  const state = window.__caskadeThemeExit ?? { picks: null };
  window.__caskadeThemeExit = state;
  const counts = {};
  const all = [...document.querySelectorAll("[data-cs]")];
  for (const el of all) counts[el.dataset.cs] = (counts[el.dataset.cs] ?? 0) + 1;
  if (!state.picks || state.picks.some(([, el]) => !el.isConnected)) {
    const picks = new Map();
    const perRoot = {};
    for (const el of all) {
      const name = el.dataset.cs;
      const states = keyAttrs
        .filter((attr) => el.hasAttribute(attr))
        .map((attr) => `${attr.slice(5)}=${el.getAttribute(attr)}`);
      const key = states.length ? `${name}[${states.join(",")}]` : name;
      const previous = picks.get(key);
      if (previous) {
        if (!visible(previous) && visible(el)) picks.set(key, el);
        continue;
      }
      if ((perRoot[name] ?? 0) >= maxKeys) continue;
      perRoot[name] = (perRoot[name] ?? 0) + 1;
      picks.set(key, el);
    }
    state.picks = [...picks];
  }

  const records = state.picks.map(([key, el]) => {
    // The first instance of each part, a visible one when there is one.
    const chosen = {};
    for (const part of el.querySelectorAll("[data-part]")) {
      const name = part.dataset.part;
      if (owner(part) !== el || (chosen[name] && visible(chosen[name]))) continue;
      if (!chosen[name] || visible(part)) chosen[name] = part;
    }
    const parts = {};
    for (const [name, part] of Object.entries(chosen)) parts[name] = styleOf(part, props);
    return {
      key,
      name: el.dataset.cs,
      visible: visible(el),
      style: styleOf(el, props),
      before: pseudo(el, "::before"),
      after: pseudo(el, "::after"),
      faces: textFaces(el),
      parts,
    };
  });

  // Each token resolved through a probe, to map element values back to tokens.
  const probe = document.createElement("div");
  probe.style.cssText = "position:absolute;visibility:hidden;pointer-events:none";
  document.body.append(probe);
  const resolved = {};
  const probeProps = {
    colour: "background-color",
    font: "font-family",
    radius: "border-top-left-radius",
  };
  for (const { name, kind } of tokens) {
    const prop = probeProps[kind];
    probe.style.setProperty(prop, `var(${name})`);
    resolved[name] = getComputedStyle(probe).getPropertyValue(prop).trim();
    probe.style.removeProperty(prop);
  }
  probe.remove();

  const loadedFaces = [...document.fonts]
    .filter((face) => face.status === "loaded")
    .map((face) => face.family.replace(/^["']|["']$/g, ""));

  const sheet = document.head.querySelectorAll("[data-cs-theme]");
  return {
    counts,
    records,
    resolved,
    loadedFaces: faces.filter((family) => loadedFaces.includes(family)),
    sheets: [...sheet].map((el) => `${el.dataset.csTheme} ${el.getAttribute("href") ?? "inline"}`),
  };
}

/** Whether `<html>` computes each declared token to the value the theme declares. */
function tokensInPage(declared) {
  const html = getComputedStyle(document.documentElement);
  const probe = document.createElement("div");
  document.body.append(probe);
  const normal = (value) => value.trim().replace(/\s+/g, " ");
  const wrong = [];
  for (const [name, value] of Object.entries(declared)) {
    probe.style.setProperty("--theme-exit-probe", value);
    const expected = normal(getComputedStyle(probe).getPropertyValue("--theme-exit-probe"));
    const actual = normal(html.getPropertyValue(name));
    if (expected !== actual) wrong.push({ name, expected, actual });
  }
  probe.remove();
  return wrong;
}

// ── Comparison ──

function changes(a, b) {
  const found = [];
  const compare = (target, left, right, props) => {
    for (const prop of props) {
      if ((left?.[prop] ?? "") !== (right?.[prop] ?? "")) {
        found.push({ target, prop, from: left?.[prop] ?? null, to: right?.[prop] ?? null });
      }
    }
  };
  compare("root", a.style, b.style, PROPS);
  compare("::before", a.before, b.before, PSEUDO_PROPS);
  compare("::after", a.after, b.after, PSEUDO_PROPS);
  for (const part of Object.keys(a.parts)) {
    if (b.parts[part]) compare(`part=${part}`, a.parts[part], b.parts[part], PROPS);
  }
  if (a.faces.join("|") !== b.faces.join("|")) {
    found.push({ target: "root", prop: "font faces", from: a.faces, to: b.faces });
  }
  return found.map((change) => ({ ...change, category: category(change.prop) }));
}

/**
 * Elements whose default value is a token's value, where the theme changed
 * every token with that value: `stale` when the element kept the default
 * value, `diverged` when it took a value none of those tokens has now.
 */
function tokenFindings(a, b, base, next, tokens) {
  const findings = [];
  const byKind = (kind) => tokens.filter((token) => token.kind === kind).map(({ name }) => name);
  const check = (kind, target, prop, from, to) => {
    const candidates = byKind(kind).filter((name) => base.resolved[name] === from);
    if (candidates.length === 0) return;
    if (candidates.some((name) => next.resolved[name] === base.resolved[name])) return;
    const expected = candidates.map((name) => next.resolved[name]);
    if (to === from) findings.push({ kind, problem: "stale", target, prop, value: to, candidates });
    else if (kind !== "colour" && !expected.includes(to)) {
      findings.push({ kind, problem: "diverged", target, prop, value: to, expected, candidates });
    }
  };
  const targets = [["root", a.style, b.style]];
  for (const part of Object.keys(a.parts)) {
    if (b.parts[part]) targets.push([`part=${part}`, a.parts[part], b.parts[part]]);
  }
  for (const [target, left, right] of targets) {
    for (const prop of COLOUR) {
      if (prop !== "background-image") check("colour", target, prop, left[prop], right[prop]);
    }
    for (const prop of RADIUS) check("radius", target, prop, left[prop], right[prop]);
    check("font", target, "font-family", left["font-family"], right["font-family"]);
  }
  // The root's own text: each default face built on a changed font token
  // must give way to the theme's value of it.
  for (const face of a.faces) {
    const candidates = byKind("font").filter((name) => base.resolved[name] === face);
    if (candidates.length === 0) continue;
    if (candidates.some((name) => next.resolved[name] === base.resolved[name])) continue;
    const expected = candidates.map((name) => next.resolved[name]);
    const finding = { kind: "font", target: "root", prop: "font faces", candidates };
    if (b.faces.includes(face)) findings.push({ ...finding, problem: "stale", value: face });
    else if (!expected.some((family) => b.faces.includes(family))) {
      findings.push({ ...finding, problem: "diverged", value: b.faces, expected });
    }
  }
  return findings;
}

// ── Run ──

function chromePath() {
  const explicit = args.chrome ?? process.env.CHROME_PATH;
  if (explicit) return explicit;
  const local = join(homedir(), ".local/bin/chromium");
  if (existsSync(local)) return local;
  for (const dir of (process.env.PATH ?? "").split(delimiter)) {
    for (const name of ["chromium", "chromium-browser", "google-chrome", "google-chrome-stable"]) {
      if (dir && existsSync(join(dir, name))) return join(dir, name);
    }
  }
  throw new Error("no Chromium found: pass --chrome <path> or set CHROME_PATH");
}

async function freePort() {
  const server = createNetServer();
  await new Promise((done) => server.listen(0, "127.0.0.1", done));
  const { port } = server.address();
  await new Promise((done) => server.close(done));
  return port;
}

async function startDevServer() {
  process.env.VITE_CASKADE_FIXTURES = "1";
  const { createServer } = await import("vite");
  const port = await freePort();
  const server = await createServer({
    root: appDir,
    configFile: join(appDir, "vite.config.ts"),
    cacheDir: join(tmpdir(), "caskade-theme-exit-vite"),
    logLevel: "warn",
    clearScreen: false,
    // Peers' edits in a shared checkout must not reload the page mid-run.
    server: { host: "127.0.0.1", port, strictPort: true, watch: null },
  });
  await server.listen();
  return { url: `http://127.0.0.1:${port}`, close: () => server.close() };
}

const outDir = resolve(repo, args.out);
mkdirSync(outDir, { recursive: true });

const base = loadTheme(args.base);
const next = loadTheme(args.theme);
const themes = [base, next];
for (const theme of themes) {
  theme.css = compile(
    theme,
    themes.filter((other) => other !== theme),
  );
  theme.href = `${THEME_ORIGIN}/${encodeURIComponent(theme.id)}/theme.css`;
  // The OS prefers dark (emulated below): the theme's dark mode, else its first.
  theme.scheme = theme.modes.includes("dark") ? "dark" : (theme.modes[0] ?? "dark");
}
const declared = declaredTokens(next.css);
const declaredFaces = fontFaces(next.css).filter((family) =>
  Object.entries(declared).some(
    ([name, value]) => name.startsWith("--cs-font-") && value.includes(family),
  ),
);

const catalogue = new Set(
  Object.keys(JSON.parse(readFileSync(join(repo, "packages/ui/anatomy.json"), "utf8")).roots),
);
const manifest = JSON.parse(readFileSync(join(repo, "packages/tokens/manifest.json"), "utf8"));
const tokens = manifest.flatMap(({ name, type }) => {
  if (type === "color") return [{ name, kind: "colour" }];
  if (type === "fontFamily") return [{ name, kind: "font" }];
  if (type === "dimension" && name.startsWith("--cs-radius-")) return [{ name, kind: "radius" }];
  return [];
});

const dev = args.url
  ? { url: args.url.replace(/\/$/, ""), close: async () => {} }
  : await startDevServer();
const browser = await chromium.launch({ executablePath: chromePath(), headless: true });
const failures = [];
const warnings = [];
const report = [];
try {
  const context = await browser.newContext({
    viewport: { width: 1440, height: 900 },
    colorScheme: "dark",
    reducedMotion: "reduce",
  });
  await context.route(`${THEME_ORIGIN}/**`, serveThemes(themes));
  // Past first-run setup: the fixtures read this before they start.
  await context.addInitScript(() => localStorage.setItem("caskade-fixtures-onboarded", "1"));
  const page = await context.newPage();
  const errors = [];
  page.on("pageerror", (error) => errors.push(error.message));

  // Warm-up: every route once, so the dev server has optimized its
  // dependencies before anything is measured.
  for (const view of views) {
    await page.goto(dev.url + view.path, { waitUntil: "load" });
    await waitForView(page, view, 120_000);
  }

  const navigations = [];
  page.on("framenavigated", (frame) => {
    if (frame === page.mainFrame()) navigations.push(frame.url());
  });

  /** Both themes on one load of the view; null when the page navigated
   * underneath (a dev server reload), to try again. */
  async function measure(view) {
    errors.length = 0;
    await page.goto(dev.url + view.path, { waitUntil: "load" });
    const notReady = await waitForView(page, view, 60_000);
    navigations.length = 0;
    try {
      return { notReady, runs: await measureThemes(view) };
    } catch (error) {
      if (!/context was destroyed|navigat/i.test(String(error))) throw error;
      console.log(`  ${view.name}: the page navigated (${navigations.join(", ")}); retrying`);
      return null;
    }
  }

  async function measureThemes(view) {
    const runs = [];
    for (const theme of themes) {
      const before = await page.evaluate(applyInPage, {
        id: theme.id,
        href: theme.href,
        scheme: theme.scheme,
      });
      if (before !== 1) {
        failures.push(`${view.name}: expected one [data-cs-theme] element, found ${before}`);
      }
      await page.evaluate(settleInPage, { quietMs: 300, maxMs: 5000 });
      const run = await page.evaluate(recordInPage, {
        props: PROPS,
        pseudoProps: PSEUDO_PROPS,
        keyAttrs: KEY_ATTRS,
        maxKeys: MAX_KEYS_PER_ROOT,
        tokens,
        faces: declaredFaces,
      });
      const ours = `${theme.id} ${theme.href}`;
      if (run.sheets.length !== 1 || run.sheets[0] !== ours) {
        failures.push(`${view.name}: the app replaced the theme sheet (${run.sheets.join(", ")})`);
      }
      run.screenshot = `${view.name}-${theme.slug}.png`;
      await page.screenshot({
        path: join(outDir, run.screenshot),
        animations: "disabled",
        caret: "hide",
      });
      runs.push(run);
    }
    return runs;
  }

  for (const view of views) {
    let measured = null;
    for (let attempt = 0; attempt < 3 && !measured; attempt += 1) measured = await measure(view);
    if (!measured) throw new Error(`${view.name}: the page kept navigating away`);
    const { notReady, runs } = measured;
    const [a, b] = runs;
    const viewFailures = notReady.map((problem) => `panel ${problem}`);

    // The second theme's tokens are in effect on <html>.
    for (const wrong of await page.evaluate(tokensInPage, declared)) {
      viewFailures.push(
        `token ${wrong.name} computes to "${wrong.actual}", the theme declares "${wrong.expected}"`,
      );
    }
    for (const family of declaredFaces) {
      if (!b.loadedFaces.includes(family)) viewFailures.push(`font "${family}" didn't load`);
    }

    // Roots: restyled, allowlisted, or a failure.
    const pairs = a.records
      .map((record) => [record, b.records.find((other) => other.key === record.key)])
      .filter(([, other]) => other);
    const roots = {};
    const categories = new Set();
    const findings = [];
    for (const name of Object.keys({ ...a.counts, ...b.counts }).sort()) {
      roots[name] = {
        catalogued: catalogue.has(name),
        instances: b.counts[name] ?? 0,
        restyled: false,
        changed: [],
        visible: false,
        paired: false,
      };
    }
    for (const [left, right] of pairs) {
      const root = roots[left.name];
      const found = changes(left, right);
      root.paired = true;
      root.visible ||= left.visible;
      if (found.length > 0) root.restyled = true;
      for (const change of found) {
        categories.add(change.category);
        if (!root.changed.includes(change.category)) root.changed.push(change.category);
      }
      root.changes ??= [];
      root.changes.push(...found.map((change) => ({ key: left.key, ...change })));
      for (const finding of tokenFindings(left, right, a, b, tokens)) {
        findings.push({ key: left.key, root: left.name, ...finding });
      }
    }
    for (const [name, root] of Object.entries(roots)) {
      if (!root.catalogued) root.note = "not in the anatomy catalogue";
      if (root.restyled) {
        if (UNCHANGED_ALLOWLIST[name]) root.note = "allowlisted as unchanged, but restyled";
        continue;
      }
      if (UNCHANGED_ALLOWLIST[name]) {
        root.note = `unchanged: ${UNCHANGED_ALLOWLIST[name]}`;
      } else if (!root.paired) {
        root.note = "rendered under one theme only, not compared";
        warnings.push(`${view.name}: root ${name} was rendered under one theme only`);
      } else {
        viewFailures.push(`root ${name} is identical under both themes and not allowlisted`);
      }
    }
    for (const required of ["colour", "typography", "radius"]) {
      if (!categories.has(required)) viewFailures.push(`no ${required} change on any root`);
    }
    for (const finding of findings) {
      const allow = TOKEN_ALLOWLIST[`${finding.root}>${finding.target}`];
      const text = `${finding.key} ${finding.target} ${finding.prop}: ${finding.problem} ${finding.kind} token (${finding.candidates.join(", ")}) value ${JSON.stringify(finding.value)}${finding.expected ? `, expected ${JSON.stringify(finding.expected)}` : ""}`;
      if (allow) finding.allowlisted = allow;
      else if (finding.kind === "colour") warnings.push(`${view.name}: ${text}`);
      else viewFailures.push(text);
    }
    for (const error of errors) warnings.push(`${view.name}: page error: ${error}`);

    failures.push(...viewFailures.map((failure) => `${view.name}: ${failure}`));
    report.push({
      view: view.name,
      path: view.path,
      note: view.note,
      screenshots: { [base.slug]: a.screenshot, [next.slug]: b.screenshot },
      changed: [...categories].sort(),
      failures: viewFailures,
      roots,
      tokenFindings: findings,
      records: { [base.slug]: a.records, [next.slug]: b.records },
    });
    console.log(`${viewFailures.length ? "FAIL" : "ok  "} ${view.name}`);
  }

  const summary = {
    base: { id: base.id, name: base.name },
    theme: { id: next.id, name: next.name },
    browser: `Chromium ${browser.version()}`,
    url: dev.url,
    date: new Date().toISOString(),
    passed: failures.length === 0,
    failures,
    warnings,
    allowlist: { unchanged: UNCHANGED_ALLOWLIST, tokens: TOKEN_ALLOWLIST },
    views: report,
  };
  writeFileSync(join(outDir, "coverage.json"), `${JSON.stringify(summary, null, 2)}\n`);
  writeFileSync(join(outDir, "coverage.md"), markdown(summary));
} finally {
  await browser.close();
  await dev.close();
}

console.log(`\n${failures.length} failure(s), ${warnings.length} warning(s); report in ${outDir}`);
for (const failure of failures) console.log(`  ${failure}`);
process.exitCode = failures.length ? 1 : 0;

function markdown(summary) {
  const yes = (value) => (value ? "yes" : "no");
  const lines = [
    `# Theme exit test: ${summary.base.name} → ${summary.theme.name}`,
    "",
    `${summary.browser}, fixture build at ${summary.url}, ${summary.date}.`,
    `Result: **${summary.passed ? "pass" : "fail"}** (${summary.failures.length} failures, ${summary.warnings.length} warnings).`,
    "",
    "| View | Path | Roots | Colour | Typography | Radius | Screenshots |",
    "| --- | --- | --- | --- | --- | --- | --- |",
    ...summary.views.map(
      (view) =>
        `| ${view.view} | \`${view.path}\` | ${Object.keys(view.roots).length} | ${yes(view.changed.includes("colour"))} | ${yes(view.changed.includes("typography"))} | ${yes(view.changed.includes("radius"))} | ${Object.values(view.screenshots).join(", ")} |`,
    ),
    "",
  ];
  if (summary.failures.length) {
    lines.push("## Failures", "", ...summary.failures.map((failure) => `- ${failure}`), "");
  }
  for (const view of summary.views) {
    lines.push(`## ${view.view}`, "");
    if (view.note) lines.push(view.note, "");
    lines.push(
      `| Root | Catalogued | Instances | Restyled by ${summary.theme.name} | Changed | Note |`,
      "| --- | --- | --- | --- | --- | --- |",
    );
    for (const [name, root] of Object.entries(view.roots)) {
      lines.push(
        `| ${name} | ${yes(root.catalogued)} | ${root.instances} | ${yes(root.restyled)} | ${root.changed.sort().join(", ")} | ${root.note ?? ""} |`,
      );
    }
    lines.push("");
  }
  if (summary.warnings.length) {
    lines.push("## Warnings", "", ...summary.warnings.map((warning) => `- ${warning}`), "");
  }
  return `${lines.join("\n")}\n`;
}
