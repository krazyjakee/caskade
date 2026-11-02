import { useLingui } from "@lingui/react/macro";
import { useQuery } from "@tanstack/react-query";
import type { SaveVersion } from "../../api/saves";
import { syncKey, useProvidedSyncApi } from "../../api/sync";

/** Names the device that made a save version: this device, a paired
 * device by its name, or another device once it's no longer paired. */
export function useSaveDeviceName() {
  const { t } = useLingui();
  const api = useProvidedSyncApi();
  const status = useQuery({
    queryKey: [...syncKey, "status"],
    queryFn: () => {
      if (!api) throw new Error("Devices are unavailable");
      return api.status();
    },
    enabled: Boolean(api),
    retry: false,
  });
  return (version: Pick<SaveVersion, "deviceId" | "local">): string => {
    if (version.local) return t({ message: "This device", context: "save version" });
    const device = status.data?.devices.find((each) => each.id === version.deviceId);
    return device?.name ?? t({ message: "Another device", context: "save version" });
  };
}
