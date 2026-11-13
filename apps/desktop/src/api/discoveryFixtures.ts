/** Explicit dev data only (`VITE_CASKADE_FIXTURES=1`): a finished Linux
 * discovery run with synthetic launchers. Never a fallback for failed IPC. */
import type { DiscoveryApi, DiscoverySnapshot } from "./discovery";

const preview: DiscoverySnapshot = {
  revision: 1,
  os: "linux",
  checks: [
    { id: "desktop", label: "Desktop entries", sources: ["desktop"] },
    { id: "xdg", label: "Flatpak and Snap application records", sources: ["flatpak", "snap"] },
    { id: "wine", label: "Wine and Proton prefixes", sources: ["prefixes"] },
  ],
  phase: "complete",
  launchers: [
    { source: "steam", found: true, path: "/fixture/Steam", version: null, kind: "native" },
    { source: "heroic", found: true, path: "/fixture/Heroic", version: "2.15", kind: "flatpak" },
    { source: "lutris", found: false, path: null, version: null, kind: null },
  ],
  sources: [
    { source: "steam", count: 14, durationMs: 42, warnings: [], error: null },
    { source: "heroic", count: 6, durationMs: 18, warnings: [], error: null },
    { source: "desktop", count: 4, durationMs: 7, warnings: [], error: null },
  ],
  found: 24,
  imported: 24,
  review: [
    {
      id: "review-1",
      title: "Fixture Tool",
      sources: ["desktop"],
      location: "/fixture/apps/tool.desktop",
      evidence: ["Desktop entry in the Game category"],
    },
  ],
  warnings: [],
  error: null,
  watching: true,
};

/** Every call answers `snapshot`. */
export function createDiscoveryFixtureApi(snapshot = preview): DiscoveryApi {
  return {
    status: async () => snapshot,
    launchers: async () => snapshot,
    scan: async () => snapshot,
    cancel: async () => snapshot,
    review: async () => snapshot,
    subscribe: async () => () => {},
  };
}
