import { Trans } from "@lingui/react/macro";
import { useStorageApi } from "../../api/StorageProvider";
import { StoragePanel } from "./StoragePanel";
/** Production and tests receive an explicit backend adapter. */
export function StorageSettings() {
  const api = useStorageApi();
  return api ? (
    <StoragePanel api={api} />
  ) : (
    <p role="alert" className="p-4 text-sm text-fg-muted">
      <Trans>Storage is unavailable. Open the Caskade desktop app to manage locations.</Trans>
    </p>
  );
}
