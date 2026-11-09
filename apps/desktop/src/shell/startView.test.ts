import { beforeEach, describe, expect, it } from "vitest";
import { rememberedView, rememberView, startHref, startLocation } from "./startView";

beforeEach(() => localStorage.clear());

describe("startHref", () => {
  it("opens Home by default and for anything unknown", () => {
    expect(startHref("home", "/library")).toBeUndefined();
    expect(startHref(undefined, "/library")).toBeUndefined();
    expect(startHref("store", undefined)).toBeUndefined();
  });

  it("opens the Library and Collections", () => {
    expect(startHref("library", undefined)).toBe("/library");
    expect(startHref("collections", undefined)).toBe("/collections");
  });

  it("opens the last view, or Home without one", () => {
    expect(startHref("last", "/collections/7")).toBe("/collections/7");
    expect(startHref("last", "/")).toBeUndefined();
    expect(startHref("last", undefined)).toBeUndefined();
  });
});

describe("the last view", () => {
  it("is remembered across starts", () => {
    rememberView("/library?filter=all&sort=title&layout=grid");
    expect(rememberedView()).toBe("/library?filter=all&sort=title&layout=grid");
    rememberView("/settings?tab=general");
    expect(rememberedView()).toBe("/settings?tab=general");
  });

  it("is only ever an app page", () => {
    rememberView("/activity");
    rememberView("https://example.com/");
    rememberView("//example.com/library");
    expect(rememberedView()).toBe("/activity");
    localStorage.setItem("caskade.lastView", "javascript:alert(1)");
    expect(rememberedView()).toBeUndefined();
  });
});

describe("startLocation", () => {
  const library = async () => "library";

  it("applies to a load on Home", async () => {
    await expect(startLocation({ pathname: "/", console: false, setting: library })).resolves.toBe(
      "/library",
    );
  });

  it("keeps a load elsewhere, such as webview recovery's", async () => {
    await expect(
      startLocation({ pathname: "/games/1", console: false, setting: library }),
    ).resolves.toBeUndefined();
  });

  it("leaves console mode its own start", async () => {
    await expect(
      startLocation({ pathname: "/", console: true, setting: library }),
    ).resolves.toBeUndefined();
  });

  it("opens the view remembered before this start", async () => {
    rememberView("/collections");
    await expect(
      startLocation({ pathname: "/", console: false, setting: async () => "last" }),
    ).resolves.toBe("/collections");
  });

  it("opens Home when the settings can't be read", async () => {
    await expect(
      startLocation({
        pathname: "/",
        console: false,
        setting: () => Promise.reject(new Error("gone")),
      }),
    ).resolves.toBeUndefined();
  });
});
