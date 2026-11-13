/** Synthetic ROM scanners for the fixture backend: a fixed file list per
 * scanner, grouped like the core pipeline would. Nothing is read from disk. */
import type {
  ProfileRef,
  ReviewChoice,
  ReviewGame_Serialize,
  RomScanner_Serialize,
  ScanJob_Serialize,
  ScanSummary_Serialize,
} from "./bindings";
import type { EmulationTransport } from "./emulationTransport";
import type { Reply } from "./reply";

const ok = async <T>(data: T): Promise<Reply<T>> => ({
  status: "ok",
  data: structuredClone(data),
});
interface Override {
  title?: string;
  platform?: string;
  regions?: string[];
  group?: string;
}
interface FixtureProfile {
  platforms: string[];
  extensions: string[];
  emulatorName: string;
  flatpak: boolean;
}

/** The fixture's subset of the §6.3 syntax: exact paths (`bios/`) and
 * `>` globs; regexes are only checked. */
function excluded(file: string, patterns: string[]) {
  return patterns.some((pattern) => {
    if (pattern.startsWith(">?") || pattern.startsWith("?")) return false;
    if (pattern.startsWith(">")) {
      const glob = pattern.slice(1).replace(/[.+^${}()|[\]\\]/g, "\\$&");
      const regex = new RegExp(`^${glob.replace(/\*/g, ".*").replace(/\?/g, ".")}$`, "i");
      return regex.test(file) || regex.test(file.split("/").pop() ?? file);
    }
    const path = pattern.replace(/\/$/, "").toLowerCase();
    const lower = file.toLowerCase();
    return lower === path || lower.startsWith(`${path}/`);
  });
}

/** `Title (Region) (Disc n).ext` → title, regions and disc label. */
function parse(file: string) {
  const name = file.replace(/\.[^.]+$/, "");
  const groups = [...name.matchAll(/\(([^)]*)\)/g)].map((match) => match[1] ?? "");
  const disc = groups.find((group) => /^Disc \d+/.test(group)) ?? null;
  const regions = groups.filter((group) => /^(USA|Europe|Japan)/.test(group));
  return { title: name.replace(/\s*\([^)]*\)/g, "").trim(), regions, disc };
}

