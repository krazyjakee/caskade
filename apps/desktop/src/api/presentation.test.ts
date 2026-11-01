import { i18n } from "@caskade/i18n";
import { afterEach, describe, expect, it, vi } from "vitest";
import {
  compareTitles,
  findFolded,
  foldSearch,
  formatBytes,
  formatDay,
  formatDuration,
  formatInstallState,
  formatNumber,
  formatPercent,
  formatPlaytime,
  formatRelativeDay,
  formatReleaseDate,
  formatSize,
  formatSource,
  formatTime,
  nonPcPlatforms,
  platformLine,
  titleLetter,
} from "./presentation";

afterEach(() => {
  vi.useRealTimers();
});

/** Switches the language as the app does (the test setup puts English back). */
const inLanguage = (locale: string) => i18n.loadAndActivate({ locale, messages: {} });
/** Intl puts no-break spaces between a number and its unit. */
const plain = (text: string | null) => text?.replace(/\s/g, " ");

it("says how long without doubling words or hiding short sessions", () => {
  expect(formatDuration(0)).toBe("0m");
  expect(formatDuration(6)).toBe("<1m");
  expect(formatDuration(25 * 60)).toBe("25m");
  expect(formatDuration(4500)).toBe("1.3h");
  expect(formatPlaytime(0)).toBe("Not played");
  expect(formatPlaytime(6)).toBe("<1m played");
  expect(formatPlaytime(4500)).toBe("1.3h played");
});

it("names only the platforms that aren't PC", () => {
  expect(nonPcPlatforms("")).toBe("");
  expect(nonPcPlatforms("PC")).toBe("");
  expect(nonPcPlatforms("PC (Microsoft Windows) · Linux · pc_windows · Windows")).toBe("");
  expect(nonPcPlatforms("SNES")).toBe("SNES");
  expect(nonPcPlatforms("pc · nintendo_gba · Sony PlayStation")).toBe(
    "nintendo_gba · Sony PlayStation",
  );
  expect(nonPcPlatforms("PC Engine · PC-98")).toBe("PC Engine · PC-98");
});

it("counts a game's other platforms on its card", () => {
  expect(platformLine("PC")).toBe("");
  expect(platformLine("PC · Sega Genesis")).toBe("Sega Genesis");
  expect(platformLine("Nintendo Game Boy · Sega Genesis · PC · Sony PlayStation")).toBe(
    "Nintendo Game Boy +2",
  );
});

it("formats numbers, dates, sizes and durations in German", () => {
  inLanguage("de");
  expect(formatNumber(1234)).toBe("1.234");
  expect(formatReleaseDate("2026-10-28")).toBe("28.10.2026");
  expect(formatDay(new Date(2026, 9, 28, 12))).toBe("28.10.2026");
  expect(plain(formatSize(1.5 * 1024 ** 3))).toBe("1,5 GB");
  expect(plain(formatBytes(1.5 * 1024 ** 3))).toBe("1,5 GiB");
  expect(plain(formatPercent(42))).toBe("42 %");
  expect(formatDuration(4500)).toBe("1,3h");
});

it("translates units through the language's own data", () => {
  inLanguage("fr");
  expect(plain(formatSize(1.5 * 1024 ** 3))).toBe("1,5 Go");
  inLanguage("en");
  expect(plain(formatSize(1.5 * 1024 ** 3))).toBe("1.5 GB");
  expect(plain(formatSize(40 * 1024 ** 2))).toBe("40.0 MB");
  expect(plain(formatBytes(140 * 1024 ** 3))).toBe("140 GiB");
  expect(formatNumber(1234)).toBe("1,234");
});

