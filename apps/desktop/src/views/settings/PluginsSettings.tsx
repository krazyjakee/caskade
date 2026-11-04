import { Button, StatusDot, Switch } from "@caskade/ui";
import { i18n, type MessageDescriptor } from "@lingui/core";
import { msg, t as translate } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation, useQueryClient } from "@tanstack/react-query";
import { FolderOpen, Power, ShieldCheck, Trash2 } from "lucide-react";
import { useState } from "react";
import { Dialog, Heading, Modal, ModalOverlay } from "react-aria-components";
import {
  type PluginSummary,
  useManagedPlugins,
  usePluginManager,
  useSetPluginEnabled,
} from "../../api/pluginManager";
import { useActionMenus } from "../../components/ActionMenu";
import { AddonCatalogue } from "../../components/addons/AddonCatalogue";
import { usePathPicker } from "../../components/pathBrowser/PathPicker";
import { useDropZone } from "../../drop/DropProvider";
import { PluginSettingsPages } from "../../plugins/PluginViews";
import { hookNames, providerName, slotName } from "./pluginTerms";

type Permissions = NonNullable<PluginSummary["permissions"]>;

/** The capabilities a plugin asks for, in plain words (TECH-PLAN §7), in
 * the current language (called while rendering). */
export function capabilityLines(permissions: Permissions | null): string[] {
  if (!permissions) return [];
  const lines: string[] = [];
  if (permissions.network.length > 0)
    lines.push(translate`Connect to ${{ hosts: permissions.network.join(", ") }}`);
  for (const folder of permissions.fs) {
    if (folder.endsWith(":ro"))
      lines.push(translate`Read files in ${{ path: folder.slice(0, -3) }}`);
    else lines.push(translate`Read and change files in ${{ path: folder.replace(/:rw$/, "") }}`);
  }
  if (permissions.process) lines.push(translate`Run programs and change how games are launched`);
  if (permissions.library === "read") lines.push(translate`Read your library`);
  if (permissions.library === "write") lines.push(translate`Read and change your library`);
  return lines;
}

/** Everything a plugin can do, its UI half included: UI modules run in the
 * app's page with the app's IPC access (TECH-PLAN §7's known gap). */
function accessLines(plugin: PluginSummary): string[] {
  return [
    ...(plugin.hasUi
      ? [translate`Run interface code inside Caskade, with the same access as the app`]
      : []),
    ...capabilityLines(plugin.permissions),
  ];
}

function contributionLines(plugin: PluginSummary): string[] {
  const contributes = plugin.contributes;
  if (!contributes) return [];
  const lines: string[] = [];
  // Ids in words (#53); `i18n._` because this runs while rendering.
  const say = (descriptor: MessageDescriptor) => i18n._(descriptor);
  for (const view of contributes.views) lines.push(translate`View: ${{ title: view.title }}`);
  for (const slot of contributes.slots)
    lines.push(`${slotName(say, slot.slot)}: ${slot.title ?? slot.id}`);
  const { changes, hears } = hookNames(say, contributes.hooks);
  if (changes.length > 0) lines.push(translate`Can change ${{ what: changes.join(", ") }}`);
  if (hears.length > 0) lines.push(translate`Is told about ${{ what: hears.join(", ") }}`);
  if (contributes.providers.length > 0)
    lines.push(
      translate`Provides ${{
        providers: contributes.providers.map((id) => providerName(say, id)).join(", "),
      }}`,
    );
  if (contributes.settings) lines.push(translate`Settings page`);
  return lines;
}

const statusLabels: Record<PluginSummary["status"], MessageDescriptor> = {
  invalid: msg({ message: "Invalid", context: "plugin status" }),
  disabled: msg({ message: "Off", context: "plugin status" }),
  needs_consent: msg({ message: "Needs your approval", context: "plugin status" }),
  enabled: msg({ message: "On", context: "plugin status" }),
  active: msg({ message: "Running", context: "plugin status" }),
};

const statusTone = (status: PluginSummary["status"]) =>
  status === "active" || status === "enabled"
    ? ("ok" as const)
    : status === "needs_consent"
      ? ("warning" as const)
      : status === "invalid"
        ? ("danger" as const)
        : ("off" as const);

