/** A game's achievements on its page (#190): one panel per store account's
 * set, chosen when there are several, with its counts, how its last fetch
 * went and what to do about it, and every achievement with its state,
 * progress, unlock time, rarity and points where the service gives them.
 * Hidden achievements stay concealed until unlocked or revealed. A pad
 * reaches every row, Reveal and the panel's controls; nothing needs hover. */
import { Button, SegmentedControl, Switch } from "@caskade/ui";
import { plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { useNavigate } from "@tanstack/react-router";
import { EyeOff, Lock, RefreshCw, Trophy } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Meter as AriaMeter } from "react-aria-components";
import {
  type Achievement,
  type AchievementSet,
  setKey,
  useAchievementIcon,
  useAchievementsApi,
  useGameAchievements,
  useRefreshAchievements,
} from "../../api/achievements";
import { formatDateTime, formatNumber } from "../../api/presentation";

type Show = "all" | "unlocked" | "locked";

/** The heading the summary in the hero moves focus to. */
const HEADING = "achievements-heading";

export function GameAchievements({ gameId }: { gameId: string }) {
  const { t } = useLingui();
  const query = useGameAchievements(gameId);
  const refresh = useRefreshAchievements(gameId);
  const api = useAchievementsApi();
  const [chosen, setChosen] = useState<string | null>(null);
  const sets = query.data ?? [];
  if (query.isPending)
    return (
      <section data-cs="achievements" data-state="loading" className="mt-8">
        <p role="status" className="text-sm text-fg-muted">
          <Trans>Loading achievements…</Trans>
        </p>
      </section>
    );
  if (query.isError)
    return (
      <section data-cs="achievements" data-state="error" className="mt-8">
        <p role="alert" className="text-sm text-danger">
          {t`Achievements couldn't be read: ${query.error.message}`}
        </p>
        <Button className="mt-2" onPress={() => query.refetch()}>
          <Trans>Try again</Trans>
        </Button>
      </section>
    );
  // No connected account offers this game's achievements.
  if (sets.length === 0) return null;
  const set = sets.find((s) => setKey(s) === chosen) ?? (sets[0] as AchievementSet);
  return (
    <section
      data-cs="achievements"
      data-state={set.sync.status}
      aria-labelledby={HEADING}
      className="mt-8 flex flex-col gap-4"
    >
      <div className="flex flex-wrap items-center gap-3">
        <h2
          id={HEADING}
          tabIndex={-1}
          className="flex-1 font-display text-xl font-semibold outline-none"
        >
          <Trans>Achievements</Trans>
        </h2>
        {refresh.isPending ? (
          <Button
            data-part="cancel"
            data-spatial="focusable"
            onPress={() => void api.cancel(gameId)}
          >
            <Trans>Stop refreshing</Trans>
          </Button>
        ) : (
          <Button
            data-part="refresh"
            data-spatial="focusable"
            variant="secondary"
            icon={RefreshCw}
            onPress={() => refresh.mutate()}
          >
            <Trans>Refresh achievements</Trans>
          </Button>
        )}
      </div>
      {refresh.isPending && (
        <p role="status" className="text-sm text-fg-muted">
          <Trans>Refreshing achievements…</Trans>
        </p>
      )}
      {refresh.isError && (
        <p role="alert" className="text-sm text-danger">
          {t`The refresh didn't finish: ${refresh.error.message}`}
        </p>
      )}
      {sets.length > 1 && (
        <SegmentedControl
          data-part="sets"
          aria-label={t`Achievements of`}
          value={setKey(set)}
          onChange={setChosen}
          options={sets.map((s) => ({
            id: setKey(s),
            label: t`${s.account.providerName} · ${s.account.displayName}`,
          }))}
        />
      )}
      <SetView key={setKey(set)} gameId={gameId} set={set} />
    </section>
  );
}

