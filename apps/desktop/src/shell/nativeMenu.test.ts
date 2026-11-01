import { afterEach, expect, it } from "vitest";
import { suppressNativeContextMenu } from "./nativeMenu";

let undo = () => {};
afterEach(() => {
  undo();
  document.body.innerHTML = "";
  window.getSelection()?.removeAllRanges();
});

/** Right-clicks `element`; true when WebKit's own menu would show. */
const nativeMenuShows = (element: Element, init: MouseEventInit = {}) =>
  element.dispatchEvent(
    new MouseEvent("contextmenu", { bubbles: true, cancelable: true, button: 2, ...init }),
  );

it("keeps WebKit's menu off everything but text fields and selected text", () => {
  undo = suppressNativeContextMenu(window, false);
  document.body.innerHTML = `
    <a href="/games/1">Game</a>
    <img alt="" />
    <input type="checkbox" />
    <input />
    <input type="search" />
    <textarea></textarea>
    <div contenteditable="true"><span>Note</span></div>
    <p>Some text</p>`;
  const [link, image, checkbox, input, search, textarea, span, text] = [
    "a",
    "img",
    '[type="checkbox"]',
    "input:not([type])",
    '[type="search"]',
    "textarea",
    "span",
    "p",
  ].map((selector) => document.querySelector(selector) as Element);
  expect(nativeMenuShows(link as Element)).toBe(false);
  expect(nativeMenuShows(image as Element)).toBe(false);
  expect(nativeMenuShows(checkbox as Element)).toBe(false);
  expect(nativeMenuShows(text as Element)).toBe(false);
  expect(nativeMenuShows(input as Element)).toBe(true);
  expect(nativeMenuShows(search as Element)).toBe(true);
  expect(nativeMenuShows(textarea as Element)).toBe(true);
  expect(nativeMenuShows(span as Element)).toBe(true);
  window.getSelection()?.selectAllChildren(text as Element);
  expect(nativeMenuShows(text as Element)).toBe(true);
});

it("shows WebKit's menu on Shift+right click in debug builds only", () => {
  document.body.innerHTML = "<p>Text</p>";
  const text = document.querySelector("p") as Element;
  undo = suppressNativeContextMenu(window, true);
  expect(nativeMenuShows(text, { shiftKey: true })).toBe(true);
  expect(nativeMenuShows(text)).toBe(false);
  undo();
  undo = suppressNativeContextMenu(window, false);
  expect(nativeMenuShows(text, { shiftKey: true })).toBe(false);
});

it("leaves Caskade's own menus to open", () => {
  undo = suppressNativeContextMenu(window, false);
  document.body.innerHTML = "<button>Card</button>";
  const card = document.querySelector("button") as Element;
  let opened = false;
  card.addEventListener("contextmenu", () => {
    opened = true;
  });
  nativeMenuShows(card);
  expect(opened).toBe(true);
});
