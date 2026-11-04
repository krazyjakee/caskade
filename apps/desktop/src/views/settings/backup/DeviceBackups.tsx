/** Backups kept on the user's other devices (#243), in Settings → Backup:
 * which paired device also keeps each backup, and restoring one it keeps,
 * fetched here first, through the usual restore. Shown once this device
 * has paired devices. */
import { Button, Select, SettingRow } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation, useQuery } from "@tanstack/react-query";
import { MonitorSmartphone } from "lucide-react";
import { type DeviceBackups, useBackupApi } from "../../../api/backup";
import { formatBytes, formatDateTime } from "../../../api/presentation";
import { type SyncApi, syncKey, useProvidedSyncApi } from "../../../api/sync";
import { SettingsStatus, useSettingsSection } from "../useSettings";

/** Paired devices, or none without devices. */
function usePaired(api: SyncApi) {
  return useQuery({
    queryKey: [...syncKey, "status"],
    queryFn: () => api.status(),
    retry: false,
  }).data?.devices;
}

/** Also keep backups on: a paired device, or only here. */
export function KeepOnDevice() {
  const api = useProvidedSyncApi();
  return api ? <KeepOn api={api} /> : null;
}

function KeepOn({ api }: { api: SyncApi }) {
  const { t } = useLingui();
  const paired = usePaired(api);
  const preferences = useSettingsSection<{ keepOnDevice?: string | null }>("core.backups");
  const settings = preferences.settings;
  if (!paired?.length || !settings) return null;
  const chosen = settings.keepOnDevice ?? "";
  const options = [
    { id: "", label: t({ message: "Only this device", context: "keep backups on" }) },
    ...paired.map((device) => ({ id: device.id, label: device.name })),
  ];
  // A device forgotten since stays listed until changed.
  if (chosen && !options.some((option) => option.id === chosen))
    options.push({ id: chosen, label: t`A device no longer paired` });
  const label = t`Also keep backups on`;
  return (
    <>
      <SettingRow
        label={label}
        description={t`Each backup also goes to that device the next time they sync, even when it was away. It keeps them apart from its own, as many as are kept here.`}
      >
        <Select
          options={options}
          value={chosen}
          onChange={(device) =>
            preferences.setSettings((current) => ({ ...current, keepOnDevice: device || null }))
          }
        />
      </SettingRow>
      <SettingsStatus section={preferences} what={label} />
    </>
  );
}

/** Backups paired devices keep, each restorable here. */
export function DeviceBackupList({
  open,
  busy,
}: {
  /** Opens a backup here for the restore preview. */
  open: (path: string) => Promise<void>;
  busy: boolean;
}) {
  const api = useProvidedSyncApi();
  const backups = useBackupApi();
  return api && backups.onDevices ? <List open={open} busy={busy} /> : null;
}

function List({ open, busy }: { open: (path: string) => Promise<void>; busy: boolean }) {
  const { t } = useLingui();
  const api = useBackupApi();
  const held = useQuery({
    queryKey: ["backups", "on-devices"],
    queryFn: () => api.onDevices?.() ?? Promise.resolve([] as DeviceBackups[]),
  });
  const fetch = useMutation({
    mutationFn: async (hash: string) => {
      if (!api.fetchFromDevice) throw new Error("Devices are unavailable");
      const path = await api.fetchFromDevice(hash);
      await open(path);
    },
  });
  const devices = held.data ?? [];
  if (devices.length === 0) return null;
  return (
    <div data-part="device-backups" className="mt-4 flex flex-col gap-3">
      <h3 className="flex items-center gap-2 text-sm font-semibold">
        <MonitorSmartphone aria-hidden className="size-4 text-fg-muted" />
        <Trans>On your other devices</Trans>
      </h3>
      {devices.map((device) => {
        const holder = device.deviceName;
        return (
          <section key={device.deviceId} aria-label={t`Kept on ${holder}`}>
            <p className="text-xs text-fg-muted">{t`Kept on ${holder}`}</p>
            <ul className="mt-1 flex flex-col gap-1">
              {device.backups.map((backup) => {
                const from =
                  backup.originName || t({ message: "Another device", context: "backup of" });
                const when = formatDateTime(backup.createdMs);
                const fetching = fetch.isPending && fetch.variables === backup.hash;
                return (
                  <li
                    key={backup.hash}
                    className="flex flex-wrap items-center justify-between gap-3 text-sm"
                  >
                    <span>
                      {t`${from}, ${when}`}
                      <span className="ml-2 text-xs text-fg-muted">{formatBytes(backup.size)}</span>
                    </span>
                    <Button
                      data-spatial="focusable"
                      size="sm"
                      isDisabled={busy || fetch.isPending}
                      aria-label={t`Restore the backup of ${from} from ${when}`}
                      onPress={() => fetch.mutate(backup.hash)}
                    >
                      {fetching ? (
                        <Trans>Getting it from {holder}…</Trans>
                      ) : (
                        <Trans>Restore…</Trans>
                      )}
                    </Button>
                  </li>
                );
              })}
            </ul>
          </section>
        );
      })}
      {fetch.isError && (
        <p role="alert" className="text-sm text-danger">
          {fetch.error.message}
        </p>
      )}
    </div>
  );
}