function SetView({ gameId, set }: { gameId: string; set: AchievementSet }) {
  const { t } = useLingui();
  const navigate = useNavigate();
  const { account, sync } = set;
  const provider = account.providerName;
  const name = account.displayName;
  return (
    <div className="flex flex-col gap-3">
      <p data-part="account" className="text-sm text-fg-muted">
        {t`${provider} account ${name}`}
        {sync.fetchedAtMs !== null && ` · ${t`updated ${formatDateTime(sync.fetchedAtMs)}`}`}
      </p>
      {sync.status === "ready" && sync.error && (
        <p data-part="stale" role="status" className="text-sm text-warning">
          {sync.fetchedAtMs === null
            ? t`The last refresh failed: ${sync.error.summary}`
            : t`Showing achievements from ${formatDateTime(sync.fetchedAtMs)}. The last refresh failed: ${sync.error.summary}`}
        </p>
      )}
      {sync.retryAtMs !== null && sync.retryAtMs > Date.now() && (
        <p data-part="wait" className="text-sm text-fg-muted">
          {t`${provider} asked Caskade to wait; refreshing works again after ${formatDateTime(sync.retryAtMs)}.`}
        </p>
      )}
      {sync.status === "not_fetched" && (
        <p data-part="notice" className="text-sm text-fg-muted">
          <Trans>Not fetched yet. Refresh achievements to see them.</Trans>
        </p>
      )}
      {sync.status === "no_achievements" && (
        <p data-part="notice" className="text-sm text-fg-muted">
          {t`${provider} lists no achievements for this game.`}
        </p>
      )}
      {sync.status === "unsupported" && (
        <p data-part="notice" className="text-sm text-fg-muted">
          {t`${provider} doesn't offer this game's achievements.`}
        </p>
      )}
      {sync.status === "private" && (
        <p data-part="notice" className="text-sm text-fg-muted">
          {t`${name}'s achievements are private on ${provider}. Make your game details public there, then refresh.`}
        </p>
      )}
      {sync.status === "sign_in_needed" && (
        <div data-part="notice" className="flex flex-wrap items-center gap-3">
          <p className="text-sm text-fg-muted">
            {t`Sign in to ${provider} as ${name} again to see these achievements.`}
          </p>
          <Button
            data-part="reconnect"
            data-spatial="focusable"
            onPress={() => void navigate({ to: "/settings", search: { tab: "stores" } })}
          >
            <Trans>Open store accounts</Trans>
          </Button>
        </div>
      )}
      {set.total > 0 && <Achievements gameId={gameId} set={set} />}
    </div>
  );
}

function Achievements({ gameId, set }: { gameId: string; set: AchievementSet }) {
  const { t } = useLingui();
  const [show, setShow] = useState<Show>("all");
  const [revealAll, setRevealAll] = useState(false);
  const [revealed, setRevealed] = useState<ReadonlySet<string>>(new Set());
  const { capabilities } = set;
  const hiddenLocked = set.achievements.filter(
    (a) => a.definition.hidden && !a.state.unlocked,
  ).length;
  const points = capabilities.points ? totals(set.achievements) : null;
  const shown = set.achievements.filter(
    (a) => show === "all" || (show === "unlocked") === a.state.unlocked,
  );
  const percent = Math.round((set.unlocked * 100) / Math.max(1, set.total));
  return (
    <>
      <div data-part="summary" className="flex flex-col gap-1.5">
        <p className="text-sm">
          {t`${set.unlocked} of ${set.total} unlocked (${percent}%)`}
          {set.hardcoreUnlocked !== null && ` · ${t`${set.hardcoreUnlocked} in hardcore`}`}
          {points && ` · ${t`${points.earned} of ${points.total} points`}`}
        </p>
        <Meter value={set.unlocked} max={set.total} label={t`Achievements unlocked`} />
      </div>
      <div className="flex flex-wrap items-center gap-3">
        <SegmentedControl
          data-part="filter"
          aria-label={t`Show`}
          value={show}
          onChange={setShow}
          options={[
            { id: "all", label: t({ message: "All", context: "achievements" }) },
            { id: "unlocked", label: t({ message: "Unlocked", context: "achievements" }) },
            { id: "locked", label: t({ message: "Locked", context: "achievements" }) },
          ]}
        />
        {hiddenLocked > 0 && (
          <Switch data-part="reveal-all" isSelected={revealAll} onChange={setRevealAll}>
            {plural(hiddenLocked, {
              one: "Show # hidden achievement",
              other: "Show # hidden achievements",
            })}
          </Switch>
        )}
      </div>
      {shown.length === 0 ? (
        <p className="text-sm text-fg-subtle">
          {show === "unlocked" ? t`Nothing unlocked yet.` : t`Everything is unlocked.`}
        </p>
      ) : (
        <ul data-part="list" aria-label={t`Achievements`} className="flex flex-col gap-2">
          {shown.map((achievement) => (
            <Row
              key={achievement.definition.id}
              gameId={gameId}
              set={set}
              achievement={achievement}
              revealed={revealAll || revealed.has(achievement.definition.id)}
              reveal={() =>
                setRevealed((current) => new Set(current).add(achievement.definition.id))
              }
            />
          ))}
        </ul>
      )}
    </>
  );
}

