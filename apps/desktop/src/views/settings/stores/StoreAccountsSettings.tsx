/** Settings → Store accounts (#165): connect a store, see each account's
 * sync health, refresh, reconnect, disconnect and remove. Admins only. */

import {
  Button,
  Select,
  SettingRow,
  SettingsSection,
  StatusDot,
  Switch,
  TextField,
} from "@caskade/ui";
import type { MessageDescriptor } from "@lingui/core";
import { msg, plural } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { ExternalLink, KeyRound, Link2, RefreshCw, Store } from "lucide-react";
import { type ReactNode, useEffect, useState } from "react";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import { formatDateTime } from "../../../api/presentation";
import {
  type StoreAccountSummary,
  type StoreAuthFlow,
  type StoreConnectionState,
  type StoreProviderCapabilities,
  type StoreRemovalPreview,
  useStoreAccountAction,
  useStoreAccounts,
} from "../../../api/storeAccounts";
import { AchievementSettings } from "../../../components/achievements/AchievementSettings";
import { FailureNotice } from "../../../components/FailureNotice";
import { SettingsLoading, SettingsStatus, useSettingsSection } from "../useSettings";

const stateLabels: Record<StoreConnectionState, MessageDescriptor> = {
  notConnected: msg({ message: "Disconnected", context: "store account" }),
  authenticating: msg({ message: "Signing in…", context: "store account" }),
  connected: msg({ message: "Connected", context: "store account" }),
  syncing: msg({ message: "Refreshing…", context: "store account" }),
  offline: msg({ message: "Offline", context: "store account" }),
  expired: msg({ message: "Sign-in expired", context: "store account" }),
  unsupported: msg({ message: "Not available", context: "store account" }),
};

const stateTones: Record<StoreConnectionState, "ok" | "off" | "warning" | "danger"> = {
  notConnected: "off",
  authenticating: "warning",
  connected: "ok",
  syncing: "ok",
  offline: "warning",
  expired: "danger",
  unsupported: "off",
};

export function StoreAccountsSettings() {
  const { t } = useLingui();
  const query = useStoreAccounts();
  if (query.error || !query.data) {
    return <SettingsLoading error={query.error} reload={() => void query.refetch()} />;
  }
  const { providers, accounts, flows } = query.data;
  const unsupported = accounts.filter((a) => a.capabilities === null);
  return (
    <div className="flex flex-col gap-6">
      {providers.length === 0 && (
        <SettingsSection icon={Store} title={t`Store accounts`}>
          <p className="text-sm text-fg-muted">
            <Trans>
              No store can be connected in this version of Caskade yet. Games your launchers have
              installed are still found by Library Discovery.
            </Trans>
          </p>
        </SettingsSection>
      )}
      {providers.map((provider) => (
        <ProviderSection
          key={provider.id}
          provider={provider}
          accounts={accounts.filter((a) => a.account.provider === provider.id)}
          flow={flows.find((f) => f.provider === provider.id && f.reconnect === null)}
          flows={flows}
        />
      ))}
      {unsupported.length > 0 && (
        <SettingsSection
          icon={Store}
          title={t`Other accounts`}
          description={t`Their stores aren't available in this version of Caskade. Their games stay in your library.`}
        >
          {unsupported.map((summary) => (
            <AccountRow key={summary.account.id} summary={summary} flows={flows} />
          ))}
        </SettingsSection>
      )}
      <RefreshSettings />
      <AchievementSettings />
      <HelpSection />
    </div>
  );
}

