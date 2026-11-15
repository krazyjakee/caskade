// Run the exact injected script without a browser, network or desktop.
import assert from "node:assert/strict";
import { readFileSync } from "node:fs";
import test from "node:test";
import vm from "node:vm";

const script = readFileSync(new URL("./find.js", import.meta.url), "utf8");
const player = "https://cdn.example/Unity/index.html";
const html = "<!doctype html><html><canvas id='unity'></canvas></html>";
const settle = () => new Promise((resolve) => setImmediate(resolve));

function page(top) {
  const elements = [];
  const listeners = new Map();
  let blobId = 0;
  class PageURL extends URL {
    static createObjectURL() { return `blob:https://portal.example/${++blobId}`; }
    static revokeObjectURL() {}
  }
  class PageResponse {
    constructor(url, body = html, type = "text/html; charset=utf-8") {
      this.url = url;
      this.body = body;
      this.headers = new Headers({ "content-type": type });
    }
    async text() { return this.body; }
    async blob() { return new Blob([this.body], { type: this.headers.get("content-type") }); }
  }
  const context = vm.createContext({
    URL: PageURL, Response: PageResponse, Blob,
    document: { baseURI: "https://portal.example/", querySelectorAll: () => elements },
    getComputedStyle: () => ({ visibility: "visible", opacity: "1" }),
    addEventListener: (type, callback) => listeners.set(type, callback),
    postMessage: (data) => listeners.get("message")?.({ data, source: {} }),
    location: { href: "https://portal.example/loader/" },
    innerWidth: 960, innerHeight: 540,
  });
  context.window = context;
  context.top = top || context;
  vm.runInContext(script, context);
  return {
    context,
    response: (url = player, body = html, type) => new PageResponse(url, body, type),
    frame: (src) => elements.push({
      localName: "iframe", src,
      getBoundingClientRect: () => ({ width: 960, height: 540 }),
    }),
    found: () => JSON.parse(vm.runInContext("JSON.stringify(__caskadeFind())", context)),
  };
}

test("the Angry Birds wrapper's fetched text becomes the original Unity URL", async () => {
  const wrapper = page();
  // jsDelivr serves this game's HTML as text/plain. The wrapper explicitly
  // converts it to text/html when it builds the Blob.
  const response = wrapper.response(player, html, "text/plain; charset=utf-8");
  const text = await response.text();
  const blob = new Blob([text], { type: "text/html" });
  const href = wrapper.context.URL.createObjectURL(blob);
  wrapper.frame(href);
  // The real wrapper revokes it as soon as its frame loads.
  wrapper.context.URL.revokeObjectURL(href);
  await settle();
  assert.deepEqual(wrapper.found(), [{
    kind: "iframe", src: player, fromBlob: true, width: 960, height: 540,
  }]);
});

test("Response.blob works for a player from the wrapper's own origin", async () => {
  const wrapper = page();
  const source = "https://portal.example/player/index.html";
  const blob = await wrapper.response(source).blob();
  wrapper.frame(wrapper.context.URL.createObjectURL(blob));
  assert.equal(wrapper.found()[0].src, source);
  assert.equal(wrapper.found()[0].fromBlob, true);
});

test("a nested loader tells the top page where its Blob came from", async () => {
  const wrapper = page();
  const loader = page(wrapper.context);
  const text = await loader.response().text();
  const href = loader.context.URL.createObjectURL(new Blob([text], { type: "text/html" }));
  wrapper.frame(href);
  await settle();
  assert.equal(wrapper.found()[0].src, player);
});

test("transformed HTML and ambiguous identical responses stay unresolved", async () => {
  const wrapper = page();
  const text = await wrapper.response().text();
  wrapper.frame(wrapper.context.URL.createObjectURL(new Blob([text + "changed"], { type: "text/html" })));
  await wrapper.response("https://other.example/index.html").text();
  wrapper.frame(wrapper.context.URL.createObjectURL(new Blob([text], { type: "text/html" })));
  await settle();
  assert.ok(wrapper.found().every((frame) => !frame.fromBlob && frame.src.startsWith("blob:")));
});

test("non-HTML, oversized and non-HTTP responses cannot identify a player", async () => {
  for (const [source, body, type] of [
    [player, html, "application/octet-stream"],
    [player, "x".repeat(2 * 1024 * 1024 + 1), "text/html"],
    ["data:text/html,hello", html, "text/html"],
  ]) {
    const wrapper = page();
    const text = await wrapper.response(source, body, type).text();
    wrapper.frame(wrapper.context.URL.createObjectURL(new Blob([text], { type: "text/html" })));
    await settle();
    assert.equal(wrapper.found()[0].fromBlob, false);
  }
});

test("tracking forgets older HTML bodies instead of retaining an unbounded history", async () => {
  const wrapper = page();
  for (let i = 0; i < 5; i++) await wrapper.response(`${player}?v=${i}`, `${html}${i}`).text();
  wrapper.frame(wrapper.context.URL.createObjectURL(new Blob([`${html}0`], { type: "text/html" })));
  wrapper.frame(wrapper.context.URL.createObjectURL(new Blob([`${html}4`], { type: "text/html" })));
  await settle();
  assert.equal(wrapper.found()[0].fromBlob, false);
  assert.equal(wrapper.found()[1].src, `${player}?v=4`);
});

test("normal iframes retain their existing detection", () => {
  const wrapper = page();
  wrapper.frame(player);
  assert.equal(wrapper.found()[0].src, player);
  assert.equal(wrapper.found()[0].fromBlob, false);
});
