/** Ports for the browser preview and tests: every game has the same two. */
import type { GamePorts, Port, PortsApi } from "./ports";

export const fixturePorts: Port[] = [
  {
    project: "github:zelda64recomp/zelda64recomp",
    name: "Zelda64Recomp",
    owner: "Zelda64Recomp",
    url: "https://github.com/Zelda64Recomp/Zelda64Recomp",
    gameTitle: "The Legend of Zelda: Majora's Mask",
    console: "Nintendo 64",
    matchedBy: "igdb",
    runsOn: ["linux", "macos-arm64", "windows"],
    here: {
      state: "installable",
      asset: "Zelda64Recompiled-v1.2.2-Linux-X64.zip",
      release: "v1.2.2",
      checked: true,
    },
    needs: [
      {
        label: "Majora's Mask US ROM",
        kind: "game",
        region: "USA",
        how: "inAppPicker",
        have: true,
      },
    ],
    notes: ["Only the North American (NTSC-U) version of the game is supported"],
    unsure: [],
    entryId: null,
  },
  {
    project: "github:harbourmasters/2ship2harkinian",
    name: "2ship2harkinian",
    owner: "HarbourMasters",
    url: "https://github.com/HarbourMasters/2ship2harkinian",
    gameTitle: "The Legend of Zelda: Majora's Mask",
    console: "Nintendo 64",
    matchedBy: "title",
    runsOn: ["windows"],
    here: { state: "otherSystems" },
    needs: [
      {
        label: "Majora's Mask ROM",
        kind: "game",
        region: null,
        how: "inAppPicker",
        have: true,
      },
    ],
    notes: [],
    unsure: ["Which ROM revisions are accepted"],
    entryId: null,
  },
];

export function createPortsFixtureApi(ports: Port[] = fixturePorts): PortsApi & {
  installed: string[];
} {
  const installed: string[] = [];
  const answer = (): GamePorts => ({
    status: {
      enabled: true,
      available: true,
      projects: 1877,
      checkedAtMs: Date.UTC(2026, 10, 2),
      creditUrl: "https://github.com/TanookiStudios/decomp-buddy",
    },
    ports: ports.map((port) =>
      installed.includes(port.project) ? { ...port, entryId: `port-${port.name}` } : port,
    ),
  });
  return {
    installed,
    game: async () => answer(),
    install: async (_gameId, project) => {
      installed.push(project);
      return `job-${project}`;
    },
  };
}
