/** Where a game's play time comes from (#169): Caskade's own sessions and
 * what stores or launchers reported beyond them. */
import { Trans, useLingui } from "@lingui/react/macro";
import { formatDateTime, formatPlaytime } from "../../api/presentation";
import { useGamePlaytime } from "../../api/storeAccounts";

const providerNames: Record<string, string> = {
  steam: "Steam",
  gog: "GOG",
  epic: "Epic Games",
  lutris: "Lutris",
  heroic: "Heroic",
  legendary: "Legendary",
};

export function PlaytimeSources({
  gameId,
  totalSeconds,
}: {
  gameId: string;
  totalSeconds: number;
}) {
  const { t } = useLingui();
  const { data } = useGamePlaytime(gameId, totalSeconds);
  // Nothing beyond Caskade's own sessions: a source that reported no time
  // has nothing to add to the line above.
  if (!data || data.sources.length === 0 || data.reportedMs === 0) return null;
  const tracked = formatPlaytime(data.trackedMs / 1000);
  const reported = formatPlaytime(data.reportedMs / 1000);
  const names = [
    ...new Set(
      data.sources.map((source) => {
        const provider = providerNames[source.provider] ?? source.provider;
        return source.accountName ? `${provider} (${source.accountName})` : provider;
      }),
    ),
  ].join(", ");
  const details = data.sources
    .map((source) => {
      const provider = providerNames[source.provider] ?? source.provider;
      const total = source.totalMs === null ? t`unknown` : formatPlaytime(source.totalMs / 1000);
      const read = formatDateTime(new Date(source.observedAtMs));
      return t`${provider}: ${total}, read ${read}`;
    })
    .join("\n");
  return (
    <span data-part="playtime-sources" title={details} className="text-xs text-fg-subtle">
      <Trans>
        {tracked} in Caskade, {reported} more reported by {names}
      </Trans>
    </span>
  );
}
