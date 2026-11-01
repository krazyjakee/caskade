/** The refresh of chosen games' details in Activity (#176): how far it is,
 * what it changed, Cancel while it runs, and retrying the games that failed
 * or continuing a cancelled one. */
import { Button } from "@caskade/ui";
import { plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { RefreshCw } from "lucide-react";
import { useGamesRefresh } from "../../api/gameEdit";

export function RefreshCard() {
  const { t } = useLingui();
  const refresh = useGamesRefresh();
  const status = refresh?.status;
  if (!refresh || !status) return null;
  const state = status.running ? "running" : status.cancelled ? "cancelled" : "done";
  const percent = Math.round((status.done * 100) / Math.max(1, status.total));
  const again = (gameIds: string[]) => refresh.start.mutate({ gameIds, mode: status.mode });
  return (
    <section
      data-cs="games-refresh"
      data-status={state}
      aria-label={t`Refreshing game details`}
      className="mb-4 flex flex-col gap-3 rounded-panel border border-border bg-surface-raised p-4"
    >
      <div className="flex flex-wrap items-center gap-2">
        <RefreshCw
          aria-hidden
          // Looping, so it pauses with the ambient motion (reduced motion,
          // a CPU-painted window).
          style={{ animationPlayState: "var(--cs-motion-ambient, running)" }}
          className={`size-4 ${status.running ? "animate-worker text-accent-alt motion-reduce:animate-none" : ""}`}
        />
        <h2 data-part="title" className="mr-auto font-semibold">
          {state === "running"
            ? t`Refreshing game details`
            : state === "cancelled"
              ? t`Refreshing game details was cancelled`
              : t`Game details refreshed`}
        </h2>
        <span data-part="mode" className="text-xs text-fg-muted">
          {status.mode === "missing" ? t`Missing details only` : t`Automatic details refreshed`}
        </span>
      </div>
      <div
        data-part="progress"
        role="progressbar"
        aria-valuemin={0}
        aria-valuemax={status.total}
        aria-valuenow={status.done}
        aria-label={t`${status.done} of ${status.total} games`}
        className="h-1.5 overflow-hidden rounded-full bg-surface-base"
      >
        <div className="h-full bg-accent" style={{ width: `${percent}%` }} />
      </div>
      <ul data-part="counts" className="flex flex-wrap gap-x-5 gap-y-1 text-sm">
        <li>{plural(status.updated, { one: "# updated", other: "# updated" })}</li>
        <li className="text-fg-muted">
          {plural(status.unchanged, { one: "# unchanged", other: "# unchanged" })}
        </li>
        <li className="text-fg-muted">
          {plural(status.unmatched, { one: "# not found on IGDB", other: "# not found on IGDB" })}
        </li>
        <li className="text-fg-muted">
          {plural(status.skipped, { one: "# skipped", other: "# skipped" })}
        </li>
        {status.failed.length > 0 && (
          <li className="text-danger">
            {plural(status.failed.length, { one: "# failed", other: "# failed" })}
          </li>
        )}
      </ul>
      {refresh.start.error && (
        <p role="alert" className="text-sm text-danger">
          {refresh.start.error.message}
        </p>
      )}
      <div className="flex flex-wrap gap-2">
        {status.running && (
          <Button
            data-part="cancel"
            data-spatial="focusable"
            size="sm"
            onPress={() => void refresh.cancel()}
          >
            <Trans>Cancel</Trans>
          </Button>
        )}
        {!status.running && status.failed.length > 0 && (
          <Button
            data-part="retry"
            data-spatial="focusable"
            size="sm"
            variant="primary"
            isDisabled={refresh.start.isPending}
            onPress={() => again(status.failed)}
          >
            {plural(status.failed.length, {
              one: "Retry the failed game",
              other: "Retry the # failed games",
            })}
          </Button>
        )}
        {status.cancelled && status.remaining.length > 0 && (
          <Button
            data-part="continue"
            data-spatial="focusable"
            size="sm"
            isDisabled={refresh.start.isPending}
            onPress={() => again(status.remaining)}
          >
            {plural(status.remaining.length, {
              one: "Continue with the last game",
              other: "Continue with the # games left",
            })}
          </Button>
        )}
      </div>
    </section>
  );
}
