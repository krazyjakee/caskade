import type { MessageDescriptor } from "@lingui/core";
import { msg } from "@lingui/core/macro";
import { useLingui } from "@lingui/react/macro";
import { useQuery } from "@tanstack/react-query";
import type { RootStatus } from "../../api/bindings";
import { useStorageApi } from "../../api/StorageProvider";

const statusLabels: Record<RootStatus, MessageDescriptor> = {
  online: msg({ message: "online", context: "storage location status" }),
  offline: msg({ message: "offline", context: "storage location status" }),
  read_only: msg({ message: "read only", context: "storage location status" }),
  identity_mismatch: msg({ message: "identity mismatch", context: "storage location status" }),
};

export function StorageSummary() {
  const { t } = useLingui();
  const api = useStorageApi();
  const roots = useQuery({
    queryKey: ["storage", "roots"],
    queryFn: async () => (api ? api.list() : []),
    enabled: Boolean(api),
    refetchOnWindowFocus: true,
  });
  const current = roots.data?.find((root) => root.root.isDefault);
  return (
    <span className="truncate font-mono text-xs text-fg-subtle">
      {roots.isError
        ? t`Locations unavailable`
        : current
          ? `${current.root.path} · ${t(statusLabels[current.status])}`
          : roots.isPending && api
            ? t`Checking locations…`
            : t`No default location configured`}
    </span>
  );
}
