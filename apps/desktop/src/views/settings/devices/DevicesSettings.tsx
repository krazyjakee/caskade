/** Settings → Devices (#231): this device, the devices paired with it,
 * Caskade on this network to pair, what syncs, and copies and shares under
 * way. Devices find each other on the local network and sync without the
 * internet; elsewhere they connect directly, or through a relay unless
 * that is turned off.
 *
 * Every account sees the list; only an admin pairs, forgets, renames this
 * device or changes what syncs (the core refuses the rest whatever this
 * shows). Waking a device is #234. */
import { Button, Select, SettingRow, SettingsSection, Switch, TextField } from "@caskade/ui";
import { msg } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import {
  Download,
  KeyRound,
  Link2,
  Monitor,
  MonitorSmartphone,
  Radar,
  RefreshCw,
  ShieldCheck,
  Trash2,
} from "lucide-react";
import { useState } from "react";
import { formatDateTime } from "../../../api/presentation";
import {
  type DeviceKind,
  type NearbyDevice,
  type PairedDevice,
  useSyncAction,
  useSyncStatus,
} from "../../../api/sync";
import { FailureNotice } from "../../../components/FailureNotice";
import { DeviceIcon, LinkBadge, osNames } from "../../../components/sync/common";
import { DevicePlaying } from "../../game/PlayingElsewhere";
import { useSettingsSection } from "../useSettings";
import { useDeviceAdmin } from "./admin";
import { CopiesAndShares } from "./CopiesAndShares";
import { SavesSection } from "./SavesSection";
import { SyncChoices, type SyncSettingsValue } from "./SyncChoices";

export function DevicesSettings() {
  const admin = useDeviceAdmin();
  return (
    <div className="flex flex-col gap-6">
      {!admin && (
        <p role="note" className="text-sm text-fg-muted">
          <Trans>Only an admin can pair devices, forget them or change what syncs.</Trans>
        </p>
      )}
      <IdentityNotice />
      <ThisDeviceSection />
      <PairedSection />
      {admin && <NearbySection />}
      <SyncChoices />
      <SavesSection />
      <CopiesAndShares />
    </div>
  );
}

/** After the keychain lost this device's key: a new identity, once asked. */
function IdentityNotice() {
  const admin = useDeviceAdmin();
  const { data } = useSyncStatus();
  const create = useSyncAction((api, _: undefined) => api.createIdentity());
  const retry = useSyncAction((api, _: undefined) => api.retryIdentity());
  const [confirming, setConfirming] = useState(false);
  const identity = data?.thisDevice.identity;
  if (!identity || identity === "ready" || identity === "starting") return null;
  return (
    <div
      role="alert"
      className="flex flex-col gap-3 rounded-panel border border-warning/60 bg-surface-raised p-4"
    >
      <p className="flex items-center gap-2 font-medium">
        <KeyRound aria-hidden className="size-4 text-warning" />
        {identity === "keychainUnavailable" ? (
          <Trans>Sync is waiting for the system keychain</Trans>
        ) : (
          <Trans>This device's sync key is gone</Trans>
        )}
      </p>
      <p className="text-sm text-fg-muted">
        {identity === "keychainUnavailable" ? (
          <Trans>
            The keychain didn't answer. Unlock it, then try again. Everything else works meanwhile.
          </Trans>
        ) : (
          <Trans>
            The keychain no longer holds the key your other devices know this one by. A new key
            makes this a new device to them: pair each of them again afterwards.
          </Trans>
        )}
      </p>
      {admin && (
        <div className="flex flex-wrap gap-2">
          {identity === "keychainUnavailable" ? (
            <Button icon={RefreshCw} onPress={() => retry.mutate(undefined)}>
              <Trans>Try again</Trans>
            </Button>
          ) : confirming ? (
            <>
              <Button variant="danger" onPress={() => create.mutate(undefined)}>
                <Trans>Make a new key</Trans>
              </Button>
              <Button variant="ghost" onPress={() => setConfirming(false)}>
                <Trans>Not now</Trans>
              </Button>
            </>
          ) : (
            <Button icon={KeyRound} onPress={() => setConfirming(true)}>
              <Trans>Make a new key…</Trans>
            </Button>
          )}
        </div>
      )}
      {(create.error ?? retry.error) && (
        <FailureNotice variant="inline" error={create.error ?? retry.error} />
      )}
    </div>
  );
}

const kinds = [
  { id: "desktop", label: msg({ message: "Desktop", context: "device kind" }) },
  { id: "laptop", label: msg({ message: "Laptop", context: "device kind" }) },
  { id: "handheld", label: msg({ message: "Handheld", context: "device kind" }) },
  { id: "tv", label: msg({ message: "TV", context: "device kind" }) },
] as const;

