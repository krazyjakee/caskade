/** Set up from another device (#276): after its library arrived and this
 * device's folder is chosen, the games that device has installed and that
 * run here can be copied over the local network. Nothing copies until
 * chosen; each chosen game becomes a copy job in Activity (#233), and setup
 * goes on while they arrive. */
import { Button, Checkbox } from "@caskade/ui";
import { plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation, useQuery } from "@tanstack/react-query";
import { HardDrive } from "lucide-react";
import { useState } from "react";
import { errorMessage } from "../api/reply";
import { useStorageApi } from "../api/StorageProvider";
import { formatBytes } from "../api/storage";
import { useProvidedSyncApi, useSyncStatus } from "../api/sync";
import { StepHeading } from "./Onboarding";
import { Continue } from "./Questions";

export function GamesStep({ onNext }: { onNext(): void }) {
  const { t } = useLingui();
  const api = useProvidedSyncApi();
  const storage = useStorageApi();
  const source = useSyncStatus().data?.devices[0];
  const games = useQuery({
    queryKey: ["onboarding", "deviceGames", source?.id],
    queryFn: () => (api && source ? api.deviceGames(source.id) : Promise.resolve([])),
    enabled: Boolean(api && source),
  });
  const roots = useQuery({
    queryKey: ["storage", "roots"],
    queryFn: () => storage?.list() ?? Promise.resolve([]),
  });
  const [chosen, setChosen] = useState<Set<string>>(new Set());
  const copy = useMutation({
    mutationFn: async () => {
      if (!api || !source) return;
      for (const gameId of chosen) await api.copyHere(gameId, source.id);
    },
    onSuccess: onNext,
  });

  const list = games.data ?? [];
  const root = roots.data?.find((row) => row.root.isDefault) ?? roots.data?.[0];
  const free = root?.capabilities.freeBytes ?? null;
  const sizes = list.filter((game) => chosen.has(game.gameId)).map((game) => game.sizeBytes ?? 0);
  const needed = sizes.reduce((total, size) => total + size, 0);
  // A copy arrives in the cache before it installs: on one disk, the
  // largest game takes twice its size while it does (#233).
  const peak = needed + Math.max(0, ...sizes);
  const fits = free === null || peak <= free;
  const name = source?.name ?? "";
  const toggle = (gameId: string, on: boolean) =>
    setChosen((current) => {
      const next = new Set(current);
      if (on) next.add(gameId);
      else next.delete(gameId);
      return next;
    });

  if (games.isPending && source)
    return (
      <section data-part="question" data-question="games">
        <StepHeading title={t`Games from ${name}`} description={t`Looking at what it has…`} />
      </section>
    );

  if (list.length === 0)
    return (
      <section data-part="question" data-question="games">
        <StepHeading
          title={t`Games from ${name}`}
          description={t`${name} has no installed games that can be copied to this device. Install games here as usual.`}
        />
        <Continue onPress={onNext} />
      </section>
    );

  const count = chosen.size;
  const neededText = formatBytes(needed);
  const peakText = formatBytes(peak);
  const freeText = free !== null ? formatBytes(free) : null;
  return (
    <section data-part="question" data-question="games">
      <StepHeading
        title={t`Games from ${name}`}
        description={t`Choose the games to copy here over your network. They copy in the background while you finish setting up. Nothing copies unless you choose it.`}
      />
      <ul
        aria-label={t`Games to copy`}
        className="flex max-h-[40vh] flex-col gap-1 overflow-y-auto rounded-panel border border-border-strong bg-surface-raised p-2"
      >
        {list.map((game) => (
          <li
            key={game.gameId}
            data-part="device-game"
            className="flex items-center justify-between gap-4 rounded-control px-3 py-2"
          >
            <Checkbox
              isSelected={chosen.has(game.gameId)}
              onChange={(on) => toggle(game.gameId, on)}
            >
              {game.title}
            </Checkbox>
            <span className="shrink-0 text-xs text-fg-muted">
              {game.sizeBytes !== null ? formatBytes(game.sizeBytes) : t`Size unknown`}
              {game.throughProton && ` · ${t`Runs through Proton`}`}
            </span>
          </li>
        ))}
      </ul>
      <p
        data-part="space"
        data-status={fits ? "fits" : "too-big"}
        className={`mt-3 flex items-center gap-2 text-sm ${fits ? "text-fg-muted" : "text-danger"}`}
        role={fits ? undefined : "alert"}
      >
        <HardDrive aria-hidden className="size-4 shrink-0" />
        {freeText === null
          ? t`${neededText} chosen`
          : fits
            ? t`${neededText} chosen, ${freeText} free here`
            : t`${neededText} chosen needs ${peakText} while it copies, but only ${freeText} is free here`}
      </p>
      {copy.error && (
        <p role="alert" className="mt-3 text-center text-sm text-danger">
          {errorMessage(copy.error)}
        </p>
      )}
      <Continue
        onPress={() => (count === 0 ? onNext() : copy.mutate())}
        isDisabled={!fits || copy.isPending}
      >
        {count === 0 ? (
          <Trans>Continue without copying</Trans>
        ) : (
          plural(count, { one: "Copy # game", other: "Copy # games" })
        )}
      </Continue>
      {count > 0 && (
        <div className="mt-3 flex justify-center">
          <Button variant="ghost" onPress={onNext} isDisabled={copy.isPending}>
            <Trans>Not now</Trans>
          </Button>
        </div>
      )}
    </section>
  );
}