it("says how long ago a day was in the language", () => {
  vi.useFakeTimers();
  vi.setSystemTime(new Date(2026, 9, 28, 15));
  const daysAgo = (days: number) => new Date(2026, 9, 28 - days, 9).toISOString();
  expect(formatRelativeDay(daysAgo(0))).toBe("Today");
  expect(formatRelativeDay(daysAgo(1))).toBe("Yesterday");
  expect(formatRelativeDay(daysAgo(3))).toBe("3 days ago");
  expect(formatRelativeDay(daysAgo(35))).toBe("5 weeks ago");
  expect(formatRelativeDay(daysAgo(0), { standalone: false })).toBe("today");
  inLanguage("de");
  expect(formatRelativeDay(daysAgo(1))).toBe("Gestern");
  expect(formatRelativeDay(daysAgo(3), { standalone: false })).toBe("vor 3 Tagen");
  expect(formatRelativeDay(daysAgo(100))).toBe("20.07.2026");
});

it("tells the time in the language's hours unless the setting says which", () => {
  const afternoon = new Date(2026, 9, 28, 14, 5);
  inLanguage("sv");
  expect(formatTime(afternoon)).toBe("14:05");
  expect(plain(formatTime(afternoon, { format: "12h" }))).toMatch(/^02:05 (em|e\.m\.)$/i);
  inLanguage("en-US");
  expect(plain(formatTime(afternoon))).toBe("02:05 PM");
  expect(formatTime(afternoon, { format: "24h" })).toBe("14:05");
});

it("sorts titles and files them under the language's letters", () => {
  const titles = ["Zork", "Åsa", "Ärlig", "Öga", "Apa", "Élan"];
  inLanguage("sv");
  expect(titles.toSorted(compareTitles)).toEqual(["Apa", "Élan", "Zork", "Åsa", "Ärlig", "Öga"]);
  expect(titles.map(titleLetter)).toEqual(["Z", "Å", "Ä", "Ö", "A", "E"]);
  inLanguage("de");
  expect(titles.toSorted(compareTitles)).toEqual(["Apa", "Ärlig", "Åsa", "Élan", "Öga", "Zork"]);
  expect(titleLetter("Ärger")).toBe("A");
  inLanguage("en");
  expect(titleLetter("élan")).toBe("E");
  expect(titleLetter("007")).toBe("#");
  expect(titleLetter("ゼルダ")).toBe("#");
});

it("searches without case or accents", () => {
  expect(foldSearch("Pokémon")).toBe("pokemon");
  expect(findFolded("Pokémon Snap", "pokemon")).toEqual([0, 7]);
  expect(findFolded("Pokémon", "KÉM")).toEqual([2, 5]);
  // A decomposed é is two characters; the match covers both.
  expect(findFolded("Poke\u0301mon", "pokemon")).toEqual([0, 8]);
  expect(findFolded("Zelda", "pokemon")).toBeNull();
  expect(findFolded("Zelda", "  ")).toBeNull();
});

it("names library sources and install states for people", () => {
  expect(formatSource("steam")).toBe("Steam");
  expect(formatSource("steam.shortcuts")).toBe("Steam shortcuts");
  expect(formatSource("romm:home")).toBe("RomM");
  expect(formatSource("caskade-storage")).toBe("Installs");
  expect(formatSource("desktop")).toBe("Desktop entries");
  expect(formatSource("plugin:my-store")).toBe("my-store");
  expect(formatSource("newstore")).toBe("Newstore");
  expect(formatInstallState("installed")).toBe("Installed");
  expect(formatInstallState("partial")).toBe("Partly installed");
});

describe("filesystem path display", () => {
  it("formats verbatim drive and UNC paths without changing device namespaces", async () => {
    const { formatPath } = await import("./presentation");
    expect(formatPath(String.raw`\\?\C:\Users\player\Games`)).toBe(
      String.raw`C:\Users\player\Games`,
    );
    expect(formatPath(String.raw`\\?\UNC\server\share\Games`)).toBe(
      String.raw`\\server\share\Games`,
    );
    for (const path of [
      "\\\\?\\Volume{guid}\\",
      String.raw`\\.\PhysicalDrive0`,
      "/games",
      String.raw`C:\Games`,
    ])
      expect(formatPath(path)).toBe(path);
  });
});