function ThisDeviceSection() {
  const { t } = useLingui();
  const admin = useDeviceAdmin();
  const { data } = useSyncStatus();
  const sync = useSettingsSection<SyncSettingsValue>("core.sync");
  const rename = useSyncAction((api, name: string) => api.rename(name));
  const setKind = useSyncAction((api, kind: DeviceKind) => api.setKind(kind));
  const kindOptions = kinds.map(({ id, label }) => ({ id, label: t(label) }));
  const [name, setName] = useState<string | null>(null);
  if (!data) return null;
  const device = data.thisDevice;
  const addresses = device.lanAddresses.join(", ");
  const neverRelay = sync.settings?.neverRelay ?? device.neverRelay;
  return (
    <SettingsSection
      icon={Monitor}
      title={t`This device`}
      description={t`Your other devices know it by this name. Devices on the same network find each other and sync without the internet.`}
    >
      <div className="flex flex-wrap items-end gap-4">
        <TextField
          label={t`Device name`}
          value={name ?? device.name}
          isReadOnly={!admin}
          onChange={setName}
          onBlur={() => {
            if (name !== null && name !== device.name) rename.mutate(name);
          }}
          className="min-w-64 flex-1"
        />
        <Select
          label={t`Kind`}
          options={kindOptions}
          value={device.kind}
          isDisabled={!admin}
          onChange={(kind) => setKind.mutate(kind)}
          className="w-44"
        />
      </div>
      <p className="mt-2 text-xs text-fg-subtle">
        {addresses ? (
          <Trans>On this network at {addresses}.</Trans>
        ) : (
          <Trans>Not on a local network.</Trans>
        )}{" "}
        {device.internet === true ? (
          <Trans>Connected to the internet.</Trans>
        ) : device.internet === false ? (
          <Trans>No internet: devices on this network still sync.</Trans>
        ) : null}
      </p>
      {(rename.error ?? setKind.error) && (
        <FailureNotice variant="inline" error={rename.error ?? setKind.error} />
      )}
      <div className="mt-4">
        <SettingRow
          icon={ShieldCheck}
          label={t`Never use relays`}
          description={t`Devices connect only directly: on this network, or over the internet where the routers allow it. Some devices away from home may then not connect.`}
        >
          <Switch
            isSelected={neverRelay}
            isDisabled={!admin || !sync.settings || sync.saving}
            onChange={(on) => sync.setSettings((current) => ({ ...current, neverRelay: on }))}
          />
        </SettingRow>
        <SettingRow
          icon={Download}
          label={t`Accept copies from my devices`}
          description={t`A game another of your devices sends here is copied into the default storage without asking.`}
        >
          <Switch
            isSelected={sync.settings?.acceptCopies ?? false}
            isDisabled={!admin || !sync.settings || sync.saving}
            onChange={(on) => sync.setSettings((current) => ({ ...current, acceptCopies: on }))}
          />
        </SettingRow>
      </div>
    </SettingsSection>
  );
}

function PairedSection() {
  const { t } = useLingui();
  const { data } = useSyncStatus();
  if (!data) return null;
  return (
    <SettingsSection
      icon={MonitorSmartphone}
      title={t`Your devices`}
      description={t`Paired devices keep your library, play history and collections in step. Each keeps its own games, folders and emulators.`}
    >
      {data.devices.length === 0 ? (
        <p className="text-sm text-fg-muted">
          <Trans>No devices paired yet.</Trans>
        </p>
      ) : (
        <ul className="flex flex-col gap-2">
          {data.devices.map((device) => (
            <DeviceRow key={device.id} device={device} />
          ))}
        </ul>
      )}
    </SettingsSection>
  );
}

function DeviceRow({ device }: { device: PairedDevice }) {
  const { t } = useLingui();
  const admin = useDeviceAdmin();
  const sync = useSyncAction((api, id: string) => api.syncNow(id));
  const forget = useSyncAction((api, id: string) => api.forget(id));
  const [confirming, setConfirming] = useState(false);
  const os = osNames[device.os];
  const name = device.name;
  const last = device.lastSyncMs === null ? null : formatDateTime(device.lastSyncMs);
  return (
    <li
      data-cs="paired-device"
      data-status={device.link}
      className="flex flex-wrap items-center gap-4 rounded-control border border-border p-3"
    >
      <DeviceIcon kind={device.kind} className="size-6 shrink-0 text-fg-muted" />
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <span className="flex flex-wrap items-center gap-3">
          <span className="font-medium">{name}</span>
          <LinkBadge link={device.link} />
        </span>
        <span data-part="sync" className="text-xs text-fg-subtle">
          {device.syncing ? (
            <Trans>{os} · Syncing…</Trans>
          ) : last ? (
            <Trans>
              {os} · Synced {last}
            </Trans>
          ) : (
            <Trans>{os} · Not synced yet</Trans>
          )}
        </span>
        {device.syncError && !device.syncing && (
          <span data-part="sync-error" className="text-xs text-fg-muted">
            <Trans>Last try didn't reach it. It syncs when it's back.</Trans>
          </span>
        )}
        <DevicePlaying deviceId={device.id} />
      </div>
      <div className="flex flex-wrap gap-2">
        <Button
          size="sm"
          icon={RefreshCw}
          isDisabled={device.syncing || sync.isPending}
          onPress={() => sync.mutate(device.id)}
        >
          <Trans>Sync now</Trans>
        </Button>
        {admin &&
          (confirming ? (
            <>
              <Button size="sm" variant="danger" onPress={() => forget.mutate(device.id)}>
                <Trans>Forget {name}</Trans>
              </Button>
              <Button size="sm" variant="ghost" onPress={() => setConfirming(false)}>
                <Trans>Keep</Trans>
              </Button>
            </>
          ) : (
            <Button
              size="sm"
              variant="ghost"
              icon={Trash2}
              aria-label={t`Forget ${name}`}
              onPress={() => setConfirming(true)}
            />
          ))}
      </div>
      {confirming && (
        <p className="w-full text-xs text-fg-muted">
          <Trans>
            It stops syncing with this device. Nothing is deleted on either device, and what synced
            stays.
          </Trans>
        </p>
      )}
      {(sync.error ?? forget.error) && (
        <FailureNotice variant="inline" error={sync.error ?? forget.error} />
      )}
    </li>
  );
}

