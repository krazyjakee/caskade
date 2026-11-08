import { Button, Checkbox, Select, SettingsSection, TextField } from "@caskade/ui";
import { plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { Combine, ListChecks, Split } from "lucide-react";
import { useState } from "react";
import type { EmulationPlatform, ReviewChoice, ScanJob } from "../../../api/emulation";
import { emulationKey, mergeScan, scanJobsKey, useEmulationApi } from "./EmulationProvider";

/** One game as the user is shaping it; `key` stays stable across edits. */
interface Item extends ReviewChoice {
  key: string;
  names: Record<string, string | null>;
}
const items = (job: ScanJob): Item[] =>
  job.review.map((game) => ({
    key: game.roms[0]?.path ?? game.title,
    roms: game.roms.map((rom) => rom.path),
    names: Object.fromEntries(game.roms.map((rom) => [rom.path, rom.name])),
    include: true,
    title: game.title,
    platform: game.platform,
    regions: game.regions,
  }));

/** The manual-scan review list: untick, fix titles,
 * platforms and regions in bulk, merge or split multi-disc games. The
 * choices are stored, so later automatic scans agree. */
export function ScanReview({ job, platforms }: { job: ScanJob; platforms: EmulationPlatform[] }) {
  const { t } = useLingui();
  const api = useEmulationApi();
  const client = useQueryClient();
  const [rows, setRows] = useState(() => items(job));
  const [selected, setSelected] = useState<Set<string>>(new Set());
  const [bulkRegion, setBulkRegion] = useState("");
  const done = (next: ScanJob) => {
    client.setQueryData<ScanJob[]>(scanJobsKey, (old) => mergeScan(old, next));
    void client.invalidateQueries({ queryKey: emulationKey });
  };
  const submit = useMutation({
    mutationFn: () =>
      api.submitReview(
        job.id,
        job.revision,
        rows.map(({ roms, include, title, platform, regions }) => ({
          roms,
          include,
          title: title.trim(),
          platform,
          regions,
        })),
      ),
    onSuccess: done,
  });
  const cancel = useMutation({ mutationFn: () => api.cancelScan(job.id), onSuccess: done });
  const update = (key: string, change: Partial<Item>) =>
    setRows((old) => old.map((row) => (row.key === key ? { ...row, ...change } : row)));
  const picked = rows.filter((row) => selected.has(row.key));
  const merge = () => {
    const [first, ...rest] = picked;
    if (!first) return;
    setRows((old) =>
      old
        .filter((row) => !rest.some((other) => other.key === row.key))
        .map((row) =>
          row.key === first.key
            ? {
                ...row,
                roms: picked.flatMap((item) => item.roms),
                names: Object.assign({}, ...picked.map((item) => item.names)),
              }
            : row,
        ),
    );
    setSelected(new Set());
  };
  const split = (item: Item) =>
    setRows((old) =>
      old.flatMap((row) =>
        row.key === item.key ? row.roms.map((rom) => ({ ...row, key: rom, roms: [rom] })) : [row],
      ),
    );
  const bulk = (change: Partial<Item>) =>
    setRows((old) => old.map((row) => (selected.has(row.key) ? { ...row, ...change } : row)));
  const included = rows.filter((row) => row.include).length;
  const busy = submit.isPending || cancel.isPending;
  const platformOptions = platforms.map((platform) => ({ id: platform.id, label: platform.name }));
  return (
    <SettingsSection
      icon={ListChecks}
      title={t`Review scan results`}
      description={t`Untick what isn't a game, fix titles, platforms and regions, and merge or split discs. Your choices are kept for later automatic scans.`}
    >
      <div data-cs="rom-review" className="mt-3 space-y-3">
        {job.warnings.map((warning) => (
          <p key={warning} className="text-xs text-warning">
            {warning}
          </p>
        ))}
        {rows.length === 0 ? (
          <p className="rounded-control border border-dashed border-border p-5 text-sm text-fg-muted">
            <Trans>No ROMs matched this scanner's profile.</Trans>
          </p>
        ) : (
          <>
            <div
              data-part="bulk"
              className="flex flex-wrap items-end gap-2 rounded-control border border-border bg-surface-overlay/40 p-3"
            >
              <Checkbox
                isSelected={picked.length === rows.length}
                isIndeterminate={picked.length > 0 && picked.length < rows.length}
                onChange={(all) => setSelected(new Set(all ? rows.map((row) => row.key) : []))}
              >
                <Trans>Select all</Trans>
              </Checkbox>
              <span className="text-xs text-fg-muted">
                <Trans comment="How many games of the scan review are selected">
                  {picked.length} selected
                </Trans>
              </span>
              <Button size="sm" icon={Combine} isDisabled={picked.length < 2} onPress={merge}>
                <Trans>Merge into one game</Trans>
              </Button>
              <Select
                aria-label={t`Set platform of selected games`}
                placeholder={t`Set platform…`}
                options={platformOptions}
                value=""
                isDisabled={picked.length === 0}
                onChange={(platform) => {
                  if (platform) bulk({ platform });
                }}
              />
              <div className="w-44">
                <TextField
                  aria-label={t`Regions for selected games`}
                  placeholder={t({
                    message: "Regions, e.g. usa",
                    comment: "Regions are comma-separated region codes; keep the example code",
                  })}
                  value={bulkRegion}
                  onChange={setBulkRegion}
                />
              </div>
              <Button
                size="sm"
                isDisabled={picked.length === 0}
                onPress={() =>
                  bulk({
                    regions: bulkRegion
                      .split(",")
                      .map((region) => region.trim())
                      .filter(Boolean),
                  })
                }
              >
                <Trans>Set regions</Trans>
              </Button>
            </div>
            <ul className="space-y-2">
              {rows.map((row) => (
                <li
                  key={row.key}
                  data-part="review-game"
                  data-included={row.include}
                  className="grid gap-3 rounded-control border border-border p-3 data-[included=false]:opacity-60 md:grid-cols-[auto_minmax(0,1fr)_14rem_auto]"
                >
                  <div className="flex flex-col gap-2">
                    <Checkbox
                      aria-label={t`Select ${{ title: row.title }}`}
                      isSelected={selected.has(row.key)}
                      onChange={(on) =>
                        setSelected((old) => {
                          const next = new Set(old);
                          if (on) next.add(row.key);
                          else next.delete(row.key);
                          return next;
                        })
                      }
                    />
                  </div>
                  <div className="min-w-0 space-y-2">
                    <TextField
                      aria-label={t`Title of ${{ rom: row.roms[0] }}`}
                      value={row.title}
                      onChange={(title) => update(row.key, { title })}
                    />
                    <ol aria-label={t`ROMs of ${{ title: row.title }}`} className="space-y-0.5">
                      {row.roms.map((rom) => (
                        <li key={rom} className="break-all font-mono text-xs text-fg-muted">
                          {rom}
                          {row.names[rom] ? ` · ${row.names[rom]}` : ""}
                        </li>
                      ))}
                    </ol>
                    {row.regions.length > 0 && (
                      <p className="text-xs text-fg-muted">
                        {t`Regions: ${{ regions: row.regions.join(", ") }}`}
                      </p>
                    )}
                  </div>
                  <Select
                    aria-label={t`Platform of ${{ title: row.title }}`}
                    options={platformOptions}
                    value={row.platform}
                    onChange={(platform) => update(row.key, { platform })}
                  />
                  <div className="flex flex-col items-end gap-2">
                    <Checkbox
                      isSelected={row.include}
                      onChange={(include) => update(row.key, { include })}
                    >
                      <Trans comment="Import this game from the scan">Import</Trans>
                    </Checkbox>
                    {row.roms.length > 1 && (
                      <Button size="sm" variant="ghost" icon={Split} onPress={() => split(row)}>
                        <Trans>Split discs</Trans>
                      </Button>
                    )}
                  </div>
                </li>
              ))}
            </ul>
          </>
        )}
        {(submit.error || cancel.error) && (
          <p role="alert" className="text-sm text-danger">
            {(submit.error ?? cancel.error)?.message}
          </p>
        )}
        <div className="flex justify-end gap-2">
          <Button isDisabled={busy} onPress={() => cancel.mutate()}>
            <Trans>Discard review</Trans>
          </Button>
          <Button
            variant="primary"
            isDisabled={busy || rows.some((row) => row.include && !row.title.trim())}
            onPress={() => submit.mutate()}
          >
            {submit.isPending
              ? t`Importing…`
              : t({
                  message: plural(
                    { total: included },
                    { one: "Import # game", other: "Import # games" },
                  ),
                })}
          </Button>
        </div>
      </div>
    </SettingsSection>
  );
}
