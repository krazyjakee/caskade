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
  });
  return {
    settings: query.data?.value as T | undefined,
    loading: query.isPending,
    saving: mutation.isPending,
    error: query.error ?? mutation.error,
    reload: () => {
      void query.refetch();
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