function ProviderSection({
  provider,
  accounts,
  flow,
  flows,
}: {
  provider: StoreProviderCapabilities;
  accounts: StoreAccountSummary[];
  flow: StoreAuthFlow | undefined;
  flows: StoreAuthFlow[];
}) {
  const { t } = useLingui();
  const begin = useStoreAccountAction((api, _: undefined) => api.begin(provider.id, null));
  const name = provider.name;
  return (
    <SettingsSection
      icon={Store}
      title={name}
      description={
        !provider.library
          ? t`Shows your ${name} achievements on the games in your library.`
          : provider.auth === "launcher"
            ? t`Reads the games the ${name} launcher on this computer knows you own.`
            : t`Brings in every game your ${name} account owns, installed or not.`
      }
      aside={
        !flow && (
          <Button
            data-focus-default
            isDisabled={begin.isPending}
            onPress={() => begin.mutate(undefined)}
          >
            <Link2 className="size-4" />
            {accounts.length > 0 ? t`Connect another` : t`Connect`}
          </Button>
        )
      }
    >
      {flow && <SignIn key={flow.flowId} flow={flow} provider={provider} />}
      {begin.error && <FailureNotice variant="inline" error={begin.error} />}
      {accounts.map((summary) => (
        <AccountRow key={summary.account.id} summary={summary} flows={flows} />
      ))}
      {accounts.length === 0 && !flow && (
        <p className="text-sm text-fg-muted">
          <Trans>No {name} account is connected.</Trans>
        </p>
      )}
    </SettingsSection>
  );
}

/** A sign-in under way: what to do, for the provider's way of signing in. */
function SignIn({ flow, provider }: { flow: StoreAuthFlow; provider: StoreProviderCapabilities }) {
  const { t } = useLingui();
  const [code, setCode] = useState("");
  const [values, setValues] = useState<Record<string, string>>({});
  const fields = flow.challenge.fields ?? [];
  const filled = fields.every((field) => (values[field.id] ?? "").trim());
  const complete = useStoreAccountAction((api, input: Parameters<typeof api.complete>[1]) =>
    api.complete(flow.flowId, input),
  );
  const cancel = useStoreAccountAction((api, _: undefined) => api.cancel(flow.flowId));
  const busy = complete.isPending || cancel.isPending;
  // A launcher's sign-in needs nothing from the user. Keyed by flow, so
  // each flow runs these once.
  useEffect(() => {
    if (flow.kind === "launcher") complete.mutate({ type: "launcher" });
  }, [flow.kind, complete.mutate]);
  // A device sign-in is checked every few seconds until it's done.
  useEffect(() => {
    if (flow.kind !== "device") return;
    const timer = setInterval(() => complete.mutate({ type: "poll" }), 5000);
    return () => clearInterval(timer);
  }, [flow.kind, complete.mutate]);
  const name = provider.name;
  const url = flow.challenge.url;
  const pending = complete.error && "code" in complete.error && complete.error.code === "busy";
  return (
    <section
      aria-label={t`Sign in to ${name}`}
      className="mb-3 flex flex-col gap-3 rounded-control border border-border bg-surface-base/30 p-4"
    >
      {flow.challenge.instructions && <p className="text-sm">{flow.challenge.instructions}</p>}
      {flow.kind === "browser" && !flow.challenge.instructions && (
        <p className="text-sm">
          <Trans>
            Sign in on {name}'s page in your browser. Caskade continues when the page sends you
            back.
          </Trans>
        </p>
      )}
      {flow.kind === "device" && (
        <div className="flex flex-col gap-2 text-sm">
          <Trans>Open {name}'s activation page and enter this code:</Trans>
          <span className="font-mono text-2xl tracking-widest" data-part="user-code">
            {flow.challenge.userCode}
          </span>
        </div>
      )}
      {url && (
        <a
          href={url}
          target="_blank"
          rel="noreferrer"
          className="inline-flex items-center gap-2 text-sm text-accent-alt underline data-focus-visible:focus-ring"
        >
          <ExternalLink className="size-4" />
          {flow.kind === "device"
            ? t`Open the activation page`
            : flow.kind === "apiKey"
              ? t`Get a key`
              : t`Open the sign-in page`}
        </a>
      )}
      {fields.length > 0 && (
        <form
          className="flex flex-col gap-3"
          onSubmit={(event) => {
            event.preventDefault();
            if (filled) complete.mutate({ type: "fields", fields: values });
          }}
        >
          {fields.map((field) => (
            <TextField
              key={field.id}
              label={field.label}
              description={field.description ?? undefined}
              value={values[field.id] ?? ""}
              onChange={(value) => setValues((current) => ({ ...current, [field.id]: value }))}
              type={field.secret ? "password" : "text"}
              icon={field.secret ? KeyRound : undefined}
            />
          ))}
          <div>
            <Button type="submit" variant="primary" isDisabled={busy || !filled}>
              <Trans>Connect</Trans>
            </Button>
          </div>
        </form>
      )}
      {fields.length === 0 && (flow.kind === "apiKey" || flow.kind === "browser") && (
        <form
          className="flex items-end gap-2"
          onSubmit={(event) => {
            event.preventDefault();
            if (code.trim()) complete.mutate({ type: "code", code: code.trim() });
          }}
        >
          <TextField
            className="flex-1"
            label={
              flow.kind === "apiKey"
                ? t`API key`
                : flow.challenge.instructions
                  ? t`Address or code`
                  : t`Or paste the code the page shows`
            }
            value={code}
            onChange={setCode}
            type="password"
            icon={KeyRound}
          />
          <Button type="submit" variant="primary" isDisabled={busy || !code.trim()}>
            <Trans>Connect</Trans>
          </Button>
        </form>
      )}
      {flow.kind === "device" && (
        <p role="status" className="text-xs text-fg-muted">
          <Trans>Waiting for you to finish on the other device…</Trans>
        </p>
      )}
      {complete.error && !pending && <FailureNotice variant="inline" error={complete.error} />}
      <div>
        <Button
          size="sm"
          variant="ghost"
          isDisabled={cancel.isPending}
          onPress={() => cancel.mutate(undefined)}
        >
          <Trans>Cancel sign-in</Trans>
        </Button>
      </div>
      <p className="text-xs text-fg-muted">
        <Trans>
          The sign-in expires {formatDateTime(new Date(flow.expiresAtMs))}. Caskade keeps it in your
          system's keychain, never in its settings or backups.
        </Trans>
      </p>
    </section>
  );
}

