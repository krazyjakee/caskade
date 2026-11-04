/** Add-ons on the user's other devices (#245), in Settings → Plugins'
 * catalogue: what this device could have too. Nothing installs by itself;
 * each opens its details, where installing a plugin still goes through its
 * permission review here. */
import { Button } from "@caskade/ui";
import { Trans, useLingui } from "@lingui/react/macro";
import { useQuery, useQueryClient } from "@tanstack/react-query";
import { MonitorSmartphone } from "lucide-react";
import { useEffect } from "react";
import { type CatalogueItem, type DeviceAddon, useAddonsApi } from "../../api/addons";
import { useProvidedSyncApi } from "../../api/sync";
import { StateBadge } from "./AddonCatalogue";

const key = ["addons", "from-devices"] as const;

export function DeviceOffers({
  items,
  open,
}: {
  items: CatalogueItem[];
  open: (id: string) => void;
}) {
  const { t } = useLingui();
  const api = useAddonsApi();
  const client = useQueryClient();
  const sync = useProvidedSyncApi();
  const offers = useQuery({
    queryKey: key,
    queryFn: () => api.fromDevices?.() ?? Promise.resolve([] as DeviceAddon[]),
    enabled: Boolean(api.fromDevices),
  });
  // A sync with another device may bring what it has.
  useEffect(() => {
    if (!sync) return;
    return sync.subscribe(() => void client.invalidateQueries({ queryKey: key }));
  }, [sync, client]);
  // One row per add-on, with every device that has it.
  const byAddon = new Map<string, DeviceAddon[]>();
  for (const offer of offers.data ?? []) {
    byAddon.set(offer.addonId, [...(byAddon.get(offer.addonId) ?? []), offer]);
  }
  const rows = [...byAddon.entries()]
    .map(([id, from]) => ({ id, from, item: items.find((each) => each.entry.id === id) }))
    // Installed here since the devices last synced: nothing to offer.
    .filter(
      ({ item }) =>
        !item ||
        (item.compatibility.state !== "installed" &&
          item.compatibility.state !== "updateAvailable"),
    );
  if (rows.length === 0) return null;
  return (
    <section
      data-part="from-devices"
      aria-labelledby="addons-from-devices"
      className="flex flex-col gap-2 rounded-control border border-border bg-surface-base/60 p-4"
    >
      <h3 id="addons-from-devices" className="flex items-center gap-2 font-semibold">
        <MonitorSmartphone aria-hidden className="size-4 text-fg-muted" />
        <Trans>On your other devices</Trans>
      </h3>
      <p className="text-sm text-fg-muted">
        <Trans>Your other devices have these. Nothing installs here until you choose to.</Trans>
      </p>
      <ul className="flex flex-col gap-2">
        {rows.map(({ id, from, item }) => {
          const devices = from.map((offer) => offer.deviceName).join(", ");
          const name = item?.entry.name ?? id;
          return (
            <li
              key={id}
              data-part="device-addon"
              className="flex flex-wrap items-center justify-between gap-3"
            >
              <div className="min-w-0">
                <p className="text-sm font-medium">{name}</p>
                <p className="text-xs text-fg-muted">
                  {item
                    ? t`On ${devices}`
                    : t`On ${devices}. It isn't in the catalogue, so install it there the way it came.`}
                </p>
              </div>
              {item && (
                <div className="flex items-center gap-2">
                  <StateBadge item={item} />
                  <Button
                    data-spatial="focusable"
                    size="sm"
                    onPress={() => open(id)}
                    aria-label={t`See ${name}`}
                  >
                    <Trans>See it</Trans>
                  </Button>
                </div>
              )}
            </li>
          );
        })}
      </ul>
    </section>
  );
}