function NearbySection() {
  const { t } = useLingui();
  const { data } = useSyncStatus();
  const pair = useSyncAction((api, id: string) => api.pair(id));
  const makeLink = useSyncAction((api, _: undefined) => api.pairingLink());
  const accept = useSyncAction((api, link: string) => api.acceptPairingLink(link));
  const [pasted, setPasted] = useState("");
  const [copied, setCopied] = useState(false);
  if (!data) return null;
  return (
    <SettingsSection
      icon={Radar}
      title={t`Pair a device`}
      description={t`Caskade on this network shows here. Both devices show a code: pair when they match.`}
    >
      {data.nearby.length === 0 ? (
        <p role="status" className="text-sm text-fg-muted">
          <Trans>Looking for Caskade on this network…</Trans>
        </p>
      ) : (
        <ul className="flex flex-col gap-2">
          {data.nearby.map((device) => (
            <NearbyRow
              key={device.id}
              device={device}
              isPending={pair.isPending || data.pairing?.state === "waiting"}
              onPair={() => pair.mutate(device.id)}
            />
          ))}
        </ul>
      )}
      {pair.error && <FailureNotice variant="inline" error={pair.error} />}
      <div className="mt-6 flex flex-col gap-3 border-t border-border pt-4">
        <p className="text-sm text-fg-muted">
          <Trans>
            Device not here? Guest Wi‑Fi and some routers keep devices apart, and devices elsewhere
            never show. Pair with a link instead: copy it here and paste it on the other device. It
            works once, for ten minutes.
          </Trans>
        </p>
        <div className="flex flex-wrap gap-2">
          <Button
            icon={Link2}
            isDisabled={makeLink.isPending}
            onPress={async () => {
              const link = await makeLink.mutateAsync(undefined);
              const wrote = await navigator.clipboard?.writeText(link.url).then(
                () => true,
                () => false,
              );
              setCopied(wrote === true);
            }}
          >
            {copied ? t`Pairing link copied` : t`Copy a pairing link`}
          </Button>
        </div>
        {makeLink.data && (
          <p
            data-part="pairing-link"
            className="break-all rounded-control border border-border p-2 font-mono text-xs text-fg-muted select-all"
          >
            {makeLink.data.url}
          </p>
        )}
        {makeLink.error && <FailureNotice variant="inline" error={makeLink.error} />}
        <TextField
          label={t`Paste a pairing link from another device`}
          value={pasted}
          onChange={setPasted}
          mono
          action={
            <Button
              isDisabled={!pasted.trim() || accept.isPending}
              onPress={() => accept.mutate(pasted.trim(), { onSuccess: () => setPasted("") })}
            >
              <Trans>Pair</Trans>
            </Button>
          }
        />
        {accept.isSuccess && (
          <p role="status" className="text-sm text-status-installed">
            <Trans>Paired with {accept.data.name}.</Trans>
          </p>
        )}
        {accept.error && <FailureNotice variant="inline" error={accept.error} />}
      </div>
    </SettingsSection>
  );
}

function NearbyRow({
  device,
  isPending,
  onPair,
}: {
  device: NearbyDevice;
  isPending: boolean;
  onPair: () => void;
}) {
  const os = osNames[device.os];
  const address = device.addresses[0] ?? "";
  return (
    <li className="flex items-center gap-4 rounded-control border border-dashed border-border-strong p-3">
      <DeviceIcon kind={device.kind} className="size-6 shrink-0 text-fg-muted" />
      <div className="flex min-w-0 flex-1 flex-col">
        <span className="font-medium">{device.name}</span>
        <span className="font-mono text-xs text-fg-subtle">
          {os} · {address}
        </span>
      </div>
      <Button variant="primary" isDisabled={isPending} onPress={onPair}>
        <Trans>Pair…</Trans>
      </Button>
    </li>
  );
}