/** Name, version, contributions and permissions, before anything is granted. */
export function ReviewDialog({
  plugin,
  installedVersion,
  action,
  busy,
  error,
  accept,
  close,
}: {
  plugin: PluginSummary;
  installedVersion?: string | null;
  action: string;
  busy: boolean;
  error?: string;
  accept: () => void;
  close: () => void;
}) {
  const { t } = useLingui();
  const capabilities = accessLines(plugin);
  const contributions = contributionLines(plugin);
  const valid = plugin.status !== "invalid";
  return (
    <ModalOverlay
      isOpen
      isDismissable={!busy}
      onOpenChange={(open) => {
        if (!open && !busy) close();
      }}
      className="fixed inset-0 z-50 grid place-items-center bg-surface-base/80 p-4 backdrop-blur-sm"
    >
      <Modal className="w-full max-w-xl rounded-panel border border-border-strong bg-surface-raised p-6">
        <Dialog data-cs="plugin-review" className="outline-none">
          <Heading slot="title" className="font-display text-xl font-semibold">
            {plugin.name}
            {plugin.version && (
              <span className="ml-2 font-mono text-sm text-fg-muted">{plugin.version}</span>
            )}
          </Heading>
          <p className="mt-1 font-mono text-xs text-fg-muted">{plugin.id}</p>
          {plugin.description && <p className="mt-3 text-sm">{plugin.description}</p>}
          {installedVersion && (
            <p className="mt-2 text-sm text-fg-muted">
              <Trans>Replaces the installed version {installedVersion}.</Trans>
            </p>
          )}
          {valid ? (
            <>
              <h3 className="mt-4 text-sm font-semibold">
                <Trans comment="Heading over a plugin's permissions">It asks to</Trans>
              </h3>
              <ul aria-label={t`Permissions`} className="mt-1 list-disc pl-5 text-sm">
                {capabilities.length > 0 ? (
                  capabilities.map((line) => <li key={line}>{line}</li>)
                ) : (
                  <li>
                    <Trans>Nothing beyond its own storage</Trans>
                  </li>
                )}
              </ul>
              {contributions.length > 0 && (
                <>
                  <h3 className="mt-4 text-sm font-semibold">
                    <Trans comment="Heading over what a plugin contributes">It adds</Trans>
                  </h3>
                  <ul aria-label={t`Contributions`} className="mt-1 list-disc pl-5 text-sm">
                    {contributions.map((line) => (
                      <li key={line}>{line}</li>
                    ))}
                  </ul>
                </>
              )}
            </>
          ) : (
            <ul aria-label={t`Problems`} className="mt-4 list-disc pl-5 text-sm text-danger">
              {plugin.errors.map((problem) => (
                <li key={`${problem.key}:${problem.message}`}>
                  {problem.key ? `${problem.key}: ` : ""}
                  {problem.message}
                </li>
              ))}
            </ul>
          )}
          {error && (
            <p role="alert" className="mt-3 text-sm text-danger">
              {error}
            </p>
          )}
          <div className="mt-6 flex justify-end gap-2">
            <Button variant="secondary" onPress={close} isDisabled={busy}>
              <Trans>Cancel</Trans>
            </Button>
            <Button onPress={accept} isDisabled={!valid || busy}>
              {action}
            </Button>
          </div>
        </Dialog>
      </Modal>
    </ModalOverlay>
  );
}

