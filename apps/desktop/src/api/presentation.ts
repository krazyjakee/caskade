/** How the webview shows numbers, sizes, times, dates and titles (#218):
 * in the app's language, as the core resolves it (#213), never the OS's
 * or hard-coded English. Everything reads the active Lingui locale when it
 * runs, so a component that formats re-renders in a new language through
 * `useLingui()`, which every marked-up component already calls. */
import { i18n, sourceLocale } from "@caskade/i18n";
import { msg, t } from "@lingui/core/macro";

/** The language numbers, dates and titles follow: the app's, in the OS's
 * region when the OS speaks it (English in the UK has en-GB's dates and
 * 24 hours), since the core resolves a language without one. */
export function formatLocale(): string {
  const locale = i18n.locale || sourceLocale;
  if (locale.includes("-")) return locale;
  const regional = globalThis.navigator?.languages?.find(
    (tag) => tag.toLowerCase().split("-")[0] === locale.toLowerCase(),
  );
  return regional ?? locale;
}

// One formatter per language and options; making them is the slow part.
const formatters = new Map<string, unknown>();
function cached<T>(kind: string, options: object, make: (locale: string) => T): T {
  const locale = formatLocale();
  const key = `${kind}|${locale}|${JSON.stringify(options)}`;
  let formatter = formatters.get(key) as T | undefined;
  if (formatter === undefined) {
    formatter = make(locale);
    formatters.set(key, formatter);
  }
  return formatter;
}
const numbers = (options: Intl.NumberFormatOptions = {}) =>
  cached("number", options, (locale) => new Intl.NumberFormat(locale, options));
const dates = (options: Intl.DateTimeFormatOptions) =>
  cached("date", options, (locale) => new Intl.DateTimeFormat(locale, options));

/** A count or other number: `1,234` in English, `1.234` in German. */
export function formatNumber(value: number, options?: Intl.NumberFormatOptions): string {
  return numbers(options).format(value);
}

/** A percentage from 0 to 100, rounded: `42%`, `42 %` in German; `—`
 * without one. */
export function formatPercent(value: number | null): string {
  return value === null
    ? "—"
    : formatNumber(value / 100, { style: "percent", maximumFractionDigits: 0 });
}

const SIZE_UNITS = ["kilobyte", "megabyte", "gigabyte", "terabyte", "petabyte"];
/** A game's or a download's size in the language's units and decimals:
 * `1.5 GB`, `1,5 GB` in German, `1,5 Go` in French; `0.0 kB` at the
 * least. */
export function formatSize(bytes: number | null): string {
  if (bytes === null)
    return t({ message: "Size unknown", comment: "A game whose size isn't known" });
  let value = Math.max(0, bytes) / 1024;
  let unit = 0;
  while (value >= 1024 && unit < SIZE_UNITS.length - 1) {
    value /= 1024;
    unit++;
  }
  return formatNumber(value, {
    style: "unit",
    unit: SIZE_UNITS[unit],
    unitDisplay: "short",
    minimumFractionDigits: 1,
    maximumFractionDigits: 1,
  });
}

const BINARY_UNITS = [
  msg({ message: "B", context: "byte unit" }),
  msg({ message: "KiB", context: "byte unit" }),
  msg({ message: "MiB", context: "byte unit" }),
  msg({ message: "GiB", context: "byte unit" }),
  msg({ message: "TiB", context: "byte unit" }),
  msg({ message: "PiB", context: "byte unit" }),
];
/** Binary bytes, as Storage and Statistics show them: `140 GiB`,
 * `1.8 TiB`, `1,8 TiB` in German. Intl has no binary units, so their
 * symbols are translated messages. */
export function formatBytes(bytes: number | null): string {
  if (bytes === null) return t({ message: "Unknown", context: "size" });
  let value = Math.max(0, bytes);
  let unit = 0;
  while (value >= 1024 && unit < BINARY_UNITS.length - 1) {
    value /= 1024;
    unit++;
  }
  const digits = unit === 0 || value >= 100 ? 0 : 1;
  const size = formatNumber(value, {
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  });
  const symbol = BINARY_UNITS[unit];
  if (!symbol) return size;
  const name = t(symbol);
  return t({ message: `${{ size }} ${{ unit: name }}`, comment: "A size and its unit: 1.5 GiB" });
}

const unit = (value: number, name: string, digits = 0) =>
  formatNumber(value, {
    style: "unit",
    unit: name,
    unitDisplay: "narrow",
    minimumFractionDigits: digits,
    maximumFractionDigits: digits,
  });
