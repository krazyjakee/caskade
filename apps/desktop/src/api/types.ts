/** UI projections. The generated IPC DTOs are mapped at the transport boundary. */
import type { AccountsApi, PinCheck } from "./accounts";
import type { ActionsApi } from "./actions";
import type { GameEditApi } from "./gameEdit";
import type { IgdbAgeRating } from "./igdb";
import type { ManualGamesApi } from "./manualGames";
import type { SavesApi } from "./saves";
import type { ScriptsApi } from "./scripts";

export type LibraryFilter =
  | "all"
  | "installed"
  | "owned"
  | "recent"
  | "emulated"
  | "favorites"
  | "web";
export type LibrarySort = "title" | "recent" | "playtime" | "added" | "released" | "popular";
export type LibraryLayout = "grid" | "list";
export interface LibraryQuery {
  filter: LibraryFilter;
  sort: LibrarySort;
  query?: string;
  /** Only games on this platform. */
  platform?: string;
}
export interface GameSummary {
  /** This account needs an admin PIN to play; searching grants nothing. */
  adminPinNeeded?: boolean;
  id: string;
  title: string;
  genre: string;
  platform: string;
  status: "installed" | "not-installed" | "emulated";
  favorite: boolean;
  playtimeSeconds: number;
  lastPlayedAt: string | null;
  /** `YYYY-MM-DD`. */
  releaseDate: string | null;
  sizeBytes: number | null;
  coverGrid: string | null;
  coverList: string | null;
  /** The 96px icon for compact rows. */
  icon: string | null;
}
export interface GameEntry {
  id: string;
  source: string;
  state: string;
  location: string | null;
  canLaunch: boolean;
  /** Launches through an emulator; readiness decides whether Play works. */
  emulated: boolean;
}
export interface Screenshot {
  /** A thumbnail for lists. */
  thumb: string;
  /** The full-size image, for the gallery. */
  full: string;
}
export interface GameDetails extends GameSummary {
  description: string;
  tags: string[];
  hero: string | null;
  /** Screenshots (RomM's or IGDB's, from the core's artwork cache). */
  screenshots: Screenshot[];
  entries: GameEntry[];
  /** The entry Play starts, when the user chose one. */
  defaultEntryId?: string | null;
  /** The game's own page on the web, live: a web game's page, also when
   * it plays from an offline copy. */
  website?: string | null;
}
export interface PlaySession {
  id: string;
  gameId: string;
  entryId: string;
  startedAt: string;
  endedAt: string | null;
  durationSeconds: number;
}
/** A platform's games in one library filter. */
export interface PlatformSummary {
  name: string;
  games: number;
  playtimeSeconds: number;
  lastPlayedAt: string | null;
}
/** Where the games starting with `letter` (`#`: anything but A to Z) begin
 * in a query sorted by title. */
export interface TitleLetter {
  letter: string;
  offset: number;
}
export interface LibraryStats {
  total: number;
  installed: number;
  recent: number;
  emulated: number;
  /** Games played in the game window (the Web tab). */
  web: number;
  favorites: number;
  /** Owned by a store account and not installed (#170). */
  owned: number;
  playtimeSeconds: number;
}
export interface GamePage {
  items: GameSummary[];
  total: number;
}
export type LibraryEvent =
  | { type: "library.changed" }
  /** Another account signed in, or nobody: its collections replace the last one's. */
  | { type: "account.changed" }
  | { type: "game.started" | "game.exited"; gameId: string; entryId: string; sessionId: string };
export interface LaunchTicket {
  gameId: string;
  entryId: string;
  sessionId: string;
}
/** Answers to an ask-at-launch target and the disc picker. */
export interface LaunchChoice {
  profile?: { emulatorId: string; profileId: string } | null;
  disc?: number | null;
  /** The play action to start (#177); none is the release's default. */
  action?: string | null;
}
/** What a launch needs first; a list is empty when that part is decided. */
export interface LaunchPrompt {
  entryId: string;
  platformId: string;
  profiles: {
    emulatorId: string;
    emulatorName: string;
    profileId: string;
    profileName: string;
    installed: boolean;
  }[];
  discs: { index: number; name: string }[];
  /** The release's play actions, when it asks which to start (#177). */
  actions?: { id: string; name: string; default: boolean }[];
}
/** Why the signed-in account needs an admin's PIN to play a game (#209). */
export interface AgeRestriction {
  accountId: string;
  accountName: string;
  gameId: string;
  title: string;
  ratings: IgdbAgeRating[];
  /** The game's minimum age; null when it is unrated. */
  minAge: number | null;
  /** The account's age limit. */
  maxAge: number;
}
export type LaunchResult =
  | { type: "started"; ticket: LaunchTicket }
  | { type: "choose"; prompt: LaunchPrompt }
  /** Launch again once `approvePlay` matched an admin's PIN. */
  | { type: "age_restricted"; restriction: AgeRestriction };
/** A list picked by hand, or a smart collection: a saved library query
 * whose games follow the library. */
