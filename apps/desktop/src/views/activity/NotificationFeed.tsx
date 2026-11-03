import { Button } from "@caskade/ui";
import type { MessageDescriptor } from "@lingui/core";
import { msg, plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { Link, useNavigate } from "@tanstack/react-router";
import { BellDot, CheckCheck, ExternalLink, Mail, MailOpen } from "lucide-react";
import { useId, useState } from "react";
import {
  type AppNotification,
  NOTIFICATION_PAGE,
  type NotificationCategory,
  type NotificationMember,
  useNotificationActions,
  useNotificationMembers,
  useNotifications,
} from "../../api/notifications";
import { formatDay, formatTime } from "../../api/presentation";
import { useActionMenus } from "../../components/ActionMenu";
import type { SettingsTab } from "../SettingsView";

const categoryLabels: Record<NotificationCategory, MessageDescriptor> = {
  gamesAdded: msg({ message: "Games added", comment: "A notification's category" }),
  jobFailed: msg({ message: "Failed job", comment: "A notification's category" }),
  updatesAvailable: msg({ message: "Updates", comment: "A notification's category" }),
  pluginDisabled: msg({ message: "Plugin", comment: "A notification's category" }),
  scriptFailed: msg({ message: "Script", comment: "A notification's category" }),
  achievementsUnlocked: msg({ message: "Achievements", comment: "A notification's category" }),
  shareReceived: msg({ message: "Shared", comment: "A notification's category" }),
};

/** Where Back from a game opened here goes. */
const FROM = "/activity?tab=notifications";

/** When a batch's additions happened: one time, a span within a day, or
 * across days. Never "the last hour": the range stays true later. */
export function timeRange(first: number, last: number) {
  const at = (ms: number) => `${formatDay(ms)}, ${formatTime(ms)}`;
  if (last - first < 60_000) return at(first);
  if (formatDay(first) === formatDay(last)) return `${at(first)}–${formatTime(last)}`;
  return `${at(first)} – ${at(last)}`;
}

/** The notification feed, beside the jobs in Activity. Showing it marks
 * nothing read: an item is read once it is opened, or marked read. */
export function NotificationFeed({ unreadOnly, open }: { unreadOnly: boolean; open?: string }) {
  const { t } = useLingui();
  const [offset, setOffset] = useState(0);
  const feed = useNotifications(unreadOnly, offset);
  const action = useNotificationActions();
  const page = feed.data;
  const shown = page?.items.length ?? 0;
  const first = offset + 1;
  const last = offset + shown;
  const total = page?.total;
  return (
    <section data-cs="notifications" aria-label={t`Notifications`}>
      <div data-part="toolbar" className="mb-4 flex flex-wrap items-center justify-between gap-3">
        <div className="flex gap-2">
          {[
            {
              label: t({
                message: "All",
                comment: "The notification feed's filter: every notification",
              }),
              unread: false,
            },
            {
              label: t({ message: "Unread", comment: "The notification feed's filter" }),
              unread: true,
            },
          ].map((filter) => {
            const selected = filter.unread === unreadOnly;
            return (
              <Link
                key={String(filter.unread)}
                to="/activity"
                search={{
                  tab: "notifications",
                  ...(filter.unread && { unread: true }),
                }}
                replace
                data-part="filter"
                data-spatial="focusable"
                aria-current={selected ? "true" : undefined}
                data-selected={selected || undefined}
                className="rounded-control border border-border px-3 py-1 text-sm text-fg-muted hover:text-fg focus-visible:focus-ring data-selected:border-accent-alt/50 data-selected:text-fg"
              >
                {filter.label}
              </Link>
            );
          })}
        </div>
        <Button
          data-part="mark-all"
          data-spatial="focusable"
          variant="ghost"
          isDisabled={!page || page.unread === 0 || action.isPending}
          onPress={() => page && action.mutate({ type: "allRead", revision: page.revision })}
        >
          <CheckCheck className="size-4" /> <Trans>Mark all read</Trans>
        </Button>
      </div>
      {feed.isPending && (
        <p role="status" className="text-fg-muted">
          <Trans>Loading notifications…</Trans>
        </p>
      )}
      {feed.isError && (
        <div role="alert" className="rounded-panel border border-border p-4">
          <p>
            <Trans>Could not load notifications.</Trans>
          </p>
          <p className="mt-1 text-sm text-fg-muted">{feed.error.message}</p>
          <Button className="mt-3" onPress={() => void feed.refetch()}>
            <Trans>Try again</Trans>
          </Button>
        </div>
      )}
      {action.isError && (
        <p role="alert" className="mb-4 text-danger">
          {action.error.message}
        </p>
      )}
      {page && page.total === 0 && (
        <p
          data-part="empty"
          className="rounded-panel border border-dashed border-border p-12 text-center text-fg-muted"
        >
          {unreadOnly
            ? t`No unread notifications.`
            : t`No notifications yet. New games, failed jobs and updates show here.`}
        </p>
      )}
      <ol className="space-y-3">
        {page?.items.map((item) => (
          <li key={item.id}>
            <NotificationItem item={item} open={open === item.id} />
          </li>
        ))}
      </ol>
      {page && page.total > NOTIFICATION_PAGE && (
        <nav
          data-part="pages"
          aria-label={t`Notification pages`}
          className="mt-4 flex items-center justify-between gap-3 text-sm text-fg-muted"
        >
          <Button
            data-spatial="focusable"
            variant="ghost"
            isDisabled={offset === 0}
            onPress={() => setOffset(Math.max(0, offset - NOTIFICATION_PAGE))}
          >
            <Trans>Newer</Trans>
          </Button>
          <span>
            {t({
              message: `${first}–${last} of ${total}`,
              comment: "The notifications shown, of how many: 1–20 of 45",
            })}
          </span>
          <Button
            data-spatial="focusable"
            variant="ghost"
            isDisabled={offset + NOTIFICATION_PAGE >= page.total}
            onPress={() => setOffset(offset + NOTIFICATION_PAGE)}
          >
            <Trans>Older</Trans>
          </Button>
        </nav>
      )}
    </section>
  );
}

function openLabel(item: AppNotification, open: boolean): MessageDescriptor {
  switch (item.target.type) {
    case "members":
      if (open) return msg`Hide list`;
      return item.category === "gamesAdded" ? msg`Show games` : msg`Show updates`;
    case "game":
      return msg`Open game`;
    case "jobs":
      return msg`Open jobs`;
    case "settings":
      return msg`Open settings`;
  }
}

function NotificationItem({ item, open }: { item: AppNotification; open: boolean }) {
  const { t } = useLingui();
  const title = item.title;
  const heading = useId();
  const navigate = useNavigate();
  const action = useNotificationActions();
  const menus = useActionMenus();
  const markRead = () => action.mutate({ type: "read", id: item.id, version: item.version });
  const toggleRead = () =>
    item.unread ? markRead() : action.mutate({ type: "unread", id: item.id });
  // Opening reads it: only the version shown, so later arrivals stay unread.
  const openItem = () => {
    if (item.unread) markRead();
    const target = item.target;
    switch (target.type) {
      case "members":
        // In the whole feed: a read item leaves the unread filter.
        void navigate({
          to: "/activity",
          search: { tab: "notifications", ...(!open && { open: item.id }) },
          replace: true,
        });
        return;
      case "game":
        void navigate({
          to: "/games/$gameId",
          params: { gameId: target.gameId },
          search: { from: FROM },
        });
        return;
      case "jobs":
        void navigate({ to: "/activity", search: { tab: "jobs" } });
        return;
      case "settings":
        void navigate({
          to: "/settings",
          search: {
            tab: target.tab as SettingsTab,
            ...(target.section && { section: target.section }),
          },
        });
        return;
    }
  };
  const when =
    item.category === "gamesAdded"
      ? timeRange(item.firstAtMs, item.lastAtMs)
      : timeRange(item.updatedAtMs, item.updatedAtMs);
  return (
    <article
      {...menus.target(item.title, [
        { id: "open", label: t(openLabel(item, open)), icon: ExternalLink, onAction: openItem },
        {
          id: "read",
          label: item.unread ? t`Mark read` : t`Mark unread`,
          icon: item.unread ? MailOpen : Mail,
          isDisabled: action.isPending,
          onAction: toggleRead,
        },
      ])}
      aria-labelledby={heading}
      data-cs="notification"
      data-status={item.unread ? "unread" : "read"}
      data-severity={item.severity}
      data-category={item.category}
      data-resolved={item.resolved || undefined}
      className="rounded-panel border border-border bg-surface-raised p-4 data-[status=unread]:border-accent-alt/50"
    >
      <div className="flex flex-wrap items-start justify-between gap-3">
        <div className="flex min-w-0 gap-3">
          <span
            data-part="indicator"
            aria-hidden="true"
            className={`mt-2 size-2 shrink-0 rounded-full ${item.unread ? "bg-accent-alt" : "bg-transparent"}`}
          />
          <div className="min-w-0">
            <p data-part="meta" className="font-mono text-xs text-fg-subtle uppercase">
              {t(categoryLabels[item.category])} · <time data-part="time">{when}</time>
              {item.resolved &&
                ` · ${t({ message: "Resolved", comment: "A notification whose problem was fixed" })}`}
              {item.unread && (
                <span className="sr-only">
                  {" "}
                  · {t({ message: "Unread", comment: "A notification not read yet" })}
                </span>
              )}
            </p>
            <h2
              id={heading}
              data-part="title"
              className={`mt-1 break-words font-display text-lg ${item.unread ? "font-semibold" : ""} ${item.severity === "error" && !item.resolved ? "text-danger" : ""}`}
            >
              {item.title}
            </h2>
            {item.body && (
              <p data-part="body" className="mt-1 break-words text-sm text-fg-muted">
                {item.body}
              </p>
            )}
          </div>
        </div>
        <div data-part="actions" className="flex gap-2">
          <Button
            data-spatial="focusable"
            variant={item.unread ? "primary" : "secondary"}
            aria-expanded={item.target.type === "members" ? open : undefined}
            onPress={openItem}
          >
            {t(openLabel(item, open))}
          </Button>
          <Button
            data-spatial="focusable"
            variant="ghost"
            aria-label={item.unread ? t`Mark read: ${title}` : t`Mark unread: ${title}`}
            isDisabled={action.isPending}
            onPress={toggleRead}
          >
            {item.unread ? <MailOpen className="size-4" /> : <BellDot className="size-4" />}
            {item.unread ? t`Mark read` : t`Mark unread`}
          </Button>
        </div>
      </div>
      {open && item.target.type === "members" && <MemberList id={item.id} />}
      {menus.menu}
    </article>
  );
}

const MEMBER_PAGE = 50;

/** What an item is about: a batch's games, each linked to its page (or the
 * game it was merged into), or the updates on offer. */
function MemberList({ id }: { id: string }) {
  const { t } = useLingui();
  const [limit, setLimit] = useState(MEMBER_PAGE);
  const members = useNotificationMembers(id, limit);
  const page = members.data;
  return (
    <div data-part="members" className="mt-4 border-t border-border pt-3">
      {members.isPending && (
        <p role="status" className="text-sm text-fg-muted">
          {t`Loading…`}
        </p>
      )}
      {members.isError && (
        <div role="alert" className="text-sm">
          <p>
            <Trans>Could not load the list.</Trans>
          </p>
          <Button className="mt-2" onPress={() => void members.refetch()}>
            <Trans>Try again</Trans>
          </Button>
        </div>
      )}
      {page && page.total === 0 && (
        <p className="text-sm text-fg-muted">
          <Trans>Nothing left to show: they have all gone.</Trans>
        </p>
      )}
      <ul className="grid gap-1 sm:grid-cols-2">
        {page?.items.map((member) => (
          <li
            key={`${member.kind}:${member.atMs}:${member.label}`}
            data-part="member"
            data-status={member.state}
            className="text-sm"
          >
            <MemberLink member={member} />
            {member.source && (
              <span className="ml-2 font-mono text-xs text-fg-subtle">{member.source}</span>
            )}
          </li>
        ))}
      </ul>
      {page && page.items.length < page.total && (
        <Button
          className="mt-3"
          data-spatial="focusable"
          variant="ghost"
          onPress={() => setLimit(limit + MEMBER_PAGE)}
        >
          {plural(page.total - page.items.length, {
            one: "Show more (# left)",
            other: "Show more (# left)",
          })}
        </Button>
      )}
    </div>
  );
}

function MemberLink({ member }: { member: NotificationMember }) {
  const { t } = useLingui();
  const into = member.title;
  const link = "text-accent-alt hover:underline focus-visible:focus-ring rounded-control";
  if (member.state === "removed")
    return (
      <span className="text-fg-subtle">
        {member.label}{" "}
        <span className="text-xs">
          <Trans>(no longer in the library)</Trans>
        </span>
      </span>
    );
  // Streaming's programs are updated in Settings → Streaming (#266).
  if (member.state === "other" && member.kind === "toolUpdate")
    return (
      <Link to="/settings" search={{ tab: "streaming" }} data-spatial="focusable" className={link}>
        {member.label}
      </Link>
    );
  if (member.state === "other")
    return (
      <Link
        to="/settings"
        search={{ tab: "emulators", section: "emulators" }}
        data-spatial="focusable"
        className={link}
      >
        {member.label}
      </Link>
    );
  if (!member.gameId) return <span>{member.label}</span>;
  return (
    <Link
      to="/games/$gameId"
      params={{ gameId: member.gameId }}
      search={{ from: FROM }}
      data-spatial="focusable"
      className={link}
    >
      {member.label}
      {member.state === "merged" && member.title && (
        <>
          {" "}
          <span className="text-xs text-fg-subtle">
            {t({
              message: `(now part of ${into})`,
              comment: "A game added to the library that was later merged into another game",
            })}
          </span>
        </>
      )}
    </Link>
  );
}