/** A length of time: `0m`, `<1m`, `42m`, `3.5h` (`3,5h` in German). */
export function formatDuration(seconds: number) {
  if (seconds <= 0) return unit(0, "minute");
  if (seconds < 60)
    return t({
      message: `<${{ duration: unit(1, "minute") }}`,
      comment: "Under a minute of playtime: <1m",
    });
  if (seconds < 3600) return unit(Math.floor(seconds / 60), "minute");
  return unit(Math.round(seconds / 360) / 10, "hour", 1);
}
/** A game's total playtime: `3.5h played`, or `Not played` before any. */
export function formatPlaytime(seconds: number) {
  if (seconds <= 0) return t({ message: "Not played", comment: "A game's playtime, before any" });
  const duration = formatDuration(seconds);
  return t({ message: `${duration} played`, comment: "A game's total playtime: 3.5h played" });
}

/** Stores and launchers by their own names. */
const STORE_NAMES: Record<string, string> = {
  amazon: "Amazon",
  battlenet: "Battle.net",
  ea: "EA app",
  epic: "Epic",
  "epic.app": "Epic",
  flatpak: "Flatpak",
  gog: "GOG",
  heroic: "Heroic",
  itch: "itch.io",
  legendary: "Epic",
  lutris: "Lutris",
  minigalaxy: "GOG",
  steam: "Steam",
  ubisoft: "Ubisoft",
  xbox: "Xbox",
};
/** A library source (an entry's `source`) for people: `romm:<server>` is
 * RomM, `steam.shortcuts` Steam shortcuts, and so on. */
export function formatSource(source: string): string {
  if (source.startsWith("romm:")) return "RomM";
  if (source.startsWith("plugin:")) return source.slice("plugin:".length);
  if (source.startsWith("store:"))
    return t({ message: "Store account", context: "library source" });
  if (source.startsWith("stream:")) return t({ message: "Streaming", context: "library source" });
  if (source.startsWith("peer:"))
    return t({ message: "Another of your devices", context: "library source" });
  switch (source) {
    case "steam.shortcuts":
      // Games added to Steam by hand.
      return t`Steam shortcuts`;
    case "desktop":
      return t({ message: "Desktop entries", context: "library source" });
    case "roms":
      return t({ message: "ROM folders", context: "library source" });
    case "web":
      return t({ message: "Web games", context: "library source" });
    case "caskade-storage":
    case "caskade-handoff":
      return t({ message: "Installs", context: "library source" });
    case "manual":
      return t({ message: "Added by hand", context: "library source" });
  }
  return STORE_NAMES[source] ?? source.charAt(0).toUpperCase() + source.slice(1);
}

/** A release's install state (an entry's `state`) for people. */
export function formatInstallState(state: string): string {
  switch (state) {
    case "installed":
      return t({ message: "Installed", context: "release state" });
    case "updating":
      return t({ message: "Updating", context: "release state" });
    case "partial":
      return t({ message: "Partly installed", context: "release state" });
    case "broken":
      return t({ message: "Broken", context: "release state" });
  }
  return state;
}

/** A `YYYY-MM-DD` release date as a calendar date, without a time zone
 * shifting the day. */
export function formatReleaseDate(date: string) {
  const value = new Date(`${date}T00:00:00Z`);
  if (Number.isNaN(value.getTime())) return date;
  return dates({ dateStyle: "medium", timeZone: "UTC" }).format(value);
}
/** A month of a year: `Jun 2027`, `juni 2027` in Swedish. */
export function formatMonth(year: number, month: number) {
  return dates({ month: "short", year: "numeric", timeZone: "UTC" }).format(
    Date.UTC(year, month - 1, 1),
  );
}
/** A day: `28 Oct 2026`, `28.10.2026` in German. */
export function formatDay(value: Date | number) {
  return dates({ dateStyle: "medium" }).format(value);
}
/** A day and time: `28 Oct 2026, 14:05`. */
export function formatDateTime(value: Date | number) {
  return dates({ dateStyle: "medium", timeStyle: "short" }).format(value);
}
/** When the last play started; `Never played` without one. */
export function formatDate(iso: string | null) {
  if (!iso) return t({ message: "Never played", comment: "A game's last play, before any" });
  const date = new Date(iso);
  if (Number.isNaN(date.getTime()))
    return t({ message: "Date unavailable", comment: "A date that couldn't be read" });
  return formatDateTime(date);
}
/** The Time format setting (`core.general.timeFormat`): the language's
 * own clock, or 12 or 24 hours whatever the language. */
export type TimeFormat = "system" | "12h" | "24h";
/** A time of day: `14:05`, or `02:05 PM` where the language (or the
 * setting) uses 12 hours; `seconds` adds them. */
export function formatTime(
  value: Date | number,
  { format = "system", seconds = false }: { format?: TimeFormat; seconds?: boolean } = {},
) {
  const options: Intl.DateTimeFormatOptions = { hour: "2-digit", minute: "2-digit" };
  if (seconds) options.second = "2-digit";
  if (format === "12h") options.hour12 = true;
  if (format === "24h") options.hourCycle = "h23";
  return dates(options).format(value);
}

const DAY = 86_400_000;
/** "Today", "Yesterday", "3 days ago", "5 weeks ago", or the date, in the
 * language (`Intl.RelativeTimeFormat`). `standalone: false` leaves the
 * language's own case for use inside a sentence ("updated today"). */
