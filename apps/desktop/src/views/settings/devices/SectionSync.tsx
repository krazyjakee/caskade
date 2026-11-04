/** A settings section that syncs, kept on this device instead (#244):
 * `core.sync.sections`. Shown in the section itself, once this device has
 * paired devices; only an admin changes it, as with what syncs. */
import { SettingRow, Switch } from "@caskade/ui";
import { useLingui } from "@lingui/react/macro";
import { useQuery } from "@tanstack/react-query";
import { type SyncApi, syncKey, useProvidedSyncApi } from "../../../api/sync";
import { SettingsStatus, useSettingsSection } from "../useSettings";
import { useDeviceAdmin } from "./admin";
import type { CategoryState, SyncCategory, SyncSettingsValue } from "./SyncChoices";

/** The sections that can stay here on their own, and their categories. */
const SECTIONS = {
  "core.appearance": "appearance",
  "core.accounts": "accounts",
  "core.romm": "rommServers",
} as const satisfies Record<string, SyncCategory>;

export type SyncedSection = keyof typeof SECTIONS;

type Value = SyncSettingsValue & { sections?: Partial<Record<SyncedSection, CategoryState>> };

export function SectionSync({ namespace }: { namespace: SyncedSection }) {
  // Without devices (the browser preview, a panel on its own) there's
  // nothing to choose, and nothing else is asked for.
  const api = useProvidedSyncApi();
  return api ? <Choice namespace={namespace} api={api} /> : null;
}

function Choice({ namespace, api }: { namespace: SyncedSection; api: SyncApi }) {
  const { t } = useLingui();
  const admin = useDeviceAdmin();
  const status = useQuery({
    queryKey: [...syncKey, "status"],
    queryFn: () => api.status(),
    retry: false,
  });
  const sync = useSettingsSection<Value>("core.sync");
  if (!status.data?.devices.length) return null;
  const what = t`Same on your other devices`;
  if (!sync.settings) return <SettingsStatus section={sync} what={what} />;
  const categoryOn = (sync.settings.categories[SECTIONS[namespace]] ?? "sync") === "sync";
  const kept = sync.settings.sections?.[namespace] === "thisDeviceOnly";
  const description = !categoryOn
    ? t`Not synced: this kind of setting is This device only in Settings → Devices.`
    : kept
      ? t`Kept on this device. Changes here stay here, and other devices' don't arrive.`
      : t`Changes here reach your other devices.`;
  return (
    <>
      <SettingRow label={what} description={description}>
        <Switch
          isSelected={categoryOn && !kept}
          isDisabled={!admin || !categoryOn || sync.saving}
          onChange={(on) =>
            sync.setSettings((current) => ({
              ...current,
              sections: { ...current.sections, [namespace]: on ? "sync" : "thisDeviceOnly" },
            }))
          }
        />
      </SettingRow>
      <SettingsStatus section={sync} what={what} />
    </>
  );
}
