import { Button } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useQuery } from "@tanstack/react-query";
import { useEffect, useMemo, useRef, useState } from "react";
import { Label, Radio, RadioGroup } from "react-aria-components";
import { formatPath } from "../../api/presentation";
import {
  formatBytes,
  initialInstallRoot,
  type RootRequirements,
  rootBlockReason,
  type StorageApi,
  skipRootPicker,
} from "../../api/storage";

interface InstallRootPickerProps extends RootRequirements {
  api: StorageApi;
  onSelect: (rootId: string) => void;
  onCancel: () => void;
  alwaysUseDefault?: boolean;
}
/** A root id crosses IPC; UI paths and capabilities are never authoritative. */
export function InstallRootPicker({
  api,
  onSelect,
  onCancel,
  alwaysUseDefault,
  estimatedBytes,
  executable,
  symlinks,
}: InstallRootPickerProps) {
  const { t } = useLingui();
  const needs = useMemo(
    () => ({ estimatedBytes, executable, symlinks }),
    [estimatedBytes, executable, symlinks],
  );
  const roots = useQuery({
    queryKey: ["storage", "roots"],
    queryFn: () => api.list(),
    refetchOnWindowFocus: true,
  });
  const prefs = useQuery({
    queryKey: ["storage", "preferences"],
    queryFn: () => api.preferences(),
    enabled: alwaysUseDefault === undefined,
  });
  const [selected, setSelected] = useState<string | null>(null);
  const autoSelected = useRef(false);
  const preference = alwaysUseDefault ?? prefs.data?.alwaysUseDefault;
  useEffect(() => {
    if (!roots.data || preference === undefined) return;
    const skip = skipRootPicker(roots.data, preference, needs);
    if (skip && !autoSelected.current) {
      autoSelected.current = true;
      onSelect(skip);
    }
  }, [roots.data, preference, onSelect, needs]);
  const error = roots.error ?? prefs.error;
  if (error)
    return (
      <section
        data-cs="install-root-picker"
        role="alert"
        className="rounded-panel border border-danger/40 p-4"
      >
        <p>
          <Trans>Storage locations could not be loaded.</Trans>
        </p>
        <Button
          onPress={() => {
            void roots.refetch();
            void prefs.refetch();
          }}
        >
          <Trans>Try again</Trans>
        </Button>
        <Button onPress={onCancel}>
          <Trans>Cancel</Trans>
        </Button>
      </section>
    );
  if (!roots.data || preference === undefined)
    return <p role="status">{t`Checking storage locations…`}</p>;
  const value = selected ?? initialInstallRoot(roots.data, needs);
  const current = roots.data.find((root) => root.root.id === value);
  const blocked = current
    ? rootBlockReason(current, needs)
    : t`Add a storage location in Settings → Storage.`;
  return (
    <section
      data-cs="install-root-picker"
      className="flex flex-col gap-4 rounded-panel border border-border p-4"
    >
      <div>
        <h2 className="font-display text-lg font-semibold">
          <Trans>Install location</Trans>
        </h2>
        <p className="text-sm text-fg-muted">
          {needs.estimatedBytes === undefined
            ? t`Estimated space: Unknown — free space will be checked again before installation.`
            : t`Estimated space: ${{ size: formatBytes(needs.estimatedBytes) }}`}
        </p>
      </div>
      <RadioGroup value={value ?? undefined} onChange={setSelected} className="flex flex-col gap-2">
        <Label className="text-sm font-medium">
          <Trans>Storage location</Trans>
        </Label>
        {roots.data.map((root) => {
          const reason = rootBlockReason(root, needs);
          return (
            <Radio
              key={root.root.id}
              value={root.root.id}
              isDisabled={Boolean(reason)}
              data-status={root.status}
              className="flex cursor-pointer flex-col gap-1 rounded-control border border-border p-3 data-selected:border-accent-alt data-disabled:opacity-50"
            >
              <span className="font-medium">
                {root.root.label ?? formatPath(root.root.path)}
                {root.root.isDefault
                  ? ` · ${t({ message: "Default", context: "storage location" })}`
                  : ""}
              </span>
              <span className="break-all font-mono text-xs text-fg-muted">
                {formatPath(root.root.path)}
              </span>
              <span className="text-sm">
                {t`${{ free: formatBytes(root.capabilities.freeBytes) }} free`}
                {reason ? ` · ${reason}` : ""}
              </span>
            </Radio>
          );
        })}
      </RadioGroup>
      {blocked && (
        <p role="alert" className="text-sm text-warning">
          {blocked}
        </p>
      )}
      <div className="flex justify-end gap-2">
        <Button onPress={onCancel}>
          <Trans>Cancel</Trans>
        </Button>
        <Button
          isDisabled={Boolean(blocked) || !value}
          onPress={() => {
            if (value && !blocked) onSelect(value);
          }}
        >
          <Trans>Install here</Trans>
        </Button>
      </div>
    </section>
  );
}
