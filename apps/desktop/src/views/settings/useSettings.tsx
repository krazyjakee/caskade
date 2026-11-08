import { t as translate } from "@lingui/core/macro";
import { Trans, useLingui } from "@lingui/react/macro";
import { useMutation, useQuery, useQueryClient } from "@tanstack/react-query";
import { createContext, type ReactNode, useContext, useEffect } from "react";
import { nativeSettingsApi, type SettingsApi, type SettingsSnapshot } from "../../api/settings";
import { effectSubscription } from "../../api/subscription";
import { FailureNotice } from "../../components/FailureNotice";

const SettingsContext = createContext<SettingsApi | null>(null);
/** The backend a `SettingsProvider` without an `api` uses: the core, or a
 * fixture around the whole app in tests. */
export const DefaultSettingsApi = createContext<SettingsApi>(nativeSettingsApi);
export const settingsKeys = { section: (namespace: string) => ["settings", namespace] as const };
export function SettingsProvider({
  api: given,
  children,
}: {
  api?: SettingsApi;
  children: ReactNode;
}) {
  const fallback = useContext(DefaultSettingsApi);
  const api = given ?? fallback;
  const client = useQueryClient();
  useEffect(
    () =>
      effectSubscription(api.subscribe, ({ namespace }) => {
        void client.invalidateQueries({ queryKey: settingsKeys.section(namespace) });
      }),
    [api, client],
  );
  return <SettingsContext.Provider value={api}>{children}</SettingsContext.Provider>;
}

export function useSettingsApi(): SettingsApi {
  const api = useContext(SettingsContext);
  if (!api) throw new Error("SettingsProvider is required");
  return api;
}

/** Whether a `SettingsProvider` is around: parts of views that also render
 * on their own (in their tests) read settings only then. */
export function useHasSettings(): boolean {
  return useContext(SettingsContext) !== null;
}

export function useSettingsSection<T extends object>(namespace: string) {
  const api = useSettingsApi();
  const client = useQueryClient();
  const key = settingsKeys.section(namespace);
  const query = useQuery({ queryKey: key, queryFn: () => api.get(namespace), staleTime: 30_000 });
  const mutation = useMutation({
    mutationKey: key,
    scope: { id: `settings:${namespace}` },
    mutationFn: async (change: { update: (current: T) => T } | { reset: true }) => {
      // Serialized writes merge with the latest backend value. A rapid second
      // control change never submits an old whole-section snapshot.
      const latest = await api.get(namespace);
      return "reset" in change
        ? api.reset(namespace, latest.revision)
        : api.set(
            namespace,
            change.update(latest.value as T) as Record<string, unknown>,
            latest.revision,
          );
    },
    onSuccess: (snapshot: SettingsSnapshot) => {
      client.setQueryData(key, snapshot);
    },
    // After a refused write, the controls show what the core has saved.
    onError: () => {
      void client.invalidateQueries({ queryKey: key });
    },
  });
  return {
    settings: query.data?.value as T | undefined,
    loading: query.isPending,
    saving: mutation.isPending,
    error: query.error ?? mutation.error,
    /** The section didn't load (or reload). */
    loadError: query.error,
    /** The last write didn't save; the controls show what is saved. */
    saveError: mutation.error,
    reload: () => {
      void query.refetch();
    },
    /** Sends the last change again, after a failed write. */
    retry: () => {
      if (mutation.variables) mutation.mutate(mutation.variables);
    },
    setSettings: (update: (current: T) => T) => mutation.mutate({ update }),
    reset: () => mutation.mutate({ reset: true }),
  };
}

/** Read the authoritative embedded schema through the same injected transport. */
export function useSettingsDefinition(namespace: string) {
  const api = useSettingsApi();
  return useQuery({
    queryKey: ["settings", "definition", namespace],
    staleTime: 30_000,
    queryFn: async () => {
      const definition = (await api.sections()).find((section) => section.namespace === namespace);
      if (!definition) throw new Error(translate`This settings section is unavailable.`);
      return definition;
    },
  });
}

export function SettingsFeedback({ error, saving }: { error: unknown; saving: boolean }) {
  if (error) return <FailureNotice error={error} variant="inline" className="mb-3" />;
  return saving ? (
    <p role="status" className="mb-3 text-sm text-fg-muted">
      <Trans>Saving…</Trans>
    </p>
  ) : null;
}
export function SettingsLoading({ error, reload }: { error: unknown; reload: () => void }) {
  const { t } = useLingui();
  return error ? (
    <FailureNotice error={error} title={t`Settings couldn't be loaded`} onRetry={reload} />
  ) : (
    <p role="status" className="p-4 text-sm text-fg-muted">
      <Trans>Loading settings…</Trans>
    </p>
  );
}

/** What `SettingsStatus` reads from a `useSettingsSection`. */
export interface SettingsSectionState {
  settings: object | undefined;
  loadError: unknown;
  saveError: unknown;
  saving: boolean;
  reload: () => void;
  retry: () => void;
}

/** One section's load and writes, shown beside its controls (#42): the
 * load failure with Try again, Saving…, and a write that didn't save with
 * Retry, which sends the same change again. Controls keep showing the saved
 * value meanwhile. `what` names the settings ("Automatic backups"). Renders
 * nothing while the section loads or once everything saved. */
export function SettingsStatus({
  section,
  what,
  className = "mt-3",
}: {
  section: SettingsSectionState;
  what: string;
  className?: string;
}) {
  const { t } = useLingui();
  if (!section.settings) {
    return section.loadError ? (
      <FailureNotice
        variant="inline"
        className={className}
        error={section.loadError}
        title={t({
          message: `${what} couldn't be loaded`,
          comment: "{what} names a group of settings, such as Automatic backups",
        })}
        onRetry={section.reload}
      />
    ) : null;
  }
  if (section.saving) {
    return (
      <p role="status" className={`text-sm text-fg-muted ${className}`}>
        <Trans>Saving…</Trans>
      </p>
    );
  }
  if (!section.saveError) return null;
  return (
    <FailureNotice
      variant="inline"
      className={className}
      error={section.saveError}
      title={
        <>
          {t({
            message: `${what}: your change didn't save`,
            comment: "{what} names a group of settings, such as Automatic backups",
          })}
          <span className="block text-xs font-normal text-fg-muted">
            <Trans>The settings here show what's saved. Retry sends your change again.</Trans>
          </span>
        </>
      }
      onRetry={section.retry}
      retryLabel={t({ message: "Retry", context: "settings write" })}
    />
  );
}
