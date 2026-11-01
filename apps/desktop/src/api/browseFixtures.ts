import type { BrowseApi, DirListing, Place } from "./browse";

/** A fake tree: folders end in `/`, everything else is a file. */
export const fixtureTree: readonly string[] = [
  "/home/player/",
  "/home/player/.config/",
  "/home/player/Downloads/",
  "/home/player/Downloads/setup-quest.exe",
  "/home/player/Downloads/starfield-patch.zip",
  "/home/player/Downloads/notes.txt",
  "/home/player/Games/",
  "/home/player/Games/Caskade/",
  "/home/player/Emulators/",
  "/home/player/Emulators/duckstation.AppImage",
  "/home/player/ROMs/",
  "/home/player/ROMs/psx/",
  "/home/player/ROMs/psx/Ape Escape.chd",
  "/home/player/ROMs/snes/",
  "/home/player/ROMs/snes/Chrono Trigger.sfc",
  ...Array.from({ length: 40 }, (_, i) => `/home/player/ROMs/snes/Homebrew ${i + 1}.sfc`),
  "/home/player/.bashrc",
  "/run/media/player/SD Card/",
  "/run/media/player/SD Card/Games/",
  "/run/media/player/SD Card/roms.zip",
];

const places: Place[] = [
  { label: "Home", path: "/home/player" },
  { label: "File system", path: "/" },
  { label: "SD Card", path: "/run/media/player/SD Card" },
];

const parentOf = (path: string) =>
  path === "/" ? null : path.slice(0, Math.max(1, path.lastIndexOf("/")));
const nameOf = (path: string) => path.slice(path.lastIndexOf("/") + 1);

/** The browse API over `tree` (folders end in `/`; their parents are
 * implied). */
export function createBrowseFixtureApi(tree: readonly string[] = fixtureTree): BrowseApi {
  const folders = new Set<string>(["/"]);
  const files = new Set<string>();
  for (const item of tree) {
    const folder = item.endsWith("/");
    const path = folder ? item.slice(0, -1) : item;
    (folder ? folders : files).add(path);
    for (let at = parentOf(path); at; at = parentOf(at)) folders.add(at);
  }
  return {
    async list(path, showHidden = false): Promise<DirListing> {
      const dir = path ?? "/home/player";
      if (!folders.has(dir)) throw new Error(`${dir} does not exist`);
      const entries = [...folders, ...files]
        .filter((item) => item !== "/" && parentOf(item) === dir)
        .filter((item) => showHidden || !nameOf(item).startsWith("."))
        .map((item) => ({ name: nameOf(item), path: item, isDir: folders.has(item) }))
        .sort(
          (a, b) =>
            Number(b.isDir) - Number(a.isDir) ||
            a.name.toLowerCase().localeCompare(b.name.toLowerCase()),
        );
      return { path: dir, parent: parentOf(dir), entries, places };
    },
  };
}