export function formatRelativeDay(iso: string, { standalone = true } = {}) {
  const then = new Date(iso);
  if (Number.isNaN(then.getTime())) return null;
  const days = Math.max(
    0,
    Math.floor((new Date().setHours(0, 0, 0, 0) - new Date(then).setHours(0, 0, 0, 0)) / DAY),
  );
  if (days >= 60) return formatDay(then);
  const relative = cached(
    "relative",
    {},
    (locale) => new Intl.RelativeTimeFormat(locale, { numeric: "auto" }),
  );
  const text =
    days < 14 ? relative.format(-days, "day") : relative.format(-Math.floor(days / 7), "week");
  return standalone ? text.charAt(0).toLocaleUpperCase(formatLocale()) + text.slice(1) : text;
}

/** Titles in the language's alphabetical order, as the core sorts them:
 * in Swedish Å, Ä and Ö come after Z, in German Ä goes with A. */
export function compareTitles(a: string, b: string): number {
  return cached("collator", {}, (locale) => new Intl.Collator(locale)).compare(a, b);
}
/** Whether two letters are the same letter of the language's alphabet,
 * accents and case aside where the language does (É is E in English, Å
 * isn't A in Swedish). */
function sameLetter(a: string, b: string) {
  return (
    cached("letters", {}, (locale) => new Intl.Collator(locale, { sensitivity: "base" })).compare(
      a,
      b,
    ) === 0
  );
}
/** A to Z, the letters every alphabet bar has. */
export const LATIN = [..."ABCDEFGHIJKLMNOPQRSTUVWXYZ"];
/** The letter a title is filed under in the alphabet bar and letter
 * paging, as the core's `title_letters` finds it: its first letter in
 * capitals, as the A to Z letter it counts as in the language where it is
 * one (É is E in English, Ä is A in German, but Å is Å in Swedish), or `#`
 * for digits, symbols and scripts without capitals. */
export function titleLetter(title: string): string {
  const locale = formatLocale();
  const first = [...title.trim()][0] ?? "";
  const upper = [...first.toLocaleUpperCase(locale)][0] ?? "";
  if (!upper || upper === upper.toLocaleLowerCase(locale)) return "#";
  if (LATIN.includes(upper)) return upper;
  return LATIN.find((letter) => sameLetter(letter, upper)) ?? upper;
}

/** Text as search compares it: lower case, without accents, so "pokemon"
 * finds "Pokémon" as the core's search does. */
export function foldSearch(text: string): string {
  return text
    .normalize("NFD")
    .replace(/\p{M}+/gu, "")
    .toLocaleLowerCase(formatLocale());
}
/** Where `query` first occurs in `text`, case and accents aside, as
 * `[start, end)` indexes into `text`; null when it doesn't. */
export function findFolded(text: string, query: string): [number, number] | null {
  const wanted = foldSearch(query.trim());
  if (!wanted) return null;
  // Each folded character's place in `text`, and where it ends.
  let folded = "";
  const starts: number[] = [];
  const ends: number[] = [];
  let at = 0;
  for (const char of text) {
    const part = foldSearch(char);
    for (let i = 0; i < part.length; i++) {
      starts.push(at);
      ends.push(at + char.length);
    }
    folded += part;
    at += char.length;
  }
  const found = folded.indexOf(wanted);
  if (found < 0) return null;
  return [starts[found] ?? 0, ends[found + wanted.length - 1] ?? text.length];
}

const PC_WORDS = new Set(["pc", "microsoft", "windows", "win", "linux", "gnu"]);
/** A game's platforms (`GameSummary.platform`) less the PC ones (`pc`,
 * Windows, Linux, "PC (Microsoft Windows)"), which a card leaves unsaid.
 * "PC Engine" and "PC-98" are consoles and computers of their own. */
export function nonPcPlatforms(platform: string) {
  return platform
    .split(" · ")
    .filter((name) => {
      const words = name
        .toLowerCase()
        .split(/[^a-z0-9]+/)
        .filter(Boolean);
      return words.length > 0 && !words.every((word) => PC_WORDS.has(word));
    })
    .join(" · ");
}
/** A card's platform line: its one platform that isn't PC, or the first
 * and how many more, for a game released on several. */
export function platformLine(platform: string) {
  const [first, ...more] = nonPcPlatforms(platform).split(" · ");
  return more.length > 0 ? `${first} +${more.length}` : (first ?? "");
}

/** A filesystem path for display only. Keep the original for IPC, picker
 * defaults and identity. Only DOS drive and UNC verbatim paths have a
 * conventional equivalent; volume GUID and device namespaces do not. */
export function formatPath(path: string): string {
  if (/^\\\\\?\\[a-z]:\\/i.test(path)) return path.slice(4);
  if (path.slice(0, 8).toUpperCase() === "\\\\?\\UNC\\") return `\\\\${path.slice(8)}`;
  return path;
}
