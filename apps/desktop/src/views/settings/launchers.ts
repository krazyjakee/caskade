import type { DiscoverySnapshot } from "../../api/discovery";

type Detected = DiscoverySnapshot["launchers"][number];

/** A launcher as people know it, and the discovery sources that read its
 * games (#48). The switch keys in `core.launchers.enabled` are those source
 * ids: one switch turns all of a launcher's sources off. */
export interface Launcher {
  id: string;
  /** Its own name: a product's, not translated. */
  name: string;
  sources: readonly string[];
}

const known: readonly Launcher[] = [
  { id: "steam", name: "Steam", sources: ["steam"] },
  { id: "heroic", name: "Heroic Games Launcher", sources: ["heroic"] },
  { id: "lutris", name: "Lutris", sources: ["lutris"] },
  { id: "epic", name: "Epic Games Launcher", sources: ["epic", "epic.app"] },
  // Rare keeps its games in Legendary's records.
  { id: "legendary", name: "Legendary / Rare", sources: ["legendary", "rare"] },
  { id: "gog", name: "GOG Galaxy", sources: ["gog"] },
  { id: "minigalaxy", name: "Minigalaxy", sources: ["minigalaxy"] },
  { id: "ea", name: "EA app", sources: ["ea"] },
  { id: "ubisoft", name: "Ubisoft Connect", sources: ["ubisoft"] },
  { id: "battlenet", name: "Battle.net", sources: ["battlenet"] },
  { id: "itch", name: "itch", sources: ["itch"] },
  { id: "amazon", name: "Amazon Games", sources: ["amazon"] },
  { id: "riot", name: "Riot Client", sources: ["riot"] },
  { id: "rockstar", name: "Rockstar Games Launcher", sources: ["rockstar"] },
  { id: "bottles", name: "Bottles", sources: ["bottles"] },
  { id: "faugus", name: "Faugus Launcher", sources: ["faugus"] },
];

export interface DetectedLauncher extends Launcher {
  /** Where discovery found it on this device; empty when it didn't. */
  found: Detected[];
}

/** The launchers discovery looks for on this device, found ones first, each
 * once however many of its sources reported. A source this list doesn't
 * know shows under its own id. */
export function detectedLaunchers(detected: readonly Detected[]): DetectedLauncher[] {
  const launchers = new Map<string, DetectedLauncher>();
  for (const row of detected) {
    const launcher = known.find((item) => item.sources.includes(row.source)) ?? {
      id: row.source,
      name: row.source,
      sources: [row.source],
    };
    const entry = launchers.get(launcher.id) ?? { ...launcher, found: [] };
    if (row.found) entry.found.push(row);
    launchers.set(launcher.id, entry);
  }
  const order = (launcher: DetectedLauncher) =>
    known.findIndex((item) => item.id === launcher.id) >>> 0;
  return [...launchers.values()].sort(
    (a, b) => Number(b.found.length > 0) - Number(a.found.length > 0) || order(a) - order(b),
  );
}

/** On unless one of its sources was turned off. */
export const launcherEnabled = (launcher: Launcher, enabled: Record<string, boolean>) =>
  launcher.sources.every((source) => enabled[source] !== false);

/** `enabled` with every source of `launcher` turned on or off. */
export const withLauncher = (
  enabled: Record<string, boolean>,
  launcher: Launcher,
  on: boolean,
): Record<string, boolean> => ({
  ...enabled,
  ...Object.fromEntries(launcher.sources.map((source) => [source, on])),
});