export type CollectionKind = { type: "manual" } | { type: "smart"; query: LibraryQuery };
export interface CollectionSummary {
  id: string;
  name: string;
  description: string;
  pinned: boolean;
  kind: CollectionKind;
  /** When it was made, or last changed (a manual one's games included). */
  updatedAt: string | null;
  /** Its games that aren't hidden. */
  games: number;
  playtimeSeconds: number;
  /** Grid thumbnails of its first games' covers, up to four. */
  covers: string[];
}
/** A new collection (no `id`), or an existing one's name, description and
 * rules. */
export interface SaveCollectionInput {
  id?: string;
  name: string;
  description: string;
  kind: CollectionKind;
}
export interface CollectionsApi {
  /** Pinned first, then by name. */
  list(): Promise<CollectionSummary[]>;
  /** A manual collection's games, in the order they were added. */
  memberIds(id: string): Promise<string[]>;
  /** A smart collection's games in its saved sort, a manual one's by title. */
  games(id: string, offset: number, limit: number): Promise<GamePage>;
  /** Resolves to the collection's id. */
  save(input: SaveCollectionInput): Promise<string>;
  setPinned(id: string, pinned: boolean): Promise<void>;
  /** Deletes the collection; its games stay in the library. */
  delete(id: string): Promise<void>;
  addGames(id: string, gameIds: string[]): Promise<void>;
  removeGames(id: string, gameIds: string[]): Promise<void>;
  /** The manual collections a game is in. */
  ofGame(gameId: string): Promise<string[]>;
}
export interface LibraryApi {
  /** Collections, where the library keeps them. */
  collections?: CollectionsApi;
  /** Local accounts (#202), where the library keeps them. */
  accounts?: AccountsApi;
  /** Editing games' details (#172), where the library can. */
  edits?: GameEditApi;
  /** Adding games by hand (#174), where the library can. */
  manual?: ManualGamesApi;
  /** Releases' launch actions (#177, #178), where the library has them. */
  actions?: ActionsApi;
  /** Save locations for each release on this device (#238). */
  saves?: SavesApi;
  /** Lifecycle scripts (#179, #180), where the library has them. */
  scripts?: ScriptsApi;
  listGames(query: LibraryQuery & { offset: number; limit: number }): Promise<GamePage>;
  getGame(id: string): Promise<GameDetails | null>;
  /** Downloads the images only the game's page shows (screenshots, the
   * hero) that it lacks; library events bring them in. */
  pageImages?(gameId: string): Promise<void>;
  libraryStats(): Promise<LibraryStats>;
  /** The platforms of the games in a filter, by name. */
  libraryPlatforms?(filter: LibraryFilter): Promise<PlatformSummary[]>;
  /** Where each initial letter starts in `query`'s games sorted by title,
   * in order (its own sort is ignored). */
  titleLetters?(query: LibraryQuery): Promise<TitleLetter[]>;
  listSessions(gameId?: string): Promise<PlaySession[]>;
  /** Hides a game from every library view, or shows it again. Scans that
   * find a hidden game again leave it hidden. */
  setHidden?(gameId: string, hidden: boolean): Promise<void>;
  /** The hidden games, by title. */
  hiddenGames?(): Promise<GameSummary[]>;
  /** Marks a game as a favourite, or not. */
  setFavorite?(gameId: string, favorite: boolean): Promise<void>;
  /** Chooses the entry the game's Play starts (`null`: the first that can
   * launch). */
  setDefaultEntry?(gameId: string, entryId: string | null): Promise<void>;
  /** Opens the folder an entry's files are in, in the file manager. */
  revealEntry?(entryId: string): Promise<void>;
  /** How many games are running now, without loading the session history. */
  runningCount?(): Promise<number>;
  /** Ends a running game; its session keeps the playtime so far. */
  stopGame?(sessionId: string): Promise<void>;
  /** Adds the web game at a page address and resolves to its game id; a
   * page already in the library resolves to its game. */
  addWebGame?(url: string): Promise<string>;
  /** Opens the guided capture that keeps a web game in a storage root for
   * offline play; resolves once the window is open. */
  saveWebGameOffline?(gameId: string, rootId: string): Promise<void>;
  /** Removes a web game from the library, with its offline copy and saves. */
  removeWebGame?(gameId: string): Promise<void>;
  /** Removes a game Caskade installed or handed off from the library for
   * good, uninstalling its installed files first. Resolves `true` when the
   * game is gone, `false` when an uninstall in Activity removes it once it
   * finishes. Games a scan found, or web games, are refused. */
  removeGame?(gameId: string): Promise<boolean>;
  /** Deletes a web game's offline copy; it plays online again. */
  removeWebOffline?(gameId: string): Promise<void>;
  /** Without a choice, an emulated game that must ask returns its prompt. */
  launchGame(entryId: string, choice?: LaunchChoice): Promise<LaunchResult>;
  /** Whether the signed-in account's age limit holds the game back; null
   * when it may play. Without it, nothing is held back here. */
  ageRestriction?(gameId: string): Promise<AgeRestriction | null>;
  /** An admin PIN for this account and game: one launch, or a permanent exception. */
  approvePlay?(
    gameId: string,
    accountId: string,
    pin: string,
    permanent: boolean,
  ): Promise<PinCheck>;
  subscribe(listener: (event: LibraryEvent) => void): Promise<() => void>;
}
