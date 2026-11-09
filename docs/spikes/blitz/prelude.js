// Minimal stubs for browser APIs Blitz's Boa runtime lacks (spike only).
class StubObserver { constructor(cb) { this.cb = cb; } observe() {} unobserve() {} disconnect() {} takeRecords() { return []; } }
globalThis.ResizeObserver ??= StubObserver;
globalThis.IntersectionObserver ??= StubObserver;
globalThis.MutationObserver ??= StubObserver;
globalThis.matchMedia ??= (q) => ({ matches: false, media: q, onchange: null, addEventListener() {}, removeEventListener() {}, addListener() {}, removeListener() {} });
globalThis.performance ??= { now: () => Date.now(), mark() {}, measure() {} };
globalThis.localStorage ??= (() => { const m = new Map(); return { getItem: (k) => m.has(k) ? m.get(k) : null, setItem: (k, v) => m.set(k, String(v)), removeItem: (k) => m.delete(k), clear: () => m.clear(), key: (i) => [...m.keys()][i] ?? null, get length() { return m.size; } }; })();
globalThis.sessionStorage ??= globalThis.localStorage;
const evTarget = () => ({ addEventListener() {}, removeEventListener() {}, dispatchEvent() { return true; } });
globalThis.screen ??= Object.assign(evTarget(), { width: 1400, height: 900, availWidth: 1400, availHeight: 900, colorDepth: 24, orientation: Object.assign(evTarget(), { angle: 0, type: "landscape-primary" }) });
globalThis.visualViewport ??= Object.assign(evTarget(), { width: 1400, height: 900, scale: 1, offsetLeft: 0, offsetTop: 0, pageLeft: 0, pageTop: 0 });
globalThis.devicePixelRatio ??= 1;
