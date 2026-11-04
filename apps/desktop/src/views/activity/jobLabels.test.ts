import { i18n } from "@caskade/i18n";
import { expect, it } from "vitest";
import { jobKind, jobPhase } from "./jobLabels";

it("names the core's job kinds and nothing else", () => {
  expect(
    ["install", "uninstall", "move", "fetch", "verify", "update", "peerCopy", "moveLocation"].map(
      (kind) => i18n._(jobKind(kind)),
    ),
  ).toEqual([
    "Install",
    "Uninstall",
    "Move",
    "Download",
    "Verify",
    "Update",
    "Copy from another device",
    "Move location",
  ]);
  // A plugin's own kind, or an inherited name: the generic one.
  for (const kind of ["", "plugin.sync", "toString"]) expect(i18n._(jobKind(kind))).toBe("Job");
});

it("names the core's job phases and nothing else", () => {
  expect(i18n._(jobPhase("download"))).toBe("Downloading");
  expect(i18n._(jobPhase("install"))).toBe("Installing");
  expect(i18n._(jobPhase("move"))).toBe("Moving");
  // No phase, a plugin's own, or an inherited name: the generic one.
  for (const phase of [undefined, "", "plugin.sync", "toString"])
    expect(i18n._(jobPhase(phase))).toBe("Working");
});
