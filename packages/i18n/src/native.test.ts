import type { ExtractedCatalogType } from "@lingui/conf";
import { generateMessageId } from "@lingui/message-utils/generateMessageId";
import { describe, expect, it } from "vitest";
import { addNative } from "./extract.ts";

const entry = (message: string, origin: string, context?: string) => ({
  [generateMessageId(message, context)]: {
    message,
    context,
    comments: [],
    origin: [[origin]] as [string][],
    placeholders: {},
  },
});

describe("the native UI's messages (#91)", () => {
  it("join the webview's entries, ui before app, and new ones go to app", () => {
    const ui = entry("Cancel", "packages/ui/src/Dialog.tsx") as ExtractedCatalogType;
    const app = {
      ...entry("Quick menu", "apps/desktop/src/input/GuideMenu.tsx"),
      ...entry("Delete", "apps/desktop/src/input/OnScreenKeyboard.tsx", "on-screen keyboard"),
      ...entry("{0, plural, one {# game} other {# games}}", "apps/desktop/src/views/Library.tsx"),
    } as ExtractedCatalogType;
    const before = Object.keys(app);
    addNative({ ui, app }, [
      { id: "Cancel", references: ["apps/native/src/a.rs"] },
      { id: "Quick menu", references: ["apps/native/src/b.rs"], comments: ["A menu"] },
      { id: "Delete", context: "on-screen keyboard", references: ["apps/native/src/c.rs"] },
      { id: "# game", plural: "# games", references: ["apps/native/src/d.rs"] },
      { id: "Only native", references: ["apps/native/src/e.rs"] },
      { id: "# file", plural: "# files", references: ["apps/native/src/e.rs"] },
    ]);
    const find = (catalog: ExtractedCatalogType, message: string, context?: string) =>
      catalog[generateMessageId(message, context)];
    expect(find(ui, "Cancel")?.origin).toEqual([
      ["packages/ui/src/Dialog.tsx"],
      ["apps/native/src/a.rs"],
    ]);
    expect(find(app, "Cancel")).toBeUndefined();
    expect(find(app, "Quick menu")?.comments).toEqual(["A menu"]);
    expect(find(app, "Delete", "on-screen keyboard")?.origin).toHaveLength(2);
    expect(find(app, "{0, plural, one {# game} other {# games}}")?.origin).toHaveLength(2);
    expect(find(app, "Only native")?.origin).toEqual([["apps/native/src/e.rs"]]);
    expect(find(app, "{count, plural, one {# file} other {# files}}")).toBeDefined();
    // The webview's entries stay where they were.
    expect(Object.keys(app).slice(0, before.length)).toEqual(before);
  });
});
