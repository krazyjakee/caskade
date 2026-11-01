// Placeholder settings so the settings screen can be developed in a plain browser.
// Replaced by typed Tauri commands (JSON Schema-backed settings) once the backend exists.

type DetectStatus = "detected" | "not-found";

interface LauncherSource {
  id: string;
  name: string;
  status: DetectStatus;
  /** Hue for the placeholder logo tile. */
  hue: number;
  enabled: boolean;
}

export const launcherSources: LauncherSource[] = [
  { id: "steam", name: "Steam", status: "detected", hue: 210, enabled: true },
  { id: "heroic", name: "Heroic", status: "detected", hue: 265, enabled: true },
  { id: "lutris", name: "Lutris", status: "detected", hue: 30, enabled: true },
  { id: "epic", name: "Epic Games", status: "detected", hue: 220, enabled: true },
  { id: "gog", name: "GOG Galaxy", status: "detected", hue: 285, enabled: true },
  { id: "ea", name: "EA App", status: "detected", hue: 355, enabled: true },
  { id: "ubisoft", name: "Ubisoft Connect", status: "not-found", hue: 200, enabled: false },
  { id: "battlenet", name: "Battle.net", status: "detected", hue: 205, enabled: true },
  { id: "itch", name: "itch.io", status: "detected", hue: 0, enabled: true },
  { id: "amazon", name: "Amazon Games", status: "not-found", hue: 40, enabled: false },
];
