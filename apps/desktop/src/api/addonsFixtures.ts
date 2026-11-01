/** A fixture catalogue for the browser preview and tests (#181). */
import type {
  AddonOutcome,
  AddonProgress,
  AddonRecords,
  AddonsApi,
  CatalogueItem,
  CatalogueView,
} from "./addons";
import type { AddonRecord } from "./bindings";

const sha = (c: string) => c.repeat(64);

export function fixtureCatalogueItems(): CatalogueItem[] {
  const plugin = (
    id: string,
    name: string,
    state: CatalogueItem["compatibility"]["state"],
    extra: Partial<CatalogueItem["entry"]> = {},
  ): CatalogueItem => ({
    entry: {
      id,
      kind: "plugin",
      name,
      author: "Caskade",
      summary: `${name}, as a fixture.`,
      description: "A fixture add-on. <b>Markup</b> shows as text.",
      license: "MIT",
      homepage: null,
      categories: ["stats"],
      screenshots: [{ url: "https://example.org/shot.png", alt: `${name} screenshot` }],
      releases: [
        {
          version: "0.2.0",
          api: "^0.1",
          os: [],
          modes: ["desktop"],
          permissions: ["library.read"],
          notes: "Faster charts.",
          published: "2026-10-29",
          artifact: { url: `https://example.org/${id}-0.2.0.zip`, sha256: sha("a"), size: 2048 },
        },
      ],
      deprecated: null,
      ...extra,
    },
    compatibility: {
      state,
      installedVersion: state === "updateAvailable" || state === "installed" ? "0.1.0" : null,
      release: state === "incompatible" ? null : null,
      reasons:
        state === "incompatible"
          ? [
              {
                code: "api",
                blocking: true,
                message: "0.2.0 needs plugin API ^9.0; this Caskade has 0.1.1",
              },
            ]
          : [],
    },
  });
  const items = [
    plugin("dev.caskade.playtime-stats", "Playtime stats", "available"),
    plugin("dev.caskade.howlongtobeat", "HowLongToBeat", "updateAvailable"),
    plugin("dev.example.future", "From the future", "incompatible"),
  ];
  for (const item of items) {
    if (item.compatibility.state !== "incompatible")
      item.compatibility.release = item.entry.releases[0] ?? null;
  }
  items.push({
    entry: {
      id: "caskade.paper",
      kind: "theme",
      name: "Paper",
      author: "Caskade",
      summary: "A light, printed look.",
      description: "Warm paper and ink-red accents.",
      license: "MIT",
      homepage: null,
      categories: ["light"],
      screenshots: [],
      releases: [
        {
          version: "1.0.0",
          api: null,
          os: [],
          modes: [],
          permissions: [],
          notes: "",
          published: null,
          artifact: { url: "https://example.org/paper-1.0.0.zip", sha256: sha("b"), size: 4096 },
        },
      ],
      deprecated: null,
    },
    compatibility: { state: "installed", installedVersion: "1.0.0", release: null, reasons: [] },
  });
  return items;
}

export function createAddonsFixture(items = fixtureCatalogueItems()): AddonsApi & {
  setOffline(offline: boolean): void;
} {
  let offline = false;
  let fetchedAtMs: number | null = Date.now() - 3_600_000;
  const addons: Record<string, AddonRecord> = {};
  let records: AddonRecords = { updates: "notify", addons };
  const listeners = new Set<(progress: AddonProgress) => void>();
  const view = (): CatalogueView =>
    structuredClone({
      status: {
        origin: "https://example.org/catalogue.json",
        fetchedAtMs,
        refreshing: false,
        error: offline ? "the catalogue couldn't be fetched: offline" : null,
        offline,
        verified: true,
        leftOut: 0,
      },
      items,
    });
  const find = (id: string) => {
    const item = items.find((candidate) => candidate.entry.id === id);
    if (!item) throw new Error(`${id} isn't in the catalogue`);
    return item;
  };
  return {
    async catalogue() {
      return view();
    },
    async refresh() {
      if (!offline) fetchedAtMs = Date.now();
      return view();
    },
    async media() {
      return null;
    },
    async install(id, _mode, accept, retry): Promise<AddonOutcome> {
      const item = find(id);
      const release = item.compatibility.release;
      if (!release)
        return {
          status: "failed",
          id,
          kind: "incompatible",
          message: item.compatibility.reasons.map((reason) => reason.message).join("; "),
          rolledBack: false,
        };
      const rejected = addons[id]?.rejected;
      if (rejected && rejected.version === release.version && !retry)
        return {
          status: "failed",
          id,
          kind: rejected.kind,
          message: rejected.message,
          rolledBack: false,
        };
      if (offline)
        return {
          status: "failed",
          id,
          kind: "network",
          message: "the download failed: offline",
          rolledBack: false,
        };
      const fresh = item.compatibility.state === "available";
      if (item.entry.kind === "plugin" && fresh && !accept)
        return {
          status: "needsReview",
          id,
          version: release.version,
          preview: {
            installedVersion: null,
            plugin: {
              id,
              name: item.entry.name,
              version: release.version,
              description: item.entry.summary,
              author: item.entry.author,
              folder: `/staging/${id}`,
              status: "disabled",
              errors: [],
              permissions: { network: [], fs: [], process: false, library: "read" },
              contributes: null,
              hasUi: true,
              hasBackend: false,
              failures: 0,
              lastError: null,
              settingsError: null,
              dev: false,
            },
          },
        };
      for (const phase of ["downloading", "checking", "installing"] as const)
        for (const listener of listeners)
          listener({ id, phase, received: release.artifact.size, total: release.artifact.size });
      if (id.includes("tampered")) {
        const message = "the download's checksum doesn't match the catalogue";
        addons[id] = {
          version: null,
          skipped: null,
          rejected: { version: release.version, kind: "integrity", message },
        };
        return { status: "failed", id, kind: "integrity", message, rolledBack: false };
      }
      if (id.includes("broken"))
        return {
          status: "failed",
          id,
          kind: "activation",
          message: "the plugin didn't start",
          rolledBack: true,
        };
      item.compatibility = {
        ...item.compatibility,
        state: "installed",
        installedVersion: release.version,
      };
      addons[id] = { version: release.version, skipped: null, rejected: null };
      return {
        status: "installed",
        id,
        version: release.version,
        needsConsent: false,
        restartRequired: false,
      };
    },
    async cancel() {
      return false;
    },
    async uninstall(id) {
      const item = find(id);
      item.compatibility = {
        ...item.compatibility,
        state: "available",
        installedVersion: null,
        release: item.compatibility.release ?? item.entry.releases[0] ?? null,
      };
      delete addons[id];
    },
    async records() {
      return structuredClone(records);
    },
    async skipUpdate(id, version) {
      addons[id] = {
        version: null,
        rejected: null,
        ...addons[id],
        skipped: version,
      };
      return structuredClone(records);
    },
    async setUpdatePreference(preference) {
      records = { ...records, updates: preference };
      return structuredClone(records);
    },
    subscribeProgress(listener) {
      listeners.add(listener);
      return () => {
        listeners.delete(listener);
      };
    },
    setOffline(next) {
      offline = next;
    },
  };
}