function AccountRow({ summary, flows }: { summary: StoreAccountSummary; flows: StoreAuthFlow[] }) {
  const { t } = useLingui();
  const { account, state } = summary;
  const refresh = useStoreAccountAction((api, _: undefined) => api.refresh(account.id));
  const cancelRefresh = useStoreAccountAction((api, _: undefined) => api.cancelRefresh(account.id));
  const reconnect = useStoreAccountAction((api, _: undefined) =>
    api.begin(account.provider, account.id),
  );
  const [dialog, setDialog] = useState<"disconnect" | "remove" | null>(null);
  const flow = flows.find((f) => f.reconnect === account.id);
  const provider = summary.capabilities;
  const synced = account.sync.lastOkMs;
  const owned = summary.owned;
  const installed = summary.installed;
  const supported = provider !== null;
  return (
    <div data-part="store-account" className="border-t border-border py-3 first-of-type:border-t-0">
      <SettingRow
        label={account.displayName}
        description={
          synced === null
            ? t`Not refreshed yet.`
            : provider?.library === false
              ? t`Achievements last refreshed ${formatDateTime(new Date(synced))}.`
              : t`${plural(owned, { one: "# game owned", other: "# games owned" })}, ${installed} installed. Last refreshed ${formatDateTime(new Date(synced))}.`
        }
      >
        <StatusDot tone={stateTones[state]}>{t(stateLabels[state])}</StatusDot>
        {state === "syncing" ? (
          <Button size="sm" onPress={() => cancelRefresh.mutate(undefined)}>
            <Trans>Stop</Trans>
          </Button>
        ) : (
          <Button
            size="sm"
            aria-label={t`Refresh ${account.displayName}`}
            isDisabled={!supported || !account.enabled || refresh.isPending}
            onPress={() => refresh.mutate(undefined)}
          >
            <RefreshCw className="size-4" />
            <Trans>Refresh</Trans>
          </Button>
        )}
        {supported && (state === "expired" || state === "notConnected") && !flow && (
          <Button
            size="sm"
            variant="primary"
            isDisabled={reconnect.isPending}
            onPress={() => reconnect.mutate(undefined)}
          >
            <Trans>Reconnect</Trans>
          </Button>
        )}
        {account.enabled && (
          <Button size="sm" variant="ghost" onPress={() => setDialog("disconnect")}>
            <Trans>Disconnect</Trans>
          </Button>
        )}
        <Button size="sm" variant="ghost" onPress={() => setDialog("remove")}>
          <Trans>Remove…</Trans>
        </Button>
      </SettingRow>
      {summary.sessionOnly && (
        <p className="text-xs text-warning">
          <Trans>
            Your system has no keychain Caskade can use, so this sign-in lasts until Caskade closes.
          </Trans>
        </p>
      )}
      {summary.lost > 0 && (
        <p className="text-xs text-fg-muted">
          {plural(summary.lost, {
            one: "# game is no longer in this account; it stays in your library.",
            other: "# games are no longer in this account; they stay in your library.",
          })}
        </p>
      )}
      {account.sync.error && state !== "notConnected" && (
        <FailureNotice variant="inline" error={account.sync.error} />
      )}
      {flow && provider && <SignIn key={flow.flowId} flow={flow} provider={provider} />}
      {[refresh.error, reconnect.error].map(
        (error, index) =>
          error && (
            // biome-ignore lint/suspicious/noArrayIndexKey: one slot per action
            <FailureNotice key={index} variant="inline" error={error} />
          ),
      )}
      {dialog === "disconnect" && (
        <DisconnectDialog summary={summary} close={() => setDialog(null)} />
      )}
      {dialog === "remove" && <RemoveDialog summary={summary} close={() => setDialog(null)} />}
    </div>
  );
}