function totals(achievements: Achievement[]) {
  let earned = 0;
  let total = 0;
  for (const { definition, state } of achievements) {
    total += definition.points ?? 0;
    if (state.unlocked) earned += definition.points ?? 0;
  }
  return { earned, total };
}

function Meter({ value, max, label }: { value: number; max: number; label: string }) {
  return (
    <AriaMeter
      aria-label={label}
      minValue={0}
      maxValue={Math.max(1, max)}
      value={value}
      className="h-1.5 w-full max-w-md overflow-hidden rounded-full bg-surface-hover"
    >
      {({ percentage }) => (
        <div
          className="h-full rounded-full bg-accent transition-[width] duration-(--cs-motion-duration-fast)"
          style={{ width: `${percentage}%` }}
        />
      )}
    </AriaMeter>
  );
}

function Row({
  gameId,
  set,
  achievement,
  revealed,
  reveal,
}: {
  gameId: string;
  set: AchievementSet;
  achievement: Achievement;
  revealed: boolean;
  reveal: () => void;
}) {
  const { t } = useLingui();
  const { definition, state } = achievement;
  const { capabilities } = set;
  const concealed = definition.hidden && !state.unlocked && !revealed;
  const row = useRef<HTMLLIElement>(null);
  const name = concealed ? t`Hidden achievement` : definition.name;
  const description = concealed
    ? t`Its name and description are a spoiler until you unlock it.`
    : (definition.description ??
      (definition.hidden && !state.unlocked
        ? t`${set.account.providerName} shows its description once it's unlocked.`
        : null));
  const hardcore = capabilities.hardcore ? state.hardcore : null;
  const progress =
    capabilities.progress &&
    !state.unlocked &&
    definition.progressMax !== null &&
    state.progress !== null
      ? { value: state.progress, max: definition.progressMax }
      : null;
  return (
    <li
      ref={row}
      data-part="achievement"
      data-state={state.unlocked ? "unlocked" : "locked"}
      data-concealed={concealed ? "true" : undefined}
      data-hardcore={hardcore?.unlocked ? "true" : undefined}
      // Every row is a stop for a pad; a concealed one's stop is Reveal.
      tabIndex={concealed ? undefined : 0}
      aria-label={concealed ? undefined : `${name}, ${state.unlocked ? t`unlocked` : t`locked`}`}
      className="flex items-start gap-3 rounded-control border border-border bg-surface-raised p-3 outline-none [contain-intrinsic-size:auto_4.5rem] [content-visibility:auto] data-[state=locked]:text-fg-muted focus-visible:focus-ring"
    >
      <Icon gameId={gameId} set={set} achievement={achievement} concealed={concealed} />
      <div className="min-w-0 flex-1">
        <p className="font-semibold">{name}</p>
        {description && <p className="text-sm text-fg-muted">{description}</p>}
        <p data-part="details" className="mt-1 flex flex-wrap gap-x-3 text-xs text-fg-subtle">
          <span>
            {state.unlocked
              ? capabilities.unlockTimes && state.unlockedAtMs !== null
                ? t`Unlocked ${formatDateTime(state.unlockedAtMs)}`
                : t`Unlocked`
              : t`Locked`}
          </span>
          {hardcore && (
            <span data-part="hardcore">
              {hardcore.unlocked
                ? hardcore.unlockedAtMs !== null
                  ? t`Hardcore ${formatDateTime(hardcore.unlockedAtMs)}`
                  : t`Hardcore`
                : state.unlocked
                  ? t`Softcore only`
                  : null}
            </span>
          )}
          {capabilities.rarity && definition.rarityPercent !== null && (
            <span>
              {t`${formatNumber(definition.rarityPercent, { maximumFractionDigits: 1 })}% of players`}
            </span>
          )}
          {capabilities.points && definition.points !== null && (
            <span>{plural(definition.points, { one: "# point", other: "# points" })}</span>
          )}
        </p>
        {progress && (
          <div className="mt-1.5 flex items-center gap-2 text-xs text-fg-subtle">
            <Meter value={progress.value} max={progress.max} label={t`Progress`} />
            <span>
              {formatNumber(progress.value)}/{formatNumber(progress.max)}
            </span>
          </div>
        )}
      </div>
      {concealed && (
        <Button
          data-part="reveal"
          data-spatial="focusable"
          size="sm"
          onPress={() => {
            reveal();
            // Reveal goes away: focus stays on what it revealed.
            requestAnimationFrame(() => row.current?.focus());
          }}
        >
          <Trans>Reveal</Trans>
        </Button>
      )}
    </li>
  );
}

