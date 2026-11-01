/** The work the choices start, as it happens: each task's progress, the
 * library's count climbing and covers arriving behind it. Then the way in. */
import { GameArt } from "@caskade/ui";
import { plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { CircleAlert, CircleCheck, CircleDashed, CircleX, LoaderCircle } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import { Button as AriaButton } from "react-aria-components";
import { useGames, useLibraryStats } from "../api/hooks";
import { errorMessage } from "../api/reply";
import { useDefaultFocus } from "../input";
import { reducedMotion } from "../input/spatial";
import { useDiscoveryApi } from "../views/settings/discovery/DiscoveryProvider";
import { useEmulationApi } from "../views/settings/emulators/EmulationProvider";
import { useRommApi } from "../views/settings/romm/useRomm";
import { useSettingsApi } from "../views/settings/useSettings";
import { type Choices, plannedTasks, runSetup, type Task, type TaskState } from "./run";
import type { Sounds } from "./sound";

const icons: Record<TaskState, typeof CircleCheck> = {
  waiting: CircleDashed,
  running: LoaderCircle,
  done: CircleCheck,
  warning: CircleAlert,
  failed: CircleX,
};
const tones: Record<TaskState, string> = {
  waiting: "text-fg-subtle",
  running: "text-accent-alt animate-worker motion-reduce:animate-none",
  done: "text-status-installed",
  warning: "text-warning",
  failed: "text-danger",
};

/** Counts up to `target` instead of jumping, unless motion is reduced. */
function useCountUp(target: number) {
  const [shown, setShown] = useState(target);
  const from = useRef(target);
  useEffect(() => {
    const start = from.current;
    if (start === target) return;
    if (reducedMotion() || typeof requestAnimationFrame === "undefined") {
      from.current = target;
      setShown(target);
      return;
    }
    const began = performance.now();
    let frame = 0;
    const tick = (now: number) => {
      const t = Math.min(1, (now - began) / 900);
      const value = Math.round(start + (target - start) * (1 - (1 - t) ** 3));
      from.current = value;
      setShown(value);
      if (t < 1) frame = requestAnimationFrame(tick);
    };
    frame = requestAnimationFrame(tick);
    return () => cancelAnimationFrame(frame);
  }, [target]);
  return shown;
}

export function SetupStep({
  choices,
  sounds,
  onDone,
}: {
  choices: Choices;
  sounds: Sounds;
  onDone(): void;
}) {
  const { t } = useLingui();
  const apis = {
    settings: useSettingsApi(),
    discovery: useDiscoveryApi(),
    emulation: useEmulationApi(),
    romm: useRommApi(),
  };
  const [tasks, setTasks] = useState<Task[]>(() => plannedTasks(choices));
  const [finished, setFinished] = useState(false);
  const [error, setError] = useState<string | null>(null);
  const started = useRef(false);
  const latest = useRef({ apis, choices, sounds });
  latest.current = { apis, choices, sounds };
  useEffect(() => {
    // Once, StrictMode's second effect run included.
    if (started.current) return;
    started.current = true;
    const { apis, choices, sounds } = latest.current;
    runSetup(apis, choices, { update: setTasks, cue: (cue) => sounds.play(cue) })
      .catch((failure: unknown) => setError(errorMessage(failure)))
      .finally(() => {
        sounds.play("done");
        setFinished(true);
      });
  }, []);

  const stats = useLibraryStats();
  const total = useCountUp(stats.data?.total ?? 0);
  const games = useGames({ filter: "all", sort: "recent" }, { pageSize: 48 });
  const covers = (games.data?.pages.flatMap((page) => page.items) ?? [])
    .filter((game) => game.coverGrid)
    .slice(0, 24);
  // Refreshes the count and the covers while the work runs; library events
  // refresh them too where the core sends them.
  const { refetch: refetchStats } = stats;
  const { refetch: refetchGames } = games;
  useEffect(() => {
    if (finished) return;
    const timer = setInterval(() => {
      void refetchStats();
      void refetchGames();
    }, 1500);
    return () => clearInterval(timer);
  }, [finished, refetchStats, refetchGames]);

  const enter = useRef<HTMLButtonElement>(null);
  useDefaultFocus(enter, finished);
  const failed = tasks.some((task) => task.state === "failed" || task.state === "warning");

  return (
    <section data-part="setup" data-status={finished ? "done" : "running"} className="relative">
      <div
        data-part="covers"
        aria-hidden
        className="pointer-events-none fixed inset-0 -z-10 grid grid-cols-6 content-center gap-3 p-6 opacity-25 [mask-image:radial-gradient(ellipse_at_center,transparent_25%,black_75%)]"
      >
        {covers.map((game, i) => (
          <div
            key={game.id}
            style={{ "--i": i % 12 } as React.CSSProperties}
            className="aspect-[3/4] animate-rise overflow-hidden rounded-card"
          >
            <GameArt game={game} image={game.coverGrid} className="size-full" />
          </div>
        ))}
      </div>

      <div className="mb-8 text-center">
        <h1 className="animate-rise font-display text-3xl font-semibold">
          {finished ? t`You're all set` : t`Setting things up`}
        </h1>
        <p
          data-part="count"
          aria-live="polite"
          className="mt-6 animate-rise font-display text-6xl font-semibold text-accent-alt tabular-nums [--i:2]"
        >
          {total}
        </p>
        <p className="mt-1 animate-rise text-fg-muted [--i:3]">
          {t({
            message: plural(
              { total },
              { one: "game in your library", other: "games in your library" },
            ),
            comment: "Under the library's game count, shown large above it",
          })}
        </p>
      </div>

      <ul data-part="tasks" className="flex flex-col gap-2">
        {tasks.map((task, i) => {
          const Icon = icons[task.state];
          return (
            <li
              key={task.id}
              data-part="task"
              data-task={task.id}
              data-status={task.state}
              style={{ "--i": i + 4 } as React.CSSProperties}
              className="relative flex animate-rise items-center gap-3 overflow-hidden rounded-panel border border-border bg-surface-raised px-4 py-3"
            >
              <Icon aria-hidden className={`size-5 shrink-0 ${tones[task.state]}`} />
              <span className="font-medium">{task.label}</span>
              <span className="ml-auto min-w-0 truncate text-right text-sm text-fg-muted">
                {task.detail}
              </span>
              {task.state === "running" && (
                <span
                  aria-hidden
                  className="absolute bottom-0 left-0 h-0.5 w-2/5 animate-progress bg-linear-to-r from-transparent via-accent-alt to-transparent [animation-play-state:var(--cs-motion-ambient)]"
                />
              )}
            </li>
          );
        })}
      </ul>

      {error && (
        <p role="alert" className="mt-4 text-center text-sm text-danger">
          {error}
        </p>
      )}
      {finished && (
        <div className="mt-10 flex animate-rise flex-col items-center gap-3">
          <AriaButton
            ref={enter}
            data-part="next"
            data-focus-default
            onPress={onDone}
            className="inline-flex h-12 items-center justify-center rounded-control bg-linear-to-r from-accent to-accent-strong px-10 font-display text-base font-semibold tracking-widest text-accent-fg uppercase transition-[filter,box-shadow,scale] duration-(--cs-motion-duration-fast) data-focus-visible:focus-ring data-hovered:shadow-[0_0_28px_-4px] data-hovered:shadow-glow data-hovered:brightness-115 data-pressed:scale-[0.97]"
          >
            <Trans>Open my library</Trans>
          </AriaButton>
          {failed && (
            <p className="text-sm text-fg-subtle">
              <Trans>Anything that didn't work can be set up in Settings.</Trans>
            </p>
          )}
        </div>
      )}
    </section>
  );
}
