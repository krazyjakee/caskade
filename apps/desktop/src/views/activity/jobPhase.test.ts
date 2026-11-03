import { i18n } from "@caskade/i18n";
import { expect, it } from "vitest";
import { jobPhase } from "./jobPhase";

it("names the core's job phases and nothing else", () => {
  expect(i18n._(jobPhase("download"))).toBe("Downloading");
  expect(i18n._(jobPhase("install"))).toBe("Installing");
  expect(i18n._(jobPhase("move"))).toBe("Moving");
  // No phase, a plugin's own, or an inherited name: the generic one.
  for (const phase of [undefined, "", "plugin.sync", "toString"])
    expect(i18n._(jobPhase(phase))).toBe("Working");
});