function PluginRow({
  plugin,
  review,
}: {
  plugin: PluginSummary;
  review: (plugin: PluginSummary) => void;
}) {
  const { t } = useLingui();
  const toggle = useSetPluginEnabled();
  const api = usePluginManager();
  const client = useQueryClient();
  const [confirming, setConfirming] = useState(false);
  const remove = useMutation({
    mutationFn: () => api.uninstall(plugin.id),
    onSuccess: () => client.invalidateQueries({ queryKey: ["plugins"] }),
  });
  const on = plugin.status === "enabled" || plugin.status === "active";
  const capabilities = accessLines(plugin);
  // A right click offers the row's buttons.
  const menus = useActionMenus();
  return (
    <li
      {...menus.target(plugin.name, [
        plugin.status === "needs_consent"
          ? {
              id: "review",
              label: t`Review and allow`,
              icon: ShieldCheck,
              onAction: () => review(plugin),
            }
          : {
              id: "toggle",
              label: on ? t`Turn off` : t`Turn on`,
              icon: Power,
              isDisabled: plugin.status === "invalid" || toggle.isPending,
              onAction: () => toggle.mutate({ id: plugin.id, enabled: !on }),
            },
        {
          id: "uninstall",
          label: t`Uninstall…`,
          icon: Trash2,
          tone: "danger",
          onAction: () => setConfirming(true),
        },
      ])}
      data-part="plugin"
      data-status={plugin.status}
      className="flex flex-wrap items-start justify-between gap-4 rounded-panel border border-border bg-surface-raised p-4"
    >
      <div className="min-w-0 flex-1">
        <div className="flex flex-wrap items-center gap-3">
          <h3 className="font-semibold">{plugin.name}</h3>
          {plugin.version && (
            <span className="font-mono text-xs text-fg-muted">{plugin.version}</span>
          )}
          <StatusDot tone={statusTone(plugin.status)}>{t(statusLabels[plugin.status])}</StatusDot>
          {plugin.dev && (
            <span
              data-part="dev-badge"
              title={t`Loaded in place from its folder and reloaded on change; not installed`}
              className="rounded-control border border-accent-alt/50 px-2 py-0.5 font-mono text-[11px] text-accent-alt"
            >
              <Trans comment="Badge on a plugin loaded from its development folder">Dev</Trans>
            </span>
          )}
        </div>
        {plugin.description && <p className="mt-1 text-sm text-fg-muted">{plugin.description}</p>}
        {capabilities.length > 0 && (
          <ul
            aria-label={t`${{ plugin: plugin.name }} permissions`}
            className="mt-2 flex flex-wrap gap-2"
          >
            {capabilities.map((line) => (
              <li
                key={line}
                className="rounded-control border border-border px-2 py-0.5 text-xs text-fg-muted"
              >
                {line}
              </li>
            ))}
          </ul>
        )}
        {plugin.status === "invalid" && (
          <p className="mt-2 text-sm text-danger">
            {plugin.errors.map((e) => (e.key ? `${e.key}: ${e.message}` : e.message)).join("; ")}
          </p>
        )}
        {plugin.lastError && <p className="mt-2 text-xs text-warning">{plugin.lastError}</p>}
        {plugin.warnings && plugin.warnings.length > 0 && (
          <p className="mt-2 text-xs text-warning">
            {plugin.warnings.map((e) => (e.key ? `${e.key}: ${e.message}` : e.message)).join("; ")}
          </p>
        )}
        {(toggle.isError || remove.isError) && (
          <p role="alert" className="mt-2 text-sm text-danger">
            {(toggle.error ?? remove.error)?.message}
          </p>
        )}
      </div>
      <div className="flex shrink-0 items-center gap-2">
        {confirming ? (
          <>
            <Button
              variant="danger"
              size="sm"
              isDisabled={remove.isPending}
              onPress={() => remove.mutate()}
            >
              <Trans>Uninstall {plugin.name}</Trans>
            </Button>
            <Button variant="ghost" size="sm" onPress={() => setConfirming(false)}>
              <Trans comment="Cancels uninstalling a plugin">Keep</Trans>
            </Button>
          </>
        ) : (
          <Button variant="ghost" size="sm" onPress={() => setConfirming(true)}>
            <Trans>Uninstall</Trans>
          </Button>
        )}
        {plugin.status === "needs_consent" ? (
          <Button variant="secondary" onPress={() => review(plugin)}>
            <Trans>Review and allow</Trans>
          </Button>
        ) : (
          <Switch
            aria-label={t`Enable ${{ plugin: plugin.name }}`}
            isSelected={on}
            isDisabled={plugin.status === "invalid" || toggle.isPending}
            onChange={(enabled) => toggle.mutate({ id: plugin.id, enabled })}
          />
        )}
      </div>
      {menus.menu}
    </li>
  );
}

/** Settings → Plugins: installed plugins with their capabilities, install
 * from a folder after review, and the settings pages plugins contribute. */