export function createScanFixtures(
  profile: (ref: ProfileRef) => FixtureProfile | null,
  changed: () => void,
) {
  let nextId = 0;
  const files = new Map<string, string[]>([
    [
      "fixture-scanner",
      [
        "Chrono Trigger (USA).sfc",
        "Super Metroid (Japan, USA).sfc",
        "Final Fantasy III (USA) (Rev 1).sfc",
        "Star Ocean (Japan) (Disc 1).sfc",
        "Star Ocean (Japan) (Disc 2).sfc",
        "Star Ocean Bonus (Japan).sfc",
      ],
    ],
  ]);
  const scanners = new Map<string, RomScanner_Serialize>([
    [
      "fixture-scanner",
      {
        id: "fixture-scanner",
        name: "SNES collection",
        folder: "/fixture/ROMs/snes",
        platform: null,
        profile: { emulatorId: "fixture-retroarch", profileId: "fixture-snes" },
        launchWith: { type: "platform_default" },
        recursive: true,
        scanArchives: false,
        skipCloudPlaceholders: true,
        exclusions: ["bios/"],
        inLibraryUpdate: true,
      },
    ],
  ]);
  let globalExclusions: string[] = [];
  const overrides = new Map<string, Map<string, Override>>();
  const imported = new Map<string, Map<string, string>>();
  const state = new Map<string, { lastScanMs: number; games: number }>();
  const jobs = new Map<string, ScanJob_Serialize>();
  const listeners = new Set<(job: ScanJob_Serialize) => void>();
  const publish = (job: ScanJob_Serialize) => {
    job.revision++;
    jobs.set(job.id, job);
    for (const listener of listeners) listener(structuredClone(job));
    return job;
  };

  function find(scanner: RomScanner_Serialize): ReviewGame_Serialize[] {
    const info = profile(scanner.profile);
    if (!info) throw new Error("the scanner's profile does not exist");
    const platform = scanner.platform ?? info.platforms[0] ?? "unknown";
    const stored = overrides.get(scanner.id) ?? new Map<string, Override>();
    const owners = imported.get(scanner.id) ?? new Map<string, string>();
    const groups = new Map<string, ReviewGame_Serialize>();
    for (const file of files.get(scanner.id) ?? []) {
      if (scanner.exclusions.includes(file) || excluded(file, globalExclusions)) continue;
      const parsed = parse(file);
      const choice = stored.get(file) ?? {};
      const key = choice.group ?? `auto:${parsed.title}`;
      const game = groups.get(key) ?? {
        title: parsed.title,
        platform,
        regions: parsed.regions,
        roms: [],
        files: [],
        gameId: null,
      };
      game.roms.push({ path: file, name: parsed.disc, size: 1024 });
      game.files.push(file);
      game.title = choice.title ?? game.title;
      game.platform = choice.platform ?? game.platform;
      game.regions = choice.regions ?? game.regions;
      game.gameId = game.gameId ?? owners.get(file) ?? null;
      groups.set(key, game);
    }
    return [...groups.values()];
  }

  function importNow(scanner: RomScanner_Serialize): ScanSummary_Serialize {
    const games = find(scanner);
    const owners = imported.get(scanner.id) ?? new Map<string, string>();
    let added = 0;
    const ids = games.map((game) => {
      const known = game.roms.map((rom) => owners.get(rom.path)).find(Boolean);
      const id = known ?? `fixture-rom-game-${++nextId}`;
      if (!known) added++;
      for (const rom of game.roms) owners.set(rom.path, id);
      return id;
    });
    imported.set(scanner.id, owners);
    state.set(scanner.id, { lastScanMs: Date.now(), games: games.length });
    changed();
    return {
      scannerId: scanner.id,
      added,
      updated: 0,
      missing: 0,
      games: ids,
      warnings: [],
      error: null,
    };
  }

  const scanner = (id: string) => {
    const found = scanners.get(id);
    if (!found) throw new Error(`unknown ROM scanner ${id}`);
    return found;
  };
  const transport: Pick<
    EmulationTransport,
    | "romScannersList"
    | "romScannerSave"
    | "romScannerDelete"
    | "romScanStart"
    | "romScanJobs"
    | "romScanCancel"
    | "romScanSubmit"
    | "romScanAll"
    | "romExclusions"
    | "romExclusionsSet"
  > = {
    romScannersList: () =>
      ok(
        [...scanners.values()].map((row) => {
          const info = profile(row.profile);
          return {
            scanner: row,
            games: state.get(row.id)?.games ?? 0,
            lastScanMs: state.get(row.id)?.lastScanMs ?? null,
            platforms: row.platform ? [row.platform] : (info?.platforms ?? []),
            extensions: info?.extensions ?? [],
            problem: info ? null : "the scanner's profile does not exist",
            flatpakHint: info?.flatpak
              ? `${info.emulatorName} runs as a Flatpak and can't see this folder by itself. Caskade shares it read-only when it launches a game; the emulator's own file browser won't show it.`
              : null,
          };
        }),
      ),
    async romScannerSave(input) {
      if (!input.name.trim()) throw new Error("scanner name is required");
      if (!input.folder.startsWith("/")) throw new Error("scanner folder must be absolute");
      if (!profile(input.profile)) throw new Error("the scanner's profile does not exist");
      const saved: RomScanner_Serialize = {
        ...structuredClone(input),
        id: input.id || `fixture-scanner-${++nextId}`,
        platform: input.platform ?? null,
      };
      scanners.set(saved.id, saved);
      if (!files.has(saved.id)) files.set(saved.id, ["Sample Game (USA).sfc"]);
      changed();
      return ok(saved);
    },
    async romScannerDelete(id) {
      if (!scanners.delete(id)) throw new Error(`unknown ROM scanner ${id}`);
      changed();
      return ok(null);
    },
    async romScanStart(scannerId, mode) {
      const target = scanner(scannerId);
      const job = publish({
        id: `fixture-scan-${++nextId}`,
        revision: 0,
        scannerId,
        mode,
        status: "running",
        progress: { phase: "enumerate", files: 0, games: 0 },
        review: [],
        summary: null,
        warnings: [],
        error: null,
      });
      const finished = structuredClone(job);
      if (mode === "review") {
        finished.status = "review";
        finished.review = find(target);
      } else {
        finished.status = "done";
        finished.summary = importNow(target);
      }
      finished.progress = null;
      // Settles on the next tick, like a background scan.
      setTimeout(() => publish(finished), 0);
      return ok(job);
    },
    romScanJobs: () => ok([...jobs.values()].reverse()),
    async romScanCancel(id) {
      const job = jobs.get(id);
      if (!job) throw new Error(`unknown ROM scan ${id}`);
      return ok(publish({ ...job, status: "cancelled" }));
    },
    async romScanSubmit(id, revision, choices: ReviewChoice[]) {
      const job = jobs.get(id);
      if (job?.status !== "review") throw new Error("this scan is not waiting for a review");
      if (job.revision !== revision) throw new Error("the scan changed; review the latest results");
      const target = scanner(job.scannerId);
      const stored = overrides.get(target.id) ?? new Map<string, Override>();
      for (const choice of choices) {
        if (!choice.include) {
          target.exclusions = [...new Set([...target.exclusions, ...choice.roms])];
          continue;
        }
        const origin = job.review.find((game) =>
          game.roms.some((rom) => rom.path === choice.roms[0]),
        );
        const regrouped =
          origin?.roms.length !== choice.roms.length ||
          origin.roms.some((rom, index) => rom.path !== choice.roms[index]);
        for (const path of choice.roms)
          stored.set(path, {
            ...stored.get(path),
            ...(regrouped && { group: choice.roms[0] }),
            ...(choice.title !== origin?.title && { title: choice.title }),
            ...(choice.platform !== origin?.platform && { platform: choice.platform }),
          });
      }
      overrides.set(target.id, stored);
      return ok(
        publish({ ...job, status: "done", mode: "import", summary: importNow(target), review: [] }),
      );
    },
    romScanAll: async () =>
      ok([...scanners.values()].filter((row) => row.inLibraryUpdate).map(importNow)),
    romExclusions: () => ok(globalExclusions),
    async romExclusionsSet(patterns) {
      const kept = [...new Set(patterns.map((pattern) => pattern.trim()).filter(Boolean))];
      for (const pattern of kept) {
        const regex = pattern.startsWith(">?")
          ? pattern.slice(2)
          : pattern.startsWith("?")
            ? pattern.slice(1)
            : null;
        try {
          if (regex !== null) new RegExp(regex);
        } catch {
          throw new Error(`exclusion ${JSON.stringify(pattern)} is not a valid regex`);
        }
      }
      globalExclusions = kept;
      changed();
      return ok(globalExclusions);
    },
  };
  return {
    transport,
    /** ROMs imported for a platform, for the Platforms section. */
    romCount: (platform: string) =>
      [...scanners.values()]
        .filter((row) => (row.platform ?? profile(row.profile)?.platforms[0]) === platform)
        .reduce((total, row) => total + (imported.get(row.id)?.size ?? 0), 0),
    subscribe: async (listener: (job: ScanJob_Serialize) => void) => {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
  };
}
