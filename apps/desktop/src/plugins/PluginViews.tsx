import { Trans, useLingui } from "@lingui/react/macro";
import { useQuery } from "@tanstack/react-query";
import { Link } from "@tanstack/react-router";
import { Puzzle, X } from "lucide-react";
import { useEffect } from "react";
import { Button } from "react-aria-components";
import { useMode } from "../shell/console/mode";
import { SchemaSettings } from "../views/settings/SchemaSettings";
import { PluginBoundary } from "./PluginBoundary";
import { usePluginApi } from "./PluginProvider";
import { PluginSlot } from "./PluginSlot";
import { shownIn, useContributions, usePluginNotices, usePluginRegistry } from "./registry";
import { pluginSettingsNamespace } from "./settingsNamespace";

/** `/plugins/$pluginId/$viewId`: a plugin's full-page view. */
export function PluginViewPage({ pluginId, viewId }: { pluginId: string; viewId: string }) {
  const api = usePluginApi();
  useEffect(() => {
    api.viewOpened?.(pluginId, viewId).catch((error) => {
      console.error(`Plugin ${pluginId} view ${viewId} failed to start its backend`, error);
    });
  }, [api, pluginId, viewId]);
  const { mode } = useMode();
  const { t } = useLingui();
  const view = useContributions("views", { allModes: true }).find(
    (item) => item.plugin.id === pluginId && item.contribution.id === viewId,
  );
  if (!view)
    return (
      <p role="status" className="py-12 text-fg-muted">
        <Trans>This plugin view isn't available.</Trans>
      </p>
    );
  // A link into a view this mode hides (#152).
  if (!shownIn(view, mode))
    return (
      <p role="status" className="py-12 text-fg-muted">
        {mode === "console"
          ? t`${{ title: view.contribution.title }} isn't available in console mode.`
          : t`${{ title: view.contribution.title }} isn't available in desktop mode.`}
      </p>
    );
  const View = view.contribution.component;
  return (
    <section data-view="plugin" data-plugin={pluginId} className="mt-4">
      <h1 className="mb-4 font-display text-2xl font-semibold">{view.contribution.title}</h1>
      <PluginBoundary key={`${pluginId}/${viewId}`} plugin={view.plugin} context={view.context}>
        <View />
      </PluginBoundary>
    </section>
  );
}

/** Sidebar entries for plugin views that show in this mode, then plugins'
 * own sidebar items, styled like the sidebar's own links. */
export function PluginNav({
  className,
  itemClass,
  iconClass,
}: {
  className?: string;
  itemClass?: string;
  iconClass?: string;
}) {
  const { t } = useLingui();
  const views = useContributions("views");
  const items = useContributions("sidebar.items");
  if (views.length === 0 && items.length === 0) return null;
  return (
    <nav
      aria-label={t({ message: "Plugins", comment: "The sidebar's list of plugin views" })}
      data-part="plugin-nav"
      className={className}
    >
      {views.map(({ plugin, contribution }) => (
        <Link
          key={`${plugin.id}/${contribution.id}`}
          to="/plugins/$pluginId/$viewId"
          params={{ pluginId: plugin.id, viewId: contribution.id }}
          data-part="nav-item"
          className={itemClass}
        >
          <Puzzle data-part="icon" className={iconClass} />
          <span data-part="label" className="flex-1">
            {contribution.title}
          </span>
        </Link>
      ))}
      <PluginSlot slot="sidebar.items">
        {({ contribution: { component: Item } }) => <Item />}
      </PluginSlot>
    </nav>
  );
}

/** Plugins' messages and load failures, dismissable. */
export function PluginNotices() {
  const { t } = useLingui();
  const notices = usePluginNotices();
  const registry = usePluginRegistry();
  if (notices.length === 0) return null;
  return (
    <section
      aria-label={t`Plugin notifications`}
      aria-live="polite"
      data-cs="plugin-notices"
      className="fixed right-6 bottom-14 z-40 flex w-80 flex-col gap-2"
    >
      {notices.map((notice) => (
        <div
          key={notice.id}
          role={notice.tone === "error" ? "alert" : "status"}
          data-status={notice.tone}
          className="flex items-start gap-3 rounded-panel border border-border-strong bg-surface-raised p-3 text-sm shadow-lg data-[status=error]:border-danger"
        >
          <p className="flex-1">
            <span className="block font-mono text-[11px] text-fg-subtle">{notice.pluginName}</span>
            {notice.message}
          </p>
          <Button
            aria-label={t({ message: "Dismiss", comment: "Closes a plugin's notice" })}
            onPress={() => registry.dismiss(notice.id)}
            className="rounded-control p-1 text-fg-muted data-hovered:text-fg"
          >
            <X className="size-4" />
          </Button>
        </div>
      ))}
    </section>
  );
}

/** Settings → Plugins: each plugin's settings page. */
/** Settings → Plugins: one section per enabled plugin with settings. Its
 * own `settings.pages` component wins; otherwise the page is rendered from
 * its `contributes.settings` JSON Schema (#128). */
export function PluginSettingsPages() {
  const { t } = useLingui();
  const api = usePluginApi();
  const pages = useContributions("settings.pages");
  const installed = useQuery({
    queryKey: ["plugins", "installed"],
    queryFn: () => api.installed(),
  });
  if (installed.isPending)
    return (
      <p role="status" className="p-4 text-sm text-fg-muted">
        <Trans>Loading plugins…</Trans>
      </p>
    );
  const plugins = (installed.data ?? []).filter(
    (plugin) =>
      plugin.enabled && (plugin.hasSettings || pages.some((page) => page.plugin.id === plugin.id)),
  );
  if (plugins.length === 0)
    return (
      <div className="rounded-panel border border-dashed border-border-strong p-12 text-center text-fg-subtle">
        {installed.error ? installed.error.message : t`No plugin has settings yet.`}
      </div>
    );
  return (
    <div className="flex flex-col gap-6">
      {plugins.map((plugin) => {
        const own = pages.filter((page) => page.plugin.id === plugin.id);
        return (
          <section
            key={plugin.id}
            data-part="plugin-settings"
            data-plugin={plugin.id}
            aria-label={plugin.name}
            className="rounded-panel border border-border bg-surface-raised p-6"
          >
            <h2 className="mb-4 font-display text-lg font-semibold">{plugin.name}</h2>
            {plugin.settingsError && (
              <p role="alert" className="mb-3 text-sm text-danger">
                {plugin.settingsError}
              </p>
            )}
            {own.length > 0
              ? own.map(
                  ({ plugin: owner, context, contribution: { id, title, component: Page } }) => (
                    <PluginBoundary key={id} plugin={owner} context={context}>
                      {own.length > 1 && <h3 className="mb-2 font-medium">{title}</h3>}
                      <Page />
                    </PluginBoundary>
                  ),
                )
              : plugin.hasSettings &&
                !plugin.settingsError && (
                  <SchemaSettings
                    namespace={pluginSettingsNamespace(plugin.id)}
                    title={plugin.name}
                  />
                )}
          </section>
        );
      })}
    </div>
  );
}