export function PluginsSettings() {
  const { t } = useLingui();
  const api = usePluginManager();
  const client = useQueryClient();
  const plugins = useManagedPlugins();
  const [review, setReview] = useState<{
    plugin: PluginSummary;
    folder?: string;
    installedVersion?: string | null;
  } | null>(null);
  const picker = usePathPicker();
  const chooseFolder = () =>
    picker.pick({ kind: "folder", title: t`Plugin folder` }, () => api.chooseFolder());
  const choose = useMutation({
    // A dropped folder, or one picked (in console mode, in the app's browser).
    mutationFn: async (path?: string) => {
      const folder = path ?? (await chooseFolder());
      if (!folder) return null;
      const preview = await api.preview(folder);
      return { plugin: preview.plugin, folder, installedVersion: preview.installedVersion };
    },
    onSuccess: (offer) => {
      if (offer) setReview(offer);
    },
  });
  // Development builds: load a folder in place and reload it on change (#131).
  const unpacked = useMutation({
    mutationFn: async () => {
      const folder = await chooseFolder();
      if (folder) await api.loadUnpacked?.(folder);
    },
    onSuccess: () => client.invalidateQueries({ queryKey: ["plugins"] }),
  });
  const accept = useMutation({
    mutationFn: async () => {
      if (!review) return;
      if (review.folder) await api.install(review.folder);
      else await api.setEnabled(review.plugin.id, true);
    },
    onSuccess: async () => {
      setReview(null);
      await client.invalidateQueries({ queryKey: ["plugins"] });
    },
  });

  // A plugin folder dropped on the list opens its review, like the picker.
  const drop = useDropZone<HTMLElement>((paths) => {
    if (paths[0]) choose.mutate(paths[0]);
  });
  return (
    <div className="flex flex-col gap-6">
      <section ref={drop.ref} aria-labelledby="installed-plugins" className="flex flex-col gap-3">
        <div className="flex items-center justify-between gap-3">
          <h2 id="installed-plugins" className="font-display text-lg font-semibold">
            <Trans>Installed plugins</Trans>
          </h2>
          <Button
            variant="secondary"
            icon={FolderOpen}
            onPress={() => choose.mutate(undefined)}
            isDisabled={choose.isPending}
          >
            <Trans>Install from folder</Trans>
          </Button>
          {import.meta.env.DEV && api.loadUnpacked && (
            <Button
              variant="ghost"
              icon={FolderOpen}
              onPress={() => unpacked.mutate()}
              isDisabled={unpacked.isPending}
            >
              <Trans>Load unpacked</Trans>
            </Button>
          )}
        </div>
        {(choose.isError || unpacked.isError) && (
          <p role="alert" className="text-sm text-danger">
            {(choose.error ?? unpacked.error)?.message}
          </p>
        )}
        {plugins.isPending && (
          <p className="text-sm text-fg-muted">
            <Trans>Loading plugins…</Trans>
          </p>
        )}
        {plugins.isError && (
          <p role="alert" className="text-sm text-danger">
            {plugins.error.message}
          </p>
        )}
        {plugins.data && plugins.data.length === 0 && (
          <p className="rounded-panel border border-dashed border-border-strong p-8 text-center text-sm text-fg-muted">
            <Trans>
              No plugins yet. Get one from the catalogue below, install one from a folder, or drop
              its folder here.
            </Trans>
          </p>
        )}
        {plugins.data && plugins.data.length > 0 && (
          <ul aria-label={t`Installed plugins`} className="flex flex-col gap-3">
            {plugins.data.map((plugin) => (
              <PluginRow key={plugin.id} plugin={plugin} review={(p) => setReview({ plugin: p })} />
            ))}
          </ul>
        )}
      </section>
      <AddonCatalogue />
      <PluginSettingsPages />
      {review && (
        <ReviewDialog
          plugin={review.plugin}
          installedVersion={review.installedVersion}
          action={review.folder ? t`Install and allow` : t({ message: "Allow", context: "plugin" })}
          busy={accept.isPending}
          error={accept.isError ? accept.error.message : undefined}
          accept={() => accept.mutate()}
          close={() => {
            setReview(null);
            accept.reset();
          }}
        />
      )}
    </div>
  );
}