/** The service's icon once the row is near the screen; a drawn one when
 * there is none, it can't load, or it would give a hidden one away. */
function Icon({
  gameId,
  set,
  achievement,
  concealed,
}: {
  gameId: string;
  set: AchievementSet;
  achievement: Achievement;
  concealed: boolean;
}) {
  const ref = useRef<HTMLSpanElement>(null);
  const near = useNear(ref);
  const { definition, state } = achievement;
  const locked = !state.unlocked;
  const wanted = !concealed && Boolean(locked ? definition.lockedIconUrl : definition.iconUrl);
  const icon = useAchievementIcon(
    {
      gameId,
      storeAccountId: set.account.storeAccountId,
      productId: set.productId,
      achievementId: definition.id,
      locked,
    },
    near && wanted,
  );
  const Fallback = concealed ? EyeOff : locked ? Lock : Trophy;
  return (
    <span
      ref={ref}
      aria-hidden="true"
      data-part="icon"
      className="grid size-12 shrink-0 place-items-center overflow-hidden rounded-control border border-border bg-surface-base text-fg-subtle"
    >
      {icon.data ? (
        <img
          src={icon.data}
          alt=""
          className={`size-full object-cover ${locked && !definition.lockedIconUrl ? "grayscale" : ""}`}
        />
      ) : (
        <Fallback className="size-5" />
      )}
    </span>
  );
}

/** Whether `ref` has come near the viewport; true where that can't be told. */
function useNear(ref: React.RefObject<Element | null>) {
  const [near, setNear] = useState(() => typeof IntersectionObserver === "undefined");
  useEffect(() => {
    const element = ref.current;
    if (near || !element) return;
    const observer = new IntersectionObserver(
      (entries) => {
        if (entries.some((entry) => entry.isIntersecting)) setNear(true);
      },
      { rootMargin: "400px" },
    );
    observer.observe(element);
    return () => observer.disconnect();
  }, [near, ref]);
  return near;
}

/** The game's achievement counts for its hero: unlocked of total across
 * the sets; pressing it moves to the panel. Nothing without sets. */
export function AchievementSummary({ gameId }: { gameId: string }) {
  const { t } = useLingui();
  const sets = useGameAchievements(gameId).data ?? [];
  const counted = sets.filter((set) => set.total > 0);
  if (counted.length === 0) return null;
  const unlocked = counted.reduce((sum, set) => sum + set.unlocked, 0);
  const total = counted.reduce((sum, set) => sum + set.total, 0);
  return (
    <button
      type="button"
      data-cs="achievement-summary"
      data-spatial="focusable"
      aria-label={t`${unlocked} of ${total} achievements unlocked. Go to achievements`}
      onClick={() => {
        const heading = document.getElementById(HEADING);
        if (!heading) return;
        // Scroll the view's own scroller: scrollIntoView would move the
        // whole shell too.
        const scroller = scrollerOf(heading);
        if (scroller)
          scroller.scrollTop +=
            heading.getBoundingClientRect().top - scroller.getBoundingClientRect().top - 16;
        heading.focus({ preventScroll: true });
      }}
      className="inline-flex items-center gap-1.5 rounded-control px-1 text-fg-muted outline-none hover:text-fg focus-visible:focus-ring"
    >
      <Trophy aria-hidden="true" className="size-4" />
      <span>{t`${unlocked}/${total} achievements`}</span>
    </button>
  );
}

function scrollerOf(element: HTMLElement): HTMLElement | null {
  for (let at = element.parentElement; at; at = at.parentElement) {
    const overflow = getComputedStyle(at).overflowY;
    if ((overflow === "auto" || overflow === "scroll") && at.scrollHeight > at.clientHeight)
      return at;
  }
  return null;
}
