/** The plugin and theme catalogue in Settings → Plugins (#183): curated
 * add-ons with what each means here, searchable by text and filtered by
 * kind, read offline from the last fetch. Each opens its details, where it
 * installs, updates and is removed through the shared package service. */
import { Button, SegmentedControl, Switch, TextField } from "@caskade/ui";
import { plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation } from "@tanstack/react-query";
import { CloudOff, RefreshCw, Search } from "lucide-react";
import { useEffect, useRef, useState } from "react";
import {
  type AppMode,
  type CatalogueItem,
  useAddonRecords,
  useAddonsApi,
  useCatalogue,
  useRefreshCatalogue,
} from "../../api/addons";
import { formatDateTime } from "../../api/presentation";
import { useMode } from "../../shell/console/mode";
import { FailureNotice } from "../FailureNotice";
import { AddonDetails } from "./AddonDetails";
import { DeviceOffers } from "./DeviceOffers";

type Kind = "all" | "plugin" | "theme";

/** Fetched again on opening when the update preference says so and the
 * catalogue is older than this. */
const STALE_MS = 24 * 3_600_000;

export function AddonCatalogue() {
  const { t } = useLingui();
  const mode: AppMode = useMode().mode === "console" ? "console" : "desktop";
  const catalogue = useCatalogue(mode);
  const refresh = useRefreshCatalogue(mode);
  const records = useAddonRecords();
  const api = useAddonsApi();
  // Its failure shows under the switch, not nowhere (#55).
  const preference = useMutation({
    mutationFn: (notify: boolean) => api.setUpdatePreference(notify ? "notify" : "manual"),
    onSettled: () => records.refetch(),
  });
  const [query, setQuery] = useState("");
  const [kind, setKind] = useState<Kind>("all");
  const [open, setOpen] = useState<string | null>(null);
  const status = catalogue.data?.status;
  // Checks for updates once when Settings opens, unless the user said not to.
  const checked = useRef(false);
  useEffect(() => {
    if (checked.current || !status || !records.data) return;
    checked.current = true;
    const stale = status.fetchedAtMs === null || Date.now() - status.fetchedAtMs > STALE_MS;
    if (records.data.updates === "notify" && stale) refresh.mutate();
  }, [status, records.data, refresh]);
  const words = query.trim().toLowerCase();
  const items = (catalogue.data?.items ?? []).filter(
    (item) =>
      (kind === "all" || item.entry.kind === kind) &&
      (!words ||
        [item.entry.name, item.entry.summary, item.entry.author, ...item.entry.categories].some(
          (text) => text.toLowerCase().includes(words),
        )),
  );
  const updates = (catalogue.data?.items ?? []).filter(
    (item) =>
      item.compatibility.state === "updateAvailable" &&
      records.data?.addons?.[item.entry.id]?.skipped !== item.compatibility.release?.version,
  ).length;
  const selected = catalogue.data?.items.find((item) => item.entry.id === open) ?? null;
  return (
    <section
      data-cs="addon-catalogue"
      aria-labelledby="addon-catalogue"
      className="flex flex-col gap-3"
    >
      <div className="flex flex-wrap items-center justify-between gap-3">
        <h2 id="addon-catalogue" className="font-display text-lg font-semibold">
          <Trans>Plugin and theme catalogue</Trans>
        </h2>
        <Button
          data-part="refresh"
          variant="secondary"
          icon={RefreshCw}
          onPress={() => refresh.mutate()}
          isDisabled={refresh.isPending}
        >
          {refresh.isPending ? <Trans>Checking…</Trans> : <Trans>Check for updates</Trans>}
        </Button>
      </div>
      <p data-part="status" role="status" className="flex items-center gap-2 text-sm text-fg-muted">
        {status?.offline && <CloudOff aria-hidden className="size-4" />}
        {!status
          ? t`Loading the catalogue…`
          : status.fetchedAtMs === null
            ? status.error
              ? t`The catalogue couldn't be fetched: ${status.error}`
              : t`The catalogue hasn't been fetched yet.`
            : status.offline
              ? t`Offline: showing the catalogue from ${formatDateTime(status.fetchedAtMs)}. Installing needs a connection.`
              : status.error
                ? t`Showing the catalogue from ${formatDateTime(status.fetchedAtMs)}; the last check failed: ${status.error}`
                : t`Updated ${formatDateTime(status.fetchedAtMs)}.`}
        {updates > 0 && (
          <span data-part="updates" className="text-accent-alt">
            {plural(updates, { one: "# update", other: "# updates" })}
          </span>
        )}
      </p>
      {records.data && (
        <Switch
          data-part="update-preference"
          isSelected={
            preference.isPending ? preference.variables : records.data.updates === "notify"
          }
          isDisabled={preference.isPending}
          onChange={(notify) => preference.mutate(notify)}
        >
          {t`Check for add-on updates when Settings opens`}
        </Switch>
      )}
      {preference.error && <FailureNotice variant="inline" error={preference.error} />}
      <DeviceOffers items={catalogue.data?.items ?? []} open={setOpen} />
      <div className="flex flex-wrap items-center gap-3">
        <TextField
          aria-label={t`Search the catalogue`}
          placeholder={t`Search plugins and themes`}
          icon={Search}
          value={query}
          onChange={setQuery}
          className="min-w-56 flex-1"
        />
        <SegmentedControl
          aria-label={t`Show`}
          value={kind}
          onChange={setKind}
          options={[
            { id: "all", label: t({ message: "All", context: "add-on kind" }) },
            { id: "plugin", label: t`Plugins` },
            { id: "theme", label: t`Themes` },
          ]}
        />
      </div>
      {catalogue.isError && (
        <p role="alert" className="text-sm text-danger">
          {catalogue.error.message}
        </p>
      )}
      {catalogue.data && items.length === 0 && (
        <p className="rounded-panel border border-dashed border-border-strong p-6 text-center text-sm text-fg-muted">
          {catalogue.data.items.length === 0
            ? t`The catalogue is empty.`
            : t`Nothing in the catalogue matches.`}
        </p>
      )}
      {items.length > 0 && (
        <ul aria-label={t`Catalogue`} className="grid gap-3 md:grid-cols-2">
          {items.map((item) => (
            <Row key={item.entry.id} item={item} open={() => setOpen(item.entry.id)} />
          ))}
        </ul>
      )}
      {selected && (
        <AddonDetails
          key={selected.entry.id}
          item={selected}
          mode={mode}
          offline={status?.offline ?? false}
          record={records.data?.addons?.[selected.entry.id]}
          close={() => setOpen(null)}
        />
      )}
    </section>
  );
}

