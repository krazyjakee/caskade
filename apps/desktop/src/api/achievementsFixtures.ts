/** Fixture achievements (#190) for the browser preview and tests: a Steam
 * set of hundreds with hidden spoilers, progress and rarity, a GOG set whose
 * last refresh failed (stale), a RetroAchievements set with hardcore
 * unlocks, and sets in every other state. Refresh unlocks one more after a
 * set's baseline, as a new unlock would come in. */
import type { AchievementSet, AchievementsApi } from "./achievements";
import type { ErrorReport_Serialize } from "./bindings";

const DAY = 24 * 60 * 60 * 1000;
export const FIXTURE_NOW = Date.UTC(2026, 9, 30, 12);

function report(summary: string, overrides: Partial<ErrorReport_Serialize> = {}) {
  return {
    code: "offline",
    summary,
    detail: null,
    hints: ["checkConnection"],
    transient: true,
    reference: "fixture",
    ...overrides,
  } satisfies ErrorReport_Serialize as ErrorReport_Serialize;
}

function account(provider: string, providerName: string, displayName: string) {
  return { storeAccountId: `${provider}-${displayName}`, provider, providerName, displayName };
}

function capabilities(overrides: Partial<AchievementSet["capabilities"]> = {}) {
  return {
    unlockTimes: true,
    progress: false,
    rarity: false,
    hardcore: false,
    points: false,
    ...overrides,
  };
}

/** A set with `achievements`, counted. */
export function fixtureSet(
  overrides: Partial<AchievementSet> & Pick<AchievementSet, "account">,
): AchievementSet {
  const achievements = overrides.achievements ?? [];
  const set: AchievementSet = {
    id: `set-${overrides.account.storeAccountId}`,
    productId: "620",
    entryIds: ["entry-1"],
    sync: {
      status: "ready",
      fetchedAtMs: FIXTURE_NOW - DAY,
      lastAttemptMs: FIXTURE_NOW - DAY,
      error: null,
      failures: 0,
      retryAtMs: null,
      baseline: true,
    },
    capabilities: capabilities(),
    unlocked: 0,
    total: 0,
    hardcoreUnlocked: null,
    ...overrides,
    achievements,
  };
  return recount(set);
}

function recount(set: AchievementSet): AchievementSet {
  set.total = set.achievements.length;
  set.unlocked = set.achievements.filter((a) => a.state.unlocked).length;
  if (set.capabilities.hardcore)
    set.hardcoreUnlocked = set.achievements.filter((a) => a.state.hardcore?.unlocked).length;
  return set;
}

/** Steam: `count` achievements, every fourth unlocked, every seventh hidden,
 * some with progress, all with rarity. */
export function steamSet(count = 240): AchievementSet {
  return fixtureSet({
    account: account("steam", "Steam", "Ada"),
    productId: "620",
    capabilities: capabilities({ progress: true, rarity: true }),
    achievements: Array.from({ length: count }, (_, i) => {
      const unlocked = i % 4 === 0;
      const hidden = i % 7 === 3;
      const progress = i % 10 === 5;
      return {
        definition: {
          id: `ACH_${i}`,
          name: hidden ? `Secret ending ${i}` : `Achievement ${i + 1}`,
          description: hidden ? `The spoiler of secret ${i}.` : `Do the thing number ${i + 1}.`,
          hidden,
          iconUrl: `https://cdn.example/steam/${i}.jpg`,
          lockedIconUrl: `https://cdn.example/steam/${i}_gray.jpg`,
          progressMax: progress ? 50 : null,
          rarityPercent: (i * 37) % 100,
          points: null,
        },
        state: {
          unlocked,
          unlockedAtMs: unlocked ? FIXTURE_NOW - (count - i) * 60_000 : null,
          progress: progress && !unlocked ? 20 : null,
          hardcore: null,
        },
      };
    }),
  });
}

/** GOG: last fetched three days ago; the refresh since failed offline. */
export function staleGogSet(): AchievementSet {
  const set = fixtureSet({
    account: account("gog", "GOG", "ada_gog"),
    productId: "1207658924",
    capabilities: capabilities({ rarity: true }),
    achievements: Array.from({ length: 12 }, (_, i) => ({
      definition: {
        id: `gog-${i}`,
        name: `GOG achievement ${i + 1}`,
        description: i === 2 ? null : `GOG thing ${i + 1}.`,
        hidden: i === 5,
        iconUrl: null,
        lockedIconUrl: null,
        progressMax: null,
        rarityPercent: 12.5,
        points: null,
      },
      state: {
        unlocked: i < 5,
        unlockedAtMs: i < 5 ? FIXTURE_NOW - 4 * DAY : null,
        progress: null,
        hardcore: null,
      },
    })),
  });
  set.sync = {
    ...set.sync,
    fetchedAtMs: FIXTURE_NOW - 3 * DAY,
    lastAttemptMs: FIXTURE_NOW - 600_000,
    error: report("Caskade couldn't reach gog.com."),
    failures: 2,
  };
  return set;
}

