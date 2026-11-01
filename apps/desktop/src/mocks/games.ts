// Placeholder data so the UI can be developed in a plain browser.
// Replaced by typed Tauri commands (tauri-specta) once the backend exists.

export type GameStatus = "installed" | "not-installed" | "available" | "emulated";

/** Storefronts Caskade links out to. Caskade doesn't sell games itself. */
export const stores = {
  steam: { label: "Steam", searchUrl: "https://store.steampowered.com/search/?term=" },
  gog: { label: "GOG", searchUrl: "https://www.gog.com/en/games?query=" },
  epic: { label: "Epic Games", searchUrl: "https://store.epicgames.com/en-US/browse?q=" },
  itch: { label: "itch.io", searchUrl: "https://itch.io/search?q=" },
  humble: { label: "Humble", searchUrl: "https://www.humblebundle.com/store/search?search=" },
} satisfies Record<string, { label: string; searchUrl: string }>;

export type StoreId = keyof typeof stores;

export interface Game {
  id: string;
  title: string;
  genre: string;
  status: GameStatus;
  platform: string;
  /** Hue pair for the generated placeholder artwork. */
  art: [number, number];
  playtimeHours?: number;
  lastPlayed?: string;
  sizeGb?: number;
  /** Stores that sell a game that is available to buy. */
  stores?: StoreId[];
  description?: string;
}

export const games: Game[] = [
  {
    id: "aetherfall",
    title: "Aetherfall",
    genre: "Action RPG",
    status: "installed",
    platform: "PC",
    art: [230, 280],
    playtimeHours: 42,
    lastPlayed: "2h ago",
    sizeGb: 92,
    description:
      "Explore a shattered world of floating nations, lost technology and ancient magic. Forge your own path in a breathtaking adventure across the skies.",
  },
  {
    id: "voidstriders",
    title: "Voidstriders",
    genre: "Space Roguelike",
    status: "installed",
    platform: "PC",
    art: [270, 320],
    playtimeHours: 28,
    lastPlayed: "Yesterday",
    sizeGb: 6.4,
  },
  {
    id: "lumen-tactics",
    title: "Lumen Tactics",
    genre: "Strategy",
    status: "installed",
    platform: "PC",
    art: [195, 230],
    playtimeHours: 16,
    lastPlayed: "2 days ago",
    sizeGb: 12,
  },
  {
    id: "sands-of-kharak",
    title: "Sands of Kharak",
    genre: "Adventure",
    status: "installed",
    platform: "PC",
    art: [30, 15],
    playtimeHours: 11,
    lastPlayed: "4 days ago",
    sizeGb: 54,
  },
  {
    id: "neon-meridian",
    title: "Neon Meridian",
    genre: "Cyberpunk RPG",
    status: "not-installed",
    platform: "PC",
    art: [300, 260],
    playtimeHours: 9,
    lastPlayed: "5 days ago",
    sizeGb: 78,
  },
  {
    id: "thornwake",
    title: "Thornwake",
    genre: "Survival",
    status: "installed",
    platform: "PC",
    art: [210, 180],
    playtimeHours: 14,
    lastPlayed: "1 week ago",
    sizeGb: 22,
  },
  {
    id: "velgrim",
    title: "Velgrim",
    genre: "Soulslike",
    status: "available",
    platform: "PC",
    art: [0, 340],
    sizeGb: 60,
    stores: ["steam", "gog", "epic"],
    description:
      "Descend into a kingdom swallowed by a crimson moon. Master brutal, precise combat and outlast the knights who never stopped hunting.",
  },
  {
    id: "project-orpheus",
    title: "Project Orpheus",
    genre: "Sci-Fi Survival",
    status: "not-installed",
    platform: "PC",
    art: [220, 250],
    sizeGb: 34,
  },
  {
    id: "meadowgrove",
    title: "Meadowgrove",
    genre: "Farming Sim",
    status: "emulated",
    platform: "SNES",
    art: [100, 45],
    playtimeHours: 112,
    sizeGb: 1.2,
  },
  {
    id: "riftpunk",
    title: "Riftpunk",
    genre: "Base Building",
    status: "available",
    platform: "PC",
    art: [190, 25],
    sizeGb: 4.1,
    stores: ["steam", "itch"],
  },
  {
    id: "pixel-realms",
    title: "Pixel Realms",
    genre: "Retro RPG",
    status: "emulated",
    platform: "GBA",
    art: [140, 200],
    playtimeHours: 15,
    sizeGb: 0.3,
  },
  {
    id: "frozen-reach",
    title: "Frozen Reach",
    genre: "Survival",
    status: "installed",
    platform: "PC",
    art: [205, 225],
    playtimeHours: 9,
    sizeGb: 18,
  },
  {
    id: "cinder-and-bloom",
    title: "Cinder & Bloom",
    genre: "Life Sim",
    status: "not-installed",
    platform: "PC",
    art: [20, 350],
    playtimeHours: 12,
    sizeGb: 8.7,
  },
  {
    id: "starfall-racing",
    title: "Starfall Racing",
    genre: "Racing",
    status: "installed",
    platform: "PC",
    art: [240, 10],
    playtimeHours: 22,
    sizeGb: 16,
  },
  {
    id: "ironveil",
    title: "Ironveil",
    genre: "Tactical Shooter",
    status: "available",
    platform: "PC",
    art: [215, 40],
    sizeGb: 85,
    stores: ["steam", "epic"],
  },
  {
    id: "glimmerdeep",
    title: "Glimmerdeep",
    genre: "Metroidvania",
    status: "available",
    platform: "PC",
    art: [175, 290],
    sizeGb: 3.2,
    stores: ["itch", "gog", "humble"],
  },
  {
    id: "skyward-drift",
    title: "Skyward Drift",
    genre: "Flight Sim",
    status: "available",
    platform: "PC",
    art: [200, 160],
    sizeGb: 140,
    stores: ["steam"],
  },
  {
    id: "ashen-protocol",
    title: "Ashen Protocol",
    genre: "Stealth Action",
    status: "available",
    platform: "PC",
    art: [10, 260],
    sizeGb: 47,
    stores: ["steam", "gog", "humble"],
  },
];

/** Stands in for the store page URL the backend will provide. */
export function storeUrl(game: Game, store: StoreId): string {
  return stores[store].searchUrl + encodeURIComponent(game.title);
}
