/** The notification feed (#198): game additions in hourly batches, failed
 * jobs, updates on offer and plugins turned off, kept by the core with their
 * read state. An item is unread until it is opened or marked read; marking
 * read sends the `version` that was shown, so a change since (more games in
 * the batch) leaves it unread. The core's `notificationsChanged` event says
 * when to read the feed again. */
import { keepPreviousData, useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { isTauri } from "@tauri-apps/api/core";
import { createContext, createElement, type ReactNode, useContext, useEffect } from "react";
import { commands, events } from "./bindings";
import { unwrap } from "./reply";
import { effectSubscription } from "./subscription";

export type NotificationCategory =
  | "gamesAdded"
  | "jobFailed"
  | "updatesAvailable"
  | "pluginDisabled"
  | "scriptFailed"
  | "achievementsUnlocked"
  | "shareReceived";
export type NotificationSeverity = "info" | "warning" | "error";
export type NotificationTarget =
  | { type: "members" }
  | { type: "game"; gameId: string }
  | { type: "jobs" }
  | { type: "settings"; tab: string; section: string | null };

/** One item of the feed (`caskade_core::notifications::Notification`). */
export interface AppNotification {
  id: string;
  category: NotificationCategory;
  severity: NotificationSeverity;
  title: string;
  body: string;
  createdAtMs: number;
  updatedAtMs: number;
  /** What its members cover: a batch's first and last addition. */
  firstAtMs: number;
  lastAtMs: number;
  count: number;
  /** Game additions per source, most first. */
  sources: { source: string; count: number }[];
  target: NotificationTarget;
  /** The version shown: mark read with it. */
  version: number;
  unread: boolean;
  resolved: boolean;
}

export interface NotificationPage {
  items: AppNotification[];
  total: number;
  unread: number;
  /** The feed's revision when read: mark all read with it. */
  revision: number;
}

export type MemberState = "present" | "merged" | "removed" | "other";

export interface NotificationMember {
  kind: "game" | "gameUpdate" | "emulatorUpdate" | "toolUpdate";
  label: string;
  source: string | null;
  atMs: number;
  state: MemberState;
  /** The game to open: itself, or the one it was merged into. */
  gameId: string | null;
  title: string | null;
}

export interface MemberPage {
  items: NotificationMember[];
  total: number;
}

export interface NotificationsApi {
  list(unreadOnly: boolean, offset: number, limit: number): Promise<NotificationPage>;
  unread(): Promise<number>;
  members(id: string, offset: number, limit: number): Promise<MemberPage>;
  markRead(id: string, version: number): Promise<void>;
  markUnread(id: string): Promise<void>;
  markAllRead(revision: number): Promise<void>;
  /** Every change of the feed, with the unread count; returns the unsubscribe. */
  subscribe(listener: (unread: number) => void): () => void;
}

const emptyPage: NotificationPage = { items: [], total: 0, unread: 0, revision: 0 };

/** The browser preview has no core: an empty feed. */
export const nativeNotificationsApi: NotificationsApi = {
  list: async (unreadOnly, offset, limit) =>
    isTauri()
      ? ((await unwrap(commands.notificationsList(unreadOnly, offset, limit))) as NotificationPage)
      : emptyPage,
  unread: async () => (isTauri() ? unwrap(commands.notificationsUnread()) : 0),
  members: async (id, offset, limit) =>
    isTauri()
      ? ((await unwrap(commands.notificationMembers(id, offset, limit))) as MemberPage)
      : { items: [], total: 0 },
  markRead: async (id, version) => {
    if (isTauri()) await unwrap(commands.notificationMarkRead(id, version));
  },
  markUnread: async (id) => {
    if (isTauri()) await unwrap(commands.notificationMarkUnread(id));
  },
  markAllRead: async (revision) => {
    if (isTauri()) await unwrap(commands.notificationsMarkAllRead(revision));
  },
  subscribe: (listener) =>
    isTauri()
      ? effectSubscription(events.notificationsChanged.listen, ({ payload }) =>
          listener(payload.unread),
        )
      : () => {},
};

const NotificationsContext = createContext<NotificationsApi>(nativeNotificationsApi);

/** Tests and previews supply their own API; the app uses the native one. */
export function NotificationsProvider({
  api,
  children,
}: {
  api: NotificationsApi;
  children: ReactNode;
}) {
  return createElement(NotificationsContext.Provider, { value: api }, children);
}

export const useNotificationsApi = () => useContext(NotificationsContext);

export const notificationKeys = {
  all: ["notifications"] as const,
  unread: ["notifications", "unread"] as const,
  list: (unreadOnly: boolean, offset: number) =>
    ["notifications", "list", unreadOnly, offset] as const,
  members: (id: string, limit: number) => ["notifications", "members", id, limit] as const,
};

/** The unread count for the bell, kept current by the core's events (and
 * a slow poll should one be missed). */
export function useUnreadNotifications() {
  const api = useNotificationsApi();
  const client = useQueryClient();
  useEffect(
    () =>
      api.subscribe((unread) => {
        client.setQueryData(notificationKeys.unread, unread);
        void client.invalidateQueries({
          queryKey: notificationKeys.all,
          predicate: (query) => query.queryKey[1] !== "unread",
        });
      }),
    [api, client],
  );
  return useQuery({
    queryKey: notificationKeys.unread,
    queryFn: () => api.unread(),
    refetchInterval: 60_000,
    retry: false,
  });
}

export const NOTIFICATION_PAGE = 20;

/** A page of the feed. Reading it marks nothing read. */
export function useNotifications(unreadOnly: boolean, offset: number) {
  const api = useNotificationsApi();
  return useQuery({
    queryKey: notificationKeys.list(unreadOnly, offset),
    queryFn: () => api.list(unreadOnly, offset, NOTIFICATION_PAGE),
    placeholderData: keepPreviousData,
    retry: 1,
  });
}

/** The first `limit` members of an item. */
export function useNotificationMembers(id: string, limit: number) {
  const api = useNotificationsApi();
  return useQuery({
    queryKey: notificationKeys.members(id, limit),
    queryFn: () => api.members(id, 0, limit),
    placeholderData: keepPreviousData,
    retry: 1,
  });
}

export type NotificationAction =
  | { type: "read"; id: string; version: number }
  | { type: "unread"; id: string }
  | { type: "allRead"; revision: number };

export function useNotificationActions() {
  const api = useNotificationsApi();
  const client = useQueryClient();
  return useMutation({
    mutationFn: async (action: NotificationAction) => {
      if (action.type === "read") await api.markRead(action.id, action.version);
      else if (action.type === "unread") await api.markUnread(action.id);
      else await api.markAllRead(action.revision);
    },
    onSettled: () => {
      void client.invalidateQueries({ queryKey: notificationKeys.all });
    },
  });
}
