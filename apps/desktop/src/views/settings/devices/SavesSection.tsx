/** Settings → Devices → Saves (#241): whether saves sync (set in What syncs), how many versions
 * each game keeps, and the save-location manifest's last update. */
import { Button, Select, SettingRow, SettingsSection } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { Save } from "lucide-react";
import { formatDateTime } from "../../../api/presentation";
import { useSaveManifest } from "../../../api/saves";
import { SettingsStatus, useSettingsSection } from "../useSettings";
import { useDeviceAdmin } from "./admin";
import type { SyncSettingsValue } from "./SyncChoices";

export function SavesSection() {
  const { t } = useLingui();
  const admin = useDeviceAdmin();
  const sync = useSettingsSection<SyncSettingsValue>("core.sync");
  const saves = useSettingsSection<{ manifestUrl: string; keep: number }>("core.saves");
  const manifest = useSaveManifest();
  const title = t`Saves`;
  const description = t`A version of a game's saves is kept each time you finish playing, and the newest goes to your other devices, so you can carry on where you left off.`;
  if (!sync.settings || !saves.settings) {
    // Loading says nothing; a failed load says why, with Try again.
    return sync.loadError || saves.loadError ? (
      <SettingsSection icon={Save} title={title} description={description}>
        <SettingsStatus section={sync} what={t`Sync settings`} className="" />
        <SettingsStatus section={saves} what={title} className="" />
      </SettingsSection>
    ) : null;
  }
  const syncing = (sync.settings.categories.saves ?? "sync") === "sync";
  const keep = saves.settings.keep ?? 20;
  const options = [5, 10, 20, 50, 100].map((count) => ({
    id: String(count),
    label: t`${count} versions`,
  }));
  if (!options.some((option) => option.id === String(keep)))
    options.push({ id: String(keep), label: t`${keep} versions` });
  const status = manifest.status.data;
  const checked = status?.checkedAtMs ?? null;
  const when = checked === null ? "" : formatDateTime(checked);
  return (
    <SettingsSection icon={Save} title={title} description={description}>
      {/* Whether saves sync is What syncs' Saves row; one control (#54). */}
      <p data-part="saves-sync" className="mt-1 text-sm text-fg-muted">
        {syncing ? (
          <Trans>Saves go to your other devices. Saves in What syncs above changes that.</Trans>
        ) : (
          <Trans>Saves stay on this device. Saves in What syncs above changes that.</Trans>
        )}
      </p>
      <SettingRow
        label={t`Versions to keep`}
        description={t`For each game. The newest stay, and so does any save that isn't settled between devices yet.`}
      >
        <Select
          options={options}
          value={String(keep)}
          isDisabled={!admin || saves.saving}
          onChange={(count) =>
            saves.setSettings((current) => ({ ...current, keep: Number(count) }))
          }
        />
      </SettingRow>
      <SettingsStatus section={sync} what={t`Sync settings`} />
      <SettingsStatus section={saves} what={t`Versions to keep`} />
      {manifest.available && (
        <SettingRow
          label={t`Save locations list`}
          description={
            status && !status.available
              ? t`Not downloaded yet. It downloads once the internet can be reached; saves in folders you set and emulator saves work meanwhile.`
              : checked !== null
                ? t`Last checked ${when}. It's checked once a day while online.`
                : t`It's checked once a day while online.`
          }
        >
          <Button
            data-spatial="focusable"
            isDisabled={manifest.update.isPending}
            onPress={() => manifest.update.mutate()}
          >
            {manifest.update.isPending ? t`Updating…` : <Trans>Update now</Trans>}
          </Button>
        </SettingRow>
      )}
      {manifest.update.isError && (
        <p role="alert" className="text-sm text-danger">
          {manifest.update.error.message}
        </p>
      )}
    </SettingsSection>
  );
}
