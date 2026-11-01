import type {
  AppNotification,
  MemberPage,
  NotificationMember,
  NotificationPage,
  NotificationsApi,
} from "./notifications";

const T0 = Date.UTC(2026, 9, 28, 14, 5);
const MIN = 60_000;

/** A synthetic item, for tests and previews. */
export function fixtureNotification(overrides: Partial<AppNotification> = {}): AppNotification {
  return {
    id: "batch",
    category: "gamesAdded",
    severity: "info",
    title: "10 games added",
    body: "RomM 6 · Steam 4",
    createdAtMs: T0,
    updatedAtMs: T0 + 45 * MIN,
    firstAtMs: T0,
    lastAtMs: T0 + 45 * MIN,
    count: 10,
    sources: [
      { source: "RomM", count: 6 },
      { source: "Steam", count: 4 },
    ],
    target: { type: "members" },
    version: 1,
    unread: true,
    resolved: false,
    ...overrides,
  };
}

/** A batch, a failed update and a read plugin notice. */
export function fixtureNotifications(): AppNotification[] {
  return [
    fixtureNotification(),
    fixtureNotification({
      id: "failed",
      category: "jobFailed",
      severity: "error",
      title: "Celeste 1.4 couldn't update",
      body: "The disk is full.",
      count: 1,
      sources: [],
      target: { type: "game", gameId: "celeste" },
      version: 2,
      updatedAtMs: T0 + 30 * MIN,
    }),
    fixtureNotification({
      id: "plugin",
      category: "pluginDisabled",
      severity: "warning",
      title: "Plugin dev.hltb was turned off",
      body: "It crashed 3 times.",
      count: 1,
      sources: [],
      target: { type: "settings", tab: "plugins", section: null },
      version: 3,
      unread: false,
      updatedAtMs: T0 + 10 * MIN,
    }),
  ];
}

export function fixtureMembers(count = 10): NotificationMember[] {
  return Array.from({ length: count }, (_, i) => ({
    kind: "game" as const,
    label: `Fixture Game ${i + 1}`,
    source: i < 6 ? "RomM" : "Steam",
    atMs: T0 + i * 5 * MIN,
    state: i === 1 ? ("merged" as const) : i === 2 ? ("removed" as const) : ("present" as const),
    gameId: i === 2 ? null : i === 1 ? "game-1" : `game-${i + 1}`,
    title: i === 2 ? null : i === 1 ? "Fixture Game 1" : `Fixture Game ${i + 1}`,
  }));
}

/** An in-memory feed with the core's read rules: marking read takes the
 * version shown, mark all read the revision a page was read at. */
export function createNotificationsFixtureApi(
  items: AppNotification[] = fixtureNotifications(),
  members: Record<string, NotificationMember[]> = { batch: fixtureMembers() },
) {
  let feed = items.map((item) => ({ ...item, readVersion: item.unread ? 0 : item.version }));
  let revision = Math.max(0, ...feed.map((item) => item.version));
  const listeners = new Set<(unread: number) => void>();
  const view = (item: (typeof feed)[number]): AppNotification => {
    const { readVersion, ...rest } = item;
    return { ...rest, unread: readVersion < item.version };
  };
  const unread = () => feed.filter((item) => item.readVersion < item.version).length;
  const changed = () => {
    for (const listener of listeners) listener(unread());
  };
  const api: NotificationsApi & {
    /** Adds members to an item, as new arrivals would: unread again. */
    arrive(id: string, count: number): void;
  } = {
    list: async (unreadOnly, offset, limit): Promise<NotificationPage> => {
      const matching = feed
        .filter((item) => !unreadOnly || item.readVersion < item.version)
        .sort((a, b) => b.updatedAtMs - a.updatedAtMs);
      return {
        items: matching.slice(offset, offset + limit).map(view),
        total: matching.length,
        unread: unread(),
        revision,
      };
    },
    unread: async () => unread(),
    members: async (id, offset, limit): Promise<MemberPage> => {
      const all = members[id] ?? [];
      return { items: all.slice(offset, offset + limit), total: all.length };
    },
    markRead: async (id, version) => {
      feed = feed.map((item) =>
        item.id === id
          ? { ...item, readVersion: Math.max(item.readVersion, Math.min(version, item.version)) }
          : item,
      );
      changed();
    },
    markUnread: async (id) => {
      feed = feed.map((item) => (item.id === id ? { ...item, readVersion: 0 } : item));
      changed();
    },
    markAllRead: async (upTo) => {
      feed = feed.map((item) =>
        item.version <= upTo ? { ...item, readVersion: item.version } : item,
      );
      changed();
    },
    subscribe: (listener) => {
      listeners.add(listener);
      return () => listeners.delete(listener);
    },
    arrive: (id, count) => {
      revision += 1;
      feed = feed.map((item) =>
        item.id === id
          ? {
              ...item,
              count: item.count + count,
              title: `${item.count + count} games added`,
              version: revision,
            }
          : item,
      );
      changed();
    },
  };
  return api;
}
