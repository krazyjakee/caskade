import { Button, SettingRow, SettingsSection, Slider, Switch, TextField } from "@caskade/ui";
import { plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { useQueryClient } from "@tanstack/react-query";
import { Folder, HardDrive, Heart, ShieldCheck, Trash2 } from "lucide-react";
import { useState } from "react";
import type { RommCacheSettings, RommOverview } from "../../../api/romm";
import { formatBytes } from "../../../api/storage";
import { rommKey, useRommApi, useRommWrite } from "./useRomm";

/** Downloaded ROMs: where they go, how much may stay, and whether favourites stay. */
export function CacheSection({ overview }: { overview: RommOverview }) {
  const { t } = useLingui();
  const api = useRommApi();
  const write = useRommWrite();
  const client = useQueryClient();
  const settings = overview.settings.cache;
  const [limit, setLimit] = useState(settings.limitGb);
  const [dir, setDir] = useState(settings.dir);
  const [error, setError] = useState<string | null>(null);
  const save = (patch: Partial<RommCacheSettings>) =>
    write.mutate(
      (api) => api.saveSettings({ ...overview.settings, cache: { ...settings, ...patch } }),
      { onError: (failure) => setError(failure.message) },
    );
  const refresh = () => void client.invalidateQueries({ queryKey: rommKey });
  const { cache } = overview;
  const used = cache.limit ? Math.min(100, (cache.used / cache.limit) * 100) : 0;

  return (
    <SettingsSection
      icon={HardDrive}
      title={t`Download Cache`}
      description={t`ROMs download the first time you play them and stay here, so they start instantly and work offline.`}
      aside={
        <Button
          size="sm"
          variant="danger"
          icon={Trash2}
          onPress={() =>
            void api
              .clearCache()
              .then(refresh)
              .catch((failure: Error) => setError(failure.message))
          }
          isDisabled={cache.games.every((game) => game.pinned)}
        >
          <Trans>Clear Cache</Trans>
        </Button>
      }
    >
      <div className="mt-3">
        <div
          data-part="cache-usage"
          className="h-2 overflow-hidden rounded-full bg-surface-base/80 ring-1 ring-border ring-inset"
        >
          <div
            className="h-full bg-linear-to-r from-accent to-accent-alt transition-[width] duration-(--cs-motion-duration-base)"
            style={{ width: `${used}%` }}
          />
        </div>
        <div className="mt-2 flex justify-between font-mono text-[11px] text-fg-subtle">
          <span>
            {plural({ total: cache.games.length }, { one: "# game", other: "# games" })} ·{" "}
            {formatBytes(cache.used)}
          </span>
          <span>
            {cache.limit
              ? t({
                  message: `limit ${{ size: formatBytes(cache.limit) }}`,
                  comment: "The download cache's size limit, as a size",
                })
              : t({ message: "no limit", comment: "The download cache has no size limit" })}
          </span>
        </div>
        {error && (
          <p role="alert" className="mt-2 text-xs text-danger">
            {error}
          </p>
        )}
      </div>
      <div className="mt-2 grid gap-x-8 xl:grid-cols-2">
        <div>
          <SettingRow
            icon={Folder}
            label={t({ message: "Location", context: "RomM download cache" })}
            description={t({
              message: `Empty uses romm-cache on your default storage location. Now: ${{ folder: cache.dir }}`,
              comment: "romm-cache is a folder name; keep it as is",
            })}
          >
            <TextField
              aria-label={t`Cache folder`}
              mono
              placeholder={t({ message: "Default", comment: "The default cache folder is used" })}
              value={dir}
              onChange={setDir}
              onBlur={() => {
                if (dir !== settings.dir) save({ dir: dir.trim() });
              }}
              className="w-64"
            />
          </SettingRow>
          <SettingRow
            icon={Trash2}
            label={t`Size limit`}
            description={t`The least recently played games are removed first. 0 is no limit.`}
          >
            <Slider
              aria-label={t`Cache size limit`}
              minValue={0}
              maxValue={1000}
              step={10}
              value={limit}
              onChange={setLimit}
              onChangeEnd={(value) => save({ limitGb: value })}
              format={(value) =>
                value === 0
                  ? t({ message: "No limit", comment: "The download cache's size limit" })
                  : t({ message: `${value} GB`, comment: "A size in gigabytes" })
              }
            />
          </SettingRow>
        </div>
        <div>
          <SettingRow icon={Heart} label={t`Never remove favourites`}>
            <Switch
              isSelected={settings.keepFavorites}
              onChange={(keepFavorites) => save({ keepFavorites })}
            />
          </SettingRow>
          <SettingRow
            icon={ShieldCheck}
            label={t`Verify downloads`}
            description={t`Check each file against the hash RomM reports.`}
          >
            <Switch isSelected={settings.verify} onChange={(verify) => save({ verify })} />
          </SettingRow>
        </div>
      </div>
    </SettingsSection>
  );
}
