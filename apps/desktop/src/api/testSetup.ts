import { i18n, sourceLocale } from "@caskade/i18n";
import { beforeEach, vi } from "vitest";
import { previewFixtures } from "./previewFixtures";

// React Aria scrolls a menu to its focused item, on an animation frame that
// can outlive the test; jsdom's elements have no scrollTo. Not a spy, so it
// is still here then.
Element.prototype.scrollTo = () => {};

// Backup, Streaming, Store accounts, Shares and Devices use the dev
// preview's backends unless a test supplies its own; games have no ports.
previewFixtures(false);

beforeEach(() => {
  // Components render in English (the source text) unless a test switches
  // language, and a switch doesn't leak into the next test.
  i18n.loadAndActivate({ locale: sourceLocale, messages: {} });
  // Remembered view state (the library's filter, sort and layout, the settings tab) is per test.
  localStorage.clear();
  sessionStorage.clear();
  // Router scroll restoration is a browser concern; jsdom has no layout/scroll.
  vi.spyOn(window, "scrollTo").mockImplementation(() => {});
  // Shelves measure their containers; jsdom has no layout to observe.
  vi.stubGlobal(
    "ResizeObserver",
    class {
      observe() {}
      unobserve() {}
      disconnect() {}
    },
  );
  // jsdom can't play media (first-run setup's startup sound): silent stand-ins.
  vi.spyOn(HTMLMediaElement.prototype, "play").mockResolvedValue(undefined);
  vi.spyOn(HTMLMediaElement.prototype, "pause").mockImplementation(() => {});
});
