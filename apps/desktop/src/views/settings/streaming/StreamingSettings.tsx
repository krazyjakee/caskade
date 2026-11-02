/** Settings → Streaming (#265, STREAMING-PLAN §8.5): letting other devices
 * stream games from this one (admins), the devices paired with it and the
 * ones waiting to pair; the devices this one can stream from, pairing
 * with them and adding one by its address; and the stream quality.
 *
 * Every account sees the tab and can pair this device with another to
 * stream from; only an admin hosts, accepts pairings or changes quality
 * (the core refuses the rest whatever this shows). No technology's name
 * appears: Caskade does the streaming. */
import {
  Button,
  Select,
  SettingRow,
  SettingsSection,
  StatusDot,
  Switch,
  TextField,
} from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { Cast, Download, Gauge, MonitorPlay, Plus, RefreshCw, Trash2, Wrench } from "lucide-react";
import { useState } from "react";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import {
  type HostOutcome,
  type HostState,
  reachLabel,
  type StreamHost,
  type StreamingPart,
  useHostStatus,
  usePairedDevices,
  usePairRequests,
  useStreamHosts,
  useStreamingAction,
  useStreamingPrograms,
} from "../../../api/streaming";
import { FailureNotice } from "../../../components/FailureNotice";
import { PairRequestForm } from "../../../components/streaming/PairRequestForm";
import { useDeviceAdmin } from "../devices/admin";
import { useSettingsSection } from "../useSettings";
import { PairDialog } from "./PairDialog";

export function StreamingSettings() {
  const admin = useDeviceAdmin();
  return (
    <div data-cs="streaming-settings" className="flex flex-col gap-6">
      {!admin && (
        <p role="note" className="text-sm text-fg-muted">
          <Trans>
            Only an admin can let other devices stream from this one or change the stream quality.
            Anyone can pair this device with another to play its games.
          </Trans>
        </p>
      )}
      <HostingSection />
      <StreamFromSection />
      <QualitySection />
      <UpdatesSection />
    </div>
  );
}

const on = (state: HostState | undefined) =>
  state !== undefined && state !== "off" && state !== "unsupported";

function HostingSection() {
  const { t } = useLingui();
  const admin = useDeviceAdmin();
  const status = useHostStatus();
  const state = status.data?.state;
  const [outcome, setOutcome] = useState<HostOutcome | null>(null);
  const hosting = useStreamingAction((api, next: boolean) => api.setHosting(next));
  const repair = useStreamingAction((api, _: undefined) => api.repair());
  const switching = hosting.isPending || repair.isPending;
  const set = (next: boolean) =>
    hosting.mutate(next, { onSuccess: (result) => setOutcome(result) });
  return (
    <SettingsSection
      icon={Cast}
      title={t`Let other devices play`}
      description={t`Your other devices at home can play this device's games, streamed over the network, with their own controller.`}
    >
      <SettingRow
        label={t`Let other devices stream games from this one`}
        description={t`Setting up asks once for an administrator's permission.`}
      >
        <Switch
          isSelected={hosting.isPending ? Boolean(hosting.variables) : on(state)}
          isDisabled={!admin || switching || state === undefined || state === "unsupported"}
          onChange={set}
        />
      </SettingRow>
      <HostStateLine state={state} problem={status.data?.problem ?? null} switching={switching} />
      {admin && on(state) && (
        <div className="mt-2 flex flex-wrap items-center gap-3">
          <Button
            variant={state === "needsAttention" ? "primary" : "secondary"}
            icon={Wrench}
            isDisabled={switching}
            onPress={() => repair.mutate(undefined, { onSuccess: (result) => setOutcome(result) })}
          >
            {repair.isPending ? t`Repairing…` : t`Repair`}
          </Button>
          <span className="text-xs text-fg-subtle">
            <Trans>Sets it up again where something went missing or changed.</Trans>
          </span>
        </div>
      )}
      {(outcome === "cancelled" || outcome === "refused") && (
        <p role="status" className="mt-2 text-sm text-fg-muted">
          {outcome === "cancelled" ? (
            <Trans>Nothing changed: the administrator's permission wasn't given.</Trans>
          ) : (
            <Trans>Nothing changed: the system didn't allow it.</Trans>
          )}
        </p>
      )}
      {(hosting.error ?? repair.error) && (
        <FailureNotice variant="inline" error={hosting.error ?? repair.error} />
      )}
      {outcome === "existingFound" && <ExistingDialog close={() => setOutcome(null)} />}
      {admin && state === "ready" && <WaitingRequests />}
      {admin && on(state) && <PairedDevices />}
    </SettingsSection>
  );
}