/** RetroAchievements: softcore and hardcore unlocks apart, with points. */
export function retroSet(): AchievementSet {
  return fixtureSet({
    account: account("retroachievements", "RetroAchievements", "ada_ra"),
    productId: "1446",
    entryIds: ["entry-snes"],
    capabilities: capabilities({ rarity: true, hardcore: true, points: true }),
    achievements: Array.from({ length: 30 }, (_, i) => ({
      definition: {
        id: `${1000 + i}`,
        name: `Retro ${i + 1}`,
        description: `Clear stage ${i + 1}.`,
        hidden: false,
        iconUrl: `https://media.retroachievements.org/Badge/${i}.png`,
        lockedIconUrl: `https://media.retroachievements.org/Badge/${i}_lock.png`,
        progressMax: null,
        rarityPercent: 40 - i,
        points: (i % 5) * 5 + 5,
      },
      state: {
        unlocked: i < 10,
        unlockedAtMs: i < 10 ? FIXTURE_NOW - (10 - i) * DAY : null,
        progress: null,
        hardcore: { unlocked: i < 4, unlockedAtMs: i < 4 ? FIXTURE_NOW - (10 - i) * DAY : null },
      },
    })),
  });
}

/** A set in `status` with nothing fetched: private, unsupported, … */
export function emptySet(
  status: AchievementSet["sync"]["status"],
  provider = "steam",
  providerName = "Steam",
  name = "Bo",
): AchievementSet {
  const set = fixtureSet({ account: account(provider, providerName, name) });
  set.sync = {
    ...set.sync,
    status,
    baseline: false,
    fetchedAtMs: status === "not_fetched" ? null : set.sync.fetchedAtMs,
  };
  return set;
}

/** The preview's sets: every game shows the three stores. */
export function fixtureSets(): AchievementSet[] {
  return [steamSet(), staleGogSet(), retroSet()];
}

export function createAchievementsFixture(
  byGame: Record<string, AchievementSet[]> | ((gameId: string) => AchievementSet[]) = () =>
    fixtureSets(),
): AchievementsApi & { setOffline(offline: boolean): void; refreshes: string[] } {
  const listeners = new Set<(gameId: string) => void>();
  const games = new Map<string, AchievementSet[]>();
  const refreshes: string[] = [];
  let offline = false;
  const of = (gameId: string) => {
    let sets = games.get(gameId);
    if (!sets) {
      sets = structuredClone(
        typeof byGame === "function" ? byGame(gameId) : (byGame[gameId] ?? []),
      );
      games.set(gameId, sets);
    }
    return sets;
  };
  const copy = (gameId: string) => structuredClone(of(gameId));
  return {
    refreshes,
    setOffline(value) {
      offline = value;
    },
    async sets(gameId) {
      return copy(gameId);
    },
    async refresh(gameId) {
      refreshes.push(gameId);
      const now = Date.now();
      for (const set of of(gameId)) {
        if (set.sync.retryAtMs !== null && set.sync.retryAtMs > now) continue;
        if (["private", "sign_in_needed", "unsupported"].includes(set.sync.status)) continue;
        set.sync.lastAttemptMs = now;
        if (offline) {
          set.sync.error = report("Caskade is offline.");
          set.sync.failures += 1;
          continue;
        }
        if (set.sync.status === "not_fetched") set.sync.status = "ready";
        // After the baseline, one more unlock comes in.
        const next = set.achievements.find((a) => !a.state.unlocked && !a.definition.hidden);
        if (set.sync.baseline && next) {
          next.state.unlocked = true;
          next.state.unlockedAtMs = now;
        }
        set.sync = {
          ...set.sync,
          error: null,
          failures: 0,
          fetchedAtMs: now,
          baseline: true,
        };
        recount(set);
      }
      for (const listener of listeners) listener(gameId);
      return copy(gameId);
    },
    async cancel() {
      return false;
    },
    async icon() {
      // No service images in the preview: the page's own fallbacks show.
      return null;
    },
    subscribe(listener) {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
  };
}