/** A badge for what an entry means here. */
export function StateBadge({ item }: { item: CatalogueItem }) {
  const { t } = useLingui();
  const { state, installedVersion, release } = item.compatibility;
  const tone = {
    available: "border-border-strong text-fg-muted",
    installed: "border-success/50 text-success",
    updateAvailable: "border-accent-alt/60 text-accent-alt",
    incompatible: "border-warning/50 text-warning",
  }[state];
  const label = {
    available: t`Available`,
    installed: installedVersion ? t`Installed ${installedVersion}` : t`Installed`,
    updateAvailable: t`Update to ${release?.version ?? ""}`,
    incompatible: t`Not compatible`,
  }[state];
  return (
    <span
      data-part="state"
      data-state={state}
      className={`shrink-0 rounded-control border px-2 py-0.5 text-xs ${tone}`}
    >
      {label}
    </span>
  );
}

function Row({ item, open }: { item: CatalogueItem; open: () => void }) {
  const { t } = useLingui();
  const { entry } = item;
  return (
    <li
      data-part="item"
      data-kind={entry.kind}
      className="flex flex-col gap-2 rounded-panel border border-border bg-surface-raised p-4"
    >
      <div className="flex items-start gap-2">
        <div className="min-w-0 flex-1">
          <p className="truncate font-semibold">{entry.name}</p>
          <p className="text-xs text-fg-muted">
            {entry.kind === "plugin" ? t`Plugin` : t`Theme`} · {entry.author}
          </p>
        </div>
        <StateBadge item={item} />
      </div>
      <p className="text-sm text-fg-muted">{entry.summary}</p>
      <div>
        <Button
          data-part="details"
          data-spatial="focusable"
          size="sm"
          aria-label={t`Details of ${entry.name}`}
          onPress={open}
        >
          <Trans>Details</Trans>
        </Button>
      </div>
    </li>
  );
}