function HostStateLine({
  state,
  problem,
  switching,
}: {
  state: HostState | undefined;
  problem: string | null;
  switching: boolean;
}) {
  const shown = switching && (state === "off" || state === undefined) ? "settingUp" : state;
  return (
    <p data-part="host-state" data-status={shown} role="status" className="mt-1 text-sm">
      {shown === "ready" ? (
        <StatusDot tone="ok">
          <Trans>Ready. Devices on this network can pair and play.</Trans>
        </StatusDot>
      ) : shown === "settingUp" ? (
        <StatusDot tone="warning">
          <Trans>Setting up… The first time takes a few minutes.</Trans>
        </StatusDot>
      ) : shown === "starting" ? (
        <StatusDot tone="warning">
          <Trans>Starting…</Trans>
        </StatusDot>
      ) : shown === "needsAttention" ? (
        <StatusDot tone="danger">{problem ?? <Trans>Needs attention.</Trans>}</StatusDot>
      ) : shown === "unsupported" ? (
        <StatusDot tone="off">
          <Trans>This device can't share its games yet.</Trans>
        </StatusDot>
      ) : (
        <StatusDot tone="off">
          <Trans>Off.</Trans>
        </StatusDot>
      )}
    </p>
  );
}

/** Someone already set up game streaming here: use it with its sign-in. */
function ExistingDialog({ close }: { close: () => void }) {
  const { t } = useLingui();
  const [user, setUser] = useState("");
  const [password, setPassword] = useState("");
  const use = useStreamingAction((api, _: undefined) => api.hostThroughExisting(user, password));
  return (
    <ModalOverlay
      isOpen
      isDismissable={!use.isPending}
      onOpenChange={(open) => {
        if (!open && !use.isPending) close();
      }}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
    >
      <Modal className="w-full max-w-md rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg">
        <Dialog
          data-cs="streaming-existing"
          aria-label={t`Streaming is already set up`}
          className="outline-none"
        >
          <Heading slot="title" className="font-display text-xl font-semibold">
            {t`Streaming is already set up`}
          </Heading>
          <p className="mt-2 text-sm text-fg-muted">
            <Trans>
              Game streaming was set up on this device before. Caskade can use it, adding only its
              own games. Enter the user name and password it was set up with.
            </Trans>
          </p>
          <form
            className="mt-4 flex flex-col gap-3"
            onSubmit={(event) => {
              event.preventDefault();
              if (user && password) use.mutate(undefined, { onSuccess: close });
            }}
          >
            <TextField label={t`User name`} value={user} onChange={setUser} autoFocus />
            <TextField
              label={t`Password`}
              type="password"
              value={password}
              onChange={setPassword}
            />
            {use.error && <FailureNotice variant="inline" error={use.error} />}
            <div className="mt-2 flex justify-end gap-2">
              <Button isDisabled={use.isPending} onPress={close}>
                <Trans>Leave it off</Trans>
              </Button>
              <Button
                type="submit"
                variant="primary"
                isDisabled={!user || !password || use.isPending}
              >
                {use.isPending ? t`Checking…` : t`Use it`}
              </Button>
            </div>
          </form>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}

function WaitingRequests() {
  const requests = usePairRequests().data ?? [];
  if (requests.length === 0) return null;
  return (
    <div data-part="requests" className="mt-4 flex flex-col gap-3">
      <h3 className="text-sm font-semibold">
        <Trans>Waiting to pair</Trans>
      </h3>
      <ul className="flex flex-col gap-3">
        {requests.map((request) => (
          <li key={request.id} className="rounded-control border border-accent-alt/50 p-3">
            <PairRequestForm request={request} />
          </li>
        ))}
      </ul>
    </div>
  );
}

function PairedDevices() {
  const { t } = useLingui();
  const paired = usePairedDevices().data ?? [];
  const remove = useStreamingAction((api, uuid: string) => api.unpair(uuid));
  const removeAll = useStreamingAction((api, _: undefined) => api.unpairAll());
  const [confirming, setConfirming] = useState(false);
  return (
    <div data-part="paired" className="mt-4 flex flex-col gap-2">
      <h3 className="text-sm font-semibold">
        <Trans>Devices that can play from this one</Trans>
      </h3>
      {paired.length === 0 ? (
        <p className="text-sm text-fg-muted">
          <Trans>
            None yet. On another device, open Settings → Streaming in Caskade and choose Pair with
            this one.
          </Trans>
        </p>
      ) : (
        <ul className="flex flex-col gap-2">
          {paired.map((device) => {
            const name = device.name;
            return (
              <li
                key={device.uuid}
                data-cs="streaming-client"
                className="flex items-center justify-between gap-3 rounded-control border border-border p-3"
              >
                <span className="font-medium">{name}</span>
                <Button
                  size="sm"
                  variant="danger"
                  icon={Trash2}
                  aria-label={t`Remove ${name}`}
                  isDisabled={remove.isPending}
                  onPress={() => remove.mutate(device.uuid)}
                >
                  <Trans>Remove</Trans>
                </Button>
              </li>
            );
          })}
        </ul>
      )}
      {paired.length > 1 &&
        (confirming ? (
          <div className="flex flex-wrap items-center gap-2">
            <span className="text-sm">
              <Trans>Every device then needs a new code to play from this one.</Trans>
            </span>
            <Button
              size="sm"
              variant="danger"
              isDisabled={removeAll.isPending}
              onPress={() => removeAll.mutate(undefined, { onSuccess: () => setConfirming(false) })}
            >
              <Trans>Forget them all</Trans>
            </Button>
            <Button size="sm" variant="ghost" onPress={() => setConfirming(false)}>
              <Trans>Keep them</Trans>
            </Button>
          </div>
        ) : (
          <Button
            size="sm"
            variant="ghost"
            className="self-start"
            icon={Trash2}
            onPress={() => setConfirming(true)}
          >
            <Trans>Forget all paired devices…</Trans>
          </Button>
        ))}
      {(remove.error ?? removeAll.error) && (
        <FailureNotice variant="inline" error={remove.error ?? removeAll.error} />
      )}
    </div>
  );
}

/** Newer versions of the streaming programs (#266): Update or Skip. An
 * update never interrupts a stream. */
function UpdatesSection() {
  const { t } = useLingui();
  const admin = useDeviceAdmin();
  const programs = useStreamingPrograms().data ?? [];
  const update = useStreamingAction((api, part: StreamingPart) => api.update(part));
  const skip = useStreamingAction((api, part: StreamingPart) => api.skip(part));
  if (programs.length === 0) return null;
  return (
    <SettingsSection
      icon={Download}
      title={t`Streaming updates`}
      description={t`Caskade keeps streaming up to date when you say so, never in the middle of a game.`}
    >
      <ul className="flex flex-col gap-2">
        {programs.map((program) => {
          const latest = program.latest ?? "";
          return (
            <li
              key={program.part}
              data-cs="streaming-program"
              data-status={program.update ? "update" : "current"}
              className="flex flex-wrap items-center gap-3 rounded-control border border-border p-3"
            >
              <div className="flex min-w-0 flex-1 flex-col gap-1">
                <span className="font-medium">{program.label}</span>
                <span data-part="version" className="text-xs text-fg-subtle">
                  {program.update ? (
                    <Trans>A newer version, {latest}, is ready.</Trans>
                  ) : (
                    <Trans>Up to date.</Trans>
                  )}
                </span>
              </div>
              {program.update && admin && (
                <div className="flex gap-2">
                  <Button
                    size="sm"
                    variant="primary"
                    isDisabled={update.isPending}
                    onPress={() => update.mutate(program.part)}
                  >
                    {update.isPending && update.variables === program.part
                      ? t`Updating…`
                      : t`Update`}
                  </Button>
                  <Button
                    size="sm"
                    variant="ghost"
                    isDisabled={skip.isPending || update.isPending}
                    onPress={() => skip.mutate(program.part)}
                  >
                    <Trans>Skip this version</Trans>
                  </Button>
                </div>
              )}
            </li>
          );
        })}
      </ul>
      {(update.error ?? skip.error) && (
        <FailureNotice variant="inline" error={update.error ?? skip.error} />
      )}
    </SettingsSection>
  );
}

function StreamFromSection() {
  const { t } = useLingui();
  const hosts = useStreamHosts();
  const [pairing, setPairing] = useState<StreamHost | null>(null);
  const [address, setAddress] = useState("");
  const add = useStreamingAction((api, value: string) => api.addHost(value));
  const list = hosts.data ?? [];
  return (
    <SettingsSection
      icon={MonitorPlay}
      title={t`Play from other devices`}
      description={t`Pair once with a device that lets others play, and its games show in your library as Stream from that device.`}
      aside={
        <Button
          size="sm"
          variant="ghost"
          icon={RefreshCw}
          isDisabled={hosts.isFetching}
          onPress={() => void hosts.refetch()}
        >
          <Trans>Look again</Trans>
        </Button>
      }
    >
      {list.length === 0 ? (
        <p className="text-sm text-fg-muted">
          <Trans>
            No devices found on this network yet. On another device running Caskade, turn on Let
            other devices stream games from this one, or add it by its address below.
          </Trans>
        </p>
      ) : (
        <ul className="flex flex-col gap-2">
          {list.map((host) => (
            <HostRow key={host.id} host={host} pair={() => setPairing(host)} />
          ))}
        </ul>
      )}
      <form
        className="mt-4 flex flex-wrap items-end gap-3"
        onSubmit={(event) => {
          event.preventDefault();
          const value = address.trim();
          if (value) add.mutate(value, { onSuccess: () => setAddress("") });
        }}
      >
        <TextField
          label={t`Add a device by its address`}
          description={t`For a device on another network, or one that isn't found by itself.`}
          placeholder="192.168.1.20"
          value={address}
          onChange={setAddress}
          mono
          className="min-w-64 flex-1"
        />
        <Button type="submit" icon={Plus} isDisabled={!address.trim() || add.isPending}>
          <Trans>Add</Trans>
        </Button>
      </form>
      {add.error && <FailureNotice variant="inline" error={add.error} />}
      {pairing && <PairDialog host={pairing} close={() => setPairing(null)} />}
    </SettingsSection>
  );
}

function HostRow({ host, pair }: { host: StreamHost; pair: () => void }) {
  const { t } = useLingui();
  const forget = useStreamingAction((api, id: string) => api.forgetHost(id));
  const [confirming, setConfirming] = useState(false);
  const name = host.name;
  const games = host.games;
  return (
    <li
      data-cs="stream-host"
      data-status={host.reach}
      className="flex flex-wrap items-center gap-4 rounded-control border border-border p-3"
    >
      <div className="flex min-w-0 flex-1 flex-col gap-1">
        <span className="font-medium">{name}</span>
        <span data-part="state" className="text-xs text-fg-subtle">
          {host.paired ? (
            <Trans>
              {reachLabel(host.reach)} · Paired · {games} games
            </Trans>
          ) : (
            <Trans>{reachLabel(host.reach)} · Not paired</Trans>
          )}
        </span>
      </div>
      {host.paired ? (
        confirming ? (
          <div className="flex gap-2">
            <Button
              size="sm"
              variant="danger"
              isDisabled={forget.isPending}
              onPress={() => forget.mutate(host.id, { onSuccess: () => setConfirming(false) })}
            >
              <Trans>Forget {name}</Trans>
            </Button>
            <Button size="sm" variant="ghost" onPress={() => setConfirming(false)}>
              <Trans>Keep</Trans>
            </Button>
          </div>
        ) : (
          <Button
            size="sm"
            variant="ghost"
            icon={Trash2}
            aria-label={t`Forget ${name}`}
            onPress={() => setConfirming(true)}
          >
            <Trans>Forget</Trans>
          </Button>
        )
      ) : (
        <Button
          size="sm"
          variant="primary"
          isDisabled={host.reach === "unreachable"}
          onPress={pair}
        >
          <Trans>Pair…</Trans>
        </Button>
      )}
      {forget.error && <FailureNotice variant="inline" error={forget.error} />}
    </li>
  );
}

interface StreamingSettingsValue {
  hosting: boolean;
  addresses?: string[];
  existing?: boolean;
  quality?: { resolution?: string | null; fps?: number | null; bitrateKbps?: number | null };
}

const AUTOMATIC = "auto";

function QualitySection() {
  const { t } = useLingui();
  const admin = useDeviceAdmin();
  const section = useSettingsSection<StreamingSettingsValue>("core.streaming");
  const quality = section.settings?.quality ?? {};
  const disabled = !admin || !section.settings || section.saving;
  const change = (part: Partial<NonNullable<StreamingSettingsValue["quality"]>>) =>
    section.setSettings((current) => ({
      ...current,
      quality: { ...current.quality, ...part },
    }));
  const automatic = t({ message: "Automatic", context: "stream quality" });
  const resolutions = [
    { id: AUTOMATIC, label: automatic },
    { id: "1280x720", label: "1280 × 720" },
    { id: "1920x1080", label: "1920 × 1080" },
    { id: "2560x1440", label: "2560 × 1440" },
    { id: "3840x2160", label: "3840 × 2160" },
  ];
  const rates = [
    { id: AUTOMATIC, label: automatic },
    ...[30, 60, 90, 120].map((fps) => ({ id: String(fps), label: t`${fps} frames a second` })),
  ];
  const bitrates = [
    { id: AUTOMATIC, label: automatic },
    ...[5, 10, 20, 40, 80].map((mbps) => ({
      id: String(mbps * 1000),
      label: t({ message: `${mbps} Mbit/s`, comment: "Network speed of a game stream" }),
    })),
  ];
  const chosen = (value: number | string | null | undefined) =>
    value === null || value === undefined ? AUTOMATIC : String(value);
  const number = (id: string) => (id === AUTOMATIC ? null : Number(id));
  return (
    <SettingsSection
      icon={Gauge}
      title={t`Stream quality`}
      description={t`How games streamed to this device look. Automatic suits this screen and the network.`}
    >
      <SettingRow label={t`Resolution`}>
        <Select
          options={resolutions}
          value={chosen(quality.resolution)}
          isDisabled={disabled}
          onChange={(id) => change({ resolution: id === AUTOMATIC ? null : id })}
        />
      </SettingRow>
      <SettingRow label={t`Frame rate`}>
        <Select
          options={rates}
          value={chosen(quality.fps)}
          isDisabled={disabled}
          onChange={(id) => change({ fps: number(id) })}
        />
      </SettingRow>
      <SettingRow
        label={t`Bitrate`}
        description={t`Higher looks sharper and needs a faster network.`}
      >
        <Select
          options={bitrates}
          value={chosen(quality.bitrateKbps)}
          isDisabled={disabled}
          onChange={(id) => change({ bitrateKbps: number(id) })}
        />
      </SettingRow>
      {section.error && <FailureNotice variant="inline" error={section.error} />}
    </SettingsSection>
  );
}