function DialogFrame({
  busy,
  close,
  children,
}: {
  busy: boolean;
  close: () => void;
  children: ReactNode;
}) {
  return (
    <ModalOverlay
      isOpen
      isDismissable={!busy}
      onOpenChange={(open) => {
        if (!open && !busy) close();
      }}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
    >
      <Modal className="w-full max-w-md rounded-panel border border-border-strong bg-surface-raised p-6 shadow-lg">
        <Dialog role="alertdialog" className="space-y-4 outline-none">
          {children}
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}

function DisconnectDialog({ summary, close }: { summary: StoreAccountSummary; close: () => void }) {
  const disconnect = useStoreAccountAction((api, _: undefined) =>
    api.disconnect(summary.account.id),
  );
  const name = summary.account.displayName;
  return (
    <DialogFrame busy={disconnect.isPending} close={close}>
      <Heading slot="title" className="font-display text-xl font-semibold">
        <Trans>Disconnect {name}?</Trans>
      </Heading>
      <p className="text-sm text-fg-muted">
        <Trans>
          Caskade signs out of it and stops refreshing it. Its games stay in your library, and
          connecting it again brings it back as it was. Remove it to take its games out.
        </Trans>
      </p>
      {disconnect.error && <FailureNotice variant="inline" error={disconnect.error} />}
      <div className="flex justify-end gap-2">
        <Button data-focus-default onPress={close} isDisabled={disconnect.isPending}>
          <Trans>Cancel</Trans>
        </Button>
        <Button
          variant="danger"
          isDisabled={disconnect.isPending}
          onPress={() => disconnect.mutate(undefined, { onSuccess: close })}
        >
          <Trans>Disconnect</Trans>
        </Button>
      </div>
    </DialogFrame>
  );
}

function RemoveDialog({ summary, close }: { summary: StoreAccountSummary; close: () => void }) {
  const { t } = useLingui();
  const [preview, setPreview] = useState<StoreRemovalPreview | null>(null);
  const load = useStoreAccountAction((api, _: undefined) => api.removalPreview(summary.account.id));
  const remove = useStoreAccountAction((api, _: undefined) => api.remove(summary.account.id));
  useEffect(() => {
    load.mutate(undefined, { onSuccess: setPreview });
  }, [load.mutate]);
  const name = summary.account.displayName;
  const removed = preview?.removed.length ?? 0;
  const kept = preview?.kept.length ?? 0;
  return (
    <DialogFrame busy={remove.isPending} close={close}>
      <Heading slot="title" className="font-display text-xl font-semibold">
        <Trans>Remove {name}?</Trans>
      </Heading>
      {preview ? (
        <div className="space-y-2 text-sm text-fg-muted">
          <p>
            {plural(removed, {
              one: "# game only this account brought leaves your library.",
              other: "# games only this account brought leave your library.",
            })}{" "}
            {plural(kept, {
              one: "# game stays, because it's installed, added another way or owned by another account.",
              other:
                "# games stay, because they're installed, added another way or owned by another account.",
            })}
          </p>
          {preview.removed.length > 0 && (
            <ul
              aria-label={t`Games that leave the library`}
              className="max-h-40 overflow-y-auto pl-5 text-xs"
            >
              {preview.removed.map((game) => (
                <li key={game.gameId} className="list-disc">
                  {game.title}
                </li>
              ))}
            </ul>
          )}
        </div>
      ) : (
        <p className="text-sm text-fg-muted">
          <Trans>Checking what would change…</Trans>
        </p>
      )}
      {(load.error || remove.error) && (
        <FailureNotice variant="inline" error={load.error ?? remove.error} />
      )}
      <div className="flex justify-end gap-2">
        <Button data-focus-default onPress={close} isDisabled={remove.isPending}>
          <Trans>Cancel</Trans>
        </Button>
        <Button
          variant="danger"
          isDisabled={!preview || remove.isPending}
          onPress={() => remove.mutate(undefined, { onSuccess: close })}
        >
          <Trans>Remove</Trans>
        </Button>
      </div>
    </DialogFrame>
  );
}

function RefreshSettings() {
  const { t } = useLingui();
  const preferences = useSettingsSection<{
    backgroundRefresh: boolean;
    intervalHours: number;
    playtimeImport: "never" | "newGames" | "always";
  }>("core.storeAccounts");
  const settings = preferences.settings;
  const title = t`Refreshing`;
  const description = t`Refresh connected accounts in the background to bring in games you buy. Failures show on the account; nothing pops up.`;
  // Loading says nothing; a failed load says why, with Try again (#55).
  if (!settings)
    return preferences.loadError ? (
      <SettingsSection icon={RefreshCw} title={title} description={description}>
        <SettingsStatus section={preferences} what={title} />
      </SettingsSection>
    ) : null;
  const options = [6, 12, 24, 48, 168].map((hours) => ({
    id: String(hours),
    label: hours === 168 ? t`Weekly` : t`Every ${hours} hours`,
  }));
  if (!options.some((o) => o.id === String(settings.intervalHours))) {
    options.push({
      id: String(settings.intervalHours),
      label: t`Every ${settings.intervalHours} hours`,
    });
  }
  return (
    <SettingsSection icon={RefreshCw} title={title} description={description}>
      <SettingsStatus section={preferences} what={title} />
      <SettingRow label={t`Refresh in the background`}>
        <Switch
          isSelected={settings.backgroundRefresh}
          onChange={(backgroundRefresh) =>
            preferences.setSettings((current) => ({ ...current, backgroundRefresh }))
          }
        />
      </SettingRow>
      <SettingRow label={t`How often`}>
        <Select
          isDisabled={!settings.backgroundRefresh}
          options={options}
          value={String(settings.intervalHours)}
          onChange={(hours) =>
            preferences.setSettings((current) => ({ ...current, intervalHours: Number(hours) }))
          }
        />
      </SettingRow>
      {/* After the two refresh rows it doesn't belong between (#54). */}
      <SettingRow
        label={t`Import play time`}
        description={t`Play time a store reports is added to what Caskade tracked, counting time played through Caskade once. A store that lowers its total never lowers yours.`}
      >
        <Select
          options={[
            { id: "always", label: t`On every refresh` },
            { id: "newGames", label: t`For new games only` },
            { id: "never", label: t({ message: "Never", context: "import play time" }) },
          ]}
          value={settings.playtimeImport ?? "always"}
          onChange={(playtimeImport) =>
            preferences.setSettings((current) => ({ ...current, playtimeImport }))
          }
        />
      </SettingRow>
    </SettingsSection>
  );
}

function HelpSection() {
  const { t } = useLingui();
  return (
    <SettingsSection icon={KeyRound} title={t`About store accounts`}>
      <ul className="flex list-disc flex-col gap-2 pl-5 text-sm text-fg-muted">
        <li>
          <Trans>
            Games an account owns join your library even when they aren't installed. When a launcher
            installs one, Caskade shows it as the same game.
          </Trans>
        </li>
        <li>
          <Trans>
            Sign-ins are kept in your system's keychain, never in Caskade's settings or backups.
            After restoring a backup or moving computer, reconnect each account.
          </Trans>
        </li>
        <li>
          <Trans>
            Disconnecting keeps the account's games. Removing it takes out the games only it
            brought; installed games stay.
          </Trans>
        </li>
      </ul>
    </SettingsSection>
  );
}
